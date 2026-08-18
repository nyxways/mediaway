# Android decode (NDK `AMediaCodec`)

- Status: **Implemented, zero compile/runtime verification** — `mediaway-decoder/src/android/`
  now exists (`mod.rs` + `amediacodec/{codec,csd,nv12,video}.rs`), ADR is **Accepted**. `nv12`/
  `csd` are pure helpers with host-runnable unit tests (`cargo test -p mediaway-decoder`); the
  rest is `target_os = "android"`-gated and unverified — no Android NDK/hardware in this dev
  environment.
- ADR: [0001 (`adr/android/`)](../../../../crates/mediaway-decoder/adr/android/0001-ndk-amediacodec-h264-cpu-out.md)
  — binding choice, decode-specific design, scope, zero-verification caveat.
- Bindings: same [`ndk`](https://crates.io/crates/ndk) 0.9 `features = ["media"]` pin as
  `mediaway-encoder::android`. Local clone at `local/vendor-ref/ndk` confirms
  `MediaCodec::from_decoder_type`, `configure(format, Option<&NativeWindow>, direction)`,
  `output_buffer(index) -> Option<&[u8]>`, `MediaFormat::set_buffer("csd-0"/"csd-1", …)`.
- Codec: H.264 (`video/avc`) only, CPU NV12 output only (`VideoOutputPreference::CpuFramesOk`).
- **General GOP, not IDR-only** — unlike this crate's own `linux::vaapi` decode. `AMediaCodec`
  is a black-box HW codec: it manages DPB/reference frames internally and returns output
  buffers already in presentation order, so no DPB typestate is needed on this crate's side at
  all (a real structural win vs. VA-API, not extra work done here).
- **Output color-format is decoder-chosen, not caller-requested** — the central `AMediaCodec`
  CPU-decode pitfall (unlike encode's `KEY_COLOR_FORMAT` input request). Stage 1 accepts only
  `COLOR_FormatYUV420SemiPlanar` (`21`) from `output_format()`'s `"color-format"` key after
  `OutputFormatChanged`; any other value (`Planar`, `Flexible`, vendor-specific) is an honest
  `DecodeError::Unsupported`, never a guessed byte layout. `"stride"`/`"slice-height"`/crop
  keys strip padding into tightly packed NV12 (`android::amediacodec::nv12`).
- CSD handoff: `extra_data` (Annex-B assumed) split via `mediaway_sw::h264::split_annex_b` into
  SPS/PPS NALs → `set_buffer("csd-0"/"csd-1", …)`. Real catch: `split_annex_b` strips the start
  code from its output (verified from its own source), but `AMediaCodec` documented-ly wants
  csd buffers start-code-prefixed — this backend must re-prepend `00 00 00 01` before
  `set_buffer`, not forward `split_annex_b`'s slices as-is. **Not required**: `AMediaCodec`
  also accepts in-band SPS/PPS from the first packet's own NALs, so `open()` is eager (no lazy
  `Context`/`Surface` creation the way `linux::vaapi` needs).
- Zero-Copy: **not implemented, deferred** — `GpuBufferHandle::AndroidSurface` already exists;
  blocked on a **real** constraint: `ndk::native_window::NativeWindow` needs a JNI
  `jobject`/`JNIEnv` to source a Java `Surface` from, and this is a headless/JNI-independent
  native library (same boundary the encoder ADR drew for its own deferred stage).
- Verification: **zero compile, zero runtime** — no Android NDK/hardware in this dev
  environment, same starting point as `android-encode.md`. An Android CI job (compile-only,
  `nttld/setup-ndk` + `cargo-ndk`) is the first real gate, same plan as encode's.

## Structural differences vs. `linux::vaapi` decode

| `linux::vaapi` | `android::amediacodec` | Note |
|---|---|---|
| App parses SPS/PPS/slice headers | Only byte-splits Annex-B NALs for CSD; RBSP parsing is the device's job | Black box past NAL framing |
| **IDR-only**, no DPB | **General GOP** (P/B frames), DPB fully internal | Free with a black-box codec |
| Lazy `Context`/`Surface` creation (needs parsed SPS first) | Eager `configure()`+`start()` at `open()` | `AMediaCodec_configure` needs no parsed profile up front |
| App picks `VAImageFormat` via `query_image_formats()` | Device picks output layout; app only reads `output_format()` after the fact | On decode, the app cannot dictate output ByteBuffer layout |
