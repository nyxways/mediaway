# Android encode (NDK `AMediaCodec`) — implemented, zero compile verification yet

- Module: `mediaway-encoder::android` (`src/android/`), `cfg(target_os = "android")`
- Bindings: [`ndk`](https://crates.io/crates/ndk) 0.9
  (`MIT OR Apache-2.0`), `features = ["media"]` →
  `ndk::media::media_codec::MediaCodec` + `ndk::media::media_format::MediaFormat` — a **safe**
  wrapper, not raw `ndk-sys`/hand-bindgen. Same "existing safe wrapper over a fiddly native
  session API" shape as Linux's `cros-libva`.
- Correction: `ndk-sys` **does** have a `media` feature (bindgen over `NdkMediaCodec.h` et al.),
  and the safe `ndk` crate goes further with a real `MediaCodec` wrapper
  (`from_encoder_type`/`configure`/`start`/`dequeue_input_buffer`/`queue_input_buffer`/
  `dequeue_output_buffer`/`release_output_buffer`/`create_input_surface`) — verified via
  `docs.rs` + the crates' own `Cargo.toml`, 2026-08-12.
- Codec: H.264 (`video/avc`) only.
- CPU: `upload_and_queue` (MaybeUninit-slice copy into `input_buffer(index).buffer_mut()`), no
  `Surface` at `configure` time. `KEY_I_FRAME_INTERVAL = 0` requests every frame as a sync
  frame — device-dependent, not a byte-exact guarantee like Linux's raw bitstream approach.
- Output: opportunistic `dequeue_output_buffer` drain in `push_frame`/`poll_packet`
  (`AMediaCodec` output is not guaranteed synchronous with a given `push_frame`, unlike VA-API's
  `vaSyncSurface`); `BUFFER_FLAG_CODEC_CONFIG` buffers and `OutputFormatChanged`/
  `OutputBuffersChanged` events are skipped this stage (no `csd-0`/`csd-1` extradata capture
  yet — deferred, same discipline as Linux leaving `extra_data` empty).
- Zero-Copy: **not implemented, deferred** — `GpuBufferHandle::AndroidSurface` (`AHardwareBuffer*`)
  **already exists** in `mediaway-common::gpu` (predates any Android backend); wiring is future
  work via `MediaCodec::create_input_surface` + `ANativeWindow`.
- Typestate: `ndk::media_codec::MediaCodec` does **not** enforce configure→start→stop ordering
  at the type level (unlike `cros-libva`'s `Picture<S, T>`). Implementation deviation from the
  ADR's original sketch: a plain `flushed: bool` guard turned out sufficient instead of a full
  `SessionState` enum — `open()` runs configure+start atomically before the struct exists, so
  there is no external caller path that can invoke them out of order; the only runtime-relevant
  transition left is "has `flush()` been called", which `bool` already covers (same shape Linux
  uses). Simpler than the ADR proposed, not a coverage gap.
- ADR: [0001 (`adr/android/`)](../../../../crates/mediaway-encoder/adr/android/0001-ndk-amediacodec-h264-cpu-upload.md)
  — binding choice, scope, and the **zero compile verification** caveat (weaker starting point
  than Linux ADR-0001, which got real WSL2 `cargo check` before any hardware existed).

## ⚠️ Status: implemented, zero compile verification until CI runs

Unlike every prior backend in this workspace, this dev environment (Windows host, no Android
NDK installed) cannot even `cargo check` this module — not just "no hardware", but no compiler
pass at all as authored. A new `android` CI job (`.github/workflows/ci.yml`, `nttld/setup-ndk` +
`cargo-ndk`, `arm64-v8a` + API 21, compile+clippy only, mirroring the existing `wasm` job's
compile-only precedent) ships in the same PR as this implementation — it is the **first** real
gate this code goes through, ahead of any later hardware-verification milestone. Treat every
`ndk`/`AMediaCodec` API call in `src/android/` as unverified until that job is green.

## Structural differences vs. Linux (VA-API)

| Linux (VA-API) | Android (`AMediaCodec`) | Note |
|---|---|---|
| `Config` + `Context` (capability vs. session) | Single `MediaCodec` handle | `AMediaCodec` collapses both into one object |
| `Picture<S, T>` typestate enforced by `cros-libva` | `flushed: bool` guard owned by this crate | `ndk::media_codec` doesn't typestate-enforce ordering itself; `open()`'s atomic configure+start makes a fuller state machine unnecessary here |
| `Image` upload-on-`Drop` | Explicit `queue_input_buffer` per index, no upload-on-drop | Buffer-index dequeue/queue model, not an image object |
| `MappedCodedBuffer` segment iteration, synchronous per frame | `output_buffer(index)` slice + `release_output_buffer`, opportunistic drain | `AMediaCodec` output is not guaranteed ready within the same `push_frame` call |
