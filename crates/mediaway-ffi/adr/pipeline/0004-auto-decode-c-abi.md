# ADR-0004: Auto video decode C ABI — `AutoDecoder` reachable from C

- **Status**: Accepted
- **Date**: 2026-08-05
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi`

## Context

`mediaway-ffi`'s `pipeline` module wraps `mediaway::platform::AutoEncoder` +
`EncodeSession` (encode → fMP4) and, since ADR-0003, `AudioEncoder` — but has no
decode surface at all. `mediaway::platform::AutoDecoder` is real, working Rust
(`crates/mediaway/src/platform.rs`): `AutoDecoder::open(&VideoDecoderConfig) ->
Result<Box<dyn VideoDecoder>, DecodeError>`, dispatching to
`mediaway_decoder::windows::WindowsVideoDecoder`/`linux::LinuxVideoDecoder`. Both this
crate's own `pipeline/roadmap.md` and the workspace `docs/roadmap.md` have carried
"decode C ABI" as an explicit, unaddressed gap since the crate's first pass
(`adr/0001-auto-encode-c-abi.md` § Deferred).

`VideoDecoder` (`crates/mediaway-decoder/src/video.rs`) is a 4-method trait —
`stream_info`, `push_packet(&Packet) -> Result<(), DecodeError>`,
`poll_frame() -> Result<Option<VideoFrame>, DecodeError>`, `flush`
— the mirror image of `VideoEncoder` (frame-in/packet-out vs. packet-in/frame-out),
same push/poll/flush streaming shape this crate's C ABI already wraps twice
(video encode, audio encode).

## Decision

> Add `mediaway_decode_session_t` to `pipeline.h` (ABI v3): single-step open (the
> handle *is* the decoder, like `AudioEncoder` — no muxer to wire, so no
> `mediaway_auto_encoder_t`-style intermediate handle), `push_packet`/`poll_frame`/
> `flush`/`close`, CPU-output-only v1 (GPU decode output deferred).

### 1. Config — CPU output only, `extra_data` required at open, deferred fields explicit

```c
typedef struct mediaway_auto_video_decode_config {
    mediaway_pipeline_codec_kind_t codec;
    uint32_t width;              /* expected; may be refined from the bitstream */
    uint32_t height;
    mediaway_rational_t time_base;
    mediaway_pixel_format_t pixel_format; /* preferred output format when the backend converts */
    const uint8_t *extra_data;   /* borrowed; valid for the mediaway_decode_session_open call only */
    size_t extra_data_len;       /* NULL/0 iff no codec config is available yet */
} mediaway_auto_video_decode_config_t;
```

**Correction from this ADR's own first draft:** `extra_data` (AVCC/SPS-PPS codec
config) was initially going to be supplied via the first pushed packet's payload
instead of config, by analogy with how muxer tracks work. That analogy does not hold
for the wrapped Rust decoder: `VideoDecoderConfig.extra_data` is consumed **at
`open()`, before any packet is pushed** — confirmed against the real, already-passing
round-trip test `crates/mediaway-decoder/tests/windows/cpu_roundtrip.rs`
(`WindowsVideoDecoder::open(&dec_cfg)` where `dec_cfg.extra_data =
encoder.stream_info().extra_data().clone()`, called before any `push_packet`). A
config field, not a first-packet special case, matches the real Rust contract.

Plain value struct otherwise, no handle — like `mediaway_video_track_info_t`'s
`extra_data`, `extra_data` here is a **borrowed** input (not owned, no free
function), valid only for the duration of the `mediaway_decode_session_open` call
that reads it. **Deferred, not exposed:** `gpu_device` / `VideoOutputPreference` — v1
always opens with `output: CpuFramesOk, gpu_device: None` internally. This repeats
`adr/0001-auto-encode-c-abi.md` §1's own v1 scoping call (real, working
`mediaway_gpu_device_handle_t`/`mediaway_gpu_buffer_handle_t` exist now, unlike when
that ADR was written, but wiring GPU-output decode frames is a separate, real design
surface — a new `storage_kind`-tagged decoded-frame struct, GPU surface
lifetime/read-window contract like `device.h`'s `mediaway_desktop_frame_t` — not a
config-field-only add).

`mediaway_auto_video_decode_config_new(codec, width, height, time_base, extra_data,
extra_data_len)` constructor mirrors `mediaway_auto_video_encode_config_new` in
role, with the two extra borrowed-buffer parameters `mediaway_video_track_info_t`'s
own constructor-equivalent-by-hand already establishes the precedent for — defaults
`pixel_format` to NV12. `extra_data_len == 0`/`extra_data == NULL` opens without a
known codec config (`Unsupported` from the backend if it actually requires one
up front — a real, not silently-ignored, outcome).

### 2. Opaque handle — single-step, `poisoned`-guarded

```rust
pub struct DecodeSessionHandle {
    poisoned: bool, // push_packet/poll_frame are repeated-call APIs — same guard as
                     // MuxerHandle/DemuxerHandle/EncodeSessionHandle, unlike
                     // AutoEncoderHandle/AudioEncodeSessionHandle's no-poisoned-flag
                     // shape (those either destroy-once or never fail destructively).
    inner: Box<dyn mediaway_decoder::VideoDecoder>,
}
```

`mediaway_decode_session_open(config, out_session)` is **one step**: like
`mediaway_audio_encoder_open` (ADR-0003 §Decision), not
`mediaway_auto_encoder_open`+`mediaway_encode_session_open`'s two-step split — decode
has no muxer to wire (the caller feeds it packets from their own
`mediaway_demuxer_poll_packet`), so the two-step split's reason to exist (an
independently-failing muxer stage) does not apply. **No consumption trap**: `close`
is always safe, no "already consumed, do not call X afterward" caveat.

### 3. Function list

```c
mediaway_auto_video_decode_config_t mediaway_auto_video_decode_config_new(
    mediaway_pipeline_codec_kind_t codec, uint32_t width, uint32_t height,
    mediaway_rational_t time_base);

