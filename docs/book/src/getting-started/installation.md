# Installation

Mediaway is not published on crates.io yet ([Status & Stability](../project/status.md)).
Depend on it via git in your `Cargo.toml`, pinning a revision so pulls don't
surprise you with breakage:

```toml
[dependencies]
mediaway-common = { git = "https://github.com/nyxways/mediaway", rev = "<commit-sha>" }
mediaway-container = { git = "https://github.com/nyxways/mediaway", rev = "<commit-sha>" }
# add mediaway-encoder / mediaway-decoder / mediaway-device / mediaway-pipeline as needed
```

Pin the same `rev` across every `mediaway-*` crate you depend on — the
workspace evolves as one unit pre-1.0.

## Toolchain

Mediaway targets stable Rust (pinned in [`rust-toolchain.toml`](https://github.com/nyxways/mediaway/blob/main/rust-toolchain.toml)).
`rustup` picks up the pin automatically once your project is inside (or
depends on) the Mediaway workspace tree.

## Platform notes

Not every backend is available everywhere yet — check
[Codec Support](../reference/codec-support.md) and
[Device](../reference/device-capture.md) for the current matrix
before depending on a specific codec/platform combination.

- **Windows** — WMF/DX11 encode+decode, DXGI/WGC/WASAPI capture. No extra
  setup beyond the Rust toolchain.
- **Web (wasm32)** — add the `wasm32-unknown-unknown` target
  (`rustup target add wasm32-unknown-unknown`) for the `*-web`/`*-wasm` crates.
  WebCodecs/`getUserMedia`/`getDisplayMedia` backends run inside a real
  browser, not in a headless test runner.
- **Linux** — VA-API (`mediaway-*-linux`) needs `/dev/dri` and a working
  `libva` driver on the host; portal/PipeWire capture needs
  `xdg-desktop-portal` and a PipeWire session (typical on a desktop session,
  often absent in containers/CI/WSL2).
- **Apple / Android** — not implemented yet (see the reference tables).

## Optional: system FFmpeg

Mediaway never links FFmpeg/`libav*` in shipped crates (MIT OR Apache-2.0
only). A system `ffmpeg`/`ffprobe` on `PATH` is only ever used as an optional
test/dev oracle in this repository's own test suite — it is never required to
build or run your application.
