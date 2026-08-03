//! D3D12 native video-decode backend (`ID3D12VideoDevice`/`ID3D12VideoDecoder`) —
//! **H.264 only this stage** (HEVC/AV1 are explicit follow-ups, see ADR-0002), general
//! GOP (P/B reference frames, real DPB / reference-picture-list management), Zero-Copy
//! D3D12 texture output.
//!
//! This is a **different** path than [`crate::windows::wmf`]: that module drives Media
//! Foundation's decoder MFTs, which parse the bitstream internally. This module instead
//! drives the **native D3D12 video-decode API** end to end — real SPS/PPS/slice-header
//! parsing, POC computation, reference-picture-list construction, and DPB sliding-window
//! marking all happen in this crate (see [`h264_sps_pps`]/[`h264_slice`]/[`h264_poc`]/
//! [`h264_refs`]), driving DXVA-shaped picture-parameter buffers ([`h264_pic_params`])
//! through `ID3D12VideoDecodeCommandList1::DecodeFrame1`. See
//! [ADR-0002](../adr/0002-d3d12-native-video-decode.md) and its H.264 addendum.
//!
//! **Not wired into [`crate::windows::WindowsVideoDecoder`] yet** — this module is self-contained
//! and unregistered; a later integration pass adds `pub mod d3d12_video_decode;` to
//! `src/lib.rs`. It also does **not** implement [`crate::VideoDecoder`] yet:
//! that trait's `poll_frame` returns `mediaway_common::VideoFrame`, whose
//! `GpuBufferHandle::DirectX12` variant carries only a resource pointer — **no
//! subresource field** — so it cannot address one slot of this module's texture-array
//! DPB. [`poll_frame`](D3d12VideoDecoder::poll_frame) here returns the local
//! [`DecodedFrame`]/[`DecodedOutput`] types instead; a later integration pass either
//! extends `GpuBufferHandle::DirectX12` (cross-crate decision, see ADR-0002 Addendum) or
//! maps `DecodedOutput::Gpu` some other way.
//!
//! Split across sibling files to stay under the 1000-line source limit: [`setup`]
//! (shared `open`-time device/queue/feature-query helpers), [`dpb`] (fixed-size DPB slot
//! pool), [`ops`] (per-frame `DecodeFrame1` submission + CPU readback), [`util`] (small
//! shared recording helpers) — all four codec-generic per ADR-0002's file-layout plan —
//! plus H.264-specific [`h264`]/[`h264_sps_pps`]/[`h264_slice`]/[`h264_poc`]/
//! [`h264_refs`]/[`h264_pic_params`].

#![allow(unsafe_code)]
#![allow(
    dead_code,
    reason = "every open()/push_packet() call path here is only reachable from this \
    module's own #[cfg(test)] tests today (not wired into crate::windows::WindowsVideoDecoder \
    yet, see module doc) — same root cause as mediaway-encoder-windows's \
    d3d12_video_encode module, resolved together once a later pass wires this backend \
    into the public API"
)]
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) intentionally survives the mod d3d12_video_decode; wrapper \
    going pub once a later pass wires this backend in (see module doc) — same as \
    mediaway-encoder-windows's d3d12_video_encode module"
)]

use std::collections::VecDeque;

use crate::{DecodeError, VideoDecoderConfig, VideoOutputPreference};
use mediaway_common::{Bytes, CodecKind, GpuDeviceHandle, NativeHandle, PixelFormat};
use mediaway_sw::h264::{NalUnit, NalUnitType, split_annex_b};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_LIST_TYPE_COPY, D3D12_COMMAND_LIST_TYPE_VIDEO_DECODE, D3D12_FENCE_FLAG_NONE,
    D3D12_HEAP_TYPE_READBACK, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_COMMON,
    D3D12_RESOURCE_STATE_VIDEO_DECODE_READ, ID3D12CommandAllocator, ID3D12CommandQueue,
    ID3D12Device, ID3D12Device4, ID3D12Fence, ID3D12GraphicsCommandList, ID3D12Resource,
};
use windows::Win32::Media::MediaFoundation::{
    D3D12_VIDEO_DECODE_CONFIGURATION_FLAG_HEIGHT_ALIGNMENT_MULTIPLE_32_REQUIRED,
    ID3D12VideoDecodeCommandList1, ID3D12VideoDecoder, ID3D12VideoDecoderHeap, ID3D12VideoDevice,
};
use windows::Win32::System::Threading::CreateEventW;
use windows::core::Interface;

