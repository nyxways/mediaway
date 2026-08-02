//! Capability / permission probing for Windows capture backends. See
//! [`mediaway-device` ADR-0003](../../mediaway-device/adr/0003-capability-and-permission-probe.md).
//!
//! [`support`] performs **live** checks on the running machine, not just
//! "was a Windows backend compiled in": WGC asks the real `WinRT` contract
//! query, screen/microphone/loopback enumerate real DXGI outputs / WASAPI
//! endpoints. `Camera` is the only kind still classified at the "no code
//! exists" level, since no backend implements it yet.

#![allow(unsafe_code)]

use mediaway_common::Rational;
use mediaway_device::{CaptureError, DeviceKind, PermissionState, Support, Unavailable};
use mediaway_device_audio::AudioCaptureConfig;
use windows::Graphics::Capture::GraphicsCaptureSession;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, EDataFlow, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};

use mediaway_device_windows_audio::{ComGuard, WindowsWasapiCapture, open_process_loopback_client};

/// Live support probe for `kind` on this machine (see module docs).
#[must_use]
pub fn support(kind: DeviceKind) -> Support {
    match kind {
        DeviceKind::Window => window_capture_support(),
        DeviceKind::Screen => screen_output_support(),
        DeviceKind::Microphone => endpoint_support(eCapture),
        DeviceKind::Loopback => endpoint_support(eRender),
        DeviceKind::ProcessLoopback => process_loopback_support(),
        // Covers `Camera` (no backend yet) and any future `DeviceKind` variant
        // (it's `#[non_exhaustive]`) until this match gets a real probe for it.
        _ => Support::Unavailable(Unavailable::NotImplemented),
    }
}

/// `Windows.Graphics.Capture.GraphicsCaptureSession.IsSupported` — the real,
/// documented `WinRT` contract-presence query (Windows 10 1803+); zero cost,
/// no capture session created. The same check `wgc.rs::open` performs inline.
fn window_capture_support() -> Support {
    if GraphicsCaptureSession::IsSupported().unwrap_or(false) {
        Support::Supported
    } else {
        Support::Unavailable(Unavailable::OsVersionTooOld)
    }
}

/// Enumerates DXGI adapters/outputs (no `ID3D11Device`, no
/// `IDXGIOutput1::DuplicateOutput` — cheaper than a real
/// [`crate::WindowsScreenCapture::open`]) to answer "is there a display
/// output on this machine at all" (e.g. a headless VM/server has none).
fn screen_output_support() -> Support {
    // SAFETY: CreateDXGIFactory1 with no output pointers held past this call.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Support::Unavailable(Unavailable::NoDeviceFound);
    };
    let mut index = 0u32;
    loop {
        // SAFETY: EnumAdapters1 out-param is a fresh COM interface pointer.
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
            return Support::Unavailable(Unavailable::NoDeviceFound);
        };
        // SAFETY: EnumOutputs(0) checks for at least one output on this adapter.
        if unsafe { adapter.EnumOutputs(0) }.is_ok() {
            return Support::Supported;
        }
        index += 1;
    }
}

/// Enumerates active `WASAPI` endpoints for `data_flow` (`eCapture` /
/// `eRender`) — cheaper than opening a full [`WindowsWasapiCapture`] (no
/// `IAudioClient::Initialize`, no worker thread).
fn endpoint_support(data_flow: EDataFlow) -> Support {
    // SAFETY: COM init for this thread; `_com` runs CoUninitialize on drop.
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        return Support::Unavailable(Unavailable::NoDeviceFound);
    }
    let _com = ComGuard;
    let result = (|| -> windows_core::Result<u32> {
        // SAFETY: standard in-proc COM activation.
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER) }?;
        // SAFETY: EnumAudioEndpoints borrows nothing past this call.
        let endpoints = unsafe { enumerator.EnumAudioEndpoints(data_flow, DEVICE_STATE_ACTIVE) }?;
        // SAFETY: GetCount is a plain out-param read.
        unsafe { endpoints.GetCount() }
    })();
    match result {
        Ok(count) if count > 0 => Support::Supported,
        _ => Support::Unavailable(Unavailable::NoDeviceFound),
    }
}

