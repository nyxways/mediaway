//! Auto path selection for Windows encode (ZC → `GpuCopy` → CPU upload → Software).
//!
//! Types: [`crate::auto`]. This module owns the session so the facade
//! does not depend on this crate (no dependency cycle).

#![forbid(unsafe_code)]

use mediaway_common::{
    CodecKind, GpuBufferHandle, GpuDeviceHandle, NativeHandle, Packet, StreamInfo, VideoFrame,
};
use crate::auto::{AutoVideoEncodeConfig, Backend, BackendSelection, EncodePathClass};
use crate::{EncodeError, VideoEncoder, VideoInputPreference};
use std::fmt;

use crate::windows::WindowsVideoEncoder;

/// Concrete implementation behind an open [`AutoVideoEncoder`] session — distinct from
/// [`Backend`] (the facade's *selection* vocabulary): this is the actual object whose
/// trait methods get called. Enum (not `Box<dyn>`) for zero-cost abstraction per
/// AGENTS.md § Zero-cost abstractions. Size difference between variants is acceptable
/// for this dispatch pattern.
#[allow(clippy::large_enum_variant)]
enum EncoderImpl {
    Wmf(WindowsVideoEncoder),
    Sw(mediaway_sw::av1::Av1Encoder),
    Nvenc(crate::nvenc::NvencVideoEncoder),
    QuickSync(crate::quicksync::QuickSyncVideoEncoder),
}

/// Maps `mediaway_sw::av1::Av1Error` to `EncodeError`.
#[allow(
    clippy::match_like_matches_macro,
    clippy::match_same_arms,
    clippy::needless_pass_by_value
)]
fn map_av1_error(e: mediaway_sw::av1::Av1Error) -> EncodeError {
    use mediaway_sw::av1::Av1Error;
    match e {
        Av1Error::Unsupported => EncodeError::Unsupported,
        // Both InvalidConfig and InvalidInput errors map to InvalidInput per the task spec.
        Av1Error::InvalidConfig(_) => EncodeError::InvalidInput,
        Av1Error::InvalidInput => EncodeError::InvalidInput,
        Av1Error::Backend => EncodeError::Backend,
        Av1Error::Closed => EncodeError::Closed,
        _ => EncodeError::Backend,
    }
}

/// Windows auto-selected video encode session.
pub struct AutoVideoEncoder {
    path: EncodePathClass,
    backend: Backend,
    inner: EncoderImpl,
    /// Live D3D12→D3D11 bridge when [`EncodePathClass::GpuCopy`] was selected.
    ///
    /// Must outlive the session: the encoder holds its own COM reference to the
    /// bridged `ID3D11Device`/`ID3D11Texture2D` (see [`crate::windows::D3d12SharedEncodeBridge`]),
    /// but the caller keeps `CopyResource`-ing into the *same* shared D3D12
    /// resource every frame, so that resource (and its DX11 view) must stay alive
    /// for the session's lifetime, not just for the `open` call.
    #[cfg(windows)]
    bridge: Option<crate::windows::D3d12SharedEncodeBridge>,
}

impl fmt::Debug for AutoVideoEncoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AutoVideoEncoder")
            .field("path", &self.path)
            .field("backend", &self.backend)
            .finish_non_exhaustive()
    }
}

