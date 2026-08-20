//! DXGI Desktop Duplication screen capture (DX11 Zero-Copy).

#![allow(unsafe_code)]

use crate::desktop::{
    CaptureOutputPreference, CaptureSharing, DesktopCaptureSource, DesktopVideoCapture,
    DesktopVideoCaptureConfig,
};
use crate::windows_desktop::dxgi_exclusive::ExclusiveDuplication;
use crate::windows_desktop::dxgi_shared::{self, SharedDuplication};
use crate::{CaptureError, DeviceId, DeviceInfo, DeviceKind, Select};
use mediaway_common::{Bytes, CodecKind, GpuDeviceHandle, StreamInfo, VideoFrame, VideoGeometry};
use std::sync::Arc;
use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_NOT_FOUND, DXGI_OUTPUT_DESC, IDXGIAdapter, IDXGIDevice,
    IDXGIFactory1, IDXGIOutput,
};
use windows::core::Interface;

struct Session {
    shared: Arc<SharedDuplication>,
    consumer_id: u64,
    stream_info: StreamInfo,
    next_pts: i64,
}

struct ExclusiveSession {
    inner: ExclusiveDuplication,
    stream_info: StreamInfo,
    next_pts: i64,
}

/// Which mode `WindowsScreenCapture` is backed by — dispatches on
/// [`CaptureSharing`] at `open()` time; never changes for a session's lifetime
/// (see [ADR-0008](../adr/windows/0008-exclusive-desktop-duplication-zero-copy.md)
/// § Alternatives for why there is no live upgrade/downgrade path).
enum Backing {
    /// [`CaptureSharing::Shared`] (default) — served by `dxgi_shared`'s driver
    /// thread + ring; one mandatory `CopyResource`/frame, but joinable by
    /// another `open()` for the same output.
    Shared(Session),
    /// [`CaptureSharing::Exclusive`] — no driver thread, no copy; see
    /// `dxgi_exclusive`.
    Exclusive(ExclusiveSession),
}

/// Windows screen capture via DXGI Desktop Duplication.
///
/// **`CaptureSharing::Shared`** (default, [ADR-0006](../adr/0006-shared-desktop-duplication.md)):
/// every session — including a lone consumer — is served by a shared driver
/// thread and pays one `CopyResource` per frame into its own dedicated
/// texture, in exchange for universal in-process shareability of the same
/// output (DXGI allows only one live duplication per output per process; a
/// second [`WindowsScreenCapture::open`] on the same output succeeds instead
/// of failing with [`CaptureError::AccessDenied`]).
///
/// **`CaptureSharing::Exclusive`** ([ADR-0008](../adr/windows/0008-exclusive-desktop-duplication-zero-copy.md)):
/// true Zero-Copy, no copy at all — opt-in, for a caller that knows it is the
/// only consumer for this output.
pub struct WindowsScreenCapture {
    inner: Option<Backing>,
}