mod dpb;
mod h264;
mod h264_pic_params;
mod h264_poc;
mod h264_refs;
mod h264_slice;
mod h264_sps_pps;
mod ops;
mod setup;
mod util;

#[cfg(test)]
#[path = "d3d12_video_decode_tests.rs"]
mod tests;

use dpb::DpbPool;
use h264_poc::PocState;
use h264_refs::H264RefMeta;
use h264_sps_pps::{Pps, Sps};

/// Extra DPB slots above `max_num_ref_frames`, absorbing ordinary Zero-Copy-handle /
/// output latency without contention (ADR-0002's "`caller_headroom`", default 2-4 —
/// this module uses `3`).
const CALLER_HEADROOM: u32 = 3;
/// Extra bytes above the raw NV12 frame size for the compressed-bitstream buffer — a
/// single H.264 slice is essentially never larger than its raw picture, but this
/// guards against a pathological/adversarial input.
const BITSTREAM_SAFETY_MARGIN: u64 = 65_536;

/// Where a decoded picture's pixels live, returned by [`D3d12VideoDecoder::poll_frame`].
#[derive(Debug)]
pub(crate) enum DecodedOutput {
    /// Zero-Copy: `resource` is this session's DPB texture-array `ID3D12Resource*`,
    /// `subresource` the array-slice index (**not** yet expressible as a
    /// `mediaway_common::GpuBufferHandle::DirectX12` — see module doc). The caller
    /// must call [`D3d12VideoDecoder::release_output`] once done reading it (bounded-
    /// handle backpressure contract, ADR-0002).
    Gpu {
        resource: NativeHandle,
        subresource: u32,
    },
    /// [`ops::Session::readback_dpb_slot_to_cpu`] result — tightly-packed NV12 bytes.
    Cpu { data: Bytes },
}

/// One decoded picture.
#[derive(Debug)]
pub(crate) struct DecodedFrame {
    pub(crate) pts: i64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) output: DecodedOutput,
}

/// D3D12 objects + DPB created lazily once the first SPS is parsed (real coded
/// width/height/`max_num_ref_frames` are bitstream-derived, not known at `open`).
struct Session {
    decoder: ID3D12VideoDecoder,
    decoder_heap: ID3D12VideoDecoderHeap,

    decode_queue: ID3D12CommandQueue,
    decode_allocator: ID3D12CommandAllocator,
    decode_list: ID3D12VideoDecodeCommandList1,

    /// Separate copy-capable command objects for [`ops::Session::readback_dpb_slot_to_cpu`]
    /// — `CopyTextureRegion` is not a valid recording on a
    /// `D3D12_COMMAND_LIST_TYPE_VIDEO_DECODE` list (mirrors
    /// `mediaway-encoder-windows`'s own separate copy queue/list for its CPU-upload path).
    copy_queue: ID3D12CommandQueue,
    copy_allocator: ID3D12CommandAllocator,
    copy_list: ID3D12GraphicsCommandList,

    fence: ID3D12Fence,
    fence_event: HANDLE,
    fence_value: u64,

    dpb: DpbPool<H264RefMeta>,
    bitstream_buffer: ID3D12Resource,
    bitstream_capacity: u64,
    readback_buffer: ID3D12Resource,

    width: u32,
    height: u32,
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.fence_event.is_invalid() {
            // SAFETY: closing an owned event handle created in `ensure_session_ready`
            // via `CreateEventW`.
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.fence_event) };
        }
    }
}

/// D3D12 native H.264 decode session (general GOP, sliding-window DPB marking, Zero-
/// Copy output). See module doc for what "Zero-Copy" means here today.
pub(crate) struct D3d12VideoDecoder {
    device: ID3D12Device,
    video_device: ID3D12VideoDevice,
    device4: ID3D12Device4,