impl AutoVideoEncoder {
    /// Open a session per `config.backend` (see [`BackendSelection`]):
    ///
    /// - [`BackendSelection::Explicit`] short-circuits straight to that one backend —
    ///   [`Backend::Software`] / [`Backend::Nvenc`] / [`Backend::QuickSync`] fail hard
    ///   (no fallback) if unavailable; [`Backend::Amf`] always fails with
    ///   [`EncodeError::NoBackend`] (no implementation yet — see
    ///   `mediaway-encoder-amf` adr/0001). [`Backend::Os`] falls through to the chain
    ///   below but stops there — never a vendor SDK, never `Software`.
    /// - [`BackendSelection::Auto`] and [`BackendSelection::AutoHardwareOnly`] both start
    ///   with [`Backend::Os`]'s own GPU-handle tiers: **Zero-Copy** (`gpu_device` =
    ///   `DirectX11`) → **`GpuCopy`** (`gpu_device` = `DirectX12`, bridged through
    ///   [`crate::windows::D3d12SharedEncodeBridge`] — one GPU→GPU copy per frame, see
    ///   [ADR-0006](../adr/0006-d3d12-shared-to-d3d11.md)), gated by `max_path_class`.
    ///   These always come first — strictly cheaper than any CPU-upload path.
    /// - Before falling back to `Os`'s own CPU upload, `AutoHardwareOnly` ranks the
    ///   vendor SDKs (NVENC, then `QuickSync`; AMF has no implementation) ahead of it —
    ///   that ranking is the entire reason this variant exists, letting a caller reach a
    ///   vendor SDK without naming one explicitly. `Auto` never tries a vendor SDK here,
    ///   per ADR-0004: same silicon often backs both `Os` and a vendor SDK, so the
    ///   vendor path is not automatically faster and must be opted into.
    /// - `Os` CPU upload (`upload_cpu_nv12` — one CPU→GPU copy per frame) is tried next
    ///   if `max_path_class` allows it.
    /// - `AutoHardwareOnly`/`Explicit(Os)` stop here if nothing worked. Plain `Auto`
    ///   falls back to **Software** (pure Rust AV1 via `mediaway-sw`'s `rav1e` adapter;
    ///   other codecs have no software backend) when `max_path_class` allows it, then
    ///   reports [`EncodeError::NoBackend`] instead of the generic
    ///   [`EncodeError::Unsupported`] if `max_path_class` recognized Readback as
    ///   acceptable — an honest "recognized tier, no backend" signal, since this crate
    ///   has no DX11 staging-texture readback implementation.
    ///
    /// # Errors
    ///
    /// - [`EncodeError::InvalidInput`] — zero width/height.
    /// - [`EncodeError::Unsupported`] — no path under `max_path_class` / codec
    ///   unavailable, or a GPU device kind (Vulkan / Metal / `WebGpu`) this crate cannot
    ///   bridge.
    /// - [`EncodeError::NoBackend`] — `max_path_class` recognized Readback/Software but
    ///   neither has an implemented backend for this request, or [`Backend::Amf`] was
    ///   named explicitly.
    /// - [`EncodeError::Backend`] — MF/DXGI/vendor SDK failure after a path was chosen.
    pub fn open(config: &AutoVideoEncodeConfig) -> Result<Self, EncodeError> {
        if config.width == 0 || config.height == 0 {
            return Err(EncodeError::InvalidInput);
        }

        match config.backend {
            BackendSelection::Explicit(Backend::Software) => return Self::try_software(config),
            BackendSelection::Explicit(Backend::Nvenc) => return Self::try_nvenc(config),
            BackendSelection::Explicit(Backend::QuickSync) => return Self::try_quicksync(config),
            BackendSelection::Explicit(Backend::Amf) => return Err(EncodeError::NoBackend),
            // `Auto`, `AutoHardwareOnly`, `Explicit(Os)`, and any future
            // (`#[non_exhaustive]`) selection or backend all fall through to the `Os`
            // chain below.
            _ => {}
        }

        let ceiling = config.max_path_class;
        let hardware_only = matches!(config.backend, BackendSelection::AutoHardwareOnly);
        let mut last_err: Option<EncodeError> = None;

        // Phase 1: `Os`'s own GPU-handle tiers — strictly cheaper than any CPU-upload
        // path, so these always come first regardless of which selection reached here.
        if let Some(gpu_device) = config.gpu_device {
            match gpu_device {
                GpuDeviceHandle::DirectX11(_) => {
                    let low =
                        config.to_low_level(VideoInputPreference::ZeroCopyGpu, Some(gpu_device));
                    match WindowsVideoEncoder::open(&low) {
                        Ok(inner) => {
                            return Ok(Self::with_path(
                                EncodePathClass::ZeroCopy,
                                Backend::Os,
                                EncoderImpl::Wmf(inner),
                            ));
                        }
                        Err(e) => last_err = Some(e),
                    }
                }
                GpuDeviceHandle::DirectX12(handle) if ceiling >= EncodePathClass::GpuCopy => {
                    match Self::try_gpu_copy(config, handle) {
                        Ok(enc) => return Ok(enc),
                        Err(e) => last_err = Some(e),
                    }
                }
                // DirectX12 below the configured ceiling, or a foreign device kind
                // (Vulkan / Metal / WebGpu): no Windows bridge exists for it.
                // Record why instead of silently dropping to CPU upload.
                _ => last_err = Some(EncodeError::Unsupported),
            }
        }

        // Phase 2: CPU-upload-tier candidates. `AutoHardwareOnly` ranks the vendor SDKs
        // ahead of `Os`'s own CPU upload here — see this method's doc comment.
        if hardware_only {
            if let Ok(enc) = Self::try_nvenc(config) {
                return Ok(enc);
            }
            if let Ok(enc) = Self::try_quicksync(config) {
                return Ok(enc);
            }
        }

        if ceiling >= EncodePathClass::CpuUpload {
            let low = config.to_low_level(VideoInputPreference::CpuUploadOk, None);
            match WindowsVideoEncoder::open(&low) {
                Ok(inner) => {
                    return Ok(Self::with_path(
                        EncodePathClass::CpuUpload,
                        Backend::Os,
                        EncoderImpl::Wmf(inner),
                    ));
                }
                Err(e) => last_err = Some(e),
            }
        }

        if hardware_only || matches!(config.backend, BackendSelection::Explicit(Backend::Os)) {
            return Err(last_err.unwrap_or(EncodeError::Unsupported));
        }

        // Plain `Auto` only from here: never a vendor SDK, Software only if the
        // ceiling allows it.
        if ceiling >= EncodePathClass::Software {
            match Self::try_software(config) {
                Ok(enc) => return Ok(enc),
                Err(e) => last_err = Some(e),
            }
        }

        if ceiling >= EncodePathClass::Readback {
            return Err(EncodeError::NoBackend);
        }

        Err(last_err.unwrap_or(EncodeError::Unsupported))
    }

