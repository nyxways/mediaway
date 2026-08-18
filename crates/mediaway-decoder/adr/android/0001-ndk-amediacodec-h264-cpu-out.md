# ADR-0001: NDK `AMediaCodec` via the `ndk` crate, H.264 CPU-output decode

- **Status**: Accepted
- **Date**: 2026-08-18
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (module `mediaway-decoder::android`, ADR-0021 `#[cfg]`-gated
  backend — no separate `mediaway-decoder-android` crate)

## Context

This is the **first Android decode backend** in the workspace — `crates/mediaway-decoder/src/`
currently has no `android/` submodule at all (only `audio/`, `linux/`, `vulkan/`, `web/`,
`windows/` exist). The Android **encode** side already shipped
(`mediaway-encoder::android`, [encoder ADR-0001](../../../mediaway-encoder/adr/android/0001-ndk-amediacodec-h264-cpu-upload.md))
via the same NDK `AMediaCodec` native API, so this ADR is the decode counterpart of that one —
same binding choice, same "first backend, no NDK/no device in this dev environment" honesty
posture — but the decode-side call sequence, output shape, and DPB story are genuinely
different from encode and from this crate's own `linux::vaapi` decode precedent, which this ADR
covers.

**Critical environment constraint (unchanged from the encoder ADR):** this repo's dev
environment is a **Windows host with no Android NDK installed and no Android
hardware/emulator**. Unlike this crate's own Linux VA-API decode
([ADR linux/0001](../linux/0001-vaapi-h264-cpu-out.md)), which got real WSL2 `cargo check`
before any hardware existed, this ADR — and the implementation that follows it — will ship with
**zero compile verification and zero runtime verification**, the same starting point as the
Android encoder backend. Every API name and signature below is grounded in a **local clone**
of `rust-mobile/ndk` at `local/vendor-ref/ndk` (`ndk/src/media/media_codec.rs`,
`ndk/src/media/media_format.rs`, `ndk/src/native_window.rs`, read directly for this ADR, not
paraphrased from memory or docs.rs), not a real local build.

### Why decode needs its own design, not a mirror of encode

`AMediaCodec`'s state machine (`from_decoder_type`/`from_encoder_type` → `configure` → `start`
→ per-buffer-index `dequeue`/`queue`/`release` → `stop`) is symmetric between encode and
decode, and this ADR reuses the encoder ADR's binding-choice research verbatim (see
§ Alternatives). But three things are decode-specific and get their own design here:

1. **Codec-specific data (SPS/PPS) handoff** — `AMediaFormat_setBuffer("csd-0", …)` /
   `"csd-1", …)` at `configure()` time, not an encode concept.
