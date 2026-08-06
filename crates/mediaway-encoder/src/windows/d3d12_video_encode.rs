//! D3D12 native video-encode backend (`ID3D12VideoDevice3`/`ID3D12VideoEncoder`) — H.264,
//! HEVC, and AV1, CPU-upload NV12 input, all-intra (every pushed frame is an independent
//! key frame).
//!
//! This is a **different** path than [`crate::windows::D3d12SharedEncodeBridge`]: that bridge feeds
//! a D3D12 texture into Media Foundation's HW MFT via a shared D3D12→D3D11 heap
//! (`GpuCopy`). This module instead drives the **native D3D12 video-encode API** end to
//! end — no WMF, no `IMFTransform` — using `CreateVideoEncoder`/`CreateVideoEncoderHeap`
//! and `ID3D12VideoEncodeCommandList2::EncodeFrame`. Windows 11+, real hardware
//! (NVENC/QSV/AMF exposed through this API) required; see
//! [ADR-0007](../adr/0007-d3d12-native-video-encode.md) and its HEVC addendum.
//!
//! **Scope this stage** (mirrors `mediaway-encoder-linux`'s VA-API staging: CPU-upload
//! before Zero-Copy):
//! - H.264 Main / HEVC Main / AV1 Main profile, CPU-upload NV12 input (one CPU→GPU upload
//!   copy per frame).
//! - Every pushed frame is an **independent IDR** — no GOP, no P/B-frames, no reference
//!   picture management (`GOPLength = 1`, zero reference frames in the picture control).
//! - Fixed CQP rate control (`bitrate_bps` is not honored yet — rate control tuning is
//!   deferred, see the ADR).
//! - This module hand-writes its own Annex-B parameter sets — H.264 SPS/PPS ([`bitstream`])
//!   or HEVC VPS/SPS/PPS ([`bitstream_hevc`]) — and prepends them to every packet; the
//!   D3D12 API only ever emits the slice NAL, not parameter sets. AV1 ([`bitstream_av1`])
//!   hand-writes OBUs (temporal delimiter + sequence header + frame header) the same way,
//!   but wraps the driver's per-frame compressed tile bytes in an `OBU_FRAME` with a
//!   per-frame `leb128` size field — see [`ops_av1`].
//! - Zero-Copy GPU input and reference-frame/GOP support remain deferred for all three
//!   codecs.
//!
//! Split across sibling files to stay under the 1000-line source limit: [`setup`]/[`hevc`]/
//! [`av1`] (`open`-time D3D12 object creation per codec), [`ops`]/[`ops_hevc`]/[`ops_av1`]
//! (per-frame recording — further `impl` blocks for [`D3d12VideoEncoder`]),
//! [`bitstream`]/[`bitstream_hevc`]/[`bitstream_av1`] (hand-written parameter sets/OBUs),
//! [`util`] (small shared recording helpers used by all codecs).
//!
//! **Not wired into [`crate::windows::WindowsVideoEncoder`] / `auto` yet** — this module is
//! self-contained and unregistered; a later integration pass adds
//! `pub mod d3d12_video_encode;` to `src/lib.rs`.

#![allow(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "pub(crate) intentionally survives the mod d3d12_video_encode; wrapper going pub once a later pass wires this backend in (see module doc)"
)]
#![allow(
    dead_code,
    reason = "every open()/push_frame() call path here is only reachable from this module's own #[cfg(test)] tests today (not wired into crate::windows::WindowsVideoEncoder yet, see module doc) — rustc's dead_code pass sees a live call graph under cfg(test) but flags the same items in the plain (non-test) lib build; same root cause as the unreachable_pub allow above, resolved together once a later pass wires this backend into the public API"
)]

use std::collections::VecDeque;
use std::mem::size_of;

