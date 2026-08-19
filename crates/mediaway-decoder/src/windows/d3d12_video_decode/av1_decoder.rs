//! D3D12 native AV1 decode session — mirrors [`super::hevc_decoder::SessionHevc`]/
//! [`super::hevc_decoder::D3d12VideoDecoderHevc`] (same D3D12 object fields, `open`/
//! `ensure_session_ready`/`push_packet`/`poll_frame`/`flush`/`release_output` shape),
//! **parallel, not shared** — same reasoning `hevc_ops.rs`'s module doc gives (the
//! existing H.264-typed `Session`/`ops.rs` stay the last known-consistent baseline for the
//! still-unresolved H.264 D3D12 decode hang). `KEY_FRAME`-only, Main profile, 8-bit 4:2:0,
//! single-tile (ADR-0005 § Scope decision).
//!
//! **No `av1_refs.rs`/POC module** — ADR-0005's single largest structural simplification
//! versus HEVC: every decoded picture is an independent, reference-free `KEY_FRAME`, so
//! this module never calls [`super::dpb::SlotTable::mark_reference`] at all — `refs_before`
//! is always empty, and no RPS/eviction pass is needed before acquiring a free slot (ADR-
//! 0005 § Decision's "no `av1_refs.rs`" note).
//!
//! **Real, deliberate scope narrowing beyond ADR-0005's literal text**: only the `OBU_FRAME`
//! shape (combined `frame_header_obu()` + `tile_group_obu()` in one OBU, AV1 spec §5.10) is
//! supported — a standalone `OBU_FRAME_HEADER`/`OBU_TILE_GROUP`/`OBU_REDUNDANT_FRAME_HEADER`
//! is rejected as [`DecodeError::Unsupported`]. This is the only shape this crate's own AV1
//! encoder emits (`mediaway-encoder-windows`'s `bitstream_av1.rs`/`ops_av1.rs`, both cited
//! by ADR-0005 § Context) and the only realistically obtainable same-workspace test source.

use std::collections::VecDeque;

use crate::{DecodeError, VideoDecoderConfig, VideoOutputPreference};
use mediaway_common::{Bytes, CodecKind, GpuDeviceHandle, NativeHandle, PixelFormat};
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
use super::av1_obu::{ObuType, split_obus};
use super::av1_sequence_header::{SequenceHeader, parse_sequence_header};
use super::dpb::DpbPool;
use super::{av1_frame_header, av1_pic_params, setup, util};

/// Per-slot reference metadata for AV1's DPB — a unit-like marker, since no picture is
/// ever referenced under this module's `KEY_FRAME`-only scope. Kept as a real type rather
/// than `DpbPool<()>` to match [`DpbPool`]'s existing `M: Copy` bound and stay consistent
/// with a future inter-frame follow-up's likely field additions (ADR-0005 § File layout
/// plan).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Av1RefMeta;

/// Where a decoded AV1 picture's pixels live, returned by
/// [`D3d12VideoDecoderAv1::poll_frame`]. Same shape as [`super::DecodedOutput`]/
/// [`super::hevc_decoder::DecodedOutputHevc`] — kept as a separate, small type rather than
/// shared, same "not worth the coupling" reasoning ADR-0004's file layout plan already gave.
#[derive(Debug)]
pub(crate) enum DecodedOutputAv1 {
    /// Zero-Copy: `resource` is this session's DPB texture-array `ID3D12Resource*`,
    /// `subresource` the array-slice index. The caller must call
    /// [`D3d12VideoDecoderAv1::release_output`] once done reading it.
    Gpu {
        resource: NativeHandle,
        subresource: u32,
    },
    /// [`SessionAv1::readback_dpb_slot_to_cpu`] result — tightly-packed NV12 bytes.
    Cpu { data: Bytes },
}

/// One decoded AV1 picture.
#[derive(Debug)]
pub(crate) struct DecodedFrameAv1 {
    pub(crate) pts: i64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) output: DecodedOutputAv1,
}

/// D3D12 objects + DPB created lazily once the first sequence header is parsed — same
/// shape as [`super::Session`]/[`super::hevc_decoder::SessionHevc`], retyped for AV1.
pub(super) struct SessionAv1 {
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

    pub(super) dpb: DpbPool<Av1RefMeta>,
    pub(super) bitstream_buffer: ID3D12Resource,
    pub(super) bitstream_capacity: u64,
    pub(super) readback_buffer: ID3D12Resource,

    pub(super) width: u32,
    pub(super) height: u32,
}

impl Drop for SessionAv1 {
    fn drop(&mut self) {
        if !self.fence_event.is_invalid() {
            // SAFETY: closing an owned event handle created in `ensure_session_ready` via
            // `CreateEventW`.
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.fence_event) };
        }
    }
}

