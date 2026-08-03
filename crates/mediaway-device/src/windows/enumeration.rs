//! `enumerate(kind)` — live device enumeration bodies for this backend. See
//! [`mediaway-device` ADR-0005](../../mediaway-device/adr/0005-device-selection.md).
//!
//! Free-function shape, matching `capabilities.rs`'s `support`/
//! `request_permission` precedent exactly (ADR-0003) — no stateful
//! `Devices` type.

#![allow(unsafe_code)]

use crate::{CaptureError, DeviceId, DeviceInfo, DeviceKind};
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, EDataFlow, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole,
    eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};

use crate::windows_audio::{ComGuard, endpoint_friendly_name, endpoint_id};
use crate::windows_camera as camera;
use crate::windows_desktop as dxgi;

/// Live device enumeration for `kind` on this machine.
///
/// # Errors
///
/// Returns [`CaptureError::Unsupported`] for [`DeviceKind::ProcessLoopback`]
/// (PID-parameterized at open time, not an OS device list — never an empty
/// `Vec`, which would falsely read as "nothing is producing audio") and for
/// [`DeviceKind::Window`] (not a persistent device — out of ADR-0005's
/// enumeration scope entirely) and any other [`DeviceKind`] this backend has
/// no enumeration for yet (`#[non_exhaustive]`). Returns
/// [`CaptureError::Backend`] on COM/API failures.
pub fn enumerate(kind: DeviceKind) -> Result<Vec<DeviceInfo>, CaptureError> {
    match kind {
        DeviceKind::Microphone => enumerate_audio_endpoints(eCapture, DeviceKind::Microphone),
        DeviceKind::Loopback => enumerate_audio_endpoints(eRender, DeviceKind::Loopback),
        DeviceKind::Camera => camera::enumerate_cameras(),
        DeviceKind::Screen => dxgi::enumerate_outputs(),
        // `ProcessLoopback`/`Window` are out of ADR-0005's enumeration scope
        // (see module docs); any future `DeviceKind` falls here too.
        _ => Err(CaptureError::Unsupported),
    }
}

/// `is_default` compares each candidate's endpoint ID against
/// `GetDefaultAudioEndpoint(data_flow, eConsole)`'s own ID — real and cheap,
/// per ADR-0005's `is_default` table for Microphone/Loopback.
fn enumerate_audio_endpoints(
    data_flow: EDataFlow,
    kind: DeviceKind,
) -> Result<Vec<DeviceInfo>, CaptureError> {
    // SAFETY: COM init for this call.
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        return Err(CaptureError::Backend);
    }
    let _com = ComGuard;

    // SAFETY: standard in-proc COM activation.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER) }
            .map_err(|_| CaptureError::Backend)?;

    // SAFETY: GetDefaultAudioEndpoint may legitimately fail (no default
    // device for this data flow, e.g. a capture-only or render-only box) —
    // treated as "no default", not an error for the whole enumeration.
    let default_id = unsafe { enumerator.GetDefaultAudioEndpoint(data_flow, eConsole) }
        .ok()
        .and_then(|device| endpoint_id(&device));

    // SAFETY: EnumAudioEndpoints borrows nothing past this call.
    let collection = unsafe { enumerator.EnumAudioEndpoints(data_flow, DEVICE_STATE_ACTIVE) }
        .map_err(|_| CaptureError::Backend)?;
    // SAFETY: GetCount is a plain out-param read.
    let count = unsafe { collection.GetCount() }.map_err(|_| CaptureError::Backend)?;

    let mut out = Vec::with_capacity(count as usize);
    for ordinal in 0..count {
        // SAFETY: `ordinal` is in `0..count` from `GetCount` above.
        let Ok(device) = (unsafe { collection.Item(ordinal) }) else {
            continue;
        };
        let Some(id) = endpoint_id(&device) else {
            continue;
        };
        let name = endpoint_friendly_name(&device).unwrap_or_default();
        let is_default = default_id.as_deref() == Some(id.as_str());
        out.push(DeviceInfo {
            id: DeviceId::from_wasapi_endpoint_id(id),
            kind,
            name,
            is_default,
            ordinal,
        });
    }
    Ok(out)
}

#[cfg(test)]
#[path = "enumeration_tests.rs"]
mod tests;
