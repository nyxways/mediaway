//! C ABI status codes (`mediaway_device_status_t`).
//!
//! Fresh, distinctly-named type — not shared or numerically mirrored with
//! `mediaway-container-ffi`'s `MediawayStatus` or `mediaway-pipeline-ffi`'s
//! `MediawayPipelineStatus`. See `adr/0001-capture-c-abi.md` §3 for why.

use mediaway_device::CaptureError;

/// C ABI status code returned by fallible `mediaway-device-ffi` functions.
///
/// `InvalidArgument`/`HandlePoisoned`/`InternalPanic` are FFI-layer inventions.
/// Everything else maps onto [`CaptureError`] (`#[non_exhaustive]`, hence
/// [`Self::UnknownError`] as a catch-all). See `adr/0001-capture-c-abi.md` §3.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayDeviceStatus {
    /// Success.
    Ok = 0,
    /// Null pointer, or mismatched pointer/length pair.
    InvalidArgument = 1,
    /// A previous call already poisoned this handle; the call was refused.
    HandlePoisoned = 2,
    /// [`CaptureError::Unsupported`] — includes Screen/Window in this pass (a real
    /// Rust capability with no C ABI path yet, not merely "not implemented").
    Unsupported = 3,
    /// [`CaptureError::NoBackend`] — expected/graceful (no backend compiled in).
    NoBackend = 4,
    /// [`CaptureError::InvalidInput`].
    InvalidInput = 5,
    /// [`CaptureError::Backend`].
    BackendFailure = 6,
    /// [`CaptureError::Closed`].
    Closed = 7,
    /// [`CaptureError::AccessDenied`].
    AccessDenied = 8,
    /// `CaptureError` is `#[non_exhaustive]`; catch-all for a future variant.
    UnknownError = 9,
    /// This call caught a Rust panic; the handle is now poisoned.
    InternalPanic = 10,
    /// `mediaway_device_hotplug_register_callback` called on a handle that already has
    /// an active callback (`adr/0002-callback-event-delivery.md` §4) — call
    /// `mediaway_device_hotplug_unregister_callback` first to replace it.
    CallbackAlreadyRegistered = 11,
    /// `mediaway_device_hotplug_poll_event` called while a callback is registered on
    /// this handle (`adr/0002-callback-event-delivery.md` §4) — drains nothing; poll
    /// and callback delivery are mutually exclusive per handle.
    CallbackModeActive = 12,
    /// [`CaptureError::Timeout`] — `mediaway_camera_capture_poll_frame_blocking` /
    /// `mediaway_camera_capture_capture_once` /
    /// `mediaway_desktop_capture_poll_frame_blocking`'s deadline elapsed with no frame
    /// (`adr/0003-gpu-handle-c-abi.md` §6). Not necessarily a failure on an
    /// already-open, delta-based session — see those functions' docs.
    Timeout = 13,
}

impl From<CaptureError> for MediawayDeviceStatus {
    fn from(err: CaptureError) -> Self {
        match err {
            CaptureError::Unsupported => Self::Unsupported,
            CaptureError::NoBackend => Self::NoBackend,
            CaptureError::InvalidInput => Self::InvalidInput,
            CaptureError::Backend => Self::BackendFailure,
            CaptureError::Closed => Self::Closed,
            CaptureError::AccessDenied => Self::AccessDenied,
            CaptureError::Timeout => Self::Timeout,
            _ => Self::UnknownError,
        }
    }
}
