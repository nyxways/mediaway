# ADR-0001: Pure Rust codec scope — H.264 baseline decode first, sans-io bitstream boundary

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-sw`

## Context

`mediaway-sw` is the pure-Rust software codec fallback tier: reachable when no HW
encoder/decoder backend (`mediaway-encoder-windows`, `mediaway-decoder-windows`, …) is
available. Per [`docs/ai/wiki/license/policy.md`](../../../docs/ai/wiki/license/policy.md)
and the workspace license rule, it must have **zero C codec FFI, zero `unsafe`, and zero
GPL/LGPL/AGPL/patent-bundling dependency** (no `libav*`, no `x264`/`x265`, no FFmpeg
bindings). The crate's own roadmap ([`docs/roadmap.md`](../docs/roadmap.md)) Stage 0 left
open "ADR for pure Rust codec scope + sans-io boundary"; Stage 1 names "H.264 bitstream /
encode" without deciding whether decode or encode comes first, or what the sans-io shape
is.

The rest of the workspace already has fixed facade traits: [`VideoDecoder`
(`mediaway-decoder/src/video.rs`)](../../mediaway-decoder/src/video.rs) and
[`VideoEncoder` (`mediaway-encoder/src/video.rs`)](../../mediaway-encoder/src/video.rs),
both push/poll session traits over `mediaway_common::{Packet, VideoFrame, StreamInfo}`,
implemented today by the Windows WMF/DX11 backends. A SW fallback should be swappable
behind the same traits so callers (an `auto`-style factory, or app code) can fall back to
`mediaway-sw` without special-casing it.

## Decision

> **First concrete deliverable is an H.264 Baseline-profile decoder, not an encoder.**
> Decode is more tractable to get correct and testable first: it is driven entirely by
> conformant input bitstreams (no rate-control / mode-decision search space), and its
> correctness is checkable field-by-field against the spec and against `ffprobe` as an
> oracle ([ADR-0002](../../../adr/0002-system-oracle.md)). This updates the crate roadmap's
> Stage 1 phrasing ("H.264 bitstream / encode") to reflect decode-first; encode remains a
> later Stage 1 item, not dropped.
>
> **Staged scope within "H.264 baseline decode":**
>
> 1. Bitstream framing: Annex-B (`0x000001`/`0x00000001` start codes) and AVCC
>    (length-prefixed) NAL unit splitting, `emulation_prevention_three_byte` removal,
>    NAL header parsing (`nal_ref_idc`, `nal_unit_type`).
> 2. Header parsing: SPS (§7.3.2.1.1) and PPS (§7.3.2.2) field extraction — profile/level,
>    `seq_parameter_set_id`, chroma format, frame dimensions (with cropping), PPS entropy
>    mode and reference-index defaults. **This ADR's implementation covers steps 1–2.**
> 3. Slice header parsing + macroblock/CABAC or CAVLC pixel reconstruction (decode loop
>    proper) — **not implemented yet**, future work.
> 4. Wire a `mediaway-sw` type implementing `mediaway_decoder::video::VideoDecoder` behind
>    the codec/backend factory as the SW fallback.
> 5. H.264 encode (baseline bitstream writer) — later, after decode is real.
>
> **Sans-io boundary:** all bitstream/header modules (`h264::{nal, sps, pps, bitreader}`)
> take and return in-memory byte slices / owned buffers only — no file, socket, or device
> IO in `mediaway-sw`'s core, matching [`docs/spec/sans-io.md`](../../../spec/sans-io.md).
> When step 3–4 land, the decode *session* type (holding decoder state across
> `push_packet`/`poll_frame` calls) stays sans-io the same way `mediaway-decoder`'s trait
> already requires: callers own packet sourcing (demuxer, network, file) and frame sinks;
> `mediaway-sw` only transforms bytes/frames already in memory.
>
> **Trait fit:** the future decode session implements
> [`VideoDecoder`](../../mediaway-decoder/src/video.rs) verbatim (`push_packet`,
> `poll_frame`, `flush`, `stream_info`) so it is swappable with `mediaway-decoder-windows`.
> Because `mediaway-sw` has no GPU device, it only ever produces
> `VideoFrameStorage::Cpu` frames and should treat
> `VideoOutputPreference::ZeroCopyGpu` as `DecodeError::Unsupported` (mirrors the decoder
> Windows ADR-0001 CPU-preference stub, inverted). The future encoder session mirrors
> [`VideoEncoder`](../../mediaway-encoder/src/video.rs) the same way, accepting only
> `VideoInputPreference::CpuUploadOk`.
>
> **No new dependency yet.** This ADR's implementation adds no new Cargo dependency —
> `mediaway-common` only. Depending on `mediaway-decoder`/`mediaway-encoder` to implement
> their traits is deferred to the step-3/4 work, once there is an actual decode loop to
> back the trait (implementing the trait now with `Unsupported`-only stubs would be a
> skeleton, not the real slice this ADR scopes).
>
> **License/safety re-confirmed:** `#![forbid(unsafe_code)]` stays at the crate root (no
> exceptions — unlike HW backends, this crate's whole purpose is the no-`unsafe`,
> no-C-FFI, no-GPL fallback tier). All new code depends only on `mediaway-common`
> (MIT OR Apache-2.0, no transitive FFmpeg/x264/x265/GPL). Any future dependency (e.g.
> `rav1e` for the Stage 2 AV1 item) needs its own `deps-policy.md` review and, if heavy,
> its own ADR — out of scope here.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Encode first (per literal Stage 1 wording "bitstream / encode") | Rate control / mode decision is a much larger, less testable surface than decode; decode gives a spec-checkable slice sooner |
| Full CABAC/macroblock decode loop in this ADR's implementation | Out of scope for one session; bitstream/header parsing is the correct-sized, independently useful first slice per the roadmap's own Stage 1 ordering |
| Depend on `mediaway-decoder` now and stub `VideoDecoder` with `Unsupported` everywhere | A trait skeleton with no working decode is not a real deliverable; adds a dependency edge before there is anything to justify it |
| Skip Annex-B/AVCC distinction, support only one framing | Both appear in the workspace already (demuxers emit AVCC-style extradata; RTP/file dumps use Annex-B) — the sans-io bitstream layer needs both |

