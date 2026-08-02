# License & Dependencies

Mediaway is dual-licensed:

- [`LICENSE-MIT`](https://github.com/nyxways/mediaway/blob/main/LICENSE-MIT)
- [`LICENSE-APACHE`](https://github.com/nyxways/mediaway/blob/main/LICENSE-APACHE)

## The license/dependency boundary is a hard rule, not a preference

- **No GPL / LGPL / AGPL / SSPL / BUSL dependencies** — including FFmpeg
  crates or linking `libav*`, x264, x265. Enforced by `cargo deny` in CI.
- **No FFmpeg / `libav*` linked or vendored** in any shipped Mediaway crate.
- Software codecs, when used, are **pure Rust sans-io** only
  (`mediaway-sw` and codec cores) — explicit opt-in, never a silent
  fallback.
- A system `ffmpeg` / `ffprobe` on `PATH` may be used as an **optional
  test/dev oracle** — comparing Mediaway's own output against a
  well-known reference during testing — but it is never required to build
  or run shipped Mediaway. See
  [ADR-0002](https://github.com/nyxways/mediaway/blob/main/docs/adr/0002-system-oracle.md).

Mediaway is **not affiliated with the FFmpeg project**. The product CLIs
(`mediaway-avcli`, `mediaway-avprobe`) are independent tools.

Full detail: [`docs/spec/vision.md`](https://github.com/nyxways/mediaway/blob/main/docs/spec/vision.md)
§ License & dependency boundary,
[`docs/conventions/deps-policy.md`](https://github.com/nyxways/mediaway/blob/main/docs/conventions/deps-policy.md).
