# `examples/` layout

One capability per file, grouped by sector — not mixed end-to-end demos.

```
examples/
├── container/   # mux/demux only, no encoder — mux_demux_mp4.rs
├── encode/      # one codec direction, no container — encode_h264.rs
├── decode/      # one codec direction, no container — decode_h264.rs
├── device/      # one capture source, no encode — capture_screen/window/camera/microphone.rs
└── pipeline/    # composed end-to-end flows — encode_to_mp4.rs, screen_record.rs, trim_and_splice.rs
```

`[[example]]` entries in `examples/Cargo.toml` map `name` → `path =
"<sector>/<file>.rs"`. Adding a new example: add both the file and its
`[[example]]` block — Cargo does not auto-discover examples in
subdirectories the way it does for `examples/*.rs` directly under the root.

## Why `device/capture_camera.rs` and `capture_window.rs` skip `platform::`

`mediaway::platform` only dispatches `ScreenCapture`/`Microphone`
cross-platform — camera and window capture aren't wired into it yet (see
that crate's roadmap). Those two examples reach for
`mediaway_device::windows_camera::WindowsCameraCapture`/`mediaway_device::windows_desktop::WindowsWindowCapture`
directly; both compile on every platform (a `#[cfg(not(windows))]` stub
returns `CaptureError::Unsupported`), so no `#[cfg(windows)]` is needed in
the example itself.

`capture_window.rs` specifically only shows the config shape — `open()`
requires a caller-owned `ID3D11Device`, which means raw Win32 FFI
(`unsafe`), out of scope for a plain example (`unsafe_code = deny` outside
FFI/platform-backend crates). See `crates/mediaway-device/src/windows_desktop/lib_tests.rs`'s
`open_window_capture_foreground_or_skip` for the real, `unsafe`-contained
version.

## `harness = false` benches need a real `fn main` on every target

Gotcha found while reorganizing examples: a `[[bench]] harness = false`
target (criterion) whose entire file is gated `#![cfg(all(windows, feature =
"..."))]` fails to compile on other platforms with `E0601: main function not
found` — Cargo still tries to build the target there, and the whole-file cfg
strips away the `criterion_main!` macro invocation that would have generated
`fn main`. Cargo has no per-target OS gate for `[[bench]]`/`[[example]]`
blocks (unlike `[target.'cfg(...)'.dependencies]`), and `required-features`
doesn't help since CI passes `--all-features`.

Fix used in `crates/mediaway-encoder/benches/windows/wmf_h264_encode.rs` and
`crates/mediaway-decoder/benches/windows/wmf_h264_decode.rs`: wrap the real content
in `mod imp { ... }` gated to the real condition, call
`criterion::criterion_main!(imp::benches)` **at crate root** (must expand
there — `rustc` only looks for `fn main` at the crate root, not inside a
module), and add a `#[cfg(not(...))] fn main() {}` fallback. Applies to any
future Windows/Linux-only bench in this workspace.