    #[cfg(windows)]
    const fn with_path(path: EncodePathClass, backend: Backend, inner: EncoderImpl) -> Self {
        Self {
            path,
            backend,
            inner,
            bridge: None,
        }
    }

    #[cfg(not(windows))]
    const fn with_path(path: EncodePathClass, backend: Backend, inner: EncoderImpl) -> Self {
        Self {
            path,
            backend,
            inner,
        }
    }

    /// Attempt to open a software encoder; currently only AV1 via `rav1e`.
    fn try_software(config: &AutoVideoEncodeConfig) -> Result<Self, EncodeError> {
        // Only mediaway-sw's AV1 encoder exists today; other codecs stay NoBackend
        if config.codec != CodecKind::Av1 {
            return Err(EncodeError::Unsupported);
        }
        let mut sw_config =
            mediaway_sw::av1::Av1EncoderConfig::new(config.width, config.height, config.time_base);
        sw_config.bitrate_bps = config.bitrate_bps;
        let inner = mediaway_sw::av1::Av1Encoder::open(&sw_config).map_err(map_av1_error)?;
        Ok(Self::with_path(
            EncodePathClass::Software,
            Backend::Software,
            EncoderImpl::Sw(inner),
        ))
    }

    /// Explicit NVENC open — CPU-upload input only (no Zero-Copy input path yet). Off
    /// Windows, the crate's own stub returns [`EncodeError::Unsupported`], so no `cfg`
    /// gate is needed here.
    fn try_nvenc(config: &AutoVideoEncodeConfig) -> Result<Self, EncodeError> {
        let low = config.to_low_level(VideoInputPreference::CpuUploadOk, None);
        let inner = crate::nvenc::NvencVideoEncoder::open(&low)?;
        Ok(Self::with_path(
            EncodePathClass::CpuUpload,
            Backend::Nvenc,
            EncoderImpl::Nvenc(inner),
        ))
    }

