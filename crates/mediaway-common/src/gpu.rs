//! Opaque GPU resource handles for Zero-Copy encode/decode paths.
//!
//! Platform backends cast [`NativeHandle`] bits to native pointers. This crate stays
//! `forbid(unsafe_code)`; all casting lives in `mediaway-*-<platform>` crates.
//! Ownership and fence contracts are documented in those backends’ ADRs.
//!
//! Framework bridges (wgpu, WebGPU, Dawn): [`docs/spec/gpu-interop.md`].

#![forbid(unsafe_code)]

use core::num::NonZeroUsize;

/// Opaque non-null native pointer / `HANDLE` bits.
///
/// Backs every "raw platform handle" field in [`GpuBufferHandle`] and
/// [`GpuDeviceHandle`] (and the `gpu_device` config field in
/// `mediaway-encoder`/`mediaway-decoder`/`mediaway-device`). Never dereferenced
/// in this crate — platform backends cast [`NativeHandle::get`] to/from the
/// real pointer type. Backed by [`NonZeroUsize`] so "unset" is
/// `Option<NativeHandle>::None` instead of a `0` sentinel repeated in doc
/// comments; the niche keeps `Option<NativeHandle>` the same size as `usize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeHandle(NonZeroUsize);

impl NativeHandle {
    /// Wrap native pointer bits. `None` when `bits == 0`.
    #[must_use]
    pub const fn new(bits: usize) -> Option<Self> {
        match NonZeroUsize::new(bits) {
            Some(n) => Some(Self(n)),
            None => None,
        }
    }

    /// Recover the native pointer bits for an FFI cast.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

/// Native GPU buffer / texture handle without CPU readback.
///
/// Variants are declared early so facades can name Zero-Copy inputs. Backends
/// that do not support a variant return an explicit unsupported error — never
/// silently read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GpuBufferHandle {
    /// `ID3D11Texture2D*` (+ subresource index).
    DirectX11 {
        /// Opaque texture pointer.
        texture: NativeHandle,
        /// Subresource / array slice.
        subresource: u32,
    },
    /// `ID3D12Resource*` (or agreed shared representation in the Windows ADR).
    DirectX12 {
        /// Opaque resource pointer.
        resource: NativeHandle,
    },
    /// Windows shared `HANDLE` for cross-device / wgpu HAL export.
    DirectXShared {
        /// Opaque `HANDLE`.
        handle: NativeHandle,
    },
    /// Metal / `CVPixelBuffer` / `IOSurface` token (Apple backends).
    Metal {
        /// Opaque native pointer.
        buffer: NativeHandle,
    },
    /// `AHardwareBuffer*` (Android).
    AndroidSurface {
        /// Opaque buffer pointer.
        buffer: NativeHandle,
    },
    /// Vulkan image + binding token (layout decided in Linux ADR).
    Vulkan {
        /// Opaque `VkImage` (or wrapper).
        image: NativeHandle,
        /// Opaque device/memory cookie for the backend.
        memory: NativeHandle,
    },
    /// Browser / WASM `GPUTexture` host token.
    WebGpu {
        /// Host-defined texture id (not a raw WASM pointer).
        texture_id: u64,
    },
}

/// Native GPU **device** handle — owns the buffers submitted via [`GpuBufferHandle`].
///
/// Mirrors `GpuBufferHandle`'s platform variants but names the device, not a
/// buffer (e.g. the `ID3D11Device*` that must own submitted `DirectX11`
/// textures). Facade configs (`VideoEncoderConfig::gpu_device`, …) use
/// `Option<GpuDeviceHandle>` — `None` means no Zero-Copy device was supplied.
/// `#[non_exhaustive]`: declared ahead of backend support, like
/// [`GpuBufferHandle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GpuDeviceHandle {
    /// `ID3D11Device*`.
    DirectX11(NativeHandle),
    /// `ID3D12Device*`.
    DirectX12(NativeHandle),
    /// `VkDevice` (or wrapper token; layout decided in Linux ADR).
    Vulkan(NativeHandle),
    /// `MTLDevice` (Apple backends).
    Metal(NativeHandle),
    /// Browser / WASM `GPUDevice` host token (not a raw WASM pointer).
    WebGpu {
        /// Host-defined device id.
        device_id: u64,
    },
}