mediaway_pipeline_status_t mediaway_decode_session_open(
    const mediaway_auto_video_decode_config_t *config,
    mediaway_decode_session_t **out_session);
mediaway_pipeline_status_t mediaway_decode_session_push_packet(
    mediaway_decode_session_t *session, const mediaway_decode_packet_view_t *packet);
mediaway_pipeline_status_t mediaway_decode_session_poll_frame(
    mediaway_decode_session_t *session, mediaway_decoded_video_frame_t *out_frame,
    bool *out_has_frame);
mediaway_pipeline_status_t mediaway_decode_session_flush(
    mediaway_decode_session_t *session);
void mediaway_decode_session_close(mediaway_decode_session_t *session);
void mediaway_decoded_video_frame_free(mediaway_decoded_video_frame_t *frame);
```

### 4. Packet input — new, pipeline-scoped type, not reused from `container.h`

`push_packet`'s input needs the same fields as `container.h`'s
`mediaway_packet_view_t` (`stream_id`, `pts`, `dts`, `duration`, `is_keyframe`,
`is_discard`, borrowed `payload`/`payload_len`) — but is declared as a **new,
distinctly-named** `mediaway_decode_packet_view_t` in `pipeline.h`, not reused
directly. Reasons: (a) this crate's own established precedent
(`mediaway_video_frame_t` vs. `device.h`'s `mediaway_device_video_frame_t` — same
field shape, different name, different module) is "new name per module even when
field-identical"; (b) reusing `mediaway_packet_view_t` would make `pipeline.h`
depend on `container.h` being included first, a cross-header ordering requirement
none of this crate's other functions impose today. `stream_id` is accepted but
unused by decode (kept for call-site symmetry with the container type a caller likely
already has in hand from `mediaway_demuxer_poll_packet`); a decode session decodes
whatever bitstream it is fed regardless of `stream_id`.

### 5. Decoded frame output — new, owned-output type

```c
typedef struct mediaway_decoded_video_frame {
    int64_t pts;
    uint64_t duration;         /* 0 if unknown */
    uint32_t width;
    uint32_t height;
    mediaway_pixel_format_t pixel_format;
    uint8_t *data;              /* owned; NULL after mediaway_decoded_video_frame_free */
    size_t data_len;
} mediaway_decoded_video_frame_t;
```

CPU-only (no `storage_kind`/`gpu_buffer` — see §1's GPU deferral). Owned output,
released via `mediaway_decoded_video_frame_free` (nulls `data`/`data_len`, same
double-free-safe idiom as `mediaway_packet_free`/`mediaway_stream_info_free`). New
name, not reused from any existing frame struct — direction (owned output) and module
(pipeline, decode) together make every existing frame type a mismatch: `device.h`'s
frame types are capture output (no codec/bitstream involved), `pipeline.h`'s existing
`mediaway_video_frame_t` is borrowed *encode input*.

### 6. Status codes — two new variants

```c
MEDIAWAY_PIPELINE_STATUS_DECODER_BACKEND_FAILURE = 13, /* DecodeError::Backend */
MEDIAWAY_PIPELINE_STATUS_DECODER_CLOSED          = 14, /* DecodeError::Closed */
```

Not reusing `ENCODER_BACKEND_FAILURE`/`ENCODER_CLOSED`: a program using both encode
and decode from C would otherwise get an ambiguous status when something fails,
unable to tell which side without tracking it separately — the same reasoning
`adr/0001-auto-encode-c-abi.md` §2 already applied to keep container-ffi's and
pipeline-ffi's status enums independent. `DecodeError::Unsupported`/`NoBackend`/
`InvalidInput` map onto the existing generic `MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED`/
`NO_BACKEND`/`INVALID_INPUT` — already generic across encode and decode, no
`DECODER_`-prefixed duplicates needed for those three.

### 7. Panic safety

Same `catch_unwind(AssertUnwindSafe(...))` + `poisoned` pattern as
`MuxerHandle`/`DemuxerHandle`/`EncodeSessionHandle`. `mediaway_decode_session_open`
distinguishes the same three outcomes as `mediaway_auto_encoder_open`/
`mediaway_audio_encoder_open`: normal `Ok`, normal `Err` (`NO_BACKEND` graceful,
others context-dependent), caught panic → `INTERNAL_PANIC`.

### 8. ABI version

`MEDIAWAY_PIPELINE_FFI_ABI_VERSION` bumps 2 → 3 (new exported symbols; pre-1.0, no
stability promise).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Two-step open (`mediaway_auto_decoder_open` → `mediaway_decode_session_open`), mirroring video encode | No muxer-wiring stage exists on the decode side to justify a second construction step — would just add an unconsumed intermediate handle for no benefit, same reasoning ADR-0003 already used for audio encode |
| Reuse `mediaway_packet_view_t`/`mediaway_video_frame_t` for decode I/O | Real field-shape matches but wrong ownership direction (`mediaway_video_frame_t`) or wrong-module coupling (`mediaway_packet_view_t`, would force header include order) — see §4/§5 |
| Ship GPU decode output in this pass | Real, separate design surface (storage-kind tag, GPU handle lifetime/read-window contract) on top of a capability with no C reach at all yet; v1 should land the working CPU path first, same phasing `adr/0001` used for encode |
| Reuse `ENCODER_BACKEND_FAILURE`/`ENCODER_CLOSED` for decode errors | Ambiguous which side failed in a program using both; `adr/0001` §2 already established distinct status codes per real error source as this crate's convention |

## Consequences

### Positive

- Closes a gap open since this crate's first pass, tracked in two roadmap files.
- CPU decode is reachable from C with the same panic-safety/ownership guarantees as
  every other surface in this header.

### Negative / Trade-offs

- No Zero-Copy GPU decode path reachable from C yet — every frame takes whatever
  CPU-output path the underlying `VideoDecoder` backend provides for
  `CpuFramesOk`, even where the Rust layer could do better.
- A fourth, `pipeline`-scoped packet/frame struct pair (`mediaway_decode_packet_view_t`,
  `mediaway_decoded_video_frame_t`) alongside the three that already exist across
  `container.h`/`device.h`/`pipeline.h` — real, if bounded, header surface growth.

## Known issue (found 2026-08-05, unresolved)

The C ABI surface described above is implemented, compiles clean, and passes
`clippy --all-targets --all-features`. Its own integration test
(`mediaway-ffi/tests/decode_smoke.rs`) is `#[ignore]`d: a real, pre-existing bug
in the wrapped `WindowsVideoDecoder`'s `CpuFramesOk` H.264 path, found while
writing this test, not a defect in this ADR's design or its implementation.
`mediaway_decode_session_push_packet`/`flush` succeed and return plausible
values; `mediaway_decode_session_poll_frame` reaches a Rust std UB precondition
check (`Alignment::new_unchecked requires a power of two`) inside the wrapped
decoder and **aborts the process** — not a catchable panic, so this crate's own
`catch_unwind` cannot turn it into a status code. A pure-Rust equivalent
(`mediaway-decoder/tests/cpu_roundtrip.rs`, fixed to actually compile+run
alongside this work — it was previously an orphaned, never-executed nested test
file) reaches the same backend without crashing but also decodes zero frames
from a valid single-packet bitstream. Full write-up:
[`docs/ai/wiki/platform/windows-decode.md`](../../../../docs/ai/wiki/platform/windows-decode.md)
§ CPU decode bug, tracked in [`docs/roadmap.md`](../../../../docs/roadmap.md)
§ Windows CPU Decode Bug.

## References

- [`crates/mediaway/src/platform.rs`](../../../mediaway/src/platform.rs) — `AutoDecoder`
- [`crates/mediaway-decoder/src/video.rs`](../../../mediaway-decoder/src/video.rs) — `VideoDecoder`, `VideoDecoderConfig`
- [`crates/mediaway-decoder/src/error.rs`](../../../mediaway-decoder/src/error.rs) — `DecodeError`
- [`adr/0001-auto-encode-c-abi.md`](0001-auto-encode-c-abi.md), [`adr/0003-auto-audio-encode-c-abi.md`](0003-auto-audio-encode-c-abi.md) — precedent this ADR reuses/deviates from
- `crates/mediaway-ffi/docs/pipeline/roadmap.md`, `docs/roadmap.md` — tracked this gap

ADRs are **English**. Numbering is local to this `adr/` folder.
