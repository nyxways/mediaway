//! Exclude a window from screen capture (`WDA_EXCLUDEFROMCAPTURE`).

#![allow(unsafe_code)]

use mediaway_device::CaptureError;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};

/// Mark `hwnd` so Desktop Duplication / WGC omit it (overlay anti-feedback).
///
/// # Errors
///
/// Returns [`CaptureError::InvalidInput`] for a null hwnd, or [`CaptureError::Backend`]
/// when `SetWindowDisplayAffinity` fails.
pub fn exclude_window_from_capture(hwnd: usize) -> Result<(), CaptureError> {
    if hwnd == 0 {
        return Err(CaptureError::InvalidInput);
    }
    // SAFETY: caller owns a live HWND; user32 affinity is a no-op on unsupported OS builds.
    unsafe { SetWindowDisplayAffinity(HWND(hwnd as *mut _), WDA_EXCLUDEFROMCAPTURE) }
        .map_err(|_| CaptureError::Backend)
}
