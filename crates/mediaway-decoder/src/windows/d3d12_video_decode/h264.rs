//! `open`-time feature query + decoder/heap creation for H.264 — the codec-specific
//! sliver on top of [`super::setup`]'s generic helpers (mirrors
//! `mediaway-encoder-windows`'s `d3d12_video_encode/{setup,hevc,av1}.rs` split, where
//! `setup.rs` carried H.264 directly since encode staged H.264 first; here `setup.rs`
//! is generic from the start since ADR-0002 already names HEVC/AV1 as this file's
//! future siblings).

use crate::DecodeError;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
use windows::Win32::Media::MediaFoundation::{
    D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT, D3D12_VIDEO_DECODE_PROFILE_H264, ID3D12VideoDecoder,
    ID3D12VideoDecoderHeap, ID3D12VideoDevice,
};

use super::setup;

/// Query D3D12 H.264 decode support at `width`x`height` (NV12).
///
/// # Errors
///
/// [`DecodeError::Unsupported`] when the device/driver does not support D3D12 H.264
/// video decode at this resolution.
pub(super) fn check_support(
    video_device: &ID3D12VideoDevice,
    width: u32,
    height: u32,
) -> Result<D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT, DecodeError> {
    setup::check_decode_support(
        video_device,
        D3D12_VIDEO_DECODE_PROFILE_H264,
        width,
        height,
        DXGI_FORMAT_NV12,
    )
}

/// Create the `ID3D12VideoDecoder`/`ID3D12VideoDecoderHeap` pair for H.264 at
/// `width`x`height`, sized for `max_dpb_slots` decode-picture-buffer entries.
pub(super) fn create_decoder(
    video_device: &ID3D12VideoDevice,
    width: u32,
    height: u32,
    max_dpb_slots: u32,
) -> Result<(ID3D12VideoDecoder, ID3D12VideoDecoderHeap), DecodeError> {
    setup::create_decoder(
        video_device,
        D3D12_VIDEO_DECODE_PROFILE_H264,
        width,
        height,
        DXGI_FORMAT_NV12,
        max_dpb_slots,
    )
}
