# ADR-0003: Multi-format container C ABI (WebM first; Ogg/ADTS/FLV/MPEG-TS/MP3/WAV scoped)

- **Status**: Accepted
- **Date**: 2026-08-07
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi` (container module; still numbered under the pre-merge
  `mediaway-container-ffi` `adr/container/` folder — ADR-0021 merged the crate, not this
  folder's numbering)

## Context

`mediaway-container`'s Rust facade wraps 8 sans-io formats (`mp4`, `webm`, `flv`, `ts`,
`mp3`, `ogg`, `adts`, `wav`), but `container.h`'s C ABI — and therefore every non-Rust
binding (C, C++, C#, Python, Node) — only ever opened `mp4::Muxer`/`mp4::Demuxer`
(`mediaway_muxer_create`/`mediaway_demuxer_create` take zero arguments, hardcoded to MP4).
`docs/CHANGELOG.md`'s v0.1.3 entry already shipped real WebM VP8 mux/demux at the Rust
level ("`CodecKind::Vp8`, wired into `mediaway-container::webm` mux + demux — closes the
WebM VP8 gap") with **no C ABI path to reach it at all** — this ADR closes that specific
gap and scopes the remaining 6 formats honestly rather than force-fitting all 8 into one
shape.

### The 8 formats do not share one method shape

Investigated directly (not assumed) by reading every format module:

| Format | Track registration | Push shape | Shares `mp4`'s shape? |
|--------|---------------------|------------|------------------------|
| `mp4` | `add_track(StreamInfo) -> Result<u32, Error>`, typestated `Open`→`Live` via `begin()` | `push_packet(&Packet)` / `poll_bytes` | — (baseline) |
| `webm` | Identical: `add_track`/`begin`/`push_packet`/`poll_bytes`, same typestate | Identical | **Yes** |
| `ogg` | None — `Muxer::new(serial: u32)`, immediately live, one implicit logical bitstream | `push_packet(&Packet)` / `poll_bytes` | No track registration step |
| `adts` | None — `Muxer::new(sample_rate, channels) -> Result<Self, Error>`, immediately live | Identical push/poll | No track registration step |
| `flv` | `add_track(&StreamInfo) -> Result<(), Error>` (fixed one video + one audio slot) | `push_packet(&Packet, &mut Vec<u8>)` — writes directly into an output buffer, no `poll_bytes` | No — deliberately does not implement `Mux` (module docs) |
| `ts` | `Muxer::new(program_number, pmt_pid, &[ElementaryStream])` | `write_access_unit(pid, data, pts_90k, dts_90k, random_access, out)` — raw 90 kHz clock, not a `Packet` | No — deliberately does not implement `Mux` (90 kHz clock, module docs) |
| `mp3` | `Muxer::new(header: FrameHeader) -> Result<Self, Error>` | `write_frame(frame_body, padding: bool, out)` — padding bit has no `Packet` slot | No — deliberately does not implement `Mux` (module docs) |
| `wav` | `Muxer::new(sample_rate, channels, bits_per_sample)` | `push_packet(&Packet)` (no `Result`) + consuming `finish(self) -> Vec<u8>`; demux is a standalone one-shot `parse(&[u8]) -> Result<(StreamInfo, Packet), Error>`, not a streaming `Demuxer` | No — whole-buffer only (module docs) |

Only `mp4`/`webm` share the exact same typestated, multi-track shape. `ogg`/`adts` are
single-implicit-stream (no track registration at all). `flv`/`ts`/`mp3`/`wav` each have a
method shape their own module docs say was **deliberately** kept distinct from the shared
`Mux`/`Demux` traits — forcing a fifth "generic" C ABI shape across all 8 would misrepresent
real API differences the Rust layer itself refused to paper over (this workspace's own
simplicity rule: no abstraction that doesn't actually fit).

## Decision

> Extend the existing `mediaway_muxer_t`/`mediaway_demuxer_t` handles to also open **WebM**
> via a new `mediaway_container_format_t` enum + `mediaway_muxer_create_for_format`/
> `mediaway_demuxer_create_for_format` functions. Ogg/ADTS/FLV/MPEG-TS/MP3/WAV are
> **explicitly deferred** — each needs its own dedicated C ABI shape (single-stream handles
> for Ogg/ADTS; bespoke out-buffer/raw-clock/frame+padding/one-shot shapes for the other
> four), tracked as follow-up work, not force-fit here.

### 1. New functions, not a parameter on existing ones

`mediaway_muxer_create_for_format(format)`/`mediaway_demuxer_create_for_format(format)` are
**new** functions alongside the existing zero-argument `mediaway_muxer_create`/
`mediaway_demuxer_create` — adding a `format` parameter to an already-shipped C function
would silently break every existing binding's call at the ABI level (wrong argument count),
not just source. Same reasoning `mediaway_muxer_create_with_fragment_batch` already
established when it was added alongside `mediaway_muxer_create` rather than as a parameter.

### 2. `MuxerState`/`DemuxerHandle` become per-format enums

```rust
enum MuxerState {
    Mp4Open(mp4::mux::Muxer<mp4::mux::Open>),
    Mp4Live(mp4::mux::Muxer<mp4::mux::Live>),
    WebmOpen(webm::Muxer<webm::Open>),
    WebmLive(webm::Muxer<webm::Live>),
}
enum DemuxerState {
    Mp4(mp4::Demuxer),
    Webm(webm::Demuxer),
}
```

`add_track`/`begin` are inherent methods, not part of any shared trait spanning `Open` and
`Live` — every muxer-side FFI function is one `match` arm per variant (no forced
abstraction). The demuxer side is different: `mp4::Demuxer`/`webm::Demuxer` **do** both
implement `mediaway_container::Demux` (`push_bytes`/`streams`/`poll_packet`) identically, so
`DemuxerState::as_demux_mut()`/`as_demux()` return `&mut dyn Demux`/`&dyn Demux` once,
avoiding 3x duplicated match arms — the muxer and demuxer sides genuinely differ in how much
generic dispatch is honest here, and the code reflects that rather than picking one style
for both.

### 3. `MediawayStatus` gains 2 variants, not a new enum

`UnsupportedCodec` (webm's `add_track` on a codec with no WebM `CodecID`) and
`UnknownStream` (`push_packet` referencing an unregistered track) — both real, expected
rejections `mp4::Error`'s narrower variant set had no slot for. One shared status enum
across every format this crate's `mediaway_muxer_t`/`mediaway_demuxer_t` wraps (not a new
per-format enum) — matches this crate's existing container-vs-pipeline-vs-device status-enum
boundary (one per *capability*, not per *format-within-a-capability*).

### 4. `ClearKey` decryption stays MP4-only

`webm::Demuxer` has no `DemuxDecrypt` impl (no CENC/`ClearKey` in this crate for WebM) —
`mediaway_demuxer_set_decryption_key`/`clear_decryption_key` on a WebM-backed handle return
`MEDIAWAY_STATUS_INVALID_STATE` rather than silently no-opping.

### 5. Real, separate bug found and fixed in the same pass

`container.h`'s hand-written `mediaway_codec_kind_t` enum was missing `MEDIAWAY_CODEC_VP8`
entirely — the Rust-side `MediawayCodecKind` (`common/types.rs`) already had `Vp8 = 12`
since the v0.1.3 WebM-VP8 work, but the C header was never updated, so **no C caller could
even name VP8** regardless of this ADR. Fixed alongside this work since it directly blocks
the WebM path this ADR adds (`MEDIAWAY_CODEC_VP8 = 12` added, matching the existing Rust
discriminant).

### 6. ABI version bump

`MEDIAWAY_CONTAINER_FFI_ABI_VERSION` 0 → 1 — new enum, 2 new functions, 2 new status
variants, one corrected enum value (VP8) that changes the header's semantics even though the
integer layout of `mediaway_codec_kind_t` itself was already reserving slot 12 unused.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| One generic `mediaway_muxer_create(format)` covering all 8 formats | The Rust layer's own module docs document *why* `flv`/`ts`/`mp3`/`wav` don't implement the shared `Mux` trait — forcing them through the same C shape would either silently drop real functionality (FLV's out-buffer `push_packet`, TS's raw 90 kHz clock, MP3's padding bit, WAV's one-shot semantics) or require inventing an FFI-only abstraction the Rust core deliberately doesn't have |
| Add a `format` parameter to the existing `mediaway_muxer_create`/`mediaway_demuxer_create` | Breaks every already-shipped binding's call at the ABI level (wrong arg count), not just source — see Decision §1 |
| A new, independently-numbered status enum per format | This crate's status-enum boundary is per capability (container vs. pipeline vs. device), not per format within a capability — a `mediaway_webm_status_t` would be inconsistent with every other multi-codec/multi-format surface this crate already has (e.g. `mediaway_codec_kind_t` spans many codecs under one container status enum today) |
| Do Ogg/ADTS/FLV/MPEG-TS/MP3/WAV in the same pass | Each needs its own real design decision (single-stream handle shape for Ogg/ADTS; three genuinely different bespoke shapes for FLV/TS/MP3/WAV) — rushing all 6 without the same field-by-field verification this ADR did for WebM risks shipping an unsafe FFI surface that wasn't actually checked against its real Rust API |

## Consequences

### Positive

- WebM (with real VP8 support since v0.1.3) is now reachable from C/C++/C#/Python/Node —
  previously reachable only by depending on the Rust `mediaway-container` crate directly.
- Fixes a real, separate, already-shipped-but-unreachable bug (`container.h` missing
  `MEDIAWAY_CODEC_VP8`).
- Verified end-to-end, not just compiled: `tests/webm_container_smoke.rs` round-trips 5
  synthetic VP8 frames through `mediaway_muxer_create_for_format` → `add_video_track` →
  `begin` → `push_packet` → `poll_bytes` → `mediaway_demuxer_create_for_format` →
  `push_bytes` → `poll_packet`, plus a dedicated test confirming `set_decryption_key` fails
  honestly (not silently) on a WebM handle.

### Negative / Trade-offs

- Only 2 of 8 formats (`mp4`, `webm`) are reachable from the C ABI after this pass — Ogg,
  ADTS, FLV, MPEG-TS, MP3, WAV remain Rust-only. Each needs its own ADR/design pass (see
  Deferred below), not bundled into this one.
- No language binding (C++/C#/Python/Node) wiring in this pass — this ADR covers the C ABI
  (Rust FFI crate + hand-written header) only.

## Deferred to a later ADR

- **Ogg** (`mediaway_ogg_muxer_t`/`mediaway_ogg_demuxer_t`): dedicated handles,
  `mediaway_ogg_muxer_create(serial: u32)` — no track registration, immediately live, reuses
  `mediaway_packet_view_t`/`mediaway_packet_t` (already codec-agnostic).
- **ADTS** (`mediaway_adts_muxer_t`/`mediaway_adts_demuxer_t`): dedicated handles,
  `mediaway_adts_muxer_create(sample_rate, channels)` — same single-implicit-stream shape as
  Ogg, different constructor args.
- **FLV**: needs a `push_packet` variant that writes directly into a caller-managed output
  buffer (no `poll_bytes` step) and a fixed one-video/one-audio-slot `add_track`.
- **MPEG-TS**: needs `pts_90k`/`dts_90k` as explicit `uint64_t` parameters instead of a
  `mediaway_packet_view_t`'s track-timebase `pts`/`dts` — the 90 kHz clock is not a
  per-track choice the existing packet type can represent.
- **MP3**: needs an explicit `padding: bool` parameter `write_frame` requires for
  correct bit-reservoir framing — no slot for it in the existing packet types.
- **WAV**: needs a one-shot, whole-buffer shape (`push_packet` + consuming `finish`) on the
  mux side and a single `parse(data) -> (stream_info, packet)` call on the demux side —
  fundamentally not the incremental push/poll shape every other format above uses.
- **Language binding wiring** (C++/C#/Python/Node) for WebM, and for whichever of the above
  land — out of scope for every ADR in this list until the C ABI side is real and tested.

## References

- `crates/mediaway-container/src/{mp4,webm,ogg,adts,flv,ts,mp3,wav}.rs` — the 8 format
  modules' actual method shapes (source of truth for the table above)
- `adr/container/0001-mp4-mux-demux-c-abi.md` — the MP4-only C ABI this extends
- `adr/container/0002-clearkey-decrypt-and-fragment-batch-c-abi.md` — `DemuxDecrypt`,
  MP4-specific
- `crates/mediaway-ffi/tests/webm_container_smoke.rs` — the round-trip verification
- `docs/CHANGELOG.md` v0.1.3 — WebM VP8 mux/demux landing at the Rust level with no C ABI
  path (the gap this ADR closes for WebM)

ADRs are **English**. Numbering is local to this `adr/` folder.
