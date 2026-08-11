//! `GpuDeviceHandle`/`GpuBufferHandle` `#[repr(C)]` value-type mirrors, shared by every
//! `mediaway-*-ffi` crate that needs to cross a live GPU handle over the C ABI.
//!
//! First consumer: `mediaway-device-ffi`
//! ([`adr/0003-gpu-handle-c-abi.md`](../../mediaway-device-ffi/adr/0003-gpu-handle-c-abi.md)),
//! output-only (`GpuBufferHandle` poll results never round-trip back to Rust).
//! Second consumer: `mediaway-ffi`
//! ([`adr/0002-gpu-frame-input-c-abi.md`](../../mediaway-ffi/adr/0002-gpu-frame-input-c-abi.md)),
//! which needed the input direction too (`GpuBufferHandle::to_common`) — its previously
//! twice-deferred `gpu_device`/`max_path_class` (`adr/0001-auto-encode-c-abi.md` §1) is
//! now partially resolved (`gpu_device` only; `max_path_class` stays deferred).
//!
//! Unlike [`crate::common::types::Rational`]/[`crate::common::types::CodecKind`], both Rust enums this
//! module mirrors are data-carrying — there is no existing discriminant sequence to
//! preserve, so the C `kind` enum numbering below is a fresh FFI-layer invention (see
//! that ADR's §1 for why `GpuDeviceKind::None = 0` and `GpuBufferKind::Unknown = 255`
//! are each chosen independently).

use mediaway_common::{
    GpuBufferHandle as CommonGpuBufferHandle, GpuDeviceHandle as CommonGpuDeviceHandle,
    NativeHandle,
};

/// Discriminant for [`GpuDeviceHandle`] — mirrors `mediaway_common::GpuDeviceHandle`'s
/// variants, plus `None` for "no device supplied" (the safe zero-init default).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuDeviceKind {
    /// No device supplied.
    None = 0,
    /// `ID3D11Device*`.
    DirectX11 = 1,
    /// `ID3D12Device*`.
    DirectX12 = 2,
    /// `VkDevice` (or wrapper token; layout decided in the Linux ADR).
    Vulkan = 3,
    /// `MTLDevice` (Apple backends).
    Metal = 4,
    /// Browser / WASM `GPUDevice` host token.
    WebGpu = 5,
}

/// Native GPU **device** handle — caller-supplied input (e.g. a Screen capture config's
/// `gpu_device` field).
///
/// Plain value struct, `Copy`, no heap allocation, no free function. The caller owns the
/// underlying device and must keep it alive for at least the duration of the call that
/// consumes it — the exact contract is documented per call site (e.g.
/// `mediaway-device-ffi`'s `adr/0003-gpu-handle-c-abi.md` §2), not here, since lifetime
/// obligations differ by consumer.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuDeviceHandle {
    /// Which platform variant `native`/`webgpu_device_id` apply to.
    pub kind: GpuDeviceKind,
    /// `ID3D11Device*` / `ID3D12Device*` / `VkDevice` / `MTLDevice` bits. `0` for
    /// `None`/`WebGpu`.
    pub native: usize,
    /// `WebGpu` only; `0` otherwise.
    pub webgpu_device_id: u64,
}

impl GpuDeviceHandle {
    /// `None` for [`GpuDeviceKind::None`], or when `native == 0` for a non-`WebGpu`
    /// kind (malformed or zero-initialized input) — both are treated identically to "no
    /// device supplied", matching `NativeHandle::new(0)`'s own `None` behavior.
    #[must_use]
    pub fn to_common(self) -> Option<CommonGpuDeviceHandle> {
        match self.kind {
            GpuDeviceKind::None => None,
            GpuDeviceKind::DirectX11 => Some(CommonGpuDeviceHandle::DirectX11(NativeHandle::new(
                self.native,
            )?)),
            GpuDeviceKind::DirectX12 => Some(CommonGpuDeviceHandle::DirectX12(NativeHandle::new(
                self.native,
            )?)),
            GpuDeviceKind::Vulkan => Some(CommonGpuDeviceHandle::Vulkan(NativeHandle::new(
                self.native,
            )?)),
            GpuDeviceKind::Metal => Some(CommonGpuDeviceHandle::Metal(NativeHandle::new(
                self.native,
            )?)),
            GpuDeviceKind::WebGpu => Some(CommonGpuDeviceHandle::WebGpu {
                device_id: self.webgpu_device_id,
            }),
        }
    }
}

