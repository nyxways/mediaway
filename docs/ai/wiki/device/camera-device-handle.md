# `CaptureSource::Camera` device field — left untyped

`CaptureSource::Camera { device: usize }` (`mediaway-device/src/video.rs`) was **deliberately
not** migrated to `NativeHandle` in ADR-0013, unlike `CaptureSource::Window { window: usize }`.

## Why it's different

`NativeHandle` wraps `NonZeroUsize` — it cannot represent `0`. That's correct for `Window`
(an `HWND` is a genuine pointer; `0` means unset) but **unknown** for `Camera`:

- If `device` is meant to be an **enumeration index** (0-based, "camera #0"), `0` is a valid,
  common value — `NativeHandle` would make the first camera on every system unrepresentable.
- If `device` is meant to be an **opaque token/handle** (e.g. a symbolic-link pointer bits),
  `NativeHandle` would be the right fit, same as `Window`.

The doc comment on the field ("Opaque device token / index bits") does not disambiguate.

## Current status (corrected 2026-07-31)

**Stale until now:** this page previously said no backend implemented camera capture.
`crates/mediaway-device-windows/src/camera.rs` (`WindowsCameraCapture`) is real, wired into
that crate's public API (`mod camera;` + `pub use camera::WindowsCameraCapture;` in
`lib.rs`), and hardware-verified: its test captured real 1920x1080 frames from a physical
"WeVO WV-1080" USB webcam on the dev machine (see that crate's roadmap Stage 4).

`device` really is an **enumeration index** — confirmed from source:
`WindowsCameraCapture::open` resolves it via `MFEnumDeviceSources`'s ordinal position
(`activate_for_index`), matching `CaptureSource::Camera { device: 0 }` = "first enumerated
camera". `NativeHandle` would have been the wrong choice for this backend (§ above still
applies: index `0` is valid and common).

`crates/mediaway-device-windows/docs/roadmap.md` Stage 4 and
`mediaway_device::capability::Unavailable::NotImplemented`'s doc comment ("e.g. `Camera`
today") still say camera capture doesn't exist / isn't wired — both are now known-stale,
flagged as follow-ups against their own crates (not fixed here).

Not yet wired: cross-platform dispatch (`mediaway_pipeline::platform`) has no `Camera`
entry point — only the direct `mediaway-device-windows::WindowsCameraCapture` API and
`mediaway-device-ffi`'s own local `#[cfg]` dispatch (see
[`ffi-c-abi.md`](ffi-c-abi.md)) reach it today.

## Superseded (2026-07-31)

[`mediaway-device` ADR-0005](../../../crates/mediaway-device/adr/0005-device-selection.md)
(Proposed) replaces `CaptureSource::Camera { device: usize }` with
`{ select: Select }` (`Select::Default | Id(DeviceId) | NameContains(String)`) — the
enumeration-index resolution above still applies conceptually (`Select::Id` from
`enumerate(Camera)[n].id`, or `DeviceInfo::ordinal` for a bare "Nth camera"), it's just no
longer the *only* addressing mode. `camera.rs`'s existing `enumerate_video_activates`/
`activate_for_index`/`friendly_name` (`#[allow(dead_code)]`) are the real plumbing ADR-0005
builds on — see [selection](selection.md). ADR-0013's other decisions (`NativeHandle`,
`GpuDeviceHandle`, `StreamInfo` split) are untouched.

## References

- ADR-0013: `docs/adr/0013-native-handle-and-gpu-device.md`
- `docs/ai/wiki/zero-copy/handles.md` — `NativeHandle` / `GpuBufferHandle` sibling type
- [selection](selection.md) — `mediaway-device` ADR-0005 (supersedes this page's resolution)