    session: Option<Session>,
    active_sps: Option<Sps>,
    active_pps: Option<Pps>,
    poc_state: PocState,

    output: VideoOutputPreference,
    pending: VecDeque<DecodedFrame>,
    flushed: bool,
    status_report_counter: u32,
}

// SAFETY: all fields are `windows`-crate COM wrappers (thread-safe reference-counted
// interfaces) or plain owned data; no interior aliasing beyond what COM itself
// guarantees. The one field clippy's nursery `non_send_fields_in_send_ty` lint flags
// (`session: Option<Session>`, via `Session::fence_event: HANDLE`) is exactly the kind
// of raw-pointer-shaped handle this manual impl exists to assert Send for in the first
// place — the same false-positive shape `mediaway-encoder-windows`'s `D3d12VideoEncoder`
// (also `fence_event: HANDLE`) accepts without complaint there.
#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "HANDLE wraps a raw pointer with no auto Send impl; this manual impl is \
    exactly the intended assertion for it, not an oversight"
)]
unsafe impl Send for D3d12VideoDecoder {}

impl D3d12VideoDecoder {
    /// Open a D3D12 native H.264 decode session for `config`.
    ///
    /// D3D12 decoder/heap/DPB objects are **not** created here — real coded
    /// width/height/`max_num_ref_frames` are only known once the first SPS is parsed
    /// (see [`Self::ensure_session_ready`]), so this just validates `config` and
    /// resolves the `ID3D12Device`.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Unsupported`] for non-H.264 codecs or a non-NV12 pixel format.
    /// [`DecodeError::InvalidInput`] when `config.gpu_device` is not
    /// `Some(GpuDeviceHandle::DirectX12(_))` — this module always drives real D3D12
    /// decode regardless of [`VideoOutputPreference`], so a device handle is required
    /// even for [`VideoOutputPreference::CpuFramesOk`] (the readback path still decodes
    /// on the GPU, then copies out — see `ops::Session::readback_dpb_slot_to_cpu`).
    pub(crate) fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        if config.codec != CodecKind::H264 {
            return Err(DecodeError::Unsupported);
        }
        if config.pixel_format != PixelFormat::Nv12 {
            return Err(DecodeError::Unsupported);
        }
        let Some(GpuDeviceHandle::DirectX12(handle)) = config.gpu_device else {
            return Err(DecodeError::InvalidInput);
        };
        let device = setup::device_from_handle(handle)?;
        let video_device: ID3D12VideoDevice =
            device.cast().map_err(|_err| DecodeError::Unsupported)?;
        let device4: ID3D12Device4 = device.cast().map_err(|_err| DecodeError::Unsupported)?;

