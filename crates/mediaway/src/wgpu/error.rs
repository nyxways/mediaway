//! Errors bridging `wgpu` into Mediaway GPU handles.

#![forbid(unsafe_code)]

use mediaway_decoder::DecodeError;
use mediaway_encoder::EncodeError;
use thiserror::Error;

/// Errors from [`crate::wgpu::WgpuDx12Bridge`] / [`crate::wgpu::WgpuDx12DecodeBridge`] (and future
/// non-Windows bridges).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WgpuInteropError {
    /// No wgpu HAL bridge on this build/platform (Stage 1: Windows DX12 only).
    #[error("wgpu interop unsupported on this platform")]
    Unsupported,
    /// `wgpu::Device::as_hal`/`Texture::as_hal` returned `None` — the resource
    /// is not backed by the expected native GPU API backend (wrong `wgpu`
    /// `Backends` selection, `BrowserWebGpu`, or a custom backend).
    #[error("wgpu resource is not backed by the expected native GPU API backend")]
    HalUnavailable,
    /// Zero width/height, a null native pointer, or `source`'s size/format did
    /// not match the bridge target.
    #[error("invalid wgpu interop input")]
    InvalidInput,
    /// The underlying Mediaway encode-backend bridge failed (open, GPU-copy
    /// wait, or native handle extraction).
    #[error("encode backend bridge failure: {0}")]
    Bridge(#[from] EncodeError),
    /// The underlying Mediaway decode-backend bridge (`D3d11SharedDecodeBridge` on Windows)
    /// failed — open, `CopySubresourceRegion` / query-poll wait, or native handle extraction.
    #[error("decode backend bridge failure: {0}")]
    DecodeBridge(#[from] DecodeError),
    /// D3D11 and D3D12 device adapter mismatch.
    ///
    /// Declared per [ADR-0002](https://github.com/nyxways/mediaway/blob/main/crates/mediaway-wgpu/adr/0002-decode-to-wgpu-texture-bridge.md)'s
    /// public contract as a distinct, non-overloaded variant. In the current implementation
    /// `D3d11SharedDecodeBridge::open`'s own two-sided LUID check already folds an adapter
    /// mismatch into `DecodeError::InvalidInput` (surfaced here as
    /// [`WgpuInteropError::DecodeBridge`]), so [`WgpuDx12DecodeBridge::new`](crate::wgpu::WgpuDx12DecodeBridge::new)
    /// does not construct this variant today — kept so a future caller-side LUID check (or a
    /// `DecodeError` variant split) has a home without a breaking enum change.
    #[error("D3D11/D3D12 device adapter mismatch")]
    AdapterMismatch,
}
