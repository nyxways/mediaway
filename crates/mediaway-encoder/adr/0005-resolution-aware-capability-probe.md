# ADR-0005: Resolution-aware encoder capability probe

- **Status**: Accepted
- **Date**: 2026-09-18
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: mediaway-encoder

## Context

`windows::auto::support(codec)` probes each `Backend` by opening a real encoder session
and mapping the outcome to `EncodeSupport::{Supported, Unavailable}`. It opened every
session at a fixed **64×64** — chosen as a cheap throwaway size.

Hardware encoders have minimum dimensions. 64×64 is below NVENC's, so `open` returned
`EncodeError::Backend`, which the probe maps to `EncodeUnavailable::NoDevice` — the same
answer it gives for "there is no NVIDIA GPU in this machine."

Measured on an RTX 4090 + Intel UHD 770 (2026-09-18), `BackendSelection::Explicit(Nvenc)`:

| Resolution | AV1 | H.264 |
|---|---|---|
| 64×64 | `Err(Backend)` | `Err(Backend)` |
| 128×128 | `Err(Backend)` | `Err(Backend)` |
| 256×256 | `Ok`, `CpuUpload` | `Ok`, `CpuUpload` |
| 1920×1080 | `Ok`, `CpuUpload` | `Ok`, `CpuUpload` |

So the one API whose entire purpose is an honest capability answer reported *no NVIDIA
hardware encoder* on a card with two AV1 encoders. Under
`docs/spec/caveats-and-clarity.md` (ADR-0006) this is a correctness bug, not a
nice-to-have: a downstream recorder that refuses to run rather than silently falling back
to software — the behaviour that spec asks for — refuses on a fully capable machine.

Raising the constant alone would not fix the class of problem. Support is genuinely
resolution-dependent in **both** directions, which the same measurement session confirmed
after the fix: for AV1, `Backend::Vulkan` reports `Supported` at 256×256 and
`NotImplemented` at 1920×1080. Any fixed probe size is wrong for somebody.

## Decision

> We make probe resolution part of the query, and keep the resolution-free form as a
> documented convenience.

```rust
// mediaway-encoder
pub fn support_at(codec: CodecKind, width: u32, height: u32) -> Vec<EncoderCapability>;
pub fn support(codec: CodecKind) -> Vec<EncoderCapability>;   // unchanged signature

pub const DEFAULT_PROBE_WIDTH: u32 = 256;
pub const DEFAULT_PROBE_HEIGHT: u32 = 256;
```

`support` delegates to `support_at` at the default. The facade mirrors this exactly:
`mediaway::platform::encoder_support_at` / `encoder_support`.

- `DEFAULT_PROBE_WIDTH`/`HEIGHT` are documented as *"the smallest round size measured to
  clear every backend's minimum"*, not as "small and cheap". The distinction is the whole
  bug.
- Callers that know their capture size — a screen recorder does — are directed to
  `support_at` in the rustdoc of both functions.
- `width`/`height` are ignored for audio codecs, which have no geometry.

### Scope

Additive. No existing caller breaks; `support(codec)` keeps its signature and simply stops
lying about NVENC.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Raise the constant to 256×256 and stop | Fixes this machine and leaves the class of bug intact. The post-fix measurement immediately produced the mirror-image failure (Vulkan AV1 `Supported` at 256×256, `NotImplemented` at 1080p), so a fixed size under-reports at one end and over-reports at the other. |
| Make `support(codec)` itself take width/height | Breaking change for every caller, including ones that genuinely have no resolution in hand yet (a settings screen opened before a source is chosen). The convenience form has real users. |
| Query driver capability tables instead of opening a session | No uniform cross-backend table exists; WMF, NVENC, QuickSync and Vulkan each expose different surfaces, and several report capabilities the runtime then rejects. Opening a real session is what makes the probe *live* — `mediaway-device` ADR-0003's reasoning applies unchanged. |
| Map "below minimum dimensions" to a distinct `EncodeUnavailable` variant | Requires each backend to tell dimension rejection apart from every other `Backend` failure, which the vendor APIs do not reliably do. Asking at the right size sidesteps needing to classify the wrong answer. |

## Consequences

### Positive

- The probe stops producing false negatives on real hardware — the RTX 4090 now reports
  `Nvenc => Supported(CpuUpload)` for both AV1 and H.264.
- Resolution-dependence becomes visible in the API instead of hiding in a constant, so the
  next backend with unusual limits is a correct answer rather than a new bug.
- Callers that care about a specific capture size can get an answer about *that* size.

### Negative / Trade-offs

- Two functions where there was one, and a caller who uses the convenience form still gets
  an answer about 256×256 rather than their own resolution. Mitigated by rustdoc on both,
  not by removing the convenience.
- `support_at` is as costly as `support` — it opens real sessions. Calling it per
  resolution multiplies that cost. Unchanged guidance: call it to populate a settings list,
  not per frame.
- The default remains a measured value on one machine class. It is documented as such,
  with the measurement in this ADR, rather than presented as a vendor-guaranteed floor.

## References

- `docs/spec/caveats-and-clarity.md` · workspace ADR-0006 — honest capability reporting
- [`mediaway-device` ADR-0003](../../mediaway-device/adr/0003-capability-and-permission-probe.md)
  — the live-probe pattern this mirrors
- [ADR-0003](0003-auto-encode.md) — `AutoVideoEncoder` backend chain the probe drives
- `src/windows/auto_tests.rs` —
  `default_probe_resolution_does_not_under_report_working_backends` (implication-shaped, so
  it is meaningful on hardware and a no-op without it) and
  `support_at_below_a_backend_minimum_reports_unavailable_or_skip`