        Ok(Self {
            device,
            video_device,
            device4,
            session: None,
            active_sps: None,
            active_pps: None,
            poc_state: PocState::default(),
            output: config.output,
            pending: VecDeque::new(),
            flushed: false,
            status_report_counter: 0,
        })
    }

    /// Create D3D12 decoder/heap/DPB/command objects on first use of `sps` — a no-op
    /// once a session already exists (this stage does not support mid-stream SPS
    /// changes; a stream that signals a different `seq_parameter_set_id` after the
    /// session is created is not specially detected, a real gap noted in the ADR
    /// Addendum).
    fn ensure_session_ready(&mut self, sps: &Sps) -> Result<(), DecodeError> {
        if self.session.is_some() {
            return Ok(());
        }
        let width = sps.mb_width * 16;
        let mut height = sps.mb_height * 16;

        let support = h264::check_support(&self.video_device, width, height)?;
        if support
            .ConfigurationFlags
            .contains(D3D12_VIDEO_DECODE_CONFIGURATION_FLAG_HEIGHT_ALIGNMENT_MULTIPLE_32_REQUIRED)
        {
            height = util::align_up_u32(height, 32);
        }

        let max_dpb_slots = sps.max_num_ref_frames + CALLER_HEADROOM + 1; // +1 for the current picture's own slot
        let (decoder, decoder_heap) =
            h264::create_decoder(&self.video_device, width, height, max_dpb_slots)?;

        let (decode_queue, decode_allocator, decode_list) =
            setup::create_command_objects::<ID3D12VideoDecodeCommandList1>(
                &self.device,
                &self.device4,
                D3D12_COMMAND_LIST_TYPE_VIDEO_DECODE,
            )?;
        let (copy_queue, copy_allocator, copy_list) =
            setup::create_command_objects::<ID3D12GraphicsCommandList>(
                &self.device,
                &self.device4,
                D3D12_COMMAND_LIST_TYPE_COPY,
            )?;

        // SAFETY: plain POD call, no borrowed pointers.
        let fence: ID3D12Fence = unsafe { self.device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }
            .map_err(|_err| DecodeError::Backend)?;
        // SAFETY: manual-reset=false, initial-state=false, no name; standard CPU wait event.
        let fence_event = unsafe { CreateEventW(None, false, false, None) }
            .map_err(|_err| DecodeError::Backend)?;

        let texture = setup::create_dpb_texture_array(&self.device, width, height, max_dpb_slots)?;
        let dpb = DpbPool::new(texture, max_dpb_slots);

        let bitstream_capacity = u64::from(width) * u64::from(height) + BITSTREAM_SAFETY_MARGIN;
        let bitstream_buffer = setup::create_linear_buffer(
            &self.device,
            D3D12_HEAP_TYPE_UPLOAD,
            bitstream_capacity,
            D3D12_RESOURCE_STATE_VIDEO_DECODE_READ,
        )?;
        // Real hardware finding (D3D12 debug layer, `ID3D12InfoQueue`): the readback
        // buffer must be sized for `CopyTextureRegion`'s **row-pitch-aligned** placed
        // footprints (`D3D12_TEXTURE_DATA_PITCH_ALIGNMENT`-rounded row pitch), not the
        // tightly-packed `nv12_size` — the debug layer reported "PlacedFootprint
        // extends past the end of the buffer" for both the luma and chroma copies when
        // this used `nv12_size` (6144 bytes for 64x64) instead of the row-pitch-aligned
        // size (24384 bytes required). Mirrors `mediaway-encoder-windows`'s own
        // `d3d12_video_encode.rs::open`'s `upload_size = luma_size + luma_size / 2`
        // sizing for its CPU-upload buffer (same alignment constant, same reasoning,
        // opposite data direction).
        let readback_row_pitch = util::align_up_u32(
            width,
            windows::Win32::Graphics::Direct3D12::D3D12_TEXTURE_DATA_PITCH_ALIGNMENT,
        );
        let readback_luma_size = u64::from(readback_row_pitch) * u64::from(height);
        let readback_size = readback_luma_size + readback_luma_size / 2;
        let readback_buffer = setup::create_linear_buffer(
            &self.device,
            D3D12_HEAP_TYPE_READBACK,
            readback_size,
            D3D12_RESOURCE_STATE_COMMON,
        )?;

        self.session = Some(Session {
            decoder,
            decoder_heap,
            decode_queue,
            decode_allocator,
            decode_list,
            copy_queue,
            copy_allocator,
            copy_list,
            fence,
            fence_event,
            fence_value: 0,
            dpb,
            bitstream_buffer,
            bitstream_capacity,
            readback_buffer,
            width,
            height,
        });
        Ok(())
    }

    /// Submit one compressed packet (Annex-B framed — AVCC input is not converted by
    /// this module, a real, deliberate scope cut this stage; see ADR-0002 Addendum).
    /// May decode zero or more pictures, queued for [`Self::poll_frame`].
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] when a NAL is malformed, uses an unsupported feature
    /// (see `h264_sps_pps`/`h264_slice`'s per-function docs), or a D3D12 call fails.
    pub(crate) fn push_packet(
        &mut self,
        packet: &mediaway_common::Packet,
    ) -> Result<(), DecodeError> {
        if self.flushed {
            return Err(DecodeError::Closed);
        }
        let units = split_annex_b(&packet.payload).map_err(|_err| DecodeError::InvalidInput)?;
        for unit_bytes in units {
            let nal = NalUnit::parse(unit_bytes).map_err(|_err| DecodeError::InvalidInput)?;
            match nal.unit_type {
                NalUnitType::Sps => {
                    self.active_sps = Some(h264_sps_pps::parse_sps(&nal.rbsp)?);
                }
                NalUnitType::Pps => {
                    self.active_pps = Some(h264_sps_pps::parse_pps(&nal.rbsp)?);
                }
                NalUnitType::IdrSlice | NalUnitType::NonIdrSlice => {
                    self.decode_slice(unit_bytes, &nal, packet.pts)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one linear per-picture decode sequence (SPS/PPS lookup -> POC -> ref \
        lists -> DPB slot management -> DecodeFrame1 -> output); splitting fragments \
        the DPB-state invariant"
    )]
    fn decode_slice(
        &mut self,
        nal_bytes: &[u8],
        nal: &NalUnit,
        pts: i64,
    ) -> Result<(), DecodeError> {
        let is_idr = matches!(nal.unit_type, NalUnitType::IdrSlice);
        let sps = self.active_sps.clone().ok_or(DecodeError::InvalidInput)?; // clone: `Sps` is a small config struct (not `Copy`, owns a `SmallVec`); needed independently of the `&mut self` borrow `ensure_session_ready` takes next
        let pps = self.active_pps.ok_or(DecodeError::InvalidInput)?; // `Pps` is `Copy` — no clone needed
        self.ensure_session_ready(&sps)?;

        let (sh, deemulated_bits_read) =
            h264_slice::parse_slice_header(&nal.rbsp, nal.unit_type, nal.ref_idc, &sps, &pps)?;
        // `deemulated_bits_read` counts bits in the de-emulated RBSP (`nal.rbsp`), but
        // `nal_bytes` (the actual D3D12 compressed-bitstream buffer content) still has
        // `emulation_prevention_three_byte` bytes in it — translate back to a raw bit
        // offset (real hardware bug found + fixed this session, see ADR-0002 Addendum:
        // feeding the de-emulated count directly hung the GPU).
        let raw_bit_offset_after_header =
            h264_slice::rbsp_bit_offset_to_raw_bit_offset(&nal_bytes[1..], deemulated_bits_read);
        let bit_offset_to_slice_data = 8u32.saturating_add(raw_bit_offset_after_header); // +8 for the 1-byte NAL header
        let (poc, next_poc_state) = self.poc_state.compute(&sps, &sh, is_idr, nal.ref_idc);

        let max_frame_num = 1u32 << sps.log2_max_frame_num;
        let Some(session) = self.session.as_mut() else {
            return Err(DecodeError::Backend);
        };

        let refs_before = session.dpb.table().references();
        if is_idr {
            for &(slot, _) in &refs_before {
                session.dpb.table_mut().evict(slot)?;
            }
        } else if nal.ref_idc != 0 {
            if let Some(evict_slot) = h264_refs::sliding_window_evict(
                &refs_before,
                sh.frame_num,
                max_frame_num,
                sps.max_num_ref_frames,
            ) {
                session.dpb.table_mut().evict(evict_slot)?;
            }
        }
        let refs_before = session.dpb.table().references();

        let (mut list0, mut list1) = match sh.slice_type {
            h264_slice::SliceType::I => (Vec::new(), Vec::new()),
            h264_slice::SliceType::P => {
                let list0 =
                    h264_refs::build_default_list_p(&refs_before, sh.frame_num, max_frame_num);
                (list0, Vec::new())
            }
            h264_slice::SliceType::B => h264_refs::build_default_lists_b(
                &refs_before,
                sh.frame_num,
                max_frame_num,
                poc.pic_order_cnt,
            ),
            // SP/SI slices are already rejected by `h264_slice::parse_slice_header`
            // before it returns `Ok`; this arm is defensive, not reachable in practice.
            h264_slice::SliceType::Sp | h264_slice::SliceType::Si => {
                return Err(DecodeError::Unsupported);
            }
        };
        if !matches!(sh.slice_type, h264_slice::SliceType::I) {
            h264_refs::apply_modifications(
                &mut list0,
                &sh.ref_pic_list_modification_l0,
                sh.frame_num,
                max_frame_num,
                usize::try_from(sh.num_ref_idx_l0_active_minus1 + 1).unwrap_or(0),
            )?;
        }
        if matches!(sh.slice_type, h264_slice::SliceType::B) {
            h264_refs::apply_modifications(
                &mut list1,
                &sh.ref_pic_list_modification_l1,
                sh.frame_num,
                max_frame_num,
                usize::try_from(sh.num_ref_idx_l1_active_minus1 + 1).unwrap_or(0),
            )?;
        }

        let output_slot = session.dpb.table_mut().acquire_free_slot()?;
        let num_mbs_for_slice = sps.mb_width * sps.mb_height;
        self.status_report_counter = self.status_report_counter.wrapping_add(1);
        let mut pic_params = h264_pic_params::build_pic_params(
            &sps,
            &pps,
            &sh,
            poc.top_field_order_cnt,
            poc.bottom_field_order_cnt,
            output_slot,
            nal.ref_idc,
            sh.frame_num,
            &refs_before,
            self.status_report_counter,
        );
        let mut qmatrix = h264_pic_params::flat_qmatrix();
        let mut slice_long = h264_pic_params::build_slice_long(
            &sh,
            num_mbs_for_slice,
            bit_offset_to_slice_data,
            0,
            u32::try_from(nal_bytes.len()).unwrap_or(0),
            &list0,
            &list1,
        );

        session.decode_frame(
            nal_bytes,
            &mut pic_params,
            &mut qmatrix,
            &mut slice_long,
            output_slot,
            &refs_before,
        )?;

        if nal.ref_idc != 0 {
            session.dpb.table_mut().mark_reference(
                output_slot,
                H264RefMeta {
                    frame_num: sh.frame_num,
                    poc: poc.pic_order_cnt,
                    top_field_order_cnt: poc.top_field_order_cnt,
                    bottom_field_order_cnt: poc.bottom_field_order_cnt,
                },
            );
        }
        self.poc_state = next_poc_state;

        let width = session.width;
        let height = session.height;
        let output = match self.output {
            VideoOutputPreference::CpuFramesOk => {
                let data = session.readback_dpb_slot_to_cpu(output_slot)?;
                session.dpb.table_mut().release_if_unused(output_slot);
                DecodedOutput::Cpu { data }
            }
            VideoOutputPreference::ZeroCopyGpu => {
                session.dpb.table_mut().mark_handle_outstanding(output_slot);
                let raw = windows::core::Interface::as_raw(session.dpb.texture()) as usize;
                let resource = NativeHandle::new(raw).ok_or(DecodeError::Backend)?;
                DecodedOutput::Gpu {
                    resource,
                    subresource: output_slot,
                }
            }
        };
        self.pending.push_back(DecodedFrame {
            pts,
            width,
            height,
            output,
        });
        Ok(())
    }

    /// Release a previously-handed-out Zero-Copy [`DecodedOutput::Gpu`] handle for
    /// `subresource` (the caller has consumed/copied the frame). Part of ADR-0002's
    /// bounded-handle backpressure contract — never call this while still reading the
    /// texture.
    pub(crate) fn release_output(&mut self, subresource: u32) {
        if let Some(session) = self.session.as_mut() {
            session.dpb.table_mut().release_handle(subresource);
        }
    }

    /// Pull the next decoded picture, if any.
    #[allow(
        clippy::unnecessary_wraps,
        reason = "mirrors crate::VideoDecoder::poll_frame's Result shape for \
        a later integration pass, even though this stage's poll_frame never actually \
        errors (queue is only ever populated, never rejected, after push_packet succeeds)"
    )]
    pub(crate) fn poll_frame(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
        Ok(self.pending.pop_front())
    }

    /// Signal end-of-input. This stage decodes and outputs pictures in **decode order**,
    /// not display order (no POC-based reorder/"bumping" buffer yet — a real, documented
    /// gap, see ADR-0002 Addendum) — `flush` has nothing buffered to drain beyond what
    /// [`Self::poll_frame`] would already return.
    #[allow(
        clippy::unnecessary_wraps,
        clippy::missing_const_for_fn,
        reason = "mirrors crate::VideoDecoder::flush's Result shape for a \
        later integration pass; not meant to be evaluated in const context"
    )]
    pub(crate) fn flush(&mut self) -> Result<(), DecodeError> {
        self.flushed = true;
        Ok(())
    }
}
