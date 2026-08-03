# mediaway-sw-opus

Pure Rust Opus encode/decode — no system libopus, no CMake/autotools toolchain.

Wraps [`unsafe-libopus`](https://crates.io/crates/unsafe-libopus) (BSD-3-Clause) behind a
safe RAII `OpusEncoder` / `OpusDecoder` surface. Kept in its own crate, separate from
`mediaway-sw`, because `unsafe-libopus`'s own public API is C-shaped and `unsafe fn` at
every call site — wrapping it needs `unsafe` glue code, which `mediaway-sw` deliberately
never contains (`#![forbid(unsafe_code)]`, no exceptions).

**Status: encode + decode implemented** (`OpusEncoder` / `OpusDecoder`, sibling-tested
round-trip). Not yet wired into a public `mediaway-encoder`/`mediaway-decoder` trait — see
[`adr/0001-unsafe-libopus-encode-decode.md`](adr/0001-unsafe-libopus-encode-decode.md) for
the design decision and [`docs/roadmap.md`](docs/roadmap.md) for remaining staging.
