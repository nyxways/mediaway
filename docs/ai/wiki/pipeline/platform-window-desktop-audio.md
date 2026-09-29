# `platform::WindowCapture` and `platform::DesktopAudio`

Single-window capture and desktop audio through the auto-dispatch facade
(`crates/mediaway/src/platform.rs`, #105). Before this, recording one application's picture
and sound meant naming backend types (`WindowsWindowCapture`, …) directly.

| Facade | Windows | Linux | Apple |
|--------|---------|-------|-------|
| `WindowCapture::open(&DesktopVideoCaptureConfig)` | WGC | portal `SourceType::Window` | ScreenCaptureKit |
| `DesktopAudio::open(&DesktopAudioCaptureConfig)` | system or per-process loopback | — | — |

Other targets: `PlatformWindowCapture = core::convert::Infallible`, so `open` cannot succeed
and the type still exists for signatures.

## Return the concrete type, not a box

The older entry points (`AutoEncoder`, `ScreenCapture`, `Microphone`, …) return
`Box<dyn Trait>`. These two return `PlatformWindowCapture` / `PlatformDesktopAudioCapture`,
because each `#[cfg]` arm compiles into exactly one binary — there is no runtime choice for
a `dyn` to abstract. A caller who wants erasure coerces `&mut PlatformWindowCapture` to
`&mut dyn DesktopVideoCapture` themselves. Reasoning:
`crates/mediaway/adr/0002-platform-dispatch-avoid-box-dyn.md`.

## Backend-specific options are not mirrored

`open` takes the config only. Options with no cross-platform meaning — the WGC capture
border, even-crop — live on the backend type
(`WindowsWindowCapture::open_with(config, options)`), see [windows-window](../device/windows-window.md).

## Config fields that can make `open` fail

- `region: Some(_)` — WGC only; DXGI, Linux, macOS and iOS return `Unsupported`.
- `cursor: Included` — DXGI screen capture and iOS return `Unsupported`. macOS used to always
  draw the pointer and now hides it by default.
- The C ABI pins `cursor: Excluded`, `region: None`, and has no Window source yet
  ([ffi-c-abi](../device/ffi-c-abi.md)).