impl WindowsScreenCapture {
    /// Open a DXGI Desktop Duplication session for `config` — `Shared` (joinable, one copy
    /// per frame) or `Exclusive` (true Zero-Copy, opt-in) per `config.sharing`.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Unsupported`] for non-screen sources or CPU output preference.
    /// Returns [`CaptureError::InvalidInput`] when `gpu_device` is unset, or when an existing
    /// shared session for this output was opened against a different `ID3D11Device` instance.
    /// Returns [`CaptureError::AccessDenied`] for `Exclusive` when another duplication (`Shared`
    /// or `Exclusive`) is already live for this output.
    pub fn open(config: &DesktopVideoCaptureConfig) -> Result<Self, CaptureError> {
        let DesktopCaptureSource::Screen { select } = &config.source else {
            return Err(CaptureError::Unsupported);
        };
        if config.output != CaptureOutputPreference::ZeroCopyGpu {
            return Err(CaptureError::Unsupported);
        }
        let Some(GpuDeviceHandle::DirectX11(handle)) = config.gpu_device else {
            return Err(CaptureError::InvalidInput);
        };

        let raw = handle.get() as *mut std::ffi::c_void;
        // SAFETY: caller guarantees `gpu_device` is a live `ID3D11Device*` for the session;
        // only used here, on the calling thread, for read-only enumeration (adapter/output
        // resolution) — never retained. The driver thread (`dxgi_shared`) / `dxgi_exclusive`
        // reconstruct their own owned reference from the same raw pointer independently.
        let device_ref =
            unsafe { ID3D11Device::from_raw_borrowed(&raw) }.ok_or(CaptureError::InvalidInput)?;

        let dxgi_device: IDXGIDevice = device_ref.cast().map_err(|_| CaptureError::Backend)?;
        // SAFETY: GetAdapter is a proven, compiling precedent, read-only query.
        let adapter = unsafe { dxgi_device.GetAdapter() }.map_err(|_| CaptureError::Backend)?;
        let output_index = resolve_output_index(&adapter, select)?;
        let device_raw = handle.get();

        match config.sharing {
            CaptureSharing::Exclusive => {
                let (excl, mut stream_info) = ExclusiveDuplication::open(device_raw, output_index)?;
                if let StreamInfo::Video { time_base, .. } = &mut stream_info {
                    *time_base = config.time_base;
                }
                Ok(Self {
                    inner: Some(Backing::Exclusive(ExclusiveSession {
                        inner: excl,
                        stream_info,
                        next_pts: 0,
                    })),
                })
            }
            CaptureSharing::Shared => {
                let output = enum_output(&adapter, output_index)?;
                // SAFETY: GetDesc reads a fixed-size struct with no retained pointers.
                let desc = unsafe { output.GetDesc() }.map_err(|_| CaptureError::Backend)?;
                let key = DeviceId::from_dxgi_output_device_name(output_device_name(&desc));

                let (shared, consumer_id, mut stream_info) =
                    dxgi_shared::attach(key, device_raw, output_index)?;

                // The shared session's geometry comes from the real DXGI query; the
                // timebase is purely caller config, substituted here rather than
                // threaded through the driver-thread spawn args.
                if let StreamInfo::Video { time_base, .. } = &mut stream_info {
                    *time_base = config.time_base;
                }

                Ok(Self {
                    inner: Some(Backing::Shared(Session {
                        shared,
                        consumer_id,
                        stream_info,
                        next_pts: 0,
                    })),
                })
            }
        }
    }
}

