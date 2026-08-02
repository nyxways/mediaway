//! C ABI facade over `mediaway-device-camera`/`-desktop`/`-audio` (Camera video
//! capture; Screen/Window video + Loopback/`ProcessLoopback` audio capture;
//! Microphone audio capture) plus device hotplug.
//!
//! Design: `adr/0001-capture-c-abi.md` — opaque handles (one per domain, each a trait
//! object plus a `poisoned` flag), a `mediaway_device_status_t`, `catch_unwind` panic
//! safety, and a hand-written header (`include/mediaway/device.h`).
//! `adr/0002-callback-event-delivery.md` adds `HotplugHandle` — poll + opt-in
//! callback-registration dual mode over `mediaway_device::DeviceHotplug`.
//! `adr/0003-gpu-handle-c-abi.md` adds a shared `mediaway_gpu_device_handle_t`/
//! `mediaway_gpu_buffer_handle_t` representation (`mediaway-common-ffi`), wires real
//! **Screen** dispatch, and adds poll-blocking/capture-once entry points.
//! `adr/0004-domain-feature-split.md` splits this crate's four Cargo features
//! (`camera`/`desktop`/`audio`/`hotplug`) so each maps to its own C module and, on
//! Windows, its own backend crate dependency — disabling a feature genuinely removes
//! that domain's code from the build, not merely its public entry points. `desktop`
//! covers Screen/Window video **and** Loopback/`ProcessLoopback` audio (both capture
//! what the desktop is already doing, not a real input device — same grouping as the
//! Rust facade split, `mediaway-device/adr/0007-domain-crate-split.md`); `audio` is
//! Microphone only.
//!
//! Third `mediaway-*-ffi` crate in the workspace, after `mediaway-container-ffi` and
//! `mediaway-pipeline-ffi`
//! ([`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md)). Depends
//! directly on the per-domain facade/backend crates, **not** `mediaway-pipeline` or the
//! `mediaway-device-windows` orchestrator (`adr/0001-capture-c-abi.md` §1,
//! `adr/0004-domain-feature-split.md`) — avoids pulling in every domain's backend for a
//! caller that only wants one. The `hotplug` feature is the one exception: v1 hotplug
//! scope spans Audio I/O (Microphone) and Desktop (Loopback) kinds, and its real
//! backend (`WindowsDeviceHotplug`) lives in the orchestrator crate for that reason
//! (`mediaway-device-windows`'s own module doc) — enabling `hotplug` on Windows
//! transitively links all three domain backends, a deliberate, documented trade-off.
//!
//! v1 ships **Camera** (video, CPU-only), **Screen** (video, GPU-only, Windows —
//! `adr/0003-gpu-handle-c-abi.md`), **Microphone** (audio), and **Loopback /
//! `ProcessLoopback`** (desktop audio) — all real, hardware-verified Windows backends.
//! **Window capture is deferred** (needs a native `HWND` C input shape,
//! `adr/0001-capture-c-abi.md` § Deferred) — a Window-kind video config always opens to
//! [`MediawayDeviceStatus::Unsupported`].

#![allow(unsafe_code)] // FFI crate — see docs/conventions/code-style.md § unsafe

mod buffer;
mod status;
mod types;

#[cfg(feature = "audio")]
mod audio;
#[cfg(feature = "camera")]
mod camera;
#[cfg(feature = "desktop")]
mod desktop_audio;
#[cfg(feature = "desktop")]
mod desktop_video;
#[cfg(feature = "hotplug")]
mod hotplug;

pub use status::MediawayDeviceStatus;
pub use types::MediawayRational;

#[cfg(any(feature = "camera", feature = "desktop"))]
pub use types::MediawayPixelFormat;
#[cfg(any(feature = "audio", feature = "desktop"))]
pub use types::MediawaySampleFormat;
#[cfg(feature = "desktop")]
pub use types::{
    MediawayGpuBufferHandle, MediawayGpuBufferKind, MediawayGpuDeviceHandle, MediawayGpuDeviceKind,
};

#[cfg(feature = "audio")]
pub use types::{MediawayAudioCaptureConfig, MediawayDeviceAudioFrame};
#[cfg(feature = "camera")]
pub use types::{MediawayCameraCaptureConfig, MediawayCameraFrame};
#[cfg(feature = "desktop")]
pub use types::{
    MediawayDesktopAudioCaptureConfig, MediawayDesktopAudioFrame, MediawayDesktopAudioSourceKind,
    MediawayDesktopCaptureConfig, MediawayDesktopCaptureSourceKind, MediawayDesktopFrame,
    MediawayVideoFrameStorageKind,
};
#[cfg(feature = "hotplug")]
pub use types::{MediawayDeviceEvent, MediawayDeviceEventKind, MediawayDeviceKind};

#[cfg(feature = "audio")]
pub use audio::{
    AudioCaptureHandle, mediaway_audio_capture_close, mediaway_audio_capture_config_microphone,
    mediaway_audio_capture_format, mediaway_audio_capture_open, mediaway_audio_capture_poll_frame,
    mediaway_audio_frame_free,
};
#[cfg(feature = "camera")]
pub use camera::{
    CameraCaptureHandle, mediaway_camera_capture_capture_once, mediaway_camera_capture_close,
    mediaway_camera_capture_config_default, mediaway_camera_capture_geometry,
    mediaway_camera_capture_open, mediaway_camera_capture_poll_frame,
    mediaway_camera_capture_poll_frame_blocking, mediaway_camera_capture_release_frame,
    mediaway_camera_frame_free,
};
#[cfg(feature = "desktop")]
pub use desktop_audio::{
    DesktopAudioCaptureHandle, mediaway_desktop_audio_capture_close,
    mediaway_desktop_audio_capture_config_loopback,
    mediaway_desktop_audio_capture_config_process_loopback, mediaway_desktop_audio_capture_format,
    mediaway_desktop_audio_capture_open, mediaway_desktop_audio_capture_poll_frame,
    mediaway_desktop_audio_frame_free,
};
#[cfg(feature = "desktop")]
pub use desktop_video::{
    DesktopCaptureHandle, mediaway_desktop_capture_close, mediaway_desktop_capture_config_screen,
    mediaway_desktop_capture_geometry, mediaway_desktop_capture_open,
    mediaway_desktop_capture_poll_frame, mediaway_desktop_capture_poll_frame_blocking,
    mediaway_desktop_capture_release_frame, mediaway_desktop_frame_free,
};
#[cfg(feature = "hotplug")]
pub use hotplug::{
    HotplugHandle, MediawayDeviceHotplugCallbackFn, mediaway_device_hotplug_close,
    mediaway_device_hotplug_event_free, mediaway_device_hotplug_open,
    mediaway_device_hotplug_poll_event, mediaway_device_hotplug_register_callback,
    mediaway_device_hotplug_unregister_callback,
};

/// Runtime ABI version, matching `MEDIAWAY_DEVICE_FFI_ABI_VERSION` in
/// `include/mediaway/device.h`.
///
/// A dynamically-loaded consumer (Python/Node/Go/...) that never compiles against the
/// header can call this to assert the loaded library matches what it was built against.
#[unsafe(no_mangle)]
pub const extern "C" fn mediaway_device_ffi_abi_version() -> u32 {
    1
}