impl From<CommonGpuDeviceHandle> for GpuDeviceHandle {
    // `GpuDeviceHandle` is `#[non_exhaustive]`; see `GpuBufferHandle`'s identical
    // wildcard-arm precedent below for why the overlap with no real arm's body is
    // intentional, not a copy-paste bug.
    fn from(handle: CommonGpuDeviceHandle) -> Self {
        match handle {
            CommonGpuDeviceHandle::DirectX11(native) => Self {
                kind: GpuDeviceKind::DirectX11,
                native: native.get(),
                webgpu_device_id: 0,
            },
            CommonGpuDeviceHandle::DirectX12(native) => Self {
                kind: GpuDeviceKind::DirectX12,
                native: native.get(),
                webgpu_device_id: 0,
            },
            CommonGpuDeviceHandle::Vulkan(native) => Self {
                kind: GpuDeviceKind::Vulkan,
                native: native.get(),
                webgpu_device_id: 0,
            },
            CommonGpuDeviceHandle::Metal(native) => Self {
                kind: GpuDeviceKind::Metal,
                native: native.get(),
                webgpu_device_id: 0,
            },
            CommonGpuDeviceHandle::WebGpu { device_id } => Self {
                kind: GpuDeviceKind::WebGpu,
                native: 0,
                webgpu_device_id: device_id,
            },
            _ => Self {
                kind: GpuDeviceKind::None,
                native: 0,
                webgpu_device_id: 0,
            },
        }
    }
}

/// Discriminant for [`GpuBufferHandle`] — mirrors `mediaway_common::GpuBufferHandle`'s
/// 7 variants.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuBufferKind {
    /// `ID3D11Texture2D*` (+ subresource index).
    DirectX11 = 0,
    /// `ID3D12Resource*`.
    DirectX12 = 1,
    /// Windows shared `HANDLE`.
    DirectXShared = 2,
    /// Metal / `CVPixelBuffer` / `IOSurface` token.
    Metal = 3,
    /// `AHardwareBuffer*` (Android).
    AndroidSurface = 4,
    /// Vulkan image + memory cookie.
    Vulkan = 5,
    /// Browser / WASM `GPUTexture` host token.
    WebGpu = 6,
    /// `GpuBufferHandle` is `#[non_exhaustive]`; decode-side catch-all for a future
    /// variant this crate doesn't know how to mirror yet. Rust never actually produces
    /// this today — output-only direction, same idiom as
    /// `mediaway-device-ffi::MediawayDeviceKind::Unknown`.
    Unknown = 255,
}

/// Native GPU **buffer**/texture handle — output only (e.g. a polled video frame's GPU
/// storage).
///
/// **Borrowed, not owned** by default: whether/how long the pointer stays valid is
/// decided by the consuming crate's own header (e.g. `mediaway-device-ffi`'s
/// `adr/0003-gpu-handle-c-abi.md` §3/§8) — this struct only carries the bits.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuBufferHandle {
    /// Which platform variant the `native_*`/`webgpu_texture_id` fields apply to.
    pub kind: GpuBufferKind,
    /// texture / resource / handle / buffer / image pointer bits, per `kind`.
    pub native_a: usize,
    /// Vulkan memory cookie only; `0` otherwise.
    pub native_b: usize,
    /// `DirectX11` only; `0` otherwise.
    pub subresource: u32,
    /// `WebGpu` only; `0` otherwise.
    pub webgpu_texture_id: u64,
}