impl DesktopVideoCapture for WindowsScreenCapture {
    fn stream_info(&self) -> &StreamInfo {
        match self.inner.as_ref() {
            Some(Backing::Shared(inner)) => &inner.stream_info,
            Some(Backing::Exclusive(inner)) => &inner.stream_info,
            None => closed_stream_info(),
        }
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, CaptureError> {
        match self.inner.as_mut().ok_or(CaptureError::Closed)? {
            Backing::Shared(inner) => dxgi_shared::poll_shared_frame(
                &inner.shared,
                inner.consumer_id,
                &mut inner.next_pts,
            ),
            Backing::Exclusive(inner) => {
                let geometry = inner.stream_info.geometry().unwrap_or(VideoGeometry {
                    width: 0,
                    height: 0,
                });
                inner.inner.poll_frame(geometry, &mut inner.next_pts)
            }
        }
    }

    fn release_frame(&mut self) -> Result<(), CaptureError> {
        match self.inner.as_mut().ok_or(CaptureError::Closed)? {
            Backing::Shared(inner) => {
                dxgi_shared::release_shared_frame(&inner.shared, inner.consumer_id)
            }
            Backing::Exclusive(inner) => inner.inner.release_frame(),
        }
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        let Some(inner) = self.inner.take() else {
            return Err(CaptureError::Closed);
        };
        match inner {
            Backing::Shared(inner) => dxgi_shared::detach(&inner.shared, inner.consumer_id),
            Backing::Exclusive(_) => {} // Drop (ExclusiveDuplication::drop) releases/closes it.
        }
        Ok(())
    }
}

fn enum_output(adapter: &IDXGIAdapter, index: u32) -> Result<IDXGIOutput, CaptureError> {
    // SAFETY: EnumOutputs is a DXGI adapter query with no retained pointers.
    unsafe { adapter.EnumOutputs(index) }.map_err(|e| {
        if e.code() == DXGI_ERROR_NOT_FOUND {
            CaptureError::InvalidInput
        } else {
            CaptureError::Backend
        }
    })
}

/// Resolve `select` to an output ordinal on `adapter` — **scoped to this one
/// adapter only** (the adapter that owns the caller's `gpu_device`), matching
/// the existing device-vs-adapter-ownership contract (ADR-0005): a
/// [`Select::Id`] naming an output on a *different* adapter is
/// [`CaptureError::InvalidInput`], not a global cross-adapter search.
///
/// # Errors
///
/// Returns [`CaptureError::Unsupported`] when a [`Select::Id`] wraps a
/// non-DXGI-output [`DeviceId`]. Returns [`CaptureError::InvalidInput`] when
/// [`Select::Id`]/[`Select::NameContains`] match no output on `adapter`, or
/// when `adapter` has no outputs at all. Returns [`CaptureError::Backend`] on
/// other DXGI failures.
fn resolve_output_index(adapter: &IDXGIAdapter, select: &Select) -> Result<u32, CaptureError> {
    match select {
        Select::Default => Ok(0),
        Select::Id(id) => {
            let device_name = id
                .as_dxgi_output_device_name()
                .ok_or(CaptureError::Unsupported)?;
            find_output_index(adapter, |desc| output_device_name(desc) == device_name)
        }
        Select::NameContains(needle) => {
            let needle = needle.to_lowercase();
            find_output_index(adapter, |desc| {
                output_device_name(desc).to_lowercase().contains(&needle)
            })
        }
    }
}

fn find_output_index(
    adapter: &IDXGIAdapter,
    mut matches: impl FnMut(&DXGI_OUTPUT_DESC) -> bool,
) -> Result<u32, CaptureError> {
    for index in 0.. {
        let Ok(output) = enum_output(adapter, index) else {
            break;
        };
        // SAFETY: GetDesc reads a fixed-size struct with no retained pointers.
        let Ok(desc) = (unsafe { output.GetDesc() }) else {
            continue;
        };
        if matches(&desc) {
            return Ok(index);
        }
    }
    Err(CaptureError::InvalidInput)
}

/// `DXGI_OUTPUT_DESC.DeviceName` is a fixed-size, nul-terminated wide-char
/// buffer (`[u16; 32]`), not a `PWSTR` — decode up to the first `0`.
fn output_device_name(desc: &DXGI_OUTPUT_DESC) -> String {
    let len = desc
        .DeviceName
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(desc.DeviceName.len());
    String::from_utf16_lossy(&desc.DeviceName[..len])
}

/// Live DXGI output enumeration for `crate::windows::enumeration` (`DeviceKind::Screen`).
///
/// **Global**, across every adapter, unlike [`resolve_output_index`]'s single-adapter
/// scoping at `open()` time (see ADR-0005): a caller resolves a `Select::Id` from here back
/// through `open()`'s adapter-scoped search, which rejects entries from a different adapter
/// than the one backing `gpu_device`.
///
/// `is_default` is `true` only for the first output found overall (ordinal
/// `0`) — the same "0 = primary" convention `DesktopVideoCaptureConfig::screen`
/// already documents, not a real EDID-based primary-monitor query (deferred,
/// see ADR-0005 § Deferred).
///
/// # Errors
///
/// Returns [`CaptureError::Backend`] on DXGI factory/adapter failures.
pub fn enumerate_outputs() -> Result<Vec<DeviceInfo>, CaptureError> {
    // SAFETY: CreateDXGIFactory1 with no output pointers held past this call.
    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory1() }.map_err(|_| CaptureError::Backend)?;

    let mut out = Vec::new();
    let mut ordinal = 0u32;
    for adapter_index in 0.. {
        // SAFETY: EnumAdapters1 out-param is a fresh COM interface pointer.
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(adapter_index) }) else {
            break;
        };
        for output_index in 0.. {
            let Ok(output) = enum_output(&adapter, output_index) else {
                break;
            };
            // SAFETY: GetDesc reads a fixed-size struct with no retained pointers.
            let Ok(desc) = (unsafe { output.GetDesc() }) else {
                continue;
            };
            let name = output_device_name(&desc);
            out.push(DeviceInfo {
                // clone: `name` is also stored in `DeviceInfo::name` below —
                // DXGI's `DeviceName` doubles as both identity and display
                // name (no separate EDID-based friendly name without
                // SetupAPI, deferred per ADR-0005).
                id: DeviceId::from_dxgi_output_device_name(name.clone()),
                kind: DeviceKind::Screen,
                name,
                is_default: ordinal == 0,
                ordinal,
            });
            ordinal += 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "dxgi_tests.rs"]
mod tests;

fn closed_stream_info() -> &'static StreamInfo {
    use mediaway_common::Rational;
    use std::sync::OnceLock;
    static INFO: OnceLock<StreamInfo> = OnceLock::new();
    INFO.get_or_init(|| StreamInfo::Video {
        id: 0,
        codec: CodecKind::RawVideo,
        time_base: Rational::new(1, 30),
        geometry: VideoGeometry {
            width: 0,
            height: 0,
        },
        extra_data: Bytes::new(),
    })
}