use crate::{EncodeError, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{
    Bytes, CodecKind, GpuDeviceHandle, Packet, PixelFormat, StreamInfo, VideoFrame,
    VideoFrameStorage, VideoGeometry,
};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_LIST_TYPE_COPY, D3D12_COMMAND_LIST_TYPE_VIDEO_ENCODE, D3D12_FENCE_FLAG_NONE,
    D3D12_HEAP_TYPE_DEFAULT, D3D12_HEAP_TYPE_READBACK, D3D12_HEAP_TYPE_UPLOAD,
    D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_GENERIC_READ,
    D3D12_TEXTURE_DATA_PITCH_ALIGNMENT, ID3D12CommandAllocator, ID3D12CommandQueue, ID3D12Device4,
    ID3D12Fence, ID3D12GraphicsCommandList, ID3D12Resource,
};
use windows::Win32::Media::MediaFoundation::{
    D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_SUBREGIONS_LAYOUT_DATA_TILES,
    D3D12_VIDEO_ENCODER_AV1_POST_ENCODE_VALUES, D3D12_VIDEO_ENCODER_AV1_SEQUENCE_STRUCTURE,
    D3D12_VIDEO_ENCODER_AV1_TIER_HIGH, D3D12_VIDEO_ENCODER_CODEC_H264,
    D3D12_VIDEO_ENCODER_FRAME_SUBREGION_METADATA, D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE,
    D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_ROW_BASED, D3D12_VIDEO_ENCODER_OUTPUT_METADATA,
    D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC, D3D12_VIDEO_ENCODER_RATE_CONTROL_CQP,
    D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264,
    D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_HEVC, D3D12_VIDEO_ENCODER_TIER_HEVC_HIGH,
    ID3D12VideoDevice3, ID3D12VideoEncodeCommandList2, ID3D12VideoEncoder, ID3D12VideoEncoderHeap,
};
use windows::Win32::System::Threading::CreateEventW;
use windows::core::Interface;

mod av1;
mod bitstream;
mod bitstream_av1;
mod bitstream_hevc;
mod gop;
mod gop_hevc;
mod hevc;
mod ops;
mod ops_av1;
mod ops_hevc;
mod setup;
mod util;

#[cfg(test)]
#[path = "d3d12_video_encode_tests.rs"]
mod tests;

/// Fixed intra QP (all-intra CQP) — rate control tuning is deferred, see ADR-0007.
const FIXED_QP: u32 = 26;
/// Fixed intra `base_q_idx` (all-intra CQP) for AV1 — a **separate** constant from
/// [`FIXED_QP`]: AV1's QP range is `0..=255`, unlike H.264/HEVC's `0..=51`, so the two
/// scales are not interchangeable (reusing [`FIXED_QP`] here would request a
/// near-lossless AV1 quality, not the intended mid-range CQP).
const FIXED_QP_AV1: u8 = 128;
/// Extra headroom above the raw NV12 frame size for the worst-case compressed bitstream.
const BITSTREAM_SAFETY_MARGIN: u64 = 65_536;

/// Per-codec GOP-structure state, carrying exactly the codec-specific persistent config
/// this backend needs at encode time. An enum rather than one `Option<T>` field per codec:
/// the active variant always matches `VideoEncoderConfig::codec` from `open`, so an enum
/// makes the "exactly one codec is active" invariant a type-level fact instead of a
/// runtime-checked pair of `Option`s.
enum GopStructure {
    H264(D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264),
    Hevc(D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_HEVC),
    Av1(D3D12_VIDEO_ENCODER_AV1_SEQUENCE_STRUCTURE),
}

/// D3D12 native video-encode session (H.264 or HEVC, CPU-upload NV12, all-intra).
pub(crate) struct D3d12VideoEncoder {
    encoder: ID3D12VideoEncoder,
    encoder_heap: ID3D12VideoEncoderHeap,

    copy_queue: ID3D12CommandQueue,
    copy_allocator: ID3D12CommandAllocator,
    copy_list: ID3D12GraphicsCommandList,

    encode_queue: ID3D12CommandQueue,
    encode_allocator: ID3D12CommandAllocator,
    encode_list: ID3D12VideoEncodeCommandList2,

    fence: ID3D12Fence,
    fence_event: HANDLE,
    fence_value: u64,

    input_texture: ID3D12Resource,
    upload_buffer: ID3D12Resource,
    metadata_buffer: ID3D12Resource,
    resolved_metadata_buffer: ID3D12Resource,
    bitstream_buffer: ID3D12Resource,
    bitstream_capacity: u64,

    width: u32,
    height: u32,
    row_pitch: u32,
    luma_size: u64,

    gop: GopStructure,
    rate_control: setup::RateControlState,
    fps_num: u32,
    fps_den: u32,

    header_bytes: Vec<u8>,
    header_len_aligned: u64,
    /// AV1-only: the fixed, byte-aligned `uncompressed_header()` bytes appended after
    /// [`Self::header_bytes`] (temporal delimiter + sequence header) and before the
    /// driver's compressed tile bytes in every packet — see [`ops_av1::D3d12VideoEncoder::read_packet_av1`].
    /// Empty for H.264/HEVC.
    av1_frame_header_bytes: Vec<u8>,

