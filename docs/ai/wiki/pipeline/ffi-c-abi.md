# mediaway-ffi — auto-encode → fMP4 C ABI

Second `mediaway-*-ffi` crate, after `mediaway-ffi`. Wraps
`mediaway_pipeline::platform::AutoEncoder` + `EncodeSession` (auto OS/GPU
video encoder wired straight into a single-track fMP4 muxer) over a C ABI.
Full design:
[`crates/mediaway-ffi/adr/0001-auto-encode-c-abi.md`](../../../../crates/mediaway-ffi/adr/0001-auto-encode-c-abi.md),
GPU input:
[`adr/0002-gpu-frame-input-c-abi.md`](../../../../crates/mediaway-ffi/adr/0002-gpu-frame-input-c-abi.md).

## Shape

- Two opaque handles: `AutoEncoderHandle = Box<dyn VideoEncoder>` (no
  `poisoned` flag — its only ops destroy the pointer unconditionally) and
  `EncodeSessionHandle { poisoned, inner: EncodeSession<Box<dyn VideoEncoder>> }`.
  `mediaway_encode_session_open` **unconditionally consumes** the encoder
  handle, success or failure (matches `EncodeSession::open(encoder: E)`
  taking `E` by value) — do not close the encoder handle afterward.
- `mediaway_auto_video_encode_config_t` is a plain value struct (no handle,
  no free) — fields: `codec`, `width`, `height`, `time_base`, `bitrate_bps`,
  `pixel_format`, `gpu_device` (ADR-0002; `MEDIAWAY_GPU_DEVICE_NONE` keeps the
  CPU-only path). **Still deferred:** `backend`, `max_path_class` —
  `AutoVideoEncodeConfig`'s own `max_path_class` default (`CpuUpload`) already
  permits Zero-Copy/GPU-copy, so `gpu_device` alone is enough to reach the GPU
  path from C.
- `mediaway_video_frame_t` (write_frame input) carries `storage_kind`
  (`CPU`/`GPU`) deciding whether `raw_bytes`/`raw_bytes_len` or `gpu_buffer`
  is read — same flat-struct-plus-discriminant idiom
  `mediaway-ffi`'s `MediawayDesktopFrame` uses, opposite ownership
  direction (borrowed *input* here vs. borrowed *output* there). See
  [zero-copy/gpu-interop.md](../zero-copy/gpu-interop.md).
- Fresh `mediaway_pipeline_status_t` (13 values, own numbering) — **not**
  named or numbered like `mediaway-ffi`'s `mediaway_status_t`: a
  reused typedef name with a different enumerator set is a real C
  redefinition hazard if a consumer ever includes both headers.
  `mediaway_pipeline_codec_kind_t`, however, **does** mirror
  `mediaway_codec_kind_t`'s numeric values 1:1 (different name, same
  numbers) — both wrap the exact same shared Rust `CodecKind`, so keeping
  the mapping aligned costs nothing and eases a future merge.
- `mediaway_pipeline_ffi_buffer_free(data, len)` — **its own name**, not
  `mediaway_buffer_free`. Real reason: this crate's `Cargo.toml` also builds
  a `staticlib`; two crates exporting the identical global symbol name is a
  guaranteed duplicate-symbol link error if both are ever statically linked
  into one consumer.
- ABI version `MEDIAWAY_PIPELINE_FFI_ABI_VERSION` is `1` (bumped from `0` by
  ADR-0002's struct growth — a breaking layout change even though existing
  field offsets are unchanged).

## Panic safety

Same `catch_unwind` + per-handle `poisoned` pattern as
`mediaway-ffi` (see that crate's wiki page). `AutoEncoderHandle` is
the one deliberate exception — no `poisoned` field, justified above.

## Deferred (see ADR-0001 § Deferred, ADR-0002 § Negative)

`backend`/`max_path_class` config fields (no way to force `Readback`/
`Software` or pin a vendor SDK from C yet); `cbindgen` adoption;
`mediaway`'s unconditional decode/device Cargo deps (same class of
gap as `mediaway-container`'s unconditional format-core deps); screen/camera
capture and decode C surfaces.

`mediaway-ffi` unification — **resolved** by
[ADR-0015](../../../../docs/adr/0015-common-ffi-unification.md): an
`rlib`-only internal crate now shares the `Rational`/`CodecKind` value-type
mirrors and the buffer leak/reclaim helper *implementation* between
`-ffi` crates. Status enums (`mediaway_status_t` vs.
`mediaway_pipeline_status_t`) and each crate's own exported free-function name
stay independent — that independence was judged a feature (zero coordination
needed to link both crates), not a gap to close.