2. **Output buffer layout is not caller-selectable.** On encode, `KEY_COLOR_FORMAT` on the
   *input* format is a request the app makes. On decode with no `Surface` (`configure(..., None,
   MediaCodecDirection::Decoder)` — this crate's CPU-output path), the **decoder** picks
   whatever ByteBuffer layout it wants and only reveals it via `output_format()` after a real
   `OutputFormatChanged` event — the app cannot ask for NV12 up front the way it can on encode.
   This is the central, well-documented `AMediaCodec` CPU-decode pitfall (community sources,
   this research pass): output buffers are not guaranteed tightly-packed NV12; `KEY_STRIDE` /
   `KEY_SLICE_HEIGHT` / `KEY_CROP_LEFT` / `KEY_CROP_TOP` / `KEY_CROP_RIGHT` / `KEY_CROP_BOTTOM`
   must be read from the negotiated output format and applied to strip padding, and the
   `KEY_COLOR_FORMAT` value itself can be a vendor-specific constant, not one of the two
   portable software formats (`COLOR_FormatYUV420Planar = 19`,
   `COLOR_FormatYUV420SemiPlanar = 21`).
3. **DPB / reference-frame management is the decoder's problem, not this crate's.** Unlike
   `linux::vaapi` decode — where VA-API's driver/app split forces this crate to parse
   SPS/PPS/slice headers itself and, for Stage 1, restrict scope to **IDR-only** pictures to
   avoid writing a DPB/reference-picture-list implementation — `AMediaCodec` is a black-box HW
   codec: the device manages its own DPB internally and hands output buffers back in
   **presentation order**, already reordered relative to decode order. This crate never sees a
   slice header. That means this backend can support **general H.264 GOP structure (P/B
   frames, not IDR-only)** from Stage 1 with no extra DPB code on this crate's side — a real
   scope difference from `linux::vaapi`, not an oversight there (see § Decision).

## Decision

> Depend on **`ndk` 0.9** (same pin as `mediaway-encoder`'s Android backend) with
> `features = ["media"]`, as a **`[target.'cfg(target_os = "android")'.dependencies]`** entry —
> mirroring `mediaway-decoder`'s existing `cros-libva` (Linux) / `windows` (Windows) gates, so
> `cargo check --workspace` on a non-Android host never invokes `ndk-sys`'s NDK-header `bindgen`
> build script. **No new `[workspace.dependencies]` entry** — the root `Cargo.toml` has no
> `ndk` pin today; `mediaway-encoder`'s Cargo.toml pins it directly
> (`ndk = { version = "0.9", features = ["media"] }`), and this crate's Cargo.toml will do the
> same, not add a workspace-level pin unasked.

- New module `mediaway-decoder::android` (`src/android/`), following this crate's existing
  `src/linux/` shape exactly: a thin `AndroidVideoDecoder` (public,
  `#[cfg(target_os = "android")] inner: Option<amediacodec::AmediaCodecVideoDecoder>` /
  `#[cfg(not(target_os = "android"))] _priv: ()`) implementing `VideoDecoder`, wrapping an inner
  `amediacodec` submodule that does the real work — matching `LinuxVideoDecoder` /
  `vaapi::VaapiH264Decoder`'s split, and `mediaway-encoder::android`'s
  `AndroidVideoEncoder`/`AmediaCodecVideoEncoder` naming.
- Decoder session: `ndk::media::media_codec::MediaCodec::from_decoder_type("video/avc")`
  (`None` → `DecodeError::Backend`, a real honest failure — the Android CDD requires at least
  one AVC decoder, but this crate does not assume a specific one exists).
  `MediaFormat::new()` + `set_str("mime", "video/avc")` + `set_i32("width", …)` /
  `set_i32("height", …)` (hints — the decoder's own SPS may report different dimensions once
  parsed; not asserted as authoritative, see below), then `configure(&format, None,
  MediaCodecDirection::Decoder)`, `start()`.
- **CSD (codec-specific data) handoff — best-effort, not required.** When
  `VideoDecoderConfig::extra_data` is non-empty, split it via
  [`mediaway_sw::h264::split_annex_b`] (already a `mediaway-decoder` dependency, reused by
  `windows::d3d12_video_decode`'s NAL framing per this crate's own `Cargo.toml` comment) into
  individual NAL units, and `set_buffer("csd-0", sps_nal)` / `set_buffer("csd-1", pps_nal)` for
  the first SPS (`nal_unit_type == 7`) / PPS (`nal_unit_type == 8`) found. **Real detail caught
  while reading `split_annex_b`'s own source this session** (`crates/mediaway-sw/src/h264/nal.rs`):
  it returns NAL bytes **without** the start code (`content_begin = pair[0] + 3`, i.e. the slice
  begins at the NAL header byte) — so a naive "split and forward" would hand `AMediaCodec` bare
  NAL bytes, not the start-code-prefixed buffers the documented convention calls for (community
  sources, this research pass: "the data required by MediaCodec must start with
  `\x00\x00\x00\x01`"; **not verified against a real device this session**). This crate must
  therefore **re-prepend** a canonical 4-byte `00 00 00 01` start code to each split SPS/PPS
  slice before `set_buffer`, not simply forward `split_annex_b`'s output as-is. Unlike
  `linux::vaapi`, this crate does **not** parse the SPS/PPS RBSP itself for decode purposes —
  the NAL split is a byte-level framing operation only, not a bitstream parse; `AMediaCodec`
  remains a black box past that framing step. `extra_data` is **not required**: `AMediaCodec`
  decoders documented-ly accept in-band SPS/PPS from the first pushed packet's own NAL units
  too, so an empty `extra_data` at `open()` is allowed (`configure()` proceeds with just the
  MIME/width/height hints) — this collapses `linux::vaapi`'s "lazy `Context`/`Surface` creation
  until first in-band SPS" complexity entirely: `AMediaCodec_configure` does not need a parsed
  profile/resolution up front the way `vaCreateContext` does, so `open()` is eager and non-lazy
  here.
- **Input path**: same `dequeue_input_buffer` → `input_buffer(index).buffer_mut()` (copy
  `Packet::payload` bytes in — Annex-B, same assumption `linux::vaapi` and
  `windows` CPU decode make for `extra_data`/packets at this stage; AVCC demuxer-framed input
  is a deferred open item shared with those two ADRs) → `queue_input_buffer(...,
  time_us_from(packet.pts, config.time_base), flags)` cycle the encoder ADR already uses,
  named `upload_and_queue` for the same cost-disclosure reason. `packet.is_keyframe`/flags are
  not needed as an input flag (decode does not request sync frames); `BUFFER_FLAG_END_OF_STREAM`
  is queued (empty buffer) on `flush()`.
- **Output path — the decode-specific part.** `dequeue_output_buffer` loop:
  - `OutputFormatChanged` → call `output_format()`, read `"color-format"` (i32). Accept only
    `COLOR_FormatYUV420SemiPlanar` (`21`) this stage — the one portable software format that
    maps directly onto this crate's `PixelFormat::Nv12` contract
    (`VideoDecoderConfig::pixel_format` is validated to be `Nv12`, matching `linux::vaapi` /
    `windows` CPU decode's own `validate()` convention). Any other value —
    `COLOR_FormatYUV420Planar` (`19`), `COLOR_FormatYUV420Flexible` (`0x7F420888`), or a
    vendor-specific constant — returns `DecodeError::Unsupported` rather than silently
    misinterpreting the byte layout; **reject, never guess**, same discipline `linux::vaapi`'s
    `Pps::parse` uses for scaling-list fields it does not handle. Also read `"stride"`,
    `"slice-height"`, `"crop-left"`, `"crop-top"`, `"crop-right"`, `"crop-bottom"` (all `i32`,
    documented `MediaFormat` keys; missing `slice-height` on some devices is a documented
    zero-means-"same as height" quirk — treated as `height` when absent) and cache them as this
    session's `OutputLayout`; update `stream_info()`'s `VideoGeometry` from the crop rect
    (`right - left`, `bottom - top`), mirroring `windows::wmf::video_cpu`'s
    `apply_stream_change` geometry update after `MF_E_TRANSFORM_STREAM_CHANGE`.
  - `Buffer(output_buffer)` → a new `android::amediacodec::nv12` module strips `stride`/
    `slice-height` padding and applies the crop rect to produce a tightly packed NV12 `Bytes`
    (luma plane: `stride` row pitch × `slice_height` rows at buffer offset `0`; chroma plane:
    same `stride`, `slice_height / 2` rows, at offset `stride * slice_height` — the documented
    semi-planar layout for this color format) — the Android-specific analog of
    `linux::vaapi::nv12`'s pitch-stripping, different math (stride/slice-height/crop keys vs.
    `VAImage.pitches[]`/`offsets[]`), same purpose and same "this is a genuine driver→CPU copy,
    already accounted for in a `CpuFramesOk`-only path" honesty note. `release_output_buffer(...,
    render: false)` always (no `Surface` this stage).
  - `TryAgainLater` → stop this drain attempt (opportunistic, same shape as the encoder ADR's
    `drain_output`, since `AMediaCodec` output readiness is not guaranteed synchronous with a
    given `push_packet` call).
- **No DPB / reference-frame typestate in this crate.** `AMediaCodec` manages decode order,
  reference pictures, and output reordering internally; this crate only ever sees flat
  input-packet-in / output-frame-out buffer indices, already in presentation order. Scope is
  therefore **general H.264 GOP** (baseline/main/high profile, any `pic_order_cnt_type`, P/B
  frames included) — **not** restricted to IDR-only the way `linux::vaapi` Stage 1 is. This is
  a direct consequence of the black-box codec model, not extra work done here.
- **Zero-Copy `ANativeWindow`/`Surface` output: deferred, not attempted this stage.**
  `configure`'s `surface: Option<&NativeWindow>` parameter and `MediaCodec::set_output_surface`
  already exist in `ndk::media::media_codec` (confirmed from the local clone read) and would
  route decoded frames through `GpuBufferHandle::AndroidSurface` (`AHardwareBuffer*` — already
  declared in `mediaway-common::gpu`, predates any Android backend, per
  [`docs/ai/wiki/zero-copy/handles.md`](../../../../docs/ai/wiki/zero-copy/handles.md)). The
  blocker is real, not scope-avoidance: `ndk::native_window::NativeWindow` construction from a
  Java `Surface` needs `jni_sys::{jobject, JNIEnv}` (confirmed:
  `ndk/src/native_window.rs` imports `jni_sys` directly) — a headless Rust decode library has
  no JVM/Activity context to source a `Surface` from, the same "no JNI, purely native" boundary
  the encoder ADR drew for its own deferred `create_input_surface` stage. `VideoOutputPreference
  ::ZeroCopyGpu` returns `DecodeError::Unsupported` this stage.

## ZCA / typestate shape

Mirrors `AndroidVideoEncoder` and `LinuxVideoDecoder`'s wrapper shape — no `Box<dyn _>`
introduced:

```
pub struct AndroidVideoDecoder {
    #[cfg(target_os = "android")]
    inner: Option<amediacodec::AmediaCodecVideoDecoder>, // closed-after-move sentinel
    #[cfg(not(target_os = "android"))]
    _priv: (),
}

pub(crate) struct AmediaCodecVideoDecoder {
    codec: MediaCodec,
    info: StreamInfo,
    time_base: Rational,
    output_layout: Option<OutputLayout>,   // set on first OutputFormatChanged
    pending: VecDeque<VideoFrame>,         // no Box<dyn> — same shape as encoder's VecDeque<Packet>
    flushed: bool,
}

struct OutputLayout { stride: u32, slice_height: u32, crop: (u32, u32, u32, u32) }
```

| `linux::vaapi` decode | `android::amediacodec` decode | Difference |
|---|---|---|
| `Config` + `Context` + `Picture<S, T>` **typestate**, created lazily on first parsed SPS | Single `MediaCodec` handle, `configure`+`start` eagerly at `open()` | `AMediaCodec_configure` needs no parsed profile/resolution up front, unlike `vaCreateContext` — no lazy-init state machine needed here |
| This crate parses SPS/PPS/slice headers (own `sps.rs`/`pps.rs`/`slice.rs`) | This crate only byte-splits Annex-B NALs (`split_annex_b`) for `csd-0`/`csd-1`; RBSP parsing is the device's job | `AMediaCodec` is a black box past NAL framing — no `BitReader`-level H.264 syntax code in this backend |
| **IDR-only** scope — no DPB, no reference-picture-list, no POC math | **General GOP** (P/B frames) — device DPB is internal and transparent | Direct consequence of black-box decode, not extra work |
| `vaCreateImage`+`vaGetImage`, explicit `VAImageFormat` queried via `query_image_formats()` | `output_format()`'s `"color-format"`/`"stride"`/`"slice-height"`/crop keys, read **after** `OutputFormatChanged` — the decoder picks the layout, this crate cannot request one | On decode (unlike encode), the app cannot dictate the output ByteBuffer layout — only accepts or rejects what the device reports |
| `GpuBufferHandle::Vulkan` / DMA-BUF import — deferred | `GpuBufferHandle::AndroidSurface` (`AHardwareBuffer*`) via `configure`'s `Surface`/`set_output_surface` — deferred, blocked on JNI `Surface` sourcing | Both defer Zero-Copy output this stage; Android's blocker is JNI/JVM context, not a missing type |

`AndroidVideoDecoder` wraps a concrete `amediacodec::AmediaCodecVideoDecoder` behind `Option`
(closed-after-move sentinel), identical to `LinuxVideoDecoder`/`AndroidVideoEncoder`. No new
heap allocation pattern beyond `VideoFrame`/`Bytes` this crate already uses for CPU frame
output; the `nv12` stride-stripping module allocates one owned `Bytes` per frame (same
unavoidable "this is a `CpuFramesOk` path, the copy is already accounted for" cost as
`linux::vaapi::nv12` and `windows::wmf::cpu`).

## Scope (this stage)

**In:**

- H.264 (`video/avc`) decode only, CPU NV12 output only (`VideoOutputPreference::CpuFramesOk`),
  no `Surface` at `configure` time.
- General GOP (not IDR-only) — see § Decision for why this is free with a black-box codec.
- Output color-format restricted to `COLOR_FormatYUV420SemiPlanar` (`21`); any other reported
  format (`Planar`, `Flexible`, vendor-specific) is an honest `DecodeError::Unsupported`.
- `mediaway-decoder::android` module only — **not** wired into any future `auto`/`capability`
  dispatch surface this stage (this crate currently has no `auto` module at all, per
  `capability.rs`'s own doc comment: "decode has exactly one implementation per platform
  today"). `AndroidVideoDecoder::open` is a standalone low-level constructor, same shape as
  `LinuxVideoDecoder::open`.

**Out (deferred, tracked in `docs/roadmap.md` / crate `docs/roadmap.md`):**

- Zero-Copy `AHardwareBuffer`/`ANativeWindow`/`Surface` output (`VideoOutputPreference::
  ZeroCopyGpu`, `GpuBufferHandle::AndroidSurface`) — blocked on a JNI `Surface` source; returns
  `DecodeError::Unsupported`.
- HEVC / AV1 / VP9 decode — `AMediaCodec` supports them per-device; this crate does not yet
  (mirrors the encoder ADR's identical H.264-only Stage 1 scope).
- `COLOR_FormatYUV420Planar` / `COLOR_FormatYUV420Flexible` / vendor-specific output layouts —
  would need either an extra named conversion (Planar→NV12 plane interleave) or the `AImage`/
  `AImageReader` API (`ndk::media::image_reader`) instead of raw ByteBuffer — left for a future
  stage once a real device's actual reported color-format is known.
- AVCC (length-prefixed) demuxer-sourced input — Annex-B assumed for both `extra_data` and
  packets, the same open item `linux::vaapi` ADR-0001 and `windows` ADR-0001 already track for
  their own crates.
- Wiring into any future decode dispatch/capability surface.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Hand-written `bindgen` FFI against NDK media headers directly | Reinvents buffer-index lifecycle safety `ndk::media::media_codec` already provides — same reasoning the encoder ADR and `linux::vaapi` ADR both used against hand-rolled bindings for their respective native APIs. |
| `ndk-sys` raw FFI directly (skip the safe `ndk` layer) | Same unsafe-ownership problem; `ndk` is a thin, actively maintained safe layer over exactly these bindings (confirmed from the local clone: `configure`/`dequeue_input_buffer`/`dequeue_output_buffer`/`output_buffer`/`release_output_buffer` are all safe `fn`s) with no meaningful downside to using it. |
| JNI `android.media.MediaCodec` via `jni`/`jni-android-sys` | Requires a live `JNIEnv`/JVM attach; ties this backend to an Activity context, the same reasoning the encoder ADR rejected it for. Would also make the deferred `Surface` Zero-Copy stage's blocker moot (a JNI-based path *could* source a `Surface` directly) — noted as a possible **future** alternative if the JNI-independence requirement is ever relaxed, but not chosen here. |
| Support only `COLOR_FormatYUV420Flexible` (documented as universally supported since API 21 `LOLLIPOP_MR1`) instead of `SemiPlanar` | `Flexible` in ByteBuffer (non-`Image`) mode still resolves to a device-chosen concrete sub-layout the app must introspect via `getInputImage`/`getOutputImage` (the `AImage`/`Image` API) to interpret correctly — using it correctly from raw `output_buffer()` bytes without the `Image` API is not well-specified. Restricting Stage 1 to the one color-format value (`SemiPlanar = 21`) whose flat-buffer layout is unambiguous from `stride`/`slice-height` alone is the narrower, honest choice; `AImageReader` is future work if devices commonly report `Flexible` instead. |
| Model DPB/reference frames explicitly (a `Picture<S, T>`-style typestate, mirroring `linux::vaapi`) | Not applicable — `AMediaCodec` never exposes slice/picture-level state to the app; there is no reference-picture-list or POC data for this crate to hold. Building one would be speculative complexity with nothing real to model. |
| Depend on system `ffmpeg`/`libavcodec` `MediaCodec` wrapper | Forbidden — FFmpeg stays a test/dev oracle only (ADR-0002), never a product dependency. |

## Consequences

### Positive

- Small, real unsafe surface: **zero** `unsafe` blocks planned in `mediaway-decoder::android`
  (all FFI unsafety lives in `ndk`/`ndk-sys`) — `#![forbid(unsafe_code)]` at the module root,
  matching `linux::vaapi`.
- **General GOP decode from Stage 1**, not IDR-only — a real scope win over this crate's own
  `linux::vaapi` decode, entirely free from the black-box codec model (no DPB code needed).
- `ndk` is already a dependency this workspace pulls for the Android encoder — no new
  dependency-review surface, only a decode-specific API subset of the same crate.
- `GpuBufferHandle::AndroidSurface` already existing in `mediaway-common` means the deferred
  Zero-Copy output stage has no blocking type-design work left, only JNI/`Surface`-sourcing
  wiring.
- Reject-not-guess policy on `"color-format"` avoids ever silently misinterpreting a
  vendor-specific output layout as NV12 — a real, documented `AMediaCodec` footgun this ADR
  designs around explicitly rather than discovering later via corrupted frames.

### Negative / Trade-offs

- **Zero compile verification and zero runtime verification as authored** — every method name,
  `MediaFormat` key string, and buffer-layout claim above is a research-pass read of the local
  `rust-mobile/ndk` clone plus public documentation, not a real local build or a real device.
  Treat all of it as unverified until an Android CI job (mirroring the encoder ADR's
  `nttld/setup-ndk` + `cargo-ndk` compile-only job) runs against this module, and until real
  hardware/emulator verification happens after that.
- Restricting output to `COLOR_FormatYUV420SemiPlanar` only means this backend may report
  `DecodeError::Unsupported` on real devices/streams that only offer `Flexible` or a
  vendor-specific format — narrower device coverage than a full `AImage`-based path would give,
  by design (see § Alternatives).
- The CSD (`csd-0`/`csd-1`) split-and-forward step assumes `extra_data`'s NAL framing and byte
  layout without parsing SPS/PPS content — if a caller's `extra_data` contains more than one SPS
  or PPS (rare but legal), only the first of each is forwarded; multi-SPS/PPS streams are an
  unhandled edge case this stage.
- Build-time hard dependency on the Android NDK toolchain for any `target_os = "android"` build
  of this crate — acceptable per the `cfg(target_os = "android")` gate (never required for
  Windows/Web/Linux/other builds), unavoidable for any Android Rust target regardless of binding
  choice.
- `ndk = "0.9"` is pre-1.0 — same semver-risk class already accepted for the encoder side of
  this same dependency.

## References

- [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md)
- [`docs/spec/crate-packaging.md`](../../../../docs/spec/crate-packaging.md) — `android`
  platform suffix (already reserved)
- [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) — honesty
  requirement this ADR follows for its own unverified-detail admissions
- `mediaway-decoder` [ADR-0021](../../../../docs/adr/0021-workspace-consolidation.md) —
  `#[cfg]`-gated backend modules, no separate platform crate
- [`adr/linux/0001-vaapi-h264-cpu-out.md`](../linux/0001-vaapi-h264-cpu-out.md) — this crate's
  own decode-scope/DPB-honesty precedent, and the structural-differences comparison this ADR
  builds on
- [`mediaway-encoder` ADR android/0001](../../../mediaway-encoder/adr/android/0001-ndk-amediacodec-h264-cpu-upload.md)
  — binding-choice research shared verbatim by this ADR, and the "zero compile verification" /
  CI-plan precedent this ADR follows
- [`ndk` on crates.io](https://crates.io/crates/ndk) ·
  [GitHub (`rust-mobile/ndk`)](https://github.com/rust-mobile/ndk) (`MIT OR Apache-2.0`) —
  local clone read for this ADR: `local/vendor-ref/ndk/ndk/src/media/media_codec.rs`,
  `media_format.rs`, `native_window.rs`
- [`ndk::media::media_codec` docs.rs](https://docs.rs/ndk/latest/ndk/media/media_codec/index.html)
- `docs/ai/wiki/zero-copy/handles.md` — `GpuBufferHandle::AndroidSurface`, already declared
- `docs/roadmap.md` § platform order (Windows → Web → Linux → other) · crate
  `docs/roadmap.md` § Stage 4 — Other
