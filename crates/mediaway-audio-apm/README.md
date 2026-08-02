# mediaway-audio-apm

Audio enhancement facade: echo cancellation (AEC3), noise suppression (NS),
automatic gain control (AGC2), and RNN voice-activity detection (VAD) —
a thin Mediaway-typed adapter over [`sonora`](https://github.com/dignifiedquire/sonora),
a pure-Rust, SIMD-accelerated port of Google's WebRTC AudioProcessing module
(BSD-3-Clause, no C/C++ toolchain, no `libav*`).

Meant to sit right after microphone capture, before anything else touches the
signal — see [`mediaway-device`](../mediaway-device)'s `AudioCapture` for the
frames this crate consumes. Not (yet) wired into `mediaway-pipeline`'s
`EncodeSession`, which is video-only today.

**Status: Proposed, no implementation yet.** See
[`adr/0001-sonora-audio-processing-adoption.md`](adr/0001-sonora-audio-processing-adoption.md)
for the design decision and [`docs/roadmap.md`](docs/roadmap.md) for staging.