/// D3D12 native AV1 decode session (`KEY_FRAME`-only, Main profile, 8-bit 4:2:0,
/// single-tile, Zero-Copy output). See [`super::D3d12VideoDecoder`]'s module doc for what
/// "Zero-Copy" means here today (unchanged by this file).
pub(crate) struct D3d12VideoDecoderAv1 {
    device: ID3D12Device,
    video_device: ID3D12VideoDevice,
    device4: ID3D12Device4,

    session: Option<SessionAv1>,
    active_seq: Option<SequenceHeader>,

    output: VideoOutputPreference,
    pending: VecDeque<DecodedFrameAv1>,
    flushed: bool,
    status_report_counter: u32,
}

// SAFETY: same reasoning as `super::D3d12VideoDecoder`/`D3d12VideoDecoderHevc`'s identical
// manual `Send` impls — every field is a `windows`-crate COM wrapper (thread-safe
// reference-counted interface) or plain owned data; the one raw-pointer-shaped field
// (`session.fence_event: HANDLE`) is exactly what this manual impl exists to assert `Send`
// for.
#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "HANDLE wraps a raw pointer with no auto Send impl; this manual impl is \
    exactly the intended assertion for it, not an oversight — mirrors D3d12VideoDecoder's \
    identical allow"
)]
unsafe impl Send for D3d12VideoDecoderAv1 {}

