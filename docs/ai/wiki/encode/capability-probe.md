# Encoder capability probe — and why it takes a resolution

`mediaway::platform::encoder_support{,_at}(codec[, w, h])` →
`Vec<EncoderCapability { backend, support }>`, where
`support = Supported(EncodePathClass) | Unavailable(NoDevice | NotImplemented)`.

- `NoDevice` — real code exists, the driver/device did not answer **here, now**.
- `NotImplemented` — no code path for that backend/codec at all.

Live probe: it **opens a real encoder session per backend and drops it**. Costly. Populate
a settings list with it, never call it per frame. Mirrors `mediaway-device` ADR-0003.

## The resolution is part of the question

Hardware encoders have minimum *and* maximum dimensions, so a probe that hides resolution
cannot be right for everyone. Prefer `support_at`/`encoder_support_at` with your real
capture size; the resolution-free forms delegate at
`capability::DEFAULT_PROBE_{WIDTH,HEIGHT}` (256×256), documented as *the smallest round
size measured to clear every backend's minimum* — not as "small and cheap".

Measured 2026-09-18, RTX 4090 + Intel UHD 770, `Explicit(Nvenc)`:

| Resolution | AV1 | H.264 |
|---|---|---|
| 64×64 | `Err(Backend)` | `Err(Backend)` |
| 128×128 | `Err(Backend)` | `Err(Backend)` |
| 256×256 | `Ok`, `CpuUpload` | `Ok`, `CpuUpload` |
| 1920×1080 | `Ok`, `CpuUpload` | `Ok`, `CpuUpload` |

**This was a live bug** ([ADR-0005](../../../../crates/mediaway-encoder/adr/0005-resolution-aware-capability-probe.md)):
the probe opened at 64×64, below NVENC's minimum, so it reported "no NVIDIA encoder" on a
card with two AV1 encoders. A caller that honestly refuses rather than silently falling
back to software would refuse on fully capable hardware.

It cuts both ways — same machine, AV1, after the fix:

| Backend | at 256×256 | at 1920×1080 |
|---|---|---|
| `Nvenc` | `Supported(CpuUpload)` | `Supported(CpuUpload)` |
| `Vulkan` | `Supported(CpuUpload)` | `Unavailable(NotImplemented)` |
| `Software` | `Supported(Software)` | `Supported(Software)` |
| `Os` / `QuickSync` / `Amf` | `NotImplemented` | `NotImplemented` |

So no fixed size is safe: 64×64 under-reports NVENC, 256×256 over-reports Vulkan at 1080p.

## Gotchas

- `NoDevice` is what a backend reports when `open` fails for *any* reason the vendor API
  does not distinguish — including a rejected geometry. Ask at the right size rather than
  trying to classify the wrong answer.
- Audio codecs ignore `width`/`height`. `Opus` is special-cased in the facade before
  backend dispatch (`mediaway-sw` on Windows, `AudioConverter` on Apple).
- Off Windows, `mediaway-encoder`'s own probe returns all-`NotImplemented` at compile time,
  and the facade returns an empty vec for non-Opus codecs — other platform crates have no
  per-backend selection surface to report on yet.
- `Backend::Amf` is always `NotImplemented` — no license-clear Rust binding on this
  workspace's MSRV (`mediaway-encoder-amf` adr/0001).

Tests: `src/windows/auto_tests.rs` —
`default_probe_resolution_does_not_under_report_working_backends` is implication-shaped
(meaningful on hardware, a no-op without it), which is how to write a regression test for
a hardware-dependent false negative.
