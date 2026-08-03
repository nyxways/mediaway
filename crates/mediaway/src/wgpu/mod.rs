//! wgpu HAL interop — bridge `wgpu::Device`/`wgpu::Texture` into Mediaway's
//! [`mediaway_common::GpuBufferHandle`] / [`mediaway_common::GpuDeviceHandle`]
//! so an app already using `wgpu` for rendering/compute can hand frames to
//! Mediaway encode without a forced CPU readback.
//!
//! `wgpu` itself has **no video-encode API surface**
//! ([gfx-rs/wgpu#2330], closed "not planned"). This crate does not add one —
//! it is purely an **import/export bridge**, per
//! [`docs/spec/gpu-interop.md`](https://github.com/nyxways/mediaway/blob/main/docs/spec/gpu-interop.md):
//! reach past `wgpu`'s own API via its documented HAL escape hatches
//! (`Device::as_hal`, `Device::create_texture_from_hal` — `unsafe`, one
//! `// SAFETY:` per call site) to recover the *native* GPU handle `wgpu`'s
//! chosen backend already holds, then hand that native handle to an existing
//! Mediaway platform encode backend. The actual encode session stays the
//! caller's own [`mediaway_encoder::VideoEncoder`] — this crate never opens or
//! drives an encoder itself (low-level APIs stay first-class; this is
//! composition, not a new encoder surface).
//!
//! ## Stage 1 — Windows DX12 → WMF `GpuCopy` ([`WgpuDx12Bridge`])
//!
//! `wgpu` has no D3D11 backend (removed upstream years ago); DX12 is its only
//! Windows-native backend. Windows Media Foundation hardware encoder MFTs
//! reject `D3D11On12`-wrapped textures, so this bridges through
//! [`mediaway_encoder::windows::D3d12SharedEncodeBridge`] — the same
//! `EncodePathClass::GpuCopy` (one GPU→GPU copy per frame, plus a documented
//! CPU↔GPU sync stall) path native D3D12 apps already use. **Not Zero-Copy** —
//! see [ADR-0001](https://github.com/nyxways/mediaway/blob/main/crates/mediaway-wgpu/adr/0001-dx12-hal-gpucopy-bridge.md)
//! for why, and what a future Vulkan-backend / true-Zero-Copy path would need.
//!
//! # Hardware-verified (2026-07-29)
//!
//! Written against `wgpu` 26.0's documented API, then actually compiled, and
//! run against this workspace's reference Windows box (NVIDIA RTX 4090 +
//! Intel UHD 770): `cargo test -p mediaway-wgpu` passes, including the real
//! `wgpu_dx12_bridge_encodes_h264_or_skip` hardware smoke test in
//! `tests/dx12_encode_smoke.rs`. Three real API-signature mistakes (guessed
//! without compiler feedback) were caught and fixed — see ADR-0001's
//! "Verification update" section: a `windows`-crate version mismatch between
//! this crate (0.62, matching the rest of the workspace) and `wgpu_hal::dx12`
//! internally (pinned to 0.58), a `PollType::Wait` struct-variant guess (it's
//! a unit variant), and a `Texture::texture_from_raw` guess (the real
//! constructor is `Device::texture_from_raw`, an associated function). The
//! smoke test currently **skips** on this exact machine (`no HW H.264 MFT for
//! BGRA DXGI input`) — confirmed to be a pre-existing, already-known hardware/
//! driver limitation shared by `mediaway-encoder-windows`'s own
//! `auto_open_gpu_copy_via_d3d12_bridge_or_skip` test (same skip reason, same
//! machine), not a bug introduced by this bridge.
//!
//! ## Stage 5 — Windows decode output → `wgpu::Texture` import ([`WgpuDx12DecodeBridge`])
//!
//! The reverse direction of Stage 1: WMF DX11 Zero-Copy decode output
//! (`GpuBufferHandle::DirectX11`, NV12) → an ordinary `wgpu::Texture`, via
//! [`mediaway_decoder::windows::D3d11SharedDecodeBridge`] (D3D11 shared texture →
//! `ID3D12Device::OpenSharedHandle`). Same `GpuCopy` cost class as Stage 1 (one D3D11→D3D11
//! copy plus a CPU↔GPU query/flush stall per imported frame), never Zero-Copy. See
//! [ADR-0002](https://github.com/nyxways/mediaway/blob/main/crates/mediaway-wgpu/adr/0002-decode-to-wgpu-texture-bridge.md).
//!
//! [gfx-rs/wgpu#2330]: https://github.com/gfx-rs/wgpu/issues/2330

#![allow(clippy::too_long_first_doc_paragraph)] // crate-root doc became module doc (ADR-0021 merge)
#![cfg_attr(windows, allow(unsafe_code))]
#![cfg_attr(not(windows), forbid(unsafe_code))]

mod error;
pub use error::WgpuInteropError;

#[cfg(windows)]
mod dx12;
#[cfg(windows)]
pub use dx12::{BRIDGE_FORMAT, WgpuDx12Bridge};

#[cfg(windows)]
mod dx12_decode;
#[cfg(windows)]
pub use dx12_decode::{DECODE_BRIDGE_FORMAT, WgpuDx12DecodeBridge};

/// Off-Windows placeholder — no wgpu HAL bridge exists yet.
///
/// Vulkan/Metal HAL interop into a real Zero-Copy encode backend is future
/// work — see the crate ADR. Kept so downstream crates can name/branch on a
/// stable type instead of a missing one; every method is
/// [`WgpuInteropError::Unsupported`].
#[cfg(not(windows))]
#[derive(Debug)]
pub struct WgpuDx12Bridge {
    _priv: (),
}

#[cfg(not(windows))]
impl WgpuDx12Bridge {
    /// Always [`WgpuInteropError::Unsupported`] off-Windows.
    ///
    /// # Errors
    ///
    /// Always returns [`WgpuInteropError::Unsupported`].
    pub const fn new(
        _device: &wgpu::Device,
        _width: u32,
        _height: u32,
    ) -> Result<Self, WgpuInteropError> {
        Err(WgpuInteropError::Unsupported)
    }
}

/// Off-Windows placeholder: no wgpu decode-import bridge exists yet — see
/// [`WgpuDx12Bridge`]'s own placeholder doc for the same rationale, mirrored here for the
/// reverse (decode-output) direction.
#[cfg(not(windows))]
#[derive(Debug)]
pub struct WgpuDx12DecodeBridge {
    _priv: (),
}

#[cfg(not(windows))]
impl WgpuDx12DecodeBridge {
    /// Always [`WgpuInteropError::Unsupported`] off-Windows.
    ///
    /// # Errors
    ///
    /// Always returns [`WgpuInteropError::Unsupported`].
    pub const fn new(
        _device: &wgpu::Device,
        _d3d11_device: mediaway_common::GpuDeviceHandle,
        _width: u32,
        _height: u32,
    ) -> Result<Self, WgpuInteropError> {
        Err(WgpuInteropError::Unsupported)
    }
}