    info: StreamInfo,
    pending: VecDeque<Packet>,
    flushed: bool,
    frame_counter: u32,

    /// H.264 GOP/intra-refresh mode only — `None` for HEVC/AV1 and for H.264
    /// IDR-only (`gop_size <= 1`, no `intra_refresh_period`, or driver
    /// capability fallback — see `gop.rs` and ADR-0007's 2026-08-06
    /// addendum). Frame-decision state and the two-slot reconstructed-picture
    /// pool always go together: one exists iff the other does.
    h264_gop_state: Option<gop::H264GopState>,
    hevc_gop_state: Option<gop_hevc::HevcGopState>,
    /// Two-slot reconstructed-picture pool, shared by whichever codec is active
    /// (only one ever is per session, see `GopStructure`) — `ReconPool` itself
    /// has no codec-specific fields.
    recon_pool: Option<setup::ReconPool>,
    /// Row-based intra-refresh wave period in frames, once the driver has
    /// actually accepted it at `open` (see `check_encoder_support`'s
    /// `IntraRefresh` parameter) — `None` whenever `h264_gop_state`/
    /// `hevc_gop_state` are in periodic-GOP or IDR-only mode instead. Read
    /// every frame in `ops`/`ops_hevc` to fill
    /// `D3D12_VIDEO_ENCODER_INTRA_REFRESH::IntraRefreshDuration`.
    intra_refresh_period: Option<u32>,
    /// Monotonic per-frame counter for `D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_H264::FrameDecodingOrderNumber`.
    /// Only meaningfully consumed in H.264 GOP mode; harmless (unread) otherwise.
    frame_decoding_order: u32,
    /// `(PictureOrderCountNumber, FrameDecodingOrderNumber)` of the most recently
    /// encoded H.264 GOP-mode frame — the next P frame's single reference
    /// descriptor. `None` before the first frame or when GOP mode is off.
    last_h264_reference: Option<(u32, u32)>,
    /// `PictureOrderCountNumber` of the most recently encoded HEVC GOP-mode
    /// frame — HEVC's reference descriptor has no `FrameDecodingOrderNumber`
    /// field, so this is simpler than its H.264 counterpart. `None` before the
    /// first frame or when GOP mode is off.
    last_hevc_reference: Option<u32>,
}

// SAFETY: all fields are `windows`-crate COM wrappers (thread-safe reference-counted
// interfaces) or plain POD/owned data; the type has no interior aliasing across threads
// beyond what COM itself guarantees.
unsafe impl Send for D3d12VideoEncoder {}

