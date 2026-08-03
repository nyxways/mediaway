# C binding

> **Status: ✅ verified** — the C ABI is real and hardware-verified; the examples in
> `examples/` link + run against it. This README is the contract the C examples
> (and every other language's wrapper) are held to.

Mediaway exposes a hand-written C ABI (`*-ffi` crates) so non-Rust languages can call
the stack. For C, **the ABI itself is the binding** — there is no wrapper layer. See
[`docs/spec/c-ffi.md`](../../docs/spec/c-ffi.md) (ADR-0004) and
[`crates/mediaway-container-ffi/`](../../crates/mediaway-container-ffi/),
[`crates/mediaway-pipeline-ffi/`](../../crates/mediaway-pipeline-ffi/),
[`crates/mediaway-device-ffi/`](../../crates/mediaway-device-ffi/) for the sources.

## What Mediaway is (the capabilities)

A streaming-first media stack. The C surface currently covers three capabilities:

1. **Container — mux + demux** (`<mediaway/container.h>`, crate feature `mux`/`demux`,
   both on by default): a *sans-io* fragmented-MP4 muxer and demuxer. The core never
   touches files or sockets: the caller owns all byte I/O — pull muxed bytes out of the
   muxer, push bytes into the demuxer. Demux also supports ClearKey decryption
   (one demuxer-wide 16-byte key; decrypt runs synchronously inside `push_bytes`).
2. **Pipeline — auto video encode → fMP4** (`<mediaway/pipeline.h>`): opens the best
   available OS/GPU H.264 (or other codec) encoder for a config, wires its output
   packets into a fragmented MP4 muxer internally, and hands the caller complete MP4
   bytes from `finish()`. **Video only.** The audio encoder is separate (ABI v2,
   adr/0003): `mediaway_audio_encoder_open` returns a session that streams AAC
   packets for the caller's own muxer.
3. **Device — capture** (`<mediaway/device.h>`): Camera video capture (CPU frames),
   Microphone/Loopback/ProcessLoopback audio capture (PCM), and device hotplug
   (poll or callback mode). **Screen capture is NOT available from C in this pass** —
   it requires a live GPU device handle (`ID3D11Device*`) with no CPU fallback, and its
   representation is deferred; `mediaway_desktop_capture_open()` on a Screen-kind
   config with the only C-constructible gpu_device (`NONE`) returns
   `MEDIAWAY_DEVICE_STATUS_INVALID_INPUT`, and a Window-kind config returns
   `MEDIAWAY_DEVICE_STATUS_UNSUPPORTED` (real, documented behavior — the
   device ABI is domain-split, `adr/0004-domain-feature-split.md`).

## The real ABI (what examples must call)

Headers: `crates/mediaway-*-ffi/include/mediaway/{container,pipeline,device}.h`.
Each header exports an ABI-version function (`mediaway_*_ffi_abi_version`).

**Ownership rules (identical across all three headers):**
- *Borrowed inputs* (track `extra_data`, `mediaway_packet_view_t.payload`,
  `mediaway_demuxer_push_bytes` data, frame `raw_bytes`, `set_decryption_key` key) are
  caller-owned, valid for the duration of the call only.
- *Owned outputs* (`mediaway_muxer_poll_bytes` buffer, `mediaway_packet_t.payload`,
  `mediaway_stream_info_t.extra_data`, `mediaway_encode_session_finish` buffer,
  polled device frames) are library-owned and MUST be released through the matching
  `_free` function (`mediaway_buffer_free`, `mediaway_packet_free`,
  `mediaway_stream_info_free`, `mediaway_pipeline_ffi_buffer_free`,
  `mediaway_camera_frame_free` / `mediaway_desktop_frame_free`,
  `mediaway_audio_frame_free`, `mediaway_desktop_audio_frame_free`).
- Every handle is **thread-confined**: moving a handle to another thread is fine, but
  concurrent calls on the SAME handle from different threads are a data race.
- Panics never cross the ABI: a caught panic poisons the handle; every later call on it
  returns `*_HANDLE_POISONED` (except `close`, which is always safe).

**Errors:** distinct per-crate status enums (`mediaway_status_t`,
`mediaway_pipeline_status_t`, `mediaway_device_status_t`), all with `OK = 0`; a
`*_NO_BACKEND` / `*_UNSUPPORTED` result is an expected, graceful outcome, not a bug —
examples must exit cleanly (or demonstrate the gap), never treat it as fatal.

**Feature flags:** container `mux`/`demux` can be slim-built; calling a symbol the
library was not built with is a *link error*. Pipeline/device have their own features;
`NO_BACKEND` surfaces when none is compiled in.

## Ideal API — the DX contract

For C the contract is: **the ABI, used directly, with ownership made explicit and
errors checked on every call.** The examples are the documentation:
- Check every status return; `CHECK(call)` macros that print the failing call and exit
  are idiomatic for example code (use them where failure means a programming bug; handle
  expected conditions — missing camera, `NO_BACKEND`, `UNSUPPORTED` — inline with a
  message instead).
- Prefer the config *constructors* (`mediaway_camera_capture_config_default`,
  `mediaway_desktop_capture_config_screen`, `mediaway_audio_capture_config_microphone`,
  `mediaway_auto_video_encode_config_h264`)
  over hand-filling structs where they exist.
- Comment the ownership of every owned output right where it is freed.

## Example scenarios

`examples/` mirrors the Rust [`examples/`](../../examples/) layout — sector
subfolders, one file per scenario (English comments only). Header comment of each
file must state what is real vs. aspirational.

| File | Capability | Real today? |
|---|---|---|
| `container/mux_roundtrip.c` | container mux + demux roundtrip (90 video + 90 audio fake packets → fMP4 → demux back) | ✅ link+run verified |
| `pipeline/encode_to_mp4.c` | auto H.264 encode of 90 synthetic NV12 frames → `out.mp4` bytes | ✅ link+run verified |
| `pipeline/encode_audio.c` | auto AAC encode of 96 synthetic F32 stereo frames → audio-only fMP4 (ABI v2) | ✅ link+run verified (96 packets → 27385 bytes fMP4) |
| `device/camera_record.c` | camera + mic capture → H.264 + AAC → ONE two-track MP4 (remuxed; audio track registered with the encoder's AudioSpecificConfig) | ✅ link+run verified on real hardware (90 frames + 263 AAC packets → ~630 KB two-track fMP4); video-only fallback without mic/audio backend |
| `device/capture_microphone.c` | microphone capture, raw PCM (no encode) | ✅ link+run verified (real mic) |
| `pipeline/screen_record.c` | screen + mic → encode → MP4 | 🚧 demonstrates the real gap — Screen + NONE gpu → `INVALID_INPUT`, Window → `UNSUPPORTED`; exits gracefully |
| `device/capture_screen.c` | screen capture only | 🚧 same gap demo, capture-only |

## Rules

- English comments only (repo language policy).
- Map existing Rust surfaces; do not invent capabilities the Rust side doesn't have.
- The three headers (`device.h`, `pipeline.h`, `container.h`) are co-includable in one TU —
  the shared value types (`mediaway_rational_t`, `mediaway_sample_format_t`, GPU handle types)
  are double-guarded, so `camera_record.c` now includes all three instead of hand-declaring.
- Not part of the Cargo workspace: not built, linted, or tested by CI.
- Durable changes to the *real* API surface require an ADR (ADR-0004) — this folder is
  exploratory input, not a substitute.

## Building & verifying on Windows

```
cargo build -p mediaway-container-ffi -p mediaway-pipeline-ffi -p mediaway-device-ffi \
    --target x86_64-pc-windows-gnu
gcc -Icrates/mediaway-container-ffi/include bindings/c/examples/container/mux_roundtrip.c \
    -Ltarget/x86_64-pc-windows-gnu/debug -lmediaway_container_ffi -o mux_roundtrip.exe
```

The DLLs must sit next to the `.exe` when running (MinGW cannot link MSVC output).
