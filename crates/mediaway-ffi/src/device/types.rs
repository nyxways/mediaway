//! C ABI struct/enum types mirroring `mediaway_common`/`mediaway_device*` types.
//!
//! Field layouts and ownership are decided in `adr/0001-capture-c-abi.md` §5/§6/§7,
//! amended by `adr/0004-domain-feature-split.md` for the per-domain type split below.
//!
//! `MediawayRational` is re-exported from `mediaway-common-ffi` rather than defined
//! locally (`docs/adr/0015-common-ffi-unification.md`) — confirmed field-identical to
//! `mediaway-container-ffi`'s/`mediaway-pipeline-ffi`'s independent copies before this
//! migration. `MediawayPixelFormat` stays a local copy of `mediaway-pipeline-ffi`'s:
//! `PixelFormat` mirroring is out of ADR-0015's decided scope (Rational/CodecKind only)
//! — a natural follow-up, not done here. `MediawaySampleFormat` and the
//! frame-direction-specific structs below are new, crate-scoped types — see
//! `adr/0001-capture-c-abi.md` §7 for why they are *not* reused/shared.
//!
//! **Per-domain feature gating** (`adr/0004-domain-feature-split.md`): types used only
//! by one Cargo feature's C functions are `#[cfg]`-gated to that feature, so disabling a
//! feature genuinely removes the domain's code (not just its entry points) from the
//! build — mirroring the Rust-side `mediaway-device-camera`/`-desktop`/`-audio` crate
//! split. Cross-domain types (`MediawayRational`, [`MediawayDeviceStatus`] in
//! `status.rs`) stay ungated for a stable, feature-independent ABI numbering.
//!
//! Video capture is split by domain, not shared: [`MediawayCameraCaptureConfig`]/
//! [`MediawayCameraFrame`] (`camera` feature) are CPU-only and carry no `gpu_device`
//! field at all — every shipped Camera backend rejects Zero-Copy today (see
//! `mediaway-device-camera`), so the field would be permanently dead weight on this
//! type, unlike [`MediawayDesktopCaptureConfig`]/[`MediawayDesktopFrame`] (`desktop`
//! feature), which stay GPU-capable (Screen's real Zero-Copy path,
//! `adr/0003-gpu-handle-c-abi.md`). Audio capture splits the same way:
//! [`MediawayAudioCaptureConfig`] (`audio` feature) is Microphone-only;
//! [`MediawayDesktopAudioCaptureConfig`] (`desktop` feature) covers Loopback/
//! `ProcessLoopback` — grouped with Desktop, not Audio I/O, because both capture *what
//! the desktop is already rendering*, not a real input device (same reasoning as the
//! Rust facade split, `mediaway-device/adr/0007-domain-crate-split.md`).
//!
//! `MediawayDeviceKind`/`MediawayDeviceEventKind`/`MediawayDeviceEvent` (hotplug) are
//! per `adr/0002-callback-event-delivery.md` §6 — `MediawayDeviceKind` is this crate's
//! only full [`DeviceKind`] mirror (the capture config structs above use
//! capability-narrowed ordinals/discriminants instead, not this general enum).

#[cfg(feature = "hotplug")]
use mediaway_device::DeviceKind;

/// Rational timebase (`num / den`, seconds) — see `mediaway-common-ffi::types::Rational`.
pub use crate::common::types::Rational as MediawayRational;

// ── GPU handles (Desktop/Screen Zero-Copy only) ────────────────────────────────

