# Audio (enhancement)

| Doc | Summary |
|-----|---------|
| [apm](apm.md) | `mediaway-sw::apm` — AEC3/NS/AGC2/VAD via `sonora`, implemented (ADR-0001) |

New category (2026-07-31): audio **enhancement** (echo cancellation, noise
suppression, gain control, voice-activity detection) is distinct from
`device` (OS capture/playback *sessions*) and `encode`/`decode` (codec
sessions) — it is a pure CPU DSP transform stage with no OS handle and no
platform split. See [`mediaway-sw::apm`](../../../../crates/mediaway-sw)
and cross-refs from [pipeline](../pipeline/index.md) and
[device](../device/index.md).

Packaging: [crate-packaging](../meta/crate-packaging.md).