    /// Explicit Quick Sync Video open — CPU-upload input only (no Zero-Copy input path
    /// yet). Off Windows, the crate's own stub returns [`EncodeError::Unsupported`], so
    /// no `cfg` gate is needed here.
    fn try_quicksync(config: &AutoVideoEncodeConfig) -> Result<Self, EncodeError> {
        let low = config.to_low_level(VideoInputPreference::CpuUploadOk, None);
        let inner = crate::quicksync::QuickSyncVideoEncoder::open(&low)?;
        Ok(Self::with_path(
            EncodePathClass::CpuUpload,
            Backend::QuickSync,
            EncoderImpl::QuickSync(inner),
        ))
    }

    /// Bridge `d3d12_device` to a native D3D11 device/texture and open the
    /// hardware encoder on it. Path class `GpuCopy` — see
    /// [`crate::windows::D3d12SharedEncodeBridge`] for the per-frame `CopyResource` contract.
    #[cfg(windows)]
    fn try_gpu_copy(
        config: &AutoVideoEncodeConfig,
        d3d12_device: NativeHandle,
    ) -> Result<Self, EncodeError> {
        let bridge =
            crate::windows::D3d12SharedEncodeBridge::open(d3d12_device, config.width, config.height)?;
        let d3d11_device = bridge.d3d11_device_handle()?;
        let low = config.to_low_level(
            VideoInputPreference::ZeroCopyGpu,
            Some(GpuDeviceHandle::DirectX11(d3d11_device)),
        );
        let inner = WindowsVideoEncoder::open(&low)?;
        Ok(Self {
            path: EncodePathClass::GpuCopy,
            backend: Backend::Os,
            inner: EncoderImpl::Wmf(inner),
            bridge: Some(bridge),
        })
    }

    /// Off-Windows builds have no D3D12→D3D11 bridge; `GpuCopy` is never selected.
    #[cfg(not(windows))]
    const fn try_gpu_copy(
        _config: &AutoVideoEncodeConfig,
        _d3d12_device: NativeHandle,
    ) -> Result<Self, EncodeError> {
        Err(EncodeError::Unsupported)
    }

    /// Path class chosen at open.
    #[must_use]
    pub const fn path_class(&self) -> EncodePathClass {
        self.path
    }

    /// Concrete backend this session resolved to — always a specific variant, never a
    /// stand-in for `Auto`/`AutoHardwareOnly`.
    #[must_use]
    pub const fn resolved_backend(&self) -> Backend {
        self.backend
    }

    /// `ID3D12Resource*` to `CopyResource` into once per frame when
    /// [`EncodePathClass::GpuCopy`] was selected (`None` for any other path).
    ///
    /// Costly path: this is a **named `GpuCopy`**, not Zero-Copy — every frame
    /// pays one GPU→GPU copy from the caller's D3D12 texture into the shared
    /// resource before [`VideoEncoder::push_frame`] (referencing
    /// [`crate::windows::D3d12SharedEncodeBridge::as_dx11_handle`]) can encode it. See
    /// [ADR-0006](../adr/0006-d3d12-shared-to-d3d11.md).
    #[cfg(windows)]
    #[must_use]
    pub fn gpu_copy_target(&self) -> Option<NativeHandle> {
        self.bridge
            .as_ref()
            .and_then(|b| b.d3d12_resource_handle().ok())
    }

    /// Off-Windows builds never select [`EncodePathClass::GpuCopy`] — always `None`.
    #[cfg(not(windows))]
    #[must_use]
    pub const fn gpu_copy_target(&self) -> Option<NativeHandle> {
        None
    }

    /// [`GpuBufferHandle::DirectX11`] to reference in each [`VideoFrame`] pushed
    /// while [`EncodePathClass::GpuCopy`] is active (`None` for any other path).
    /// Pair with [`Self::gpu_copy_target`]: `CopyResource` into the D3D12
    /// resource, then push a frame referencing this DX11 view of the same
    /// shared texture — see [`crate::windows::D3d12SharedEncodeBridge`].
    #[cfg(windows)]
    #[must_use]
    pub fn gpu_copy_dx11_frame_handle(&self) -> Option<GpuBufferHandle> {
        self.bridge.as_ref().and_then(|b| b.as_dx11_handle().ok())
    }

    /// Off-Windows builds never select [`EncodePathClass::GpuCopy`] — always `None`.
    #[cfg(not(windows))]
    #[must_use]
    pub const fn gpu_copy_dx11_frame_handle(&self) -> Option<GpuBufferHandle> {
        None
    }
}

