# `mediaway-device` feature families (ADR-0010)

Crate ADR: [`crates/mediaway-device/adr/0010-backend-feature-families.md`](../../../../crates/mediaway-device/adr/0010-backend-feature-families.md).

- `default = ["audio", "camera", "desktop"]`: unchanged behavior. Opt out with
  `default-features = false, features = ["audio"]` (a voice stack) etc.
- `audio` = mic + playback (Linux: `pipewire`); `camera` (Linux: `v4l`, bindgen + libclang);
  `desktop` = screen / window / loopback (Linux: `ashpd` + `pipewire`).
- A disabled family compiles the family's `host_stub` instead (same pattern as "this backend on
  another OS"). `support(kind)` then says `Unavailable::NotImplemented`.
- Windows: WASAPI is always compiled (enumeration and hotplug share its COM helpers; loopback is a
  WASAPI mode). Only `windows_camera` (`camera`) and `windows_desktop` (`desktop`) are gated.
- Android / Apple / Web: not trimmed yet.

## Verifying a change here

```bash
for f in "" audio camera desktop "audio,camera,desktop"; do
  cargo clippy -p mediaway-device --no-default-features --features "$f" --all-targets -- -D warnings
done
cargo check -p mediaway-device --no-default-features --target x86_64-unknown-linux-gnu   # no native deps
```

Gotchas:

- A test that drives a gated backend (e.g. `windows/gpu_tests.rs` drives screen capture) must carry
  the same `#[cfg(feature = ..)]` as the backend, with its imports.
- Linux single-family builds need `libpipewire-0.3-dev` / `libspa-0.2-dev` / `linux/videodev2.h`;
  they cannot be cross-checked from Windows (`pkg-config` is not cross-configured).
