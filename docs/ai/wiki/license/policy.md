# License policy (summary)

Mediaway identity = **MIT OR Apache-2.0** graph.

**Banned in Cargo / shipped binaries:** GPL / LGPL / AGPL / SSPL / BUSL · linking FFmpeg / x264 / x265 · Rust bindings (`ffmpeg-next`, etc. — `deny.toml` bans).

**Allowed in tests/dev only:** system `ffmpeg` / `ffprobe` as an optional oracle (not linked, not redistributed). [ADR-0002](../../../adr/0002-system-oracle.md) · [testing](../../conventions/testing.md).

**Allowed licenses (examples):** MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, CC0, MIT-0, OpenSSL, BSL-1.0 (Boost ≠ BUSL).

**SW fallback (`CPU / SW`):** pure Rust sans-io only — no C codec FFI. See root README § CPU / SW; `mediaway-sw` + reviewed Rust codec crates (e.g. `rav1e`).

Enforced by `cargo deny` (pre-push). Canonical: [`docs/conventions/security.md`](../../../conventions/security.md)
