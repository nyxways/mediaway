//! D3D12 native HEVC decode session — mirrors [`super::Session`]/[`super::D3d12VideoDecoder`]
//! (same D3D12 object fields, `open`/`ensure_session_ready`/`push_packet`/`decode_slice`/
//! `poll_frame`/`flush`/`release_output` shape), **parallel, not shared** — see
//! `hevc_ops.rs`'s module doc for why. Single-forward-reference P-slice + I/IDR, Main
//! profile, 8-bit 4:2:0, single-tile/no-WPP (ADR-0004 § Scope decision).

use std::collections::VecDeque;

use crate::{DecodeError, VideoDecoderConfig, VideoOutputPreference};
use mediaway_common::{Bytes, CodecKind, GpuDeviceHandle, NativeHandle, PixelFormat};
use mediaway_sw::h264::split_annex_b;
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

use super::CALLER_HEADROOM;
use super::dpb::DpbPool;
use super::hevc_poc::PocState;
use super::hevc_refs::HevcRefMeta;
use super::hevc_vps_sps_pps::{HevcNalUnit, HevcNalUnitType, Pps, Sps, is_reference_nal};
use super::{hevc_pic_params, hevc_refs, hevc_slice, hevc_vps_sps_pps, setup, util};

/// Where a decoded HEVC picture's pixels live, returned by
/// [`D3d12VideoDecoderHevc::poll_frame`]. Same shape as the top-level [`super::DecodedOutput`]
/// — kept as a separate, small type rather than shared (ADR-0004 § File layout plan: "not
/// worth the coupling for two ~15-line structs").
#[derive(Debug)]
pub(crate) enum DecodedOutputHevc {
    /// Zero-Copy: `resource` is this session's DPB texture-array `ID3D12Resource*`,
    /// `subresource` the array-slice index. The caller must call
    /// [`D3d12VideoDecoderHevc::release_output`] once done reading it.
    Gpu {
        resource: NativeHandle,
        subresource: u32,
    },
    /// [`SessionHevc::readback_dpb_slot_to_cpu`] result — tightly-packed NV12 bytes.
    Cpu { data: Bytes },
}

/// One decoded HEVC picture.
#[derive(Debug)]
pub(crate) struct DecodedFrameHevc {
    pub(crate) pts: i64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) output: DecodedOutputHevc,
}

/// D3D12 objects + DPB created lazily once the first SPS is parsed — same shape as
/// [`super::Session`], retyped for HEVC.
pub(super) struct SessionHevc {
    pub(super) decoder: ID3D12VideoDecoder,
    pub(super) decoder_heap: ID3D12VideoDecoderHeap,

    pub(super) decode_queue: ID3D12CommandQueue,
    pub(super) decode_allocator: ID3D12CommandAllocator,
    pub(super) decode_list: ID3D12VideoDecodeCommandList1,

    pub(super) copy_queue: ID3D12CommandQueue,
    pub(super) copy_allocator: ID3D12CommandAllocator,
    pub(super) copy_list: ID3D12GraphicsCommandList,

    pub(super) fence: ID3D12Fence,
    pub(super) fence_event: HANDLE,
    pub(super) fence_value: u64,

    pub(super) dpb: DpbPool<HevcRefMeta>,
    pub(super) bitstream_buffer: ID3D12Resource,
    pub(super) bitstream_capacity: u64,
    pub(super) readback_buffer: ID3D12Resource,

    pub(super) width: u32,
    pub(super) height: u32,
}

impl Drop for SessionHevc {
    fn drop(&mut self) {
        if !self.fence_event.is_invalid() {
            // SAFETY: closing an owned event handle created in `ensure_session_ready` via
            // `CreateEventW`.
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.fence_event) };
        }
    }
}