/// Polled GPU frame storage (borrowed) — see `mediaway-common-ffi::gpu::GpuBufferHandle`
/// and `adr/0003-gpu-handle-c-abi.md` §3/§8.
#[cfg(feature = "desktop")]
pub use crate::common::gpu::GpuBufferHandle as MediawayGpuBufferHandle;
/// GPU buffer/texture handle discriminant — see `mediaway-common-ffi::gpu::GpuBufferKind`.
#[cfg(feature = "desktop")]
pub use crate::common::gpu::GpuBufferKind as MediawayGpuBufferKind;
/// Caller-supplied GPU device handle (Screen capture's `gpu_device`) — see
/// `mediaway-common-ffi::gpu::GpuDeviceHandle` and `adr/0003-gpu-handle-c-abi.md` §1/§2.
#[cfg(feature = "desktop")]
pub use crate::common::gpu::GpuDeviceHandle as MediawayGpuDeviceHandle;
/// GPU device handle discriminant — see `mediaway-common-ffi::gpu::GpuDeviceKind`.
#[cfg(feature = "desktop")]
pub use crate::common::gpu::GpuDeviceKind as MediawayGpuDeviceKind;

// ── Video (Camera + Desktop) ────────────────────────────────────────────────────

/// Pixel layout — mirrors `mediaway_common::PixelFormat`'s 5 variants.
///
/// Reused verbatim from `mediaway-pipeline-ffi`'s `MediawayPixelFormat` — both wrap the
/// identical shared `PixelFormat` (`adr/0001-capture-c-abi.md` §7). Only `Nv12`/`Bgra8`
/// are exercised by the current Windows Camera backend today (an existing Rust-level
/// limitation, not a new FFI one).
#[cfg(any(feature = "camera", feature = "desktop"))]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayPixelFormat {
    /// 8-bit NV12 (YUV 4:2:0 semi-planar) — common HW encode input.
    Nv12 = 0,
    /// 8-bit I420 / YUV420P.
    I420 = 1,
    /// 8-bit BGRA packed.
    Bgra8 = 2,
    /// 8-bit RGBA packed.
    Rgba8 = 3,
    /// 8-bit YUYV / YUY2 packed (YUV 4:2:2).
    Yuyv = 4,
}

#[cfg(any(feature = "camera", feature = "desktop"))]
impl From<MediawayPixelFormat> for mediaway_common::PixelFormat {
    fn from(format: MediawayPixelFormat) -> Self {
        match format {
            MediawayPixelFormat::Nv12 => Self::Nv12,
            MediawayPixelFormat::I420 => Self::I420,
            MediawayPixelFormat::Bgra8 => Self::Bgra8,
            MediawayPixelFormat::Rgba8 => Self::Rgba8,
            MediawayPixelFormat::Yuyv => Self::Yuyv,
        }
    }
}

#[cfg(any(feature = "camera", feature = "desktop"))]
impl From<mediaway_common::PixelFormat> for MediawayPixelFormat {
    // `PixelFormat` is `#[non_exhaustive]`; all variants that exist today are matched
    // by name below. No "unknown" C variant exists to fall back to, so a future
    // variant maps to the safest default (NV12) — that overlap with the `Nv12` arm's
    // own body is intentional, not a copy-paste bug.
    #[allow(clippy::match_same_arms)]
    fn from(format: mediaway_common::PixelFormat) -> Self {
        use mediaway_common::PixelFormat;
        match format {
            PixelFormat::Nv12 => Self::Nv12,
            PixelFormat::I420 => Self::I420,
            PixelFormat::Bgra8 => Self::Bgra8,
            PixelFormat::Rgba8 => Self::Rgba8,
            PixelFormat::Yuyv => Self::Yuyv,
            _ => Self::Nv12,
        }
    }
}

/// Config for [`crate::device::camera::mediaway_camera_capture_open`] — plain value struct, no
/// handle, no heap allocation, no free function.
///
/// No `gpu_device` field: every shipped Camera backend rejects Zero-Copy today (always
/// `CpuFramesOk` internally) — see `mediaway-device-camera`. Unlike the pre-split
/// `MediawayVideoCaptureConfig`, this is not a "meaningless for this source" field kept
/// for a shared C struct; it simply does not exist here.
#[cfg(feature = "camera")]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediawayCameraCaptureConfig {
    /// Device ordinal (`0` = default).
    pub device_index: u32,
    /// Timestamp timebase for polled frames.
    pub time_base: MediawayRational,
}

