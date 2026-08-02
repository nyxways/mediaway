# Security, secrets, and licenses

## Secrets

- Never commit `.env`, tokens, or private keys
- pre-commit `forbid-env.sh` + optional gitleaks
- Claude PreToolUse hook blocks the same patterns

## Licenses (product identity)

Mediaway = **MIT OR Apache-2.0** stack. Keep these out of the dependency graph:

| Banned | Examples |
|--------|----------|
| GPL / LGPL / AGPL | FFmpeg, x264, x265, many GPL filters |
| SSPL / BUSL | Source-available business licenses |
| Unknown / custom copyleft | Outside the deny allow-list |

Allowed: MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, CC0-1.0, MIT-0, OpenSSL, BSL-1.0 (Boost — not BUSL).

Enforced by `deny.toml` + pre-push `cargo deny`.

**Adding crates:** deliberate review — need, license (including transitive), maintenance, cost, alternatives. Canonical process: [deps-policy.md](deps-policy.md).

**Not a Cargo dependency:** developers and CI may install and run the system `ffmpeg` / `ffprobe` **binaries** as an optional test oracle. That does not put FFmpeg in the product graph and must not be required to build or run shipped Mediaway artifacts. See [testing.md](testing.md) · [ADR-0002](../adr/0002-system-oracle.md).

**Naming:** do not ship product crates or binaries named `*ffmpeg*` / `*ffprobe*`. Use `mediaway-avcli` / `mediaway-avprobe`. Mediaway is not affiliated with the FFmpeg project.

### SW fallback (`CPU / SW` axis)

**Pure Rust sans-io only** — no linking C codec libraries (`OpenH264`, `libvpx`, `libaom`, …). Prefer unprefixed codec cores + `mediaway-sw` facade; `rav1e`-class Rust deps OK when policy-reviewed.

## Advisories

`cargo deny check advisories` — yanked = deny. Ignore only with **cannot fix upstream + exposure zero**, plus issue/ADR reference.