/// D3D12 native HEVC decode session (single-forward-reference P-slice, RPS-application
/// DPB marking, Zero-Copy output). See [`super::D3d12VideoDecoder`]'s module doc for what
/// "Zero-Copy" means here today (unchanged by this file).
pub(crate) struct D3d12VideoDecoderHevc {
    device: ID3D12Device,
    video_device: ID3D12VideoDevice,
    device4: ID3D12Device4,

    session: Option<SessionHevc>,
    active_sps: Option<Sps>,
    active_pps: Option<Pps>,
    poc_state: PocState,

    output: VideoOutputPreference,
    pending: VecDeque<DecodedFrameHevc>,
    flushed: bool,
    status_report_counter: u32,
}

// SAFETY: same reasoning as `super::D3d12VideoDecoder`'s identical manual `Send` impl —
// every field is a `windows`-crate COM wrapper (thread-safe reference-counted interface)
// or plain owned data; the one raw-pointer-shaped field (`session.fence_event: HANDLE`)
// is exactly what this manual impl exists to assert `Send` for.
#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "HANDLE wraps a raw pointer with no auto Send impl; this manual impl is \
    exactly the intended assertion for it, not an oversight — mirrors D3d12VideoDecoder's \
    identical allow"
)]
unsafe impl Send for D3d12VideoDecoderHevc {}

