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

/// One plane's byte layout within a [`DmaBufDescriptor`]'s referenced DRM object(s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DmaBufPlane {
    /// Which `DmaBufDescriptor` object this plane's bytes live in — `0` or `1`
    /// (this type caps object count at 2; see field docs on [`DmaBufDescriptor`]).
    pub object_index: u8,
    /// Byte offset of this plane within its object.
    pub offset: u32,
    /// Row pitch (stride) in bytes.
    pub pitch: u32,
}

/// Linux DRM/GEM DMA-BUF surface — VA-API's native Zero-Copy export shape.
///
/// (`vaExportSurfaceHandle` + `VADRMPRIMESurfaceDescriptor`), scoped to the ≤2-plane,
/// ≤2-object case this workspace's NV12 pipelines produce (see `mediaway-decoder`
/// `adr/linux/0003-vaapi-dmabuf-zero-copy-output.md`'s "why scoped, not general" note
/// before adding a 3rd/4th plane or object).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DmaBufDescriptor {
    /// Primary DMA-BUF fd (DRM object 0 bits, offset by `+1` so fd `0` still round-trips
    /// through `NativeHandle`'s non-zero representation — same convention
    /// `vulkan::zero_copy::build_handle` already uses for a `slot_index` of `0`).
    /// **Borrowed by convention, not owned by this struct** — see the owning ADR's
    /// § Fd lifetime contract for who calls `close(2)` and when.
    pub fd0: NativeHandle,
    /// Second DMA-BUF fd (DRM object 1), only when the driver reported `num_objects == 2`
    /// (a driver that splits Y/UV into separate objects instead of composing them).
    pub fd1: Option<NativeHandle>,
    /// `DRM_FORMAT_*` (e.g. `DRM_FORMAT_NV12`) — **not** `VA_FOURCC_*`; numerically identical
    /// for NV12 today but a distinct namespace (see the owning ADR's Open questions).
    pub fourcc: u32,
    /// DRM format modifier (tiling layout). `0` is `DRM_FORMAT_MOD_LINEAR`, itself a valid,
    /// meaningful value — never treated as "absent" (unlike `NativeHandle`'s `0`-is-None
    /// convention).
    pub modifier: u64,
    /// Surface width in pixels.
    pub width: u32,
    /// Surface height in pixels.
    pub height: u32,
    /// NV12 = 2 entries used; unused trailing entries are zeroed and ignored.
    pub planes: [DmaBufPlane; 2],
    /// Number of entries in `planes` actually populated (1 or 2).
    pub plane_count: u8,
}

/// Native GPU buffer / texture handle without CPU readback.
///
/// Variants are declared early so facades can name Zero-Copy inputs. Backends
/// that do not support a variant return an explicit unsupported error — never
/// silently read back.
///
/// **Not `Copy`**: the [`GpuBufferHandle::DmaBuf`] variant boxes its payload (see that
/// variant's doc) — a `Box` field is never `Copy`. Every other variant stays trivially
/// cheap to `Clone`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    /// Linux DRM/GEM DMA-BUF export (VA-API's native Zero-Copy mechanism). Boxed — a faithful
    /// descriptor (fd + DRM fourcc + modifier + dims + per-plane layout) is 4-5x larger than
    /// every other variant, and this enum is embedded by value in `VideoFrameStorage::Gpu` on
    /// every platform's hot path — boxing keeps `size_of::<GpuBufferHandle>()` unaffected for
    /// builds that never construct a `DmaBuf` value. See this variant's owning ADR
    /// (`mediaway-decoder` `adr/linux/0003-vaapi-dmabuf-zero-copy-output.md`) for the full
    /// rationale.
    DmaBuf(Box<DmaBufDescriptor>),
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