/// Output of [`crate::device::camera::mediaway_camera_capture_poll_frame`] — owned; release
/// with [`crate::device::camera::mediaway_camera_frame_free`].
///
/// CPU-only (no `storage_kind`/`gpu_buffer` fields, unlike
/// [`MediawayDesktopFrame`]) — matches [`MediawayCameraCaptureConfig`] dropping
/// `gpu_device` for the same reason. Does not derive `Copy`/`Clone`: owns a raw
/// pointer, and duplicating the struct would invite a double-free (same reasoning as
/// `mediaway-container-ffi`'s `MediawayPacket`).
#[cfg(feature = "camera")]
#[repr(C)]
#[derive(Debug)]
pub struct MediawayCameraFrame {
    /// Presentation timestamp in the stream timebase.
    pub pts: i64,
    /// Duration in timebase units (`0` if unknown).
    pub duration: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel layout.
    pub pixel_format: MediawayPixelFormat,
    /// Owned plane bytes. `NULL` after [`crate::device::camera::mediaway_camera_frame_free`].
    pub data: *mut u8,
    /// Length of `data` in bytes.
    pub data_len: usize,
}

/// Desktop video capture source selection — mirrors
/// `mediaway_device::desktop::DesktopCaptureSource`'s two variants.
///
/// Only `Screen` is reachable via [`crate::device::desktop_video::mediaway_desktop_capture_open`]
/// in this pass; `Window` deterministically returns
/// [`crate::device::MediawayDeviceStatus::Unsupported`] (`adr/0001-capture-c-abi.md` § Finding 2,
/// § Deferred) — kept so the full real source enum stays representable in C.
#[cfg(feature = "desktop")]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayDesktopCaptureSourceKind {
    /// DXGI Desktop Duplication / display output — supported this pass.
    Screen = 0,
    /// Window capture — no C constructor exposed this pass either (needs an `HWND`
    /// input shape not designed here).
    Window = 1,
}

/// Config for [`crate::device::desktop_video::mediaway_desktop_capture_open`] — plain value
/// struct, no handle, no heap allocation, no free function.
///
/// `gpu_device` is mandatory for `Screen` (`adr/0003-gpu-handle-c-abi.md` §4) —
/// [`crate::device::desktop_video::mediaway_desktop_capture_open`] rejects a
/// `NONE`/malformed one with `INVALID_INPUT` rather than silently ignoring it.
#[cfg(feature = "desktop")]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediawayDesktopCaptureConfig {
    /// What to capture.
    pub source_kind: MediawayDesktopCaptureSourceKind,
    /// Output ordinal (`0` = primary/default).
    pub source_index: u32,
    /// Timestamp timebase for polled frames.
    pub time_base: MediawayRational,
    /// GPU device backing this session.
    pub gpu_device: MediawayGpuDeviceHandle,
}

/// Which of [`MediawayDesktopFrame`]'s two storage fields is valid.
///
/// `adr/0003-gpu-handle-c-abi.md` §3 — added instead of a second poll function or a C
/// union, mirroring this crate's existing "kind field decides which fields matter"
/// idiom.
#[cfg(feature = "desktop")]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayVideoFrameStorageKind {
    /// `data`/`data_len` are valid; `gpu_buffer` is unused/zeroed.
    Cpu = 0,
    /// `gpu_buffer` is valid; `data` is `NULL`, `data_len` is `0`.
    Gpu = 1,
}

