# ADR-0010: Backend families (`audio` / `camera` / `desktop`) behind cargo features

- **Status**: Accepted (Windows verified; Linux per-family builds compile-unverified, see Consequences)
- **Date**: 2026-11-12
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device`

## Context

ADR-0021 merged the platform backends into this crate, so a consumer that only needs the
microphone still builds everything: on Linux that is `pipewire` (bindgen over `libpipewire`),
`v4l` (bindgen over `<linux/videodev2.h>`, needs libclang) and `ashpd` (xdg-desktop-portal
client, a D-Bus stack). A game's voice chat wants microphone capture and nothing else.

## Decision

Three additive features, `default = ["audio", "camera", "desktop"]` (no behavior change for
current consumers; CI's `--all-features` is a superset):

| Feature | Backends it enables | Native dependency it pulls (Linux) |
|---------|---------------------|-------------------------------------|
| `audio` | microphone + playback | `pipewire` |
| `camera` | camera | `v4l` |
| `desktop` | screen, window, loopback | `ashpd`, `pipewire` |

A **disabled family falls back to the same `host_stub` the other platforms already use**, so the
facade modules (`audio`, `camera`, `desktop`), `support()` and `request_permission()` keep
compiling and answer `Unsupported` / `Unavailable::NotImplemented`. The facade types are cheap
(traits and configs) and stay compiled.

- Windows: `windows_camera` needs `camera`, `windows_desktop` needs `desktop`. `windows_audio`
  (WASAPI) is always compiled because device enumeration, hotplug and the capability probe share
  its COM helpers, and loopback (`desktop`) is a WASAPI mode. The `windows` crate's feature list is
  not trimmed per family (a single shared dependency).
- Linux: the per-family modules are `cfg`-gated; `capabilities.rs` is always compiled with a
  `not(feature = ..)` twin per family-specific helper.
- Android, Apple and Web: **not** trimmed by these features yet (their backends compile as before).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Re-split into `mediaway-device-audio` etc. | Reverses ADR-0021; the facade/stub plumbing is shared. |
| Gate the facade modules too | They are plain types; gating them would force `cfg` into every stub and every consumer. |

## Consequences

### Positive

- `default-features = false, features = ["audio"]` on Linux builds no V4L2 bindings and no portal
  client; with no features at all there is no native dependency (`cargo check --target
  x86_64-unknown-linux-gnu --no-default-features` passes on a Windows host).
- Windows camera / DXGI / WGC code is not compiled for an audio-only consumer.

### Negative / Trade-offs

- **Linux builds with a strict subset of features (`audio`, `camera` or `desktop` alone) could not
  be compiled in the authoring environment** (no `libpipewire` / `videodev2.h` for the cross
  target). The all-features and no-features builds are verified; the subsets rest on review and
  on CI. Treat a failure there as a `cfg` slip in `linux/capabilities.rs`.
- Windows' `windows` crate features are a union: this saves compile time of our code only.
- `--no-default-features` on Windows reports camera / screen enumeration as `Unsupported`.

## References

- [ADR-0021](../../../docs/adr/0021-workspace-consolidation.md), [ADR-0002](0002-facade-platform-boundary.md)
- `docs/ai/wiki/device/feature-families.md`