/// Real activation attempt against this process (`ActivateAudioInterfaceAsync`
/// with the process-loopback virtual device), immediately torn down. There is
/// no cheaper live signal for "does this Windows build support per-process
/// loopback" — the OS exposes no version-contract query for it (unlike WGC's
/// `IsSupported`), so a failure here is classified as [`Unavailable::OsVersionTooOld`]
/// (the overwhelmingly common cause: pre-Windows-10-2004) even though a
/// transient audio-service failure could in principle also produce it.
fn process_loopback_support() -> Support {
    // SAFETY: COM init for this thread; `_com` runs CoUninitialize on drop.
    // `ActivateAudioInterfaceAsync` (inside `open_process_loopback_client`)
    // requires a COM-initialized calling thread, same as `run_wasapi_worker`.
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        return Support::Unavailable(Unavailable::OsVersionTooOld);
    }
    let _com = ComGuard;
    match open_process_loopback_client(std::process::id(), false) {
        Ok((client, _capture, _rate, _channels)) => {
            // SAFETY: Stop mirrors the Start already issued by activation.
            let _ = unsafe { client.Stop() };
            Support::Supported
        }
        Err(_) => Support::Unavailable(Unavailable::OsVersionTooOld),
    }
}

/// Best-effort permission probe for `kind`.
///
/// # Cost
///
/// For [`DeviceKind::Microphone`] this **opens a real `WASAPI` capture
/// endpoint and spawns the capture worker thread** (the same path as
/// [`WindowsWasapiCapture::open`]) purely to observe whether the OS grants
/// access, then closes it — there is no separate, cheap "ask the OS" call for
/// a Win32 desktop app (unlike a portal-mediated consent dialog on Linux).
/// Callers must not call this per frame; cache the result, and call
/// [`support`] first — probing permission for a kind with no device present is
/// wasted work (this function does that check internally too).
///
/// [`DeviceKind::Screen`]/[`DeviceKind::Window`] have no separate per-app
/// consent step beyond normal desktop/window access, but a real probe would
/// need a live GPU device (screen) or target `HWND` (window) that this
/// session-free call does not have — reported as [`PermissionState::Unknown`]
/// rather than guessed.
///
/// # Errors
///
/// Returns the underlying [`CaptureError`] when the microphone probe itself
/// fails for a reason other than access denial or absent device.
pub fn request_permission(kind: DeviceKind) -> Result<PermissionState, CaptureError> {
    if matches!(support(kind), Support::Unavailable(_)) {
        return Ok(PermissionState::NotSupported);
    }
    match kind {
        // Render-endpoint loopback captures system audio; Windows does not
        // gate it behind Settings > Privacy > Microphone.
        DeviceKind::Loopback | DeviceKind::ProcessLoopback => Ok(PermissionState::Granted),
        DeviceKind::Screen | DeviceKind::Window => Ok(PermissionState::Unknown),
        DeviceKind::Microphone => probe_microphone(),
        // Covers `Camera` (no backend yet) and any future `DeviceKind` variant.
        _ => Ok(PermissionState::NotSupported),
    }
}

fn probe_microphone() -> Result<PermissionState, CaptureError> {
    let cfg = AudioCaptureConfig::microphone(Rational::new(1, 48_000));
    match WindowsWasapiCapture::open_microphone(&cfg) {
        Ok(mut cap) => {
            let _ = cap.close();
            Ok(PermissionState::Granted)
        }
        Err(CaptureError::AccessDenied) => Ok(PermissionState::Denied),
        Err(CaptureError::Unsupported | CaptureError::InvalidInput) => Ok(PermissionState::Unknown),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