impl From<CommonGpuBufferHandle> for GpuBufferHandle {
    // `GpuBufferHandle` is `#[non_exhaustive]`; the wildcard arm is required even though
    // every variant that exists today is matched by name above it — see
    // `MediawayPixelFormat`'s identical `#[allow(clippy::match_same_arms)]` precedent in
    // `mediaway-device-ffi::types` for why the overlap with no real arm's body is
    // intentional, not a copy-paste bug.
    fn from(handle: CommonGpuBufferHandle) -> Self {
        match handle {
            CommonGpuBufferHandle::DirectX11 {
                texture,
                subresource,
            } => Self {
                kind: GpuBufferKind::DirectX11,
                native_a: texture.get(),
                native_b: 0,
                subresource,
                webgpu_texture_id: 0,
            },
            CommonGpuBufferHandle::DirectX12 { resource } => Self {
                kind: GpuBufferKind::DirectX12,
                native_a: resource.get(),
                native_b: 0,
                subresource: 0,
                webgpu_texture_id: 0,
            },
            CommonGpuBufferHandle::DirectXShared { handle } => Self {
                kind: GpuBufferKind::DirectXShared,
                native_a: handle.get(),
                native_b: 0,
                subresource: 0,
                webgpu_texture_id: 0,
            },
            CommonGpuBufferHandle::Metal { buffer } => Self {
                kind: GpuBufferKind::Metal,
                native_a: buffer.get(),
                native_b: 0,
                subresource: 0,
                webgpu_texture_id: 0,
            },
            CommonGpuBufferHandle::AndroidSurface { buffer } => Self {
                kind: GpuBufferKind::AndroidSurface,
                native_a: buffer.get(),
                native_b: 0,
                subresource: 0,
                webgpu_texture_id: 0,
            },
            CommonGpuBufferHandle::Vulkan { image, memory } => Self {
                kind: GpuBufferKind::Vulkan,
                native_a: image.get(),
                native_b: memory.get(),
                subresource: 0,
                webgpu_texture_id: 0,
            },
            CommonGpuBufferHandle::WebGpu { texture_id } => Self {
                kind: GpuBufferKind::WebGpu,
                native_a: 0,
                native_b: 0,
                subresource: 0,
                webgpu_texture_id: texture_id,
            },
            _ => Self {
                kind: GpuBufferKind::Unknown,
                native_a: 0,
                native_b: 0,
                subresource: 0,
                webgpu_texture_id: 0,
            },
        }
    }
}

impl GpuBufferHandle {
    /// Reverse of [`From<CommonGpuBufferHandle>`] — needed by consumers that accept a GPU
    /// buffer handle as *input* (e.g. `mediaway-ffi`'s `write_frame`), unlike
    /// `mediaway-device-ffi`, which only ever produces one as poll output.
    ///
    /// `None` for [`GpuBufferKind::Unknown`], or when a pointer-bearing field is `0` for a
    /// kind that requires one (malformed or zero-initialized input) — same "treat as
    /// absent" contract as [`GpuDeviceHandle::to_common`].
    #[must_use]
    pub fn to_common(self) -> Option<CommonGpuBufferHandle> {
        match self.kind {
            GpuBufferKind::DirectX11 => Some(CommonGpuBufferHandle::DirectX11 {
                texture: NativeHandle::new(self.native_a)?,
                subresource: self.subresource,
            }),
            GpuBufferKind::DirectX12 => Some(CommonGpuBufferHandle::DirectX12 {
                resource: NativeHandle::new(self.native_a)?,
            }),
            GpuBufferKind::DirectXShared => Some(CommonGpuBufferHandle::DirectXShared {
                handle: NativeHandle::new(self.native_a)?,
            }),
            GpuBufferKind::Metal => Some(CommonGpuBufferHandle::Metal {
                buffer: NativeHandle::new(self.native_a)?,
            }),
            GpuBufferKind::AndroidSurface => Some(CommonGpuBufferHandle::AndroidSurface {
                buffer: NativeHandle::new(self.native_a)?,
            }),
            GpuBufferKind::Vulkan => Some(CommonGpuBufferHandle::Vulkan {
                image: NativeHandle::new(self.native_a)?,
                memory: NativeHandle::new(self.native_b)?,
            }),
            GpuBufferKind::WebGpu => Some(CommonGpuBufferHandle::WebGpu {
                texture_id: self.webgpu_texture_id,
            }),
            GpuBufferKind::Unknown => None,
        }
    }
}

#[cfg(test)]
#[path = "gpu_tests.rs"]
mod tests;