/// Output of [`crate::device::desktop_video::mediaway_desktop_capture_poll_frame`] — release
/// with [`crate::device::desktop_video::mediaway_desktop_frame_free`].
///
/// Distinct name and ownership direction from `mediaway-pipeline-ffi`'s
/// `mediaway_video_frame_t` (a **borrowed input** there; this is an **owned output**
/// here — `adr/0001-capture-c-abi.md` §7). Does not derive `Copy`/`Clone`: a CPU frame
/// owns a raw pointer, and duplicating the struct would invite a double-free — holds
/// even for a GPU frame, to keep one `Copy`-ability rule for the whole type rather than
/// one that depends on `storage_kind` at runtime.
///
/// `gpu_buffer` (`adr/0003-gpu-handle-c-abi.md` §3) is a **borrowed** handle: it aliases
/// the capture session's own GPU resource and must never be freed by the caller — see
/// that ADR's §8 for the full COM-refcount / read-window hazard documentation this
/// crate's header carries.
#[cfg(feature = "desktop")]
#[repr(C)]
#[derive(Debug)]
pub struct MediawayDesktopFrame {
    /// Presentation timestamp in the stream timebase.
    pub pts: i64,
    /// Duration in timebase units (`0` if unknown).
    pub duration: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel layout.
    pub pixel_format: MediawayPixelFormat,
    /// Which of `data`/`gpu_buffer` is valid.
    pub storage_kind: MediawayVideoFrameStorageKind,
    /// Owned plane bytes (CPU only). `NULL` after
    /// [`crate::device::desktop_video::mediaway_desktop_frame_free`], and whenever
    /// `storage_kind == Gpu`.
    pub data: *mut u8,
    /// Length of `data` in bytes (CPU only); `0` whenever `storage_kind == Gpu`.
    pub data_len: usize,
    /// Borrowed GPU texture handle (GPU only); zeroed whenever `storage_kind == Cpu`.
    pub gpu_buffer: MediawayGpuBufferHandle,
}

// ── Audio I/O (Microphone) ──────────────────────────────────────────────────────

/// Audio PCM sample layout — mirrors `mediaway_common::SampleFormat`'s 3 variants.
///
/// First definition of this enum in the workspace's C headers — no mirroring
/// precedent to reconcile against (`adr/0001-capture-c-abi.md` §5).
#[cfg(any(feature = "audio", feature = "desktop"))]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawaySampleFormat {
    /// Signed 16-bit little-endian interleaved PCM.
    S16 = 0,
    /// Signed 32-bit little-endian interleaved PCM.
    S32 = 1,
    /// IEEE float32 interleaved PCM.
    F32 = 2,
}

#[cfg(any(feature = "audio", feature = "desktop"))]
impl From<MediawaySampleFormat> for mediaway_common::SampleFormat {
    fn from(format: MediawaySampleFormat) -> Self {
        match format {
            MediawaySampleFormat::S16 => Self::S16,
            MediawaySampleFormat::S32 => Self::S32,
            MediawaySampleFormat::F32 => Self::F32,
        }
    }
}

#[cfg(any(feature = "audio", feature = "desktop"))]
impl From<mediaway_common::SampleFormat> for MediawaySampleFormat {
    // `SampleFormat` is `#[non_exhaustive]`; all variants that exist today are matched
    // by name below. A future variant falls back to F32 — the format the real Windows
    // WASAPI backend already requires today, not an arbitrary choice.
    #[allow(clippy::match_same_arms)]
    fn from(format: mediaway_common::SampleFormat) -> Self {
        use mediaway_common::SampleFormat;
        match format {
            SampleFormat::S16 => Self::S16,
            SampleFormat::S32 => Self::S32,
            SampleFormat::F32 => Self::F32,
            _ => Self::F32,
        }
    }
}

/// Config for [`crate::device::audio::mediaway_audio_capture_open`] — plain value struct, no
/// handle, no heap allocation, no free function.
///
/// Microphone only — Loopback/`ProcessLoopback` moved to
/// [`MediawayDesktopAudioCaptureConfig`] (`desktop` feature).
#[cfg(feature = "audio")]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediawayAudioCaptureConfig {
    /// Capture endpoint ordinal (`0` = default).
    pub device_index: u32,
    /// Timestamp timebase for polled frames.
    pub time_base: MediawayRational,
    /// Preferred PCM format. Only `F32` is accepted by the real Windows backend today.
    pub sample_format: MediawaySampleFormat,
}