impl D3d12VideoEncoder {
    /// Open a D3D12 native video-encode session for `config`.
    ///
    /// # Errors
    ///
    /// - [`EncodeError::InvalidInput`] — missing/wrong `gpu_device`, zero or non-16-aligned
    ///   dimensions.
    /// - [`EncodeError::Unsupported`] — codec/pixel-format/input path not H.264 +
    ///   CPU-upload NV12, or the device does not support D3D12 H.264 video encode
    ///   (`D3D12_FEATURE_VIDEO_ENCODER_CODEC`).
    /// - [`EncodeError::Backend`] — D3D12 device/resource/encoder creation failure.
    #[allow(
        clippy::too_many_lines,
        reason = "linear session-construction sequence (device -> feature checks -> encoder/heap -> command objects -> buffers); splitting further fragments one straight-line setup path"
    )]
    pub(crate) fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        validate_common(config)?;
        let Some(GpuDeviceHandle::DirectX12(handle)) = config.gpu_device else {
            return Err(EncodeError::InvalidInput);
        };

        let device = setup::device_from_handle(handle)?;
        let video_device: ID3D12VideoDevice3 =
            device.cast().map_err(|_| EncodeError::Unsupported)?;
        let device4: ID3D12Device4 = device.cast().map_err(|_| EncodeError::Unsupported)?;

        let resolution = D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC {
            Width: config.width,
            Height: config.height,
        };
        let (fps_num, fps_den) = util::frame_rate(config.time_base.num, config.time_base.den);
        let rc_cqp = D3D12_VIDEO_ENCODER_RATE_CONTROL_CQP {
            ConstantQP_FullIntracodedFrame: FIXED_QP,
            ConstantQP_InterPredictedFrame_PrevRefOnly: FIXED_QP,
            ConstantQP_InterPredictedFrame_BiDirectionalRef: FIXED_QP,
        };

        let rate_control = setup::RateControlState::Cqp(rc_cqp);

        let (
            encoder,
            encoder_heap,
            gop,
            header_bytes,
            req,
            rate_control,
            av1_frame_header_bytes,
            intra_refresh_period,
        ) = match config.codec {
            CodecKind::H264 => {
                setup::check_codec_support(&video_device, D3D12_VIDEO_ENCODER_CODEC_H264)?;
                setup::check_output_resolution(
                    &video_device,
                    D3D12_VIDEO_ENCODER_CODEC_H264,
                    resolution,
                )?;
                let req = setup::check_resource_requirements(&video_device, resolution)?;

                let idr_only_gop = D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264 {
                    GOPLength: 1,
                    PPicturePeriod: 0,
                    pic_order_cnt_type: 2,
                    log2_max_frame_num_minus4: 0,
                    log2_max_pic_order_cnt_lsb_minus4: 0,
                };
                // Tiered capability-gated fallback: intra-refresh (if requested) ->
                // periodic GOP (if requested) -> IDR-only. Each tier falls back to
                // the next silently (no error) when the driver can't honor it, per
                // ADR-0007's 2026-08-06 addendum.
                let requested_gop_size = config.gop_size.max(1);
                let (gop_h264, level, effective_intra_refresh_period) = 'select: {
                    if let Some(period) = config.intra_refresh_period {
                        // Row-based intra refresh requires an unbounded GOP
                        // (GOPLength = 0) — see ADR-0007's addendum.
                        let intra_refresh_gop = D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264 {
                            GOPLength: 0,
                            PPicturePeriod: 1,
                            pic_order_cnt_type: 2,
                            log2_max_frame_num_minus4: 4,
                            log2_max_pic_order_cnt_lsb_minus4: 0,
                        };
                        // `GENERAL_SUPPORT_OK` passing is not enough on its own —
                        // real hardware separately caps the usable duration via
                        // `MaxIntraRefreshFrameDuration` (0 = unusable at this
                        // resolution even though the mode itself checks out), only
                        // enforced by the real `EncodeFrame` call, not this advisory
                        // query's pass/fail. See `setup::check_encoder_support`'s doc.
                        if let Ok((level, max_intra_refresh_duration)) =
                            setup::check_encoder_support(
                                &video_device,
                                resolution,
                                intra_refresh_gop,
                                &rate_control,
                                (fps_num, fps_den),
                                1,
                                D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_ROW_BASED,
                            )
                            && period <= max_intra_refresh_duration
                            && max_intra_refresh_duration > 0
                        {
                            break 'select (intra_refresh_gop, level, Some(period));
                        }
                    }
                    if requested_gop_size > 1 {
                        let p_frame_gop = D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264 {
                            GOPLength: requested_gop_size,
                            PPicturePeriod: 1,
                            pic_order_cnt_type: 2,
                            log2_max_frame_num_minus4: 4,
                            log2_max_pic_order_cnt_lsb_minus4: 0,
                        };
                        if let Ok((level, _)) = setup::check_encoder_support(
                            &video_device,
                            resolution,
                            p_frame_gop,
                            &rate_control,
                            (fps_num, fps_den),
                            1,
                            D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE,
                        ) {
                            break 'select (p_frame_gop, level, None);
                        }
                    }
                    let (level, _) = setup::check_encoder_support(
                        &video_device,
                        resolution,
                        idr_only_gop,
                        &rate_control,
                        (fps_num, fps_den),
                        0,
                        D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE,
                    )?;
                    (idr_only_gop, level, None)
                };
                let level_idc = setup::level_h264_to_idc(level);
                let (encoder, encoder_heap) =
                    setup::create_encoder(&video_device, resolution, level)?;

                // Real, capability-gated CBR: one extra probe at the already-chosen
                // GOP/intra-refresh tier — falls back to CQP silently (no error) if this
                // driver won't accept CBR for that exact configuration, per
                // `caveats-and-clarity.md`. `max_reference_frames_in_dpb`/`intra_refresh`
                // mirror exactly what the winning tier above already used.
                let rate_control = config.rate_control.map_or(rate_control, |rc| {
                    let cbr_state = setup::RateControlState::Cbr(setup::cbr_from_config(rc));
                    let max_ref = u32::from(gop_h264.GOPLength != 1);
                    let intra_refresh_mode = if effective_intra_refresh_period.is_some() {
                        D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_ROW_BASED
                    } else {
                        D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE
                    };
                    if setup::check_encoder_support(
                        &video_device,
                        resolution,
                        gop_h264,
                        &cbr_state,
                        (fps_num, fps_den),
                        max_ref,
                        intra_refresh_mode,
                    )
                    .is_ok()
                    {
                        cbr_state
                    } else {
                        rate_control
                    }
                });

                let width_mbs_minus1 = config.width / 16 - 1;
                let height_map_units_minus1 = config.height / 16 - 1;
                let header_bytes = bitstream::build_h264_headers(
                    width_mbs_minus1,
                    height_map_units_minus1,
                    level_idc,
                );
                (
                    encoder,
                    encoder_heap,
                    GopStructure::H264(gop_h264),
                    header_bytes,
                    req,
                    rate_control,
                    Vec::new(),
                    effective_intra_refresh_period,
                )
            }
            CodecKind::Hevc => {
                hevc::check_codec_support(&video_device)?;
                hevc::check_output_resolution(&video_device, resolution)?;
                let req = hevc::check_resource_requirements(&video_device, resolution)?;
                if config.width % hevc::MIN_CB_SIZE_PIXELS != 0
                    || config.height % hevc::MIN_CB_SIZE_PIXELS != 0
                {
                    return Err(EncodeError::InvalidInput);
                }

                let idr_only_gop_hevc = D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_HEVC {
                    GOPLength: 1,
                    PPicturePeriod: 0,
                    log2_max_pic_order_cnt_lsb_minus4: 0,
                };
                // Same tiered capability-gated fallback as H.264 above — see
                // ADR-0007's 2026-08-06 addendum.
                let requested_gop_size_hevc = config.gop_size.max(1);
                let (gop_hevc, level, effective_intra_refresh_period) = 'select: {
                    if let Some(period) = config.intra_refresh_period {
                        let intra_refresh_gop_hevc =
                            D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_HEVC {
                                GOPLength: 0,
                                PPicturePeriod: 1,
                                log2_max_pic_order_cnt_lsb_minus4: 4,
                            };
                        // See the H.264 branch above's sibling comment:
                        // `MaxIntraRefreshFrameDuration` is the real, only-enforced-
                        // at-`EncodeFrame`-time constraint.
                        if let Ok((level, max_intra_refresh_duration)) = hevc::check_encoder_support(
                            &video_device,
                            resolution,
                            intra_refresh_gop_hevc,
                            &rate_control,
                            (fps_num, fps_den),
                            1,
                            D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_ROW_BASED,
                        ) && period <= max_intra_refresh_duration
                            && max_intra_refresh_duration > 0
                        {
                            break 'select (intra_refresh_gop_hevc, level, Some(period));
                        }
                    }
                    if requested_gop_size_hevc > 1 {
                        let p_frame_gop_hevc = D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_HEVC {
                            GOPLength: requested_gop_size_hevc,
                            PPicturePeriod: 1,
                            log2_max_pic_order_cnt_lsb_minus4: 4,
                        };
                        if let Ok((level, _)) = hevc::check_encoder_support(
                            &video_device,
                            resolution,
                            p_frame_gop_hevc,
                            &rate_control,
                            (fps_num, fps_den),
                            1,
                            D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE,
                        ) {
                            break 'select (p_frame_gop_hevc, level, None);
                        }
                    }
                    let (level, _) = hevc::check_encoder_support(
                        &video_device,
                        resolution,
                        idr_only_gop_hevc,
                        &rate_control,
                        (fps_num, fps_den),
                        0,
                        D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE,
                    )?;
                    (idr_only_gop_hevc, level, None)
                };
                let general_level_idc = hevc::level_hevc_to_general_level_idc(level.Level);
                let general_tier_flag = u8::from(level.Tier == D3D12_VIDEO_ENCODER_TIER_HEVC_HIGH);
                let (encoder, encoder_heap) =
                    hevc::create_encoder(&video_device, resolution, level)?;

                // See the H.264 branch above's sibling comment — same one-extra-probe CBR
                // fallback design, `gop_hevc`/`hevc::check_encoder_support` in place of
                // `gop_h264`/`setup::check_encoder_support`.
                let rate_control = config.rate_control.map_or(rate_control, |rc| {
                    let cbr_state = setup::RateControlState::Cbr(setup::cbr_from_config(rc));
                    let max_ref = u32::from(gop_hevc.GOPLength != 1);
                    let intra_refresh_mode = if effective_intra_refresh_period.is_some() {
                        D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_ROW_BASED
                    } else {
                        D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE
                    };
                    if hevc::check_encoder_support(
                        &video_device,
                        resolution,
                        gop_hevc,
                        &cbr_state,
                        (fps_num, fps_den),
                        max_ref,
                        intra_refresh_mode,
                    )
                    .is_ok()
                    {
                        cbr_state
                    } else {
                        rate_control
                    }
                });

                let header_bytes = bitstream_hevc::build_hevc_headers(
                    config.width,
                    config.height,
                    general_tier_flag,
                    general_level_idc,
                );
                (
                    encoder,
                    encoder_heap,
                    GopStructure::Hevc(gop_hevc),
                    header_bytes,
                    req,
                    rate_control,
                    Vec::new(),
                    effective_intra_refresh_period,
                )
            }
            CodecKind::Av1 => {
                av1::check_codec_support(&video_device)?;
                av1::check_output_resolution(&video_device, resolution)?;
                let req = av1::check_resource_requirements(&video_device, resolution)?;

                let rc_cqp_av1 = D3D12_VIDEO_ENCODER_RATE_CONTROL_CQP {
                    ConstantQP_FullIntracodedFrame: u32::from(FIXED_QP_AV1),
                    ConstantQP_InterPredictedFrame_PrevRefOnly: u32::from(FIXED_QP_AV1),
                    ConstantQP_InterPredictedFrame_BiDirectionalRef: u32::from(FIXED_QP_AV1),
                };
                let gop_av1 = D3D12_VIDEO_ENCODER_AV1_SEQUENCE_STRUCTURE {
                    IntraDistance: 1,
                    InterFramePeriod: 0,
                };
                let level = av1::check_encoder_support(
                    &video_device,
                    resolution,
                    gop_av1,
                    rc_cqp_av1,
                    (fps_num, fps_den),
                )?;
                // `D3D12_VIDEO_ENCODER_AV1_LEVELS`' ordinal values already equal the AV1
                // spec's seq_level_idx table (Annex A) — see bitstream_av1::build_av1_session_prefix.
                let seq_level_idx = u8::try_from(level.Level.0).unwrap_or(0);
                let seq_tier = u8::from(level.Tier == D3D12_VIDEO_ENCODER_AV1_TIER_HIGH);
                let (encoder, encoder_heap) =
                    av1::create_encoder(&video_device, resolution, level)?;

                let header_bytes = bitstream_av1::build_av1_session_prefix(
                    config.width,
                    config.height,
                    seq_level_idx,
                    seq_tier,
                );
                let av1_frame_header_bytes = bitstream_av1::build_av1_frame_header_bytes(
                    FIXED_QP_AV1,
                    config.width,
                    config.height,
                );
                (
                    encoder,
                    encoder_heap,
                    GopStructure::Av1(gop_av1),
                    header_bytes,
                    req,
                    setup::RateControlState::Cqp(rc_cqp_av1),
                    av1_frame_header_bytes,
                    None,
                )
            }
            _ => return Err(EncodeError::Unsupported),
        };

        let (copy_queue, copy_allocator, copy_list) =
            setup::create_command_objects::<ID3D12GraphicsCommandList>(
                &device,
                &device4,
                D3D12_COMMAND_LIST_TYPE_COPY,
            )?;
        let (encode_queue, encode_allocator, encode_list) =
            setup::create_command_objects::<ID3D12VideoEncodeCommandList2>(
                &device,
                &device4,
                D3D12_COMMAND_LIST_TYPE_VIDEO_ENCODE,
            )?;

        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }
            .map_err(|_| EncodeError::Backend)?;
        // SAFETY: manual-reset=false, initial-state=false, no name; standard CPU wait event.
        let fence_event =
            unsafe { CreateEventW(None, false, false, None) }.map_err(|_| EncodeError::Backend)?;

        let input_texture = setup::create_nv12_texture(
            &device,
            config.width,
            config.height,
            D3D12_RESOURCE_FLAG_NONE,
        )?;

        let row_pitch = util::align_up_u32(config.width, D3D12_TEXTURE_DATA_PITCH_ALIGNMENT);
        let luma_size = u64::from(row_pitch) * u64::from(config.height);
        let upload_size = luma_size + luma_size / 2;
        let upload_buffer = setup::create_linear_buffer(
            &device,
            D3D12_HEAP_TYPE_UPLOAD,
            upload_size,
            D3D12_RESOURCE_STATE_GENERIC_READ,
        )?;

        let metadata_buffer = setup::create_linear_buffer(
            &device,
            D3D12_HEAP_TYPE_DEFAULT,
            u64::from(req.MaxEncoderOutputMetadataBufferSize),
            D3D12_RESOURCE_STATE_COMMON,
        )?;
        // AV1's resolved metadata layout is strictly larger than H.264/HEVC's — it appends
        // a D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_SUBREGIONS_LAYOUT_DATA_TILES and a
        // D3D12_VIDEO_ENCODER_AV1_POST_ENCODE_VALUES after the shared
        // OUTPUT_METADATA + subregions prefix (official spec § "Resolved buffer layouts
        // for ResolveEncoderOutputMetadata"). Sizing this buffer for the H.264/HEVC layout
        // on an AV1 session under-allocates a real GPU resource the driver writes into.
        let resolved_metadata_size = (size_of::<D3D12_VIDEO_ENCODER_OUTPUT_METADATA>()
            + size_of::<D3D12_VIDEO_ENCODER_FRAME_SUBREGION_METADATA>()
            + if config.codec == CodecKind::Av1 {
                size_of::<D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_SUBREGIONS_LAYOUT_DATA_TILES>()
                    + size_of::<D3D12_VIDEO_ENCODER_AV1_POST_ENCODE_VALUES>()
            } else {
                0
            }) as u64;
        let resolved_metadata_buffer = setup::create_linear_buffer(
            &device,
            D3D12_HEAP_TYPE_READBACK,
            resolved_metadata_size,
            D3D12_RESOURCE_STATE_COMMON,
        )?;

        let bitstream_align = u64::from(req.CompressedBitstreamBufferAccessAlignment.max(1));
        let raw_capacity =
            u64::from(config.width) * u64::from(config.height) * 3 + BITSTREAM_SAFETY_MARGIN;
        let bitstream_capacity = util::align_up_u64(raw_capacity, bitstream_align);
        let bitstream_buffer = setup::create_linear_buffer(
            &device,
            D3D12_HEAP_TYPE_READBACK,
            bitstream_capacity,
            D3D12_RESOURCE_STATE_COMMON,
        )?;

        let header_len_aligned = util::align_up_u64(header_bytes.len() as u64, bitstream_align);
        util::write_header_once(&bitstream_buffer, &header_bytes)?;

        let effective_gop_size = match &gop {
            GopStructure::H264(g) => g.GOPLength,
            GopStructure::Hevc(g) => g.GOPLength,
            GopStructure::Av1(_) => 1,
        };
        // Recon pool is needed whenever any reference-frame-using mode is active —
        // periodic GOP (`effective_gop_size > 1`) or intra refresh (unbounded GOP,
        // `effective_gop_size == 0`, tracked separately via `intra_refresh_period`).
        let recon_pool = if effective_gop_size > 1 || intra_refresh_period.is_some() {
            Some(setup::create_recon_pool(
                &device,
                config.width,
                config.height,
            )?)
        } else {
            None
        };
        let h264_gop_state = match (&gop, intra_refresh_period) {
            (GopStructure::H264(_), Some(period)) => {
                Some(gop::H264GopState::new_intra_refresh(period))
            }
            (GopStructure::H264(_), None) if effective_gop_size > 1 => {
                Some(gop::H264GopState::new(effective_gop_size))
            }
            _ => None,
        };
        let hevc_gop_state = match (&gop, intra_refresh_period) {
            (GopStructure::Hevc(_), Some(period)) => {
                Some(gop_hevc::HevcGopState::new_intra_refresh(period))
            }
            (GopStructure::Hevc(_), None) if effective_gop_size > 1 => {
                Some(gop_hevc::HevcGopState::new(effective_gop_size))
            }
            _ => None,
        };

        Ok(Self {
            encoder,
            encoder_heap,
            copy_queue,
            copy_allocator,
            copy_list,
            encode_queue,
            encode_allocator,
            encode_list,
            fence,
            fence_event,
            fence_value: 0,
            input_texture,
            upload_buffer,
            metadata_buffer,
            resolved_metadata_buffer,
            bitstream_buffer,
            bitstream_capacity,
            width: config.width,
            height: config.height,
            row_pitch,
            luma_size,
            gop,
            rate_control,
            fps_num,
            fps_den,
            header_bytes,
            header_len_aligned,
            av1_frame_header_bytes,
            info: stream_info_from(config),
            pending: VecDeque::new(),
            flushed: false,
            frame_counter: 0,
            h264_gop_state,
            hevc_gop_state,
            recon_pool,
            frame_decoding_order: 0,
            last_h264_reference: None,
            last_hevc_reference: None,
            intra_refresh_period,
        })
    }
}