## Consequences

### Positive

- A real, testable, spec-traceable slice lands this session (NAL framing + SPS/PPS),
  independent of how far pixel decode gets later
- Establishes the crate's module shape (`h264::{bitreader, nal, sps, pps, error}`) and
  error type future decode-loop work extends, instead of starting from an empty crate
- Confirms the "always works, no license/HW risk" fallback story stays intact: still zero
  `unsafe`, zero C FFI, zero GPL dependency after this change

### Negative / Trade-offs

- No actual decoded pixels yet — `mediaway-sw` cannot serve as a working `VideoDecoder`
  fallback until step 3/4 land; the SW fallback story remains aspirational until then
- PPS parsing rejects multiple slice groups (`num_slice_groups_minus1 > 0`, FMO/ASO) with
  a dedicated error rather than parsing them — acceptable since FMO is rare in modern
  encoders, but a real gap if a target stream uses it
- SPS/PPS parsing stops before `vui_parameters_present_flag` — VUI (timing, HRD, color
  info) is not extracted; fine for width/height/profile/level but will need revisiting for
  color-accurate decode later

## References

- [`docs/spec/sans-io.md`](../../../spec/sans-io.md)
- [`docs/spec/api-layers.md`](../../../spec/api-layers.md)
- [`mediaway-decoder/src/video.rs`](../../mediaway-decoder/src/video.rs) — `VideoDecoder` trait to mirror
- [`mediaway-encoder/src/video.rs`](../../mediaway-encoder/src/video.rs) — `VideoEncoder` trait to mirror
- [`docs/ai/wiki/license/policy.md`](../../../ai/wiki/license/policy.md) · [`sw-scaffold.md`](../../../ai/wiki/license/sw-scaffold.md)
- Crate roadmap: [`docs/roadmap.md`](../docs/roadmap.md)
- ITU-T Rec. H.264 §7.3.1 (NAL unit syntax), §7.3.2.1.1 (SPS), §7.3.2.2 (PPS), §9.1 (Exp-Golomb) — see [`docs/conventions/external-standards.md`](../../../conventions/external-standards.md) for citation policy (not reproduced here)