/// Output of [`crate::device::audio::mediaway_audio_capture_poll_frame`] — owned; release with
/// [`crate::device::audio::mediaway_audio_frame_free`].
///
/// Does not derive `Copy`/`Clone` for the same reason as [`MediawayCameraFrame`].
#[cfg(feature = "audio")]
#[repr(C)]
#[derive(Debug)]
pub struct MediawayDeviceAudioFrame {
    /// Presentation timestamp in the stream timebase.
    pub pts: i64,
    /// Duration in timebase units.
    pub duration: u64,
    /// Sample rate (Hz), as negotiated by the backend (e.g. WASAPI `GetMixFormat`).
    pub sample_rate: u32,
    /// Channel count, as negotiated by the backend.
    pub channels: u16,
    /// PCM sample format.
    pub sample_format: MediawaySampleFormat,
    /// Owned interleaved sample bytes. `NULL` after
    /// [`crate::device::audio::mediaway_audio_frame_free`].
    pub data: *mut u8,
    /// Length of `data` in bytes.
    pub data_len: usize,
}

// ── Desktop audio (Loopback / ProcessLoopback) ──────────────────────────────────

/// Desktop audio capture source selection — mirrors
/// `mediaway_device::desktop::DesktopAudioSource`'s two variants.
#[cfg(feature = "desktop")]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayDesktopAudioSourceKind {
    /// Default render endpoint opened with WASAPI loopback.
    Loopback = 0,
    /// Per-process WASAPI loopback.
    ProcessLoopback = 1,
}

/// Config for [`crate::device::desktop_audio::mediaway_desktop_audio_capture_open`] — plain
/// value struct, no handle, no heap allocation, no free function.
#[cfg(feature = "desktop")]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediawayDesktopAudioCaptureConfig {
    /// What to capture.
    pub source_kind: MediawayDesktopAudioSourceKind,
    /// Loopback endpoint ordinal; ignored for `ProcessLoopback`.
    pub device_index: u32,
    /// `ProcessLoopback` only: target process id.
    pub process_id: u32,
    /// `ProcessLoopback` only: whether descendant processes are included
    /// (`INCLUDE_TARGET_PROCESS_TREE`); ignored otherwise.
    pub include_child_processes: bool,
    /// Timestamp timebase for polled frames.
    pub time_base: MediawayRational,
    /// Preferred PCM format. Only `F32` is accepted by the real Windows backend today.
    pub sample_format: MediawaySampleFormat,
}

/// Output of [`crate::device::desktop_audio::mediaway_desktop_audio_capture_poll_frame`] —
/// owned; release with [`crate::device::desktop_audio::mediaway_desktop_audio_frame_free`].
///
/// Does not derive `Copy`/`Clone` for the same reason as [`MediawayCameraFrame`].
#[cfg(feature = "desktop")]
#[repr(C)]
#[derive(Debug)]
pub struct MediawayDesktopAudioFrame {
    /// Presentation timestamp in the stream timebase.
    pub pts: i64,
    /// Duration in timebase units.
    pub duration: u64,
    /// Sample rate (Hz), as negotiated by the backend.
    pub sample_rate: u32,
    /// Channel count, as negotiated by the backend.
    pub channels: u16,
    /// PCM sample format.
    pub sample_format: MediawaySampleFormat,
    /// Owned interleaved sample bytes. `NULL` after
    /// [`crate::device::desktop_audio::mediaway_desktop_audio_frame_free`].
    pub data: *mut u8,
    /// Length of `data` in bytes.
    pub data_len: usize,
}

// ── Hotplug ──────────────────────────────────────────────────────────────────────