impl D3d12VideoDecoderHevc {
    /// Open a D3D12 native HEVC decode session for `config`. Mirrors
    /// [`super::D3d12VideoDecoder::open`]'s doc (D3D12 decoder/heap/DPB are created lazily
    /// once the first SPS is parsed, see [`Self::ensure_session_ready`]).
    ///
    /// # Errors
    ///
    /// [`DecodeError::Unsupported`] for non-HEVC codecs or a non-NV12 pixel format.
    /// [`DecodeError::InvalidInput`] when `config.gpu_device` is not
    /// `Some(GpuDeviceHandle::DirectX12(_))`.
    pub(crate) fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        if config.codec != CodecKind::Hevc {
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

    /// Create D3D12 decoder/heap/DPB/command objects on first use of `sps` — a no-op once
    /// a session already exists. Mirrors [`super::D3d12VideoDecoder::ensure_session_ready`]
    /// (same real hardware findings this session's readback/DPB sizing carries forward:
    /// row-pitch-aligned readback buffer, NV12 two-plane texture array).
    fn ensure_session_ready(&mut self, sps: &Sps) -> Result<(), DecodeError> {
        if self.session.is_some() {
            return Ok(());
        }
        let width = sps.pic_width_in_luma_samples;
        let mut height = sps.pic_height_in_luma_samples;

        let support = super::hevc::check_support(&self.video_device, width, height)?;
        if support
            .ConfigurationFlags
            .contains(D3D12_VIDEO_DECODE_CONFIGURATION_FLAG_HEIGHT_ALIGNMENT_MULTIPLE_32_REQUIRED)
        {
            height = util::align_up_u32(height, 32);
        }

        // `sps.max_dec_pic_buffering` already includes the current picture's own slot
        // (ITU-T H.265 semantics, unlike H.264's `max_num_ref_frames` which is references
        // only — see `Sps::max_dec_pic_buffering`'s doc), so no separate "+1 for the
        // current picture" term is added here (contrast `d3d12_video_decode.rs::
        // ensure_session_ready`'s `+ 1`). DPB-sizing-formula validation against a real
        // stream's signaled value is still open (ADR-0002 Open Question #4, inherited by
        // ADR-0004 unresolved).
        let max_dpb_slots = sps.max_dec_pic_buffering.max(1) + CALLER_HEADROOM;
        let (decoder, decoder_heap) =
            super::hevc::create_decoder(&self.video_device, width, height, max_dpb_slots)?;

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

        let bitstream_capacity =
            u64::from(width) * u64::from(height) + super::BITSTREAM_SAFETY_MARGIN;
        let bitstream_buffer = setup::create_linear_buffer(
            &self.device,
            D3D12_HEAP_TYPE_UPLOAD,
            bitstream_capacity,
            D3D12_RESOURCE_STATE_VIDEO_DECODE_READ,
        )?;
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

        self.session = Some(SessionHevc {
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

    /// Submit one compressed packet (Annex-B framed). May decode zero or more pictures,
    /// queued for [`Self::poll_frame`].
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] when a NAL is malformed, uses an unsupported feature (see
    /// `hevc_vps_sps_pps`/`hevc_slice`'s per-function docs), or a D3D12 call fails.
    pub(crate) fn push_packet(
        &mut self,
        packet: &mediaway_common::Packet,
    ) -> Result<(), DecodeError> {
        if self.flushed {
            return Err(DecodeError::Closed);
        }
        let units = split_annex_b(&packet.payload).map_err(|_err| DecodeError::InvalidInput)?;
        for unit_bytes in units {
            let nal = HevcNalUnit::parse(unit_bytes)?;
            match nal.unit_type {
                HevcNalUnitType::Sps => {
                    self.active_sps = Some(hevc_vps_sps_pps::parse_sps(&nal.rbsp)?);
                }
                HevcNalUnitType::Pps => {
                    self.active_pps = Some(hevc_vps_sps_pps::parse_pps(&nal.rbsp)?);
                }
                HevcNalUnitType::Trail(_) | HevcNalUnitType::Idr => {
                    self.decode_slice(unit_bytes, &nal, packet.pts)?;
                }
                // CRA is explicitly rejected, loudly, not silently dropped — see
                // `hevc_vps_sps_pps::HevcNalUnitType::Cra`'s doc for why this module
                // doesn't support it this pass.
                HevcNalUnitType::Cra => return Err(DecodeError::Unsupported),
                // VPS (no field this module needs, see hevc_vps_sps_pps.rs's module doc)
                // and every other non-VCL type (SEI/AUD/EOS/...) are safely inert.
                HevcNalUnitType::Vps | HevcNalUnitType::Other(_) => {}
            }
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one linear per-picture decode sequence (SPS/PPS lookup -> POC -> RPS \
        eviction -> ref lists -> DPB slot management -> DecodeFrame1 -> output); \
        splitting fragments the DPB-state invariant, mirrors d3d12_video_decode.rs::\
        decode_slice's identical shape"
    )]
    fn decode_slice(
        &mut self,
        nal_bytes: &[u8],
        nal: &HevcNalUnit,
        pts: i64,
    ) -> Result<(), DecodeError> {
        let is_idr = nal.unit_type.is_idr();
        let is_ref = is_reference_nal(nal.raw_nal_unit_type);
        let sps = self.active_sps.ok_or(DecodeError::InvalidInput)?; // `Sps` is `Copy` — no clone needed
        let pps = self.active_pps.ok_or(DecodeError::InvalidInput)?; // `Pps` is `Copy` — no clone needed
        self.ensure_session_ready(&sps)?;

        let sh = hevc_slice::parse_slice_header(&nal.rbsp, nal.unit_type, &sps, &pps)?;
        let (poc, next_poc_state) =
            self.poc_state
                .compute(&sps, sh.pic_order_cnt_lsb, is_idr, is_ref);

        let Some(session) = self.session.as_mut() else {
            return Err(DecodeError::Backend);
        };

        // RPS-application eviction (ITU-T H.265 § 8.3.2): IDR clears the whole DPB;
        // otherwise evict any current reference whose POC is not named anywhere in this
        // picture's own signaled RPS (see `hevc_refs::slots_to_evict`'s doc).
        let refs_before = session.dpb.table().references();
        if is_idr {
            for &(slot, _) in &refs_before {
                session.dpb.table_mut().evict(slot)?;
            }
        } else if let Some(rps) = &sh.short_term_rps {
            let all_poc = rps.all_poc(poc);
            for slot in hevc_refs::slots_to_evict(&refs_before, &all_poc) {
                session.dpb.table_mut().evict(slot)?;
            }
        }
        let refs_before = session.dpb.table().references();

        // `RefPicList`/`PicOrderCntValList` are built from every active DPB reference
        // regardless of slice type (mirrors real DXVA producers keeping the driver's own
        // DPB bookkeeping consistent picture-to-picture); `RefPicSetStCurrBefore`/`After`
        // are only ever non-empty for a P-slice — a non-IDR I-slice's own RPS (if any)
        // legitimately has zero `used_by_curr_pic` entries (it uses no references),
        // even though the RPS itself may still list "foll" pictures kept for a *later*
        // picture's reference use (already accounted for by the eviction pass above).
        let (before, after) = sh
            .short_term_rps
            .as_ref()
            .map_or_else(Default::default, |rps| rps.curr_before_after_poc(poc));
        let ref_lists = hevc_refs::build_ref_lists(&refs_before, &before, &after)?;

        let output_slot = session.dpb.table_mut().acquire_free_slot()?;
        self.status_report_counter = self.status_report_counter.wrapping_add(1);
        let mut pic_params = hevc_pic_params::build_pic_params(
            &sps,
            &pps,
            &sh,
            poc,
            output_slot,
            is_idr,
            &ref_lists,
            self.status_report_counter,
        );
        let mut qmatrix = hevc_pic_params::flat_qmatrix();
        let mut slice_short =
            hevc_pic_params::build_slice_short(0, u32::try_from(nal_bytes.len()).unwrap_or(0));

        session.decode_frame(
            nal_bytes,
            &mut pic_params,
            &mut qmatrix,
            &mut slice_short,
            output_slot,
            &refs_before,
        )?;

        if is_ref {
            session
                .dpb
                .table_mut()
                .mark_reference(output_slot, HevcRefMeta { poc });
        }
        self.poc_state = next_poc_state;

        let width = session.width;
        let height = session.height;
        let output = match self.output {
            VideoOutputPreference::CpuFramesOk => {
                let data = session.readback_dpb_slot_to_cpu(output_slot)?;
                session.dpb.table_mut().release_if_unused(output_slot);
                DecodedOutputHevc::Cpu { data }
            }
            VideoOutputPreference::ZeroCopyGpu => {
                session.dpb.table_mut().mark_handle_outstanding(output_slot);
                let raw = windows::core::Interface::as_raw(session.dpb.texture()) as usize;
                let resource = NativeHandle::new(raw).ok_or(DecodeError::Backend)?;
                DecodedOutputHevc::Gpu {
                    resource,
                    subresource: output_slot,
                }
            }
        };
        self.pending.push_back(DecodedFrameHevc {
            pts,
            width,
            height,
            output,
        });
        Ok(())
    }

    /// Release a previously-handed-out Zero-Copy [`DecodedOutputHevc::Gpu`] handle for
    /// `subresource`. Mirrors [`super::D3d12VideoDecoder::release_output`].
    pub(crate) fn release_output(&mut self, subresource: u32) {
        if let Some(session) = self.session.as_mut() {
            session.dpb.table_mut().release_handle(subresource);
        }
    }

    /// Pull the next decoded picture, if any.
    #[allow(
        clippy::unnecessary_wraps,
        reason = "mirrors D3d12VideoDecoder::poll_frame's Result shape for a later \
        integration pass, even though this stage's poll_frame never actually errors"
    )]
    pub(crate) fn poll_frame(&mut self) -> Result<Option<DecodedFrameHevc>, DecodeError> {
        Ok(self.pending.pop_front())
    }

    /// Signal end-of-input. Same decode-order-only caveat as
    /// [`super::D3d12VideoDecoder::flush`] (no POC-based reorder/"bumping" buffer yet).
    #[allow(
        clippy::unnecessary_wraps,
        clippy::missing_const_for_fn,
        reason = "mirrors D3d12VideoDecoder::flush's Result shape for a later \
        integration pass; not meant to be evaluated in const context"
    )]
    pub(crate) fn flush(&mut self) -> Result<(), DecodeError> {
        self.flushed = true;
        Ok(())
    }
}
