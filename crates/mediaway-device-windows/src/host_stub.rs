//! Non-Windows stub.

use mediaway_device::{
    CaptureError, DeviceEvent, DeviceHotplug, DeviceInfo, DeviceKind, PermissionState, Support,
    Unavailable,
};

/// Windows audio hotplug stub.
pub struct WindowsDeviceHotplug {
    _priv: (),
}

impl WindowsDeviceHotplug {
    /// Unavailable off Windows.
    pub const fn open(_kinds: &[DeviceKind]) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl DeviceHotplug for WindowsDeviceHotplug {
    fn poll_event(&mut self) -> Result<Option<DeviceEvent>, CaptureError> {
        Err(CaptureError::Unsupported)
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

/// No Windows backend is compiled into this binary at all off Windows.
#[must_use]
pub const fn support(_kind: DeviceKind) -> Support {
    Support::Unavailable(Unavailable::NotImplemented)
}

/// No Windows backend is compiled into this binary at all off Windows.
pub const fn request_permission(_kind: DeviceKind) -> Result<PermissionState, CaptureError> {
    Ok(PermissionState::NotSupported)
}

/// No Windows backend is compiled into this binary at all off Windows.
pub const fn enumerate(_kind: DeviceKind) -> Result<Vec<DeviceInfo>, CaptureError> {
    Err(CaptureError::Unsupported)
}