/// General-purpose `DeviceKind` mirror (`mediaway_device_kind_t`,
/// `adr/0002-callback-event-delivery.md` §6).
///
/// This crate's **only** full `DeviceKind` mirror — the capture config structs above
/// use capability-narrowed ordinals/discriminants instead. Needed by
/// [`crate::device::hotplug::mediaway_device_hotplug_open`]'s `kinds` parameter and
/// [`MediawayDeviceEvent::device_kind`], both of which need the general kind.
#[cfg(feature = "hotplug")]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayDeviceKind {
    /// Display / desktop duplication.
    Screen = 0,
    /// Single-window capture.
    Window = 1,
    /// Camera / video capture device.
    Camera = 2,
    /// Microphone (capture endpoint).
    Microphone = 3,
    /// System audio / render-endpoint loopback.
    Loopback = 4,
    /// Per-process render loopback.
    ProcessLoopback = 5,
    /// `DeviceKind` is `#[non_exhaustive]` (room for a future Linux/Web variant);
    /// catch-all for a value this crate doesn't know how to mirror yet — the same
    /// reasoning [`crate::device::MediawayDeviceStatus::UnknownError`] already applies to an
    /// error enum, now applied to a *data* enum for the first time. Not reachable from
    /// any backend today: v1 hotplug scope is Microphone/Loopback only
    /// (`WindowsDeviceHotplug::open` rejects every other kind at `open()` time).
    Unknown = 255,
}

#[cfg(feature = "hotplug")]
impl From<DeviceKind> for MediawayDeviceKind {
    fn from(kind: DeviceKind) -> Self {
        match kind {
            DeviceKind::Screen => Self::Screen,
            DeviceKind::Window => Self::Window,
            DeviceKind::Camera => Self::Camera,
            DeviceKind::Microphone => Self::Microphone,
            DeviceKind::Loopback => Self::Loopback,
            DeviceKind::ProcessLoopback => Self::ProcessLoopback,
            _ => Self::Unknown,
        }
    }
}

/// Discriminant for [`MediawayDeviceEvent`] — mirrors
/// `mediaway_device::DeviceEvent`'s four variants.
#[cfg(feature = "hotplug")]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayDeviceEventKind {
    /// A device became available.
    Added = 0,
    /// A device was removed.
    Removed = 1,
    /// The OS default device for a kind changed.
    DefaultChanged = 2,
    /// A device's state changed without being added or removed.
    StateChanged = 3,
}

/// A device-change notification (`mediaway_device_hotplug_open`'s watched-`kinds`
/// events).
///
/// Flat struct + discriminant, not a C union — follows this crate's existing
/// "kind field decides which fields matter" convention rather than introducing this
/// crate's first tagged C union (`adr/0002-callback-event-delivery.md` §6).
///
/// **Ownership depends on how it was obtained**: owned by
/// [`crate::device::hotplug::mediaway_device_hotplug_poll_event`] (release with
/// [`crate::device::hotplug::mediaway_device_hotplug_event_free`]); **borrowed**, valid only
/// for the duration of the call, when delivered to a registered
/// `mediaway_device_hotplug_callback_fn` (`adr/0002-callback-event-delivery.md` §2) — a
/// callback that needs `device_id` afterward must copy it itself before returning.
/// Does not derive `Copy`/`Clone` for the same reason as [`MediawayCameraFrame`].
#[cfg(feature = "hotplug")]
#[repr(C)]
#[derive(Debug)]
pub struct MediawayDeviceEvent {
    /// What kind of change occurred.
    pub event_kind: MediawayDeviceEventKind,
    /// The kind of device the change concerns.
    pub device_kind: MediawayDeviceKind,
    /// Owned, NUL-terminated UTF-8 — `mediaway_device::DeviceId`'s `Display` form
    /// (e.g. `"wasapi:<endpoint-id>"`). `NULL` only for `DefaultChanged` when the kind
    /// now has no default, or (defensively, practically unreachable) if the identity
    /// ever contained an embedded NUL — `event_kind`/`device_kind` still carry real
    /// information even without an id.
    pub device_id: *mut std::os::raw::c_char,
}
