# ADR-0014: `mediaway-pipeline` — the convenience layer from api-layers.md

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)

## Context

`docs/spec/api-layers.md` defines a top "Convenience / pipeline" layer ("encode this
path", CLI-shaped helpers, thin, built only from layers below) that must stay separate
from the low-level trait/session layer. That layer does not exist as real product code
today:

- `examples/encode_to_mp4.rs` and `examples/screen_record.rs` each hand-roll the same
  encoder→muxer wiring: `push_frame` → loop `poll_packet` → set `pkt.stream_id` → mux,
  repeated once for the steady-state loop and again for the flush drain.
- Platform auto-selection (`open_auto_encoder`, `open_screen_capture`,
  `open_microphone`) lives in `examples/platform.rs`, whose own doc comment says:
  "When a real umbrella crate (`mediaway`) exists and handles dispatch internally, this
  module can be deleted." That crate never got built — the example file is standing in
  for it.
- Every app that wants to encode to MP4 must reimplement this plumbing from scratch.

## Decision

> Add `crates/mediaway-pipeline`: a thin facade (`mediaway-<capability>` naming, per
> `crate-packaging.md`) that composes `mediaway-encoder` + `mediaway-container` (+
> `mediaway-device` for capture) into a small `EncodeSession` type and the platform
> auto-dispatch functions migrated from `examples/platform.rs`.

- **`EncodeSession<E: VideoEncoder>`** — generic over the encoder type, not hardcoded to
  `Box<dyn VideoEncoder>`. Holds the encoder + a `mp4::Muxer<Live>` + the registered
  track id.
  - `open(encoder: E) -> Result<Self, PipelineError>` — registers `encoder.stream_info()`
    as a mux track, transitions the muxer to `Live`.
  - `write_frame(&mut self, frame: &VideoFrame) -> Result<(), PipelineError>` —
    `push_frame` then drains `poll_packet` into the muxer, stamping `stream_id`.
  - `finish(self) -> Result<Vec<u8>, PipelineError>` — flushes the encoder, drains,
    flushes the muxer, returns the fMP4 bytes.
  - Works with `AutoVideoEncoder` (Windows, unboxed) *and* `Box<dyn VideoEncoder>`
    (cross-platform dispatch) without forcing a `Box` on the concrete-type caller.
- **Blanket impl in `mediaway-encoder`**: `impl<T: VideoEncoder + ?Sized> VideoEncoder
  for Box<T>` (mirrors `std::io::Read for Box<R>`), so `Box<dyn VideoEncoder>` satisfies
  `EncodeSession`'s `E: VideoEncoder` bound directly — no new indirection introduced by
  the convenience layer itself; the `Box` only exists where cross-platform dispatch
  already requires it (ADR-0009 ZCA: minimize `Box`, don't add more of it).
- **Platform dispatch** (`open_auto_encoder`, `open_screen_capture`, `open_microphone`,
  `screen_config`) moves from `examples/platform.rs` into `mediaway-pipeline`, `#[cfg]`
  gated exactly as before. `examples/platform.rs` is deleted; examples depend on
  `mediaway-pipeline` instead.
- **`PipelineError`** (`thiserror`, `#[non_exhaustive]`) wraps `mediaway_encoder::EncodeError`
  and `mediaway_container::mp4::Error` via `#[from]`.
- **Low-level stays reachable**: `mediaway-encoder` / `mediaway-container` traits and
  types are unchanged and fully public; `EncodeSession` is additive, not a replacement.
  `examples/mux_roundtrip.rs` remains the required "never uses the convenience wrapper"
  example (api-layers.md anti-opaque-only-path rule).
- **Scope for v1: video-only, single track.** Neither current example wires an audio
  *encoder* end-to-end (screen_record.rs pushes raw audio frames with a `TODO`, not
  through `AudioEncoder`), so a dual-track `EncodeSession` would be speculative. Extend
  to audio/dual-track when a real caller needs it — new ADR at that point if the shape
  changes materially.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Add convenience helpers directly inside `mediaway-encoder` | Facade would depend on `mediaway-container` — crosses the facade boundary crate-packaging.md draws between capability facades |
| Keep `examples/platform.rs` as the de facto convenience layer | Not reusable outside the examples crate; every real app must copy-paste it |
| `EncodeSession` hardcoded to `Box<dyn VideoEncoder>` | Forces a `Box` even when the caller already holds a concrete encoder type (e.g. `AutoVideoEncoder` on Windows) |
| Build audio + multi-track support now | No current caller exercises it end-to-end; premature per "no abstractions for one-off code" |

## Consequences

### Positive

- New apps get `EncodeSession::open(encoder)` → `write_frame` → `finish()` instead of
  hand-rolled poll loops.
- Platform dispatch has one real home instead of living in the examples crate.
- Low-level surface is untouched; the anti-opaque-only-path rule holds by construction.

### Negative / Trade-offs

- One more crate to version/maintain.
- `EncodeSession` only covers the video-encode-to-MP4 shape today; screen/mic capture
  wiring in `screen_record.rs` still has manual per-frame glue (capture → encode
  conversion is inherently app-specific, not boilerplate this layer can hide).

## Platform dispatch reshaped to marker types (2026-07-31)

A DX audit (prompted by `mediaway-encoder`'s `Backend`/`BackendSelection` rework, see
ADR-0004's 2026-07-31 addendum) flagged `open_auto_encoder` / `open_auto_decoder` /
`open_screen_capture` / `open_microphone` as exactly the C-idiom `open_video(handle)`
free function AGENTS.md's Rust-idiomatic-API rule calls out by name — they read like
this ADR's own bad example, just under a different name. Each is replaced by a
zero-sized marker type with an associated `open`:

- `platform::AutoEncoder::open` (was `open_auto_encoder`)
- `platform::AutoDecoder::open` (was `open_auto_decoder`)
- `platform::ScreenCapture::open` (was `open_screen_capture`)
- `platform::Microphone::open` (was `open_microphone`)

The marker types carry no state — the `#[cfg]` dispatch body is unchanged, only the
call-site shape moves from `platform::open_auto_encoder(&config)` to
`platform::AutoEncoder::open(&config)`. `screen_config(fps)` is deleted outright (not
deprecated): it was a strictly less flexible duplicate of
`mediaway_device::VideoCaptureConfig::screen(output_index, time_base)`, hardcoding
`output_index: 0` — callers now build the config directly.

`encoder_support`, `device_support`, and `request_device_permission` are unchanged —
they're pure probes, not construction, so the free-function shape doesn't read as
C-idiomatic in the same way (mirrors `mediaway-device` ADR-0003's own `support`/
`request_permission` free functions).

No legacy aliases kept; every caller (examples, `mediaway-pipeline` tests, README,
wiki) was updated in the same change.

## References

- spec: `docs/spec/api-layers.md`, `docs/spec/crate-packaging.md`
- related ADR: ADR-0003 (crate packaging), ADR-0004 (backend preference), ADR-0009 (ZCA)
- wiki: `docs/ai/wiki/pipeline/`

ADRs are written in **English**.