impl VideoEncoder for D3d12VideoEncoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        if self.flushed {
            return Err(EncodeError::Closed);
        }
        let VideoFrameStorage::Cpu { data } = &frame.storage else {
            return Err(EncodeError::Unsupported);
        };
        if frame.width != self.width || frame.height != self.height {
            return Err(EncodeError::InvalidInput);
        }
        let nv12_len = util::nv12_size(self.width, self.height)?;
        if data.len() < nv12_len {
            return Err(EncodeError::InvalidInput);
        }

        self.upload_and_copy(data)?;
        let packet = match self.gop {
            GopStructure::H264(gop) => {
                let decision = self.h264_gop_state.as_mut().map(gop::H264GopState::decide);
                self.encode_frame_h264(frame.pts, frame.duration, gop, decision)?
            }
            GopStructure::Hevc(gop) => {
                let decision = self
                    .hevc_gop_state
                    .as_mut()
                    .map(gop_hevc::HevcGopState::decide);
                self.encode_frame_hevc(frame.pts, frame.duration, gop, decision)?
            }
            GopStructure::Av1(gop) => self.encode_frame_av1(frame.pts, frame.duration, gop)?,
        };
        self.pending.push_back(packet);
        Ok(())
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        // Every pushed frame is independently encoded and drained synchronously — no
        // pipeline depth to flush.
        self.flushed = true;
        Ok(())
    }

    /// Retargets `TargetBitRate` in place — real and live: `ops`/`ops_hevc`/`ops_av1` all
    /// rebuild `D3D12_VIDEO_ENCODER_RATE_CONTROL` from `self.rate_control` fresh on every
    /// `EncodeFrame` call (never cached once at `open` time, see
    /// `setup::rate_control_mode_and_params`'s doc), so the very next pushed frame picks up
    /// the new target with no session reopen and no dropped frames. Only meaningful when
    /// `open` actually landed in `RateControlState::Cbr` (real, capability-gated — see
    /// `open`'s doc); a `Cqp` session (no `VideoEncoderConfig::rate_control`, or this
    /// driver rejected CBR for the chosen GOP/intra-refresh tier) has no bitrate ceiling to
    /// retarget.
    fn set_bitrate(&mut self, bitrate_bps: u32) -> Result<(), EncodeError> {
        let setup::RateControlState::Cbr(cbr) = &mut self.rate_control else {
            return Err(EncodeError::Unsupported);
        };
        cbr.TargetBitRate = u64::from(bitrate_bps);
        Ok(())
    }
}

impl Drop for D3d12VideoEncoder {
    fn drop(&mut self) {
        if !self.fence_event.is_invalid() {
            // SAFETY: closing an owned event handle created in `open` via `CreateEventW`.
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.fence_event) };
        }
    }
}

fn validate_common(config: &VideoEncoderConfig) -> Result<(), EncodeError> {
    if !matches!(
        config.codec,
        CodecKind::H264 | CodecKind::Hevc | CodecKind::Av1
    ) {
        return Err(EncodeError::Unsupported);
    }
    if config.pixel_format != PixelFormat::Nv12 {
        return Err(EncodeError::Unsupported);
    }
    if !matches!(config.input, VideoInputPreference::CpuUploadOk) {
        return Err(EncodeError::Unsupported);
    }
    if config.width == 0 || config.height == 0 || config.width % 16 != 0 || config.height % 16 != 0
    {
        return Err(EncodeError::InvalidInput);
    }
    if config.time_base.den == 0 {
        return Err(EncodeError::InvalidInput);
    }
    Ok(())
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &VideoEncoderConfig) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: config.codec,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: config.width,
            height: config.height,
        },
        extra_data: Bytes::new(),
    }
}