impl D3d12VideoDecoderAv1 {
    /// Open a D3D12 native AV1 decode session for `config`. Mirrors
    /// [`super::D3d12VideoDecoder::open`]'s doc (D3D12 decoder/heap/DPB are created lazily
    /// once the first sequence header is parsed, see [`Self::ensure_session_ready`]).
    ///
    /// # Errors
    ///
    /// [`DecodeError::Unsupported`] for non-AV1 codecs or a non-NV12 pixel format.
    /// [`DecodeError::InvalidInput`] when `config.gpu_device` is not
    /// `Some(GpuDeviceHandle::DirectX12(_))`.
    pub(crate) fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        if config.codec != CodecKind::Av1 {
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
            active_seq: None,
            output: config.output,
            pending: VecDeque::new(),
            flushed: false,
            status_report_counter: 0,
        })
    }

    /// Create D3D12 decoder/heap/DPB/command objects on first use of `seq` — a no-op once
    /// a session already exists. Mirrors
    /// [`super::hevc_decoder::D3d12VideoDecoderHevc::ensure_session_ready`] (same real
    /// hardware findings this session's readback/DPB sizing carries forward: row-pitch-
    /// aligned readback buffer, NV12 two-plane texture array).
    fn ensure_session_ready(&mut self, seq: &SequenceHeader) -> Result<(), DecodeError> {
        if self.session.is_some() {
            return Ok(());
        }
        let width = seq.max_frame_width;
        let mut height = seq.max_frame_height;

        let support = super::av1::check_support(&self.video_device, width, height)?;
        if support
            .ConfigurationFlags
            .contains(D3D12_VIDEO_DECODE_CONFIGURATION_FLAG_HEIGHT_ALIGNMENT_MULTIPLE_32_REQUIRED)
        {
            height = util::align_up_u32(height, 32);
        }

        // No picture is ever held as a reference under this module's scope (§ Decision's
        // "no av1_refs.rs" note) — there is no `sps_max_dec_pic_buffering`-equivalent
        // signaled value to size against (ADR-0005 Open Question #5), so this uses a fixed
        // small constant: enough slots to absorb ordinary Zero-Copy-handle/output latency
        // plus the current picture's own slot, not validated against any real stream.
        let max_dpb_slots = CALLER_HEADROOM + 1;
        let (decoder, decoder_heap) =
            super::av1::create_decoder(&self.video_device, width, height, max_dpb_slots)?;

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

        self.session = Some(SessionAv1 {
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

    /// Submit one compressed packet (length-prefixed OBUs — AV1 spec §5.2/§5.3, **not**
    /// Annex-B framed, see [`super::av1_obu`]'s module doc). May decode zero or more
    /// pictures, queued for [`Self::poll_frame`].
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] when an OBU is malformed, uses an unsupported feature (see
    /// `av1_sequence_header`/`av1_frame_header`'s per-function docs), is a standalone
    /// `OBU_FRAME_HEADER`/`OBU_TILE_GROUP`/`OBU_REDUNDANT_FRAME_HEADER` (see module doc),
    /// or a D3D12 call fails.
    pub(crate) fn push_packet(
        &mut self,
        packet: &mediaway_common::Packet,
    ) -> Result<(), DecodeError> {
        if self.flushed {
            return Err(DecodeError::Closed);
        }
        let obus = split_obus(&packet.payload)?;
        for obu in obus {
            match obu.obu_type {
                ObuType::SequenceHeader => {
                    self.active_seq = Some(parse_sequence_header(obu.payload)?);
                }
                ObuType::Frame => {
                    self.decode_frame_obu(obu.payload, packet.pts)?;
                }
                ObuType::TemporalDelimiter | ObuType::Other(_) => {}
                ObuType::FrameHeader | ObuType::TileGroup | ObuType::RedundantFrameHeader => {
                    return Err(DecodeError::Unsupported);
                }
            }
        }
        Ok(())
    }

    /// Decode one `OBU_FRAME` payload (`frame_header_obu()` + `byte_alignment()` +
    /// `tile_group_obu()`, AV1 spec §5.10) — the sole per-picture decode entry point in
    /// this module's scope (see module doc).
    fn decode_frame_obu(&mut self, payload: &[u8], pts: i64) -> Result<(), DecodeError> {
        let seq = self.active_seq.ok_or(DecodeError::InvalidInput)?; // `SequenceHeader` is `Copy` — no clone needed
        self.ensure_session_ready(&seq)?;

        let (fh, bits_consumed) = av1_frame_header::parse_frame_header(payload, &seq)?;
        // frame_obu()'s own byte_alignment() between the frame header and the tile group
        // (AV1 spec §5.10.1) — the tile bytes start at the next byte boundary after the
        // frame header's own bits.
        let header_bytes = bits_consumed.div_ceil(8);
        let tile_bytes = payload
            .get(header_bytes..)
            .ok_or(DecodeError::InvalidInput)?;
        if tile_bytes.is_empty() {
            return Err(DecodeError::InvalidInput);
        }

        let Some(session) = self.session.as_mut() else {
            return Err(DecodeError::Backend);
        };

        // No eviction pass needed — no picture is ever held as a reference under this
        // module's scope (`SlotTable::mark_reference` is never called below), so
        // `references()` is always empty and every DPB slot is free once released.
        let output_slot = session.dpb.table_mut().acquire_free_slot()?;
        self.status_report_counter = self.status_report_counter.wrapping_add(1);
        let mut pic_params =
            av1_pic_params::build_pic_params(&seq, &fh, output_slot, self.status_report_counter);
        let mut tile = av1_pic_params::build_tile(0, u32::try_from(tile_bytes.len()).unwrap_or(0));

        session.decode_frame(tile_bytes, &mut pic_params, &mut tile, output_slot)?;

        // No `dpb.table_mut().mark_reference(...)` call — this module's scope never
        // references any decoded picture (ADR-0005 § Context finding #4).

        // This module does not support `frame_size_override_flag`-driven per-frame
        // resolution changes in the *reported* frame dimensions (same documented gap as
        // H.264/HEVC's own "no mid-stream SPS/size change" limitation) — every reported
        // `DecodedFrameAv1` uses the session's fixed (sequence-header `max_frame_*`)
        // canvas size, matching what `readback_dpb_slot_to_cpu`/the DPB texture array are
        // actually sized for. `pic_params.width`/`height` above still carry the real
        // per-frame `fh.width`/`fh.height` the driver needs.
        let width = session.width;
        let height = session.height;
        let output = match self.output {
            VideoOutputPreference::CpuFramesOk => {
                let data = session.readback_dpb_slot_to_cpu(output_slot)?;
                session.dpb.table_mut().release_if_unused(output_slot);
                DecodedOutputAv1::Cpu { data }
            }
            VideoOutputPreference::ZeroCopyGpu => {
                session.dpb.table_mut().mark_handle_outstanding(output_slot);
                let raw = windows::core::Interface::as_raw(session.dpb.texture()) as usize;
                let resource = NativeHandle::new(raw).ok_or(DecodeError::Backend)?;
                DecodedOutputAv1::Gpu {
                    resource,
                    subresource: output_slot,
                }
            }
        };
        self.pending.push_back(DecodedFrameAv1 {
            pts,
            width,
            height,
            output,
        });
        Ok(())
    }

    /// Release a previously-handed-out Zero-Copy [`DecodedOutputAv1::Gpu`] handle for
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
    pub(crate) fn poll_frame(&mut self) -> Result<Option<DecodedFrameAv1>, DecodeError> {
        Ok(self.pending.pop_front())
    }

    /// Signal end-of-input. `KEY_FRAME`-only decode/output order is always trivially
    /// decode order == display order (every picture is independent) — nothing buffered to
    /// drain beyond what [`Self::poll_frame`] would already return.
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
