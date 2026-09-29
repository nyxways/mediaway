# ADR-0007: Streaming fMP4 bytes, AAC decode and capability probes over the pipeline C ABI

- **Status**: Accepted — hardware-verified on Windows through the C ABI and the C++, C#, Python and Node.js bindings; the Apple AAC arm is compile-checked only
- **Date**: 2026-09-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi` (pipeline domain)

## Context

Since v0.1.8 the Rust pipeline gained three things the C ABI cannot reach:

1. **`EncodeSession::poll_bytes` / `finish_into`** (`mediaway` ADR-0006). A C caller holds the
   whole recording in RAM until `mediaway_encode_session_finish`; a one-hour capture is a
   one-hour buffer.
2. **AAC decode** (`mediaway_decoder::windows::WmfAacDecoder`, `apple::AacDecoder`).
   `mediaway_audio_decode_session_open` accepts only Opus, wrapped directly around the software
   `mediaway_sw::opus::OpusDecoder` (ADR-0006).
3. **Capability probes** (`mediaway::platform::encoder_support_at` / `decoder_support`). A C
   caller finds out an encoder is unavailable by opening it and failing, and encoder support is
   resolution-dependent (`mediaway-encoder` ADR-0005), so a resolution-free answer is wrong.

## Decision

> Add `mediaway_encode_session_poll_bytes`, let the audio decode session open AAC, and add
> `mediaway_encoder_support_at` / `mediaway_decoder_support`.
> `MEDIAWAY_PIPELINE_FFI_ABI_VERSION` goes 6 → 7.

### 1. Streaming bytes

`mediaway_encode_session_poll_bytes(session, uint8_t **out_data, size_t *out_len)` returns the
fMP4 bytes ready now as an **owned** buffer, released with `mediaway_pipeline_ffi_buffer_free`,
the same rule `finish` uses. Nothing ready is `*out_data == NULL`, `*out_len == 0`, and no
allocation. It is indistinguishable from "already drained", as in Rust.

`mediaway_encode_session_finish` needs no new sibling. It already calls Rust `finish`, which is
`finish_into` over a fresh `Vec`, so after polling it returns **only the unpolled tail**. That
was always the behaviour; this ADR makes it the documented contract in the header.

Rejected: a caller-supplied `(buf, cap)` out-parameter. It forces the handle to hold leftover
bytes between calls, which is a second buffer inside the session for no saving.

### 2. AAC decode

`mediaway_audio_decode_config_t` gains `extra_data` (borrowed, valid for `open` only) and
`extra_data_len`. NULL/0 is what Opus passes, so Opus callers change nothing but recompile.

`codec == AAC` opens the platform decoder: `WmfAacDecoder` on Windows, `apple::AacDecoder` on
macOS and iOS, `UNSUPPORTED` elsewhere. The session's inner decoder is an
`enum { Opus(OpusDecoder), Aac(PlatformAacDecoder) }` — enum dispatch, no `Box<dyn AudioDecoder>`.

- The raw `AudioSpecificConfig` in `extra_data` is **required**. An empty one is
  `INVALID_INPUT`, not the Rust `Unsupported`: a config mistake, and both backends refuse to
  synthesise a default because it would decode SBR/PS streams to quietly wrong output.
- Raw AAC only. ADTS input must be de-headered first (`adts-core`).
- An empty packet is Opus's loss-concealment hint and means nothing for AAC: `INVALID_INPUT`.
- Output is F32 PCM in both, matching Opus. **Unlike Opus, the AAC path is the OS's codec:**
  samples can differ between hosts and OS versions. Opus stays software so a C caller gets
  identical output everywhere; AAC cannot offer that and the header says so.

The Apple arm is **not verified on this workspace's Windows dev machine**. It is compile-checked
for `aarch64-apple-darwin` and `aarch64-apple-ios`; the first real run is macOS CI.

### 3. Capability probes

`mediaway_encoder_support_at(codec, width, height, rows **, count *)` returns an owned array
(freed with `mediaway_encoder_support_free`, the GPU adapter list's shape). A row is a backend,
a `state` (`SUPPORTED` / `NOT_IMPLEMENTED` / `NO_DEVICE`) and a `path_class`
(`ZERO_COPY` / `GPU_COPY` / `CPU_UPLOAD` / `READBACK` / `SOFTWARE`) meaningful only when
`SUPPORTED`. Flattened to two enums, not a tagged union, like every other optional in this ABI.
There is no resolution-free form: it is the wrong question (`mediaway-encoder` ADR-0005).

`mediaway_decoder_support(codec, state *)` returns one state, since decode has one
implementation per platform. It is included because AAC decode now differs by platform and a
caller should learn that without opening a session.

**Both probes are costly.** They open throwaway sessions (a real MFT / VA-API / VideoToolbox
session per row). The header says so; do not call them per frame.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Caller-supplied buffer for `poll_bytes` | Hidden second buffer in the handle (see § 1) |
| A `finish_into`-style entry | Identical to `finish`; a second name for the same call |
| `Box<dyn AudioDecoder>` inside the session | The set is closed and known at compile time; enum dispatch costs nothing (ADR-0009) |
| AAC on Windows only, Apple `UNSUPPORTED` | Ships a hole in a shipped capability; the cost is a compile check now and CI verification |
| A resolution-free `encoder_support` | Reports "no NVIDIA encoder" on a machine that encodes fine (`mediaway-encoder` ADR-0005) |
| ADTS input for AAC | Needs a de-header step this ABI does not own; callers have `adts-core` |

## Consequences

### Positive

- Bounded-memory recording from C, C++, C#, Python and Node.
- AAC files written by this workspace can be played back through the C ABI.
- A caller can decide between encoders before opening one.

### Negative / Trade-offs

- **ABI break**: `mediaway_audio_decode_config_t` grew and is passed by value, so callers
  recompile and every binding's mirror changes (pipeline 6 → 7).
- AAC output is host-dependent, and the Apple arm is unverified until macOS CI runs.
- Probes are slow and their answers can change with the driver state.

## References

- `mediaway` ADR-0006 (streaming bytes); `mediaway-decoder` ADR-0003 and windows ADR-0006 /
  apple ADR-0004 (AAC); `mediaway-encoder` ADR-0005 (resolution-aware probe)
- Pipeline ADR-0006 (Opus audio decode); ADR-0004 (borrowed `extra_data` at open)
