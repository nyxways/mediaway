//! `open`-time feature query + decoder/heap creation for AV1 — mirrors [`super::hevc`]
//! field-for-field, per ADR-0005 § File layout plan (~40 lines, no new logic): calls
//! [`super::setup`]'s codec-generic helpers with `D3D12_VIDEO_DECODE_PROFILE_AV1_PROFILE0`
//! / `DXGI_FORMAT_NV12` in place of HEVC's profile GUID.
//!
//! `_PROFILE0` (not `_PROFILE1`/`_PROFILE2`/`_12BIT_PROFILE2`/`_12BIT_PROFILE2_420`) is
//! Main profile, 8/10-bit 4:2:0 — this module's scope further restricts to 8-bit only
//! (ADR-0005 § Scope decision, enforced by [`super::av1_sequence_header::parse_sequence_header`]).
//! Confirmed present in the vendored `windows-0.62.2` source this implementation pass
//! (`Win32/Media/MediaFoundation/mod.rs`), matching ADR-0005's own citation.

use crate::DecodeError;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
use windows::Win32::Media::MediaFoundation::{
    D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT, D3D12_VIDEO_DECODE_PROFILE_AV1_PROFILE0,
    ID3D12VideoDecoder, ID3D12VideoDecoderHeap, ID3D12VideoDevice,
};

use super::setup;

/// Query D3D12 AV1 Main-profile decode support at `width`x`height` (NV12, 8-bit 4:2:0 —
/// this module's scope, ADR-0005 § Scope decision).
///
/// # Errors
///
/// [`DecodeError::Unsupported`] when the device/driver does not support D3D12 AV1 Main
/// video decode at this resolution.
pub(super) fn check_support(
    video_device: &ID3D12VideoDevice,
    width: u32,
    height: u32,
) -> Result<D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT, DecodeError> {
    setup::check_decode_support(
        video_device,
        D3D12_VIDEO_DECODE_PROFILE_AV1_PROFILE0,
        width,
        height,
        DXGI_FORMAT_NV12,
    )
}

/// Create the `ID3D12VideoDecoder`/`ID3D12VideoDecoderHeap` pair for AV1 Main profile at
/// `width`x`height`, sized for `max_dpb_slots` decode-picture-buffer entries.
pub(super) fn create_decoder(
    video_device: &ID3D12VideoDevice,
    width: u32,
    height: u32,
    max_dpb_slots: u32,
) -> Result<(ID3D12VideoDecoder, ID3D12VideoDecoderHeap), DecodeError> {
    setup::create_decoder(
        video_device,
        D3D12_VIDEO_DECODE_PROFILE_AV1_PROFILE0,
        width,
        height,
        DXGI_FORMAT_NV12,
        max_dpb_slots,
    )
}