impl VideoEncoder for AutoVideoEncoder {
    fn stream_info(&self) -> &StreamInfo {
        match &self.inner {
            EncoderImpl::Wmf(enc) => enc.stream_info(),
            EncoderImpl::Sw(enc) => enc.stream_info(),
            EncoderImpl::Nvenc(enc) => enc.stream_info(),
            EncoderImpl::QuickSync(enc) => enc.stream_info(),
        }
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        match &mut self.inner {
            EncoderImpl::Wmf(enc) => enc.push_frame(frame),
            EncoderImpl::Sw(enc) => enc.push_frame(frame).map_err(map_av1_error),
            EncoderImpl::Nvenc(enc) => enc.push_frame(frame),
            EncoderImpl::QuickSync(enc) => enc.push_frame(frame),
        }
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        match &mut self.inner {
            EncoderImpl::Wmf(enc) => enc.poll_packet(),
            EncoderImpl::Sw(enc) => enc.poll_packet().map_err(map_av1_error),
            EncoderImpl::Nvenc(enc) => enc.poll_packet(),
            EncoderImpl::QuickSync(enc) => enc.poll_packet(),
        }
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        match &mut self.inner {
            EncoderImpl::Wmf(enc) => enc.flush(),
            EncoderImpl::Sw(enc) => enc.flush().map_err(map_av1_error),
            EncoderImpl::Nvenc(enc) => enc.flush(),
            EncoderImpl::QuickSync(enc) => enc.flush(),
        }
    }
}

/// Probe every [`Backend`] for `codec` — see
/// [`crate::capability`](../../mediaway-encoder/src/capability.rs).
///
/// **Windows:** each row (other than [`Backend::Amf`], which has no implementation at
/// all yet — see `mediaway-encoder-amf` adr/0001) is a **costly, live probe**: it opens
/// a real session at a tiny throwaway resolution and immediately drops it, analogous to
/// `mediaway-device`'s `request_permission`. Call it once to populate a settings list,
/// not per frame.
///
/// **Off Windows:** this crate's real implementation is Windows-only (every backend
/// here is a compile-time `#[cfg(not(windows))]` stub — see the crate doc comment), so
/// every row is [`EncodeUnavailable::NotImplemented`] **without opening anything** —
/// filtered at compile time, not discovered via a failed live probe.
#[must_use]
pub fn support(codec: CodecKind) -> Vec<crate::capability::EncoderCapability> {
    use crate::capability::{EncodeSupport, EncodeUnavailable, EncoderCapability};

    #[cfg(windows)]
    {
        let live = [
            Backend::Os,
            Backend::Nvenc,
            Backend::QuickSync,
            Backend::Software,
        ]
        .map(|backend| {
            let cfg = AutoVideoEncodeConfig {
                backend: BackendSelection::Explicit(backend),
                ..AutoVideoEncodeConfig::new(codec, 64, 64, mediaway_common::Rational::new(1, 30))
            };
            let support = match AutoVideoEncoder::open(&cfg) {
                Ok(enc) => EncodeSupport::Supported(enc.path_class()),
                Err(EncodeError::Backend) => {
                    EncodeSupport::Unavailable(EncodeUnavailable::NoDevice)
                }
                Err(_) => EncodeSupport::Unavailable(EncodeUnavailable::NotImplemented),
            };
            EncoderCapability::new(backend, support)
        });
        live.into_iter()
            .chain([EncoderCapability::new(
                Backend::Amf,
                EncodeSupport::Unavailable(EncodeUnavailable::NotImplemented),
            )])
            .collect()
    }

    #[cfg(not(windows))]
    {
        let _ = codec;
        [
            Backend::Os,
            Backend::Nvenc,
            Backend::QuickSync,
            Backend::Amf,
            Backend::Software,
        ]
        .into_iter()
        .map(|backend| {
            EncoderCapability::new(
                backend,
                EncodeSupport::Unavailable(EncodeUnavailable::NotImplemented),
            )
        })
        .collect()
    }
}

#[cfg(test)]
#[path = "auto_tests.rs"]
mod tests;
