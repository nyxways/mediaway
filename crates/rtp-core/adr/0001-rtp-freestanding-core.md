# ADR-0001: `rtp-core` — freestanding RTP payloadization for H.264/HEVC

- **Status**: Accepted
- **Date**: 2026-08-05
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `rtp-core`

## Context

The workspace has real, hardware-verified H.264/HEVC video encoders (WMF, Vulkan,
D3D12) but no RTP layer at all — the only way to stream their output over a network
today would be a custom, non-interoperable UDP protocol. RTP (RFC 3550) plus its
H.264 (RFC 6184) and HEVC (RFC 7798) payload formats is the standard-interop path
for low-latency streaming (WebRTC, SIP, RTSP), so this is a real, previously-open
gap, not speculative scope.

## Decision

> New unprefixed freestanding crate `rtp-core` (naming: ADR-0012), sans-io, no
> Mediaway dependency. Builds and parses the RTP fixed header (RFC 3550 §5.1,
> minimal 12-byte case) and payloadizes/depayloadizes H.264 and HEVC NAL units
> only — **single-NAL-unit packets** and **FU-A/FU fragmentation**. No other
> codec, no RTCP, no SRTP, no socket I/O.

- Module split: `header.rs` (`RtpHeader` + `RtpPacket` — the 12-byte fixed
  header shared by every payload format), `h264.rs` (RFC 6184: single-NAL +
  FU-A), `hevc.rs` (RFC 7798: single-NAL + FU), `error.rs` (one crate-wide
  `thiserror` enum). `h264`/`hevc` are kept as two separate modules rather than
  one generic "FU codec" abstraction: H.264's NAL header is 1 byte with a
  1-byte FU header, HEVC's is 2 bytes with a differently-laid-out 1-byte FU
  header (RFC 7798 §4.4.3's `FuType` is 6 bits with no reserved bit, vs RFC
  6184 §5.8's 5-bit `Type` + 1 reserved bit) — sharing code across that would
  cost more than it saves for two codecs this narrow.
- **Minimal 12-byte RTP header only** — no CSRC list exposed (skipped over
  correctly, not dropped silently), no header extension, no padding. Matches
  this workspace's established "narrowest self-consistent" scope pattern for
  bitstream headers. `RtpHeader::parse` returns
  `Error::HeaderExtensionUnsupported` / `Error::PaddingUnsupported` rather than
  silently mis-parsing a packet that actually carries either — a wrong-but-
  silent parse here would be worse than an honest, named gap.
- **Sequence number is an internal `Packetizer` counter**, not caller-managed
  per call — the task's own "internal monotonic counter the session owns"
  option. `Packetizer::new` takes an `initial_sequence_number`; every packet
  `packetize` produces increments it (`wrapping_add(1)`), matching RFC 3550's
  own "SHOULD be random initial value, increments by one" without this crate
  owning random-number generation (caller supplies the seed).
- **`max_payload_size`, not raw "MTU"** — the field/parameter name is
  deliberately not `mtu`: this crate has no network-layer knowledge (no IP
  version, no IP options, no UDP), so it cannot compute payload budget from a
  raw link MTU itself. Rustdoc spells out the arithmetic the caller must do
  (`1500 - 20 - 8 - 12 = 1460` for the common Ethernet/IPv4 case) rather than
  accepting a plain "mtu" that would silently be 20-40 bytes too large if
  passed the raw link value. This is the one real "caveats and clarity"
  footgun this crate's own scope creates, and it is named + documented rather
  than left implicit.
- **90 kHz RTP clock rate is a crate constant, not configurable** —
  `RTP_VIDEO_CLOCK_RATE_HZ`, cited from RFC 6184 §4.2 and RFC 7798 §4.4 (both
  mandate it for video). `packetize` takes an already-90kHz-scaled
  `timestamp: u32` rather than a `Packet`-shaped `pts` + timebase, keeping the
  conversion (which needs a caller's own timebase type) out of this crate.
- **Depacketize assumes in-order arrival** — a first-pass reassembler, not a
  loss/reorder-tolerant one. An out-of-order FU-A/FU continuation with no
  matching start (`Error::MissingFuStart`) or a new start before the previous
  NAL's end fragment arrived (`Error::UnexpectedFuStart`) are reported as
  errors, not silently discarded or guessed at. A jitter-buffer-backed
  reordering reassembler is a deliberate, documented future expansion (see
  `docs/roadmap.md`), not assumed complexity for this pass.
- **STAP-A/STAP-B/MTAP (H.264) and AP (HEVC) aggregation packets are cut, not
  built** — see § Scope cuts below.

## Scope cuts (explicit, not silent)

| Cut | Why deferred |
|-----|---------------|
| Aggregation packets (H.264 STAP-A/STAP-B/MTAP16/MTAP24; HEVC AP) | Real efficiency feature (e.g. packing SPS+PPS+IDR-start into one packet) but adds a second packet-type family with its own size-accounting and byte-layout per codec; FU-A/FU fragmentation alone already covers correct behavior for any NAL size relative to MTU. `depacketize` reports `Error::AggregationPacketUnsupported` rather than silently dropping an aggregated packet's NAL units, so a caller talking to a real aggregation-using peer gets an honest error, not silent data loss. |
| H.264 interleaved mode (FU-B, type 29) | A rarely-used mode (out-of-order-tolerant transmission with explicit DON) with no current caller; `depacketize` reports `Error::InterleavedFragmentUnsupported`. |
| HEVC PACI (type 50) | Payload Header Extension Structure for TSCI signaling — niche, no current caller; `depacketize` reports `Error::PaciPacketUnsupported`. |
| HEVC DONL/DOND (decoding-order-number fields) | Only meaningful with `sprop-max-don-diff > 0` (out-of-order transmission signaling), which this crate's in-order-only depacketize scope never sets; never written or expected. |
| RTCP (sender/receiver reports, retransmission, congestion control) | Separate, larger scope — a full companion protocol, not a payload-format detail. |
| SRTP (encryption) | Separate scope; this crate stays a plaintext RTP marshaller. |
| Any codec other than H.264/HEVC (AAC/Opus RTP — RFC 3640/RFC 7587) | Real future work but out of this pass; matches this workspace's currently-shipped video encoders (H.264/HEVC only). |
| Socket I/O | Sans-io, per this workspace's absolute rule — every public method takes/returns byte buffers; no `std::net` anywhere in this crate. |
| Loss/reorder-tolerant depacketize (jitter buffer) | Named above — first pass assumes in-order arrival; a real reassembler is legitimate future scope, not assumed complexity here. |

## Findings from implementation (RFC fields that shaped design choices)

- RFC 6184 §5.8 states the FU indicator's `Type` field distinguishes FU-A (28)
  from FU-B (29) but the **FU header** (`S`/`E`/`R`/`Type`, 5-bit type + 1
  reserved bit) is identical for both — only the packet-level presence of a
  trailing 16-bit DON field differs. This crate rejects FU-B outright at the
  indicator-type check (before even looking at the FU header), rather than
  parsing the shared header shape and then discovering a DON field it doesn't
  know how to skip.
- RFC 7798 §4.4.3 Figure 10 (`|S|E|FuType|`, FuType 6 bits) has **no reserved
  bit at all**, unlike H.264's FU header (`|S|E|R|Type|`, 5-bit type) — an easy
  place to copy-paste a wrong bit width across the two codecs' FU header
  packing/unpacking if `h264.rs`/`hevc.rs` shared a generic implementation;
  named in this ADR's § Decision as part of why the two stayed separate
  modules.
- RFC 7798 §1.1.4's NAL unit header packs `LayerId` (6 bits) split across two
  bytes — 1 bit in the first byte (after `F`+`Type`), 5 bits in the second
  byte (before `TID`) — not byte-aligned like H.264's single-byte header. This
  crate's `decode_nal_header`/`encode_nal_header` helpers isolate that bit
  math in one place (`hevc.rs`), round-trip-tested directly
  (`nal_header_encode_decode_round_trips`) before being exercised indirectly
  through packetize/depacketize.
- No round-trip bug was found by this crate's own tests (unlike `mpeg-ts-core`'s
  adaptation-field stuffing bug) — the field layouts above were read carefully
  enough from the cached RFC text up front that the first working
  implementation passed every round-trip test. Recorded here for honesty: this
  is not a claim of superior process, just what happened this session.

## Consequences

- Real, standard-interop RTP payloadization now exists in the workspace,
  unblocking a future WebRTC/RTSP/SIP streaming adapter (not built by this
  ADR — this crate only marshals bytes, no session/signaling layer).
- Aggregation packets, RTCP, SRTP, and non-H.264/HEVC codecs remain real gaps,
  tracked in `docs/roadmap.md`, not silently dropped.
- A caller talking to a peer that only sends aggregation/interleaved/PACI
  packets gets an honest, named error from `depacketize` rather than silent
  data loss or a panic.
- No `mediaway-*` facade wiring yet (freestanding core only, matching
  `mpeg-ts-core`/`rtmp`'s own "Accepted at the design/dependency level, not a
  claim of end-to-end wiring" posture).

## References

- `local/standards/rfc-3550-rtp/rfc3550.txt` §5.1 (RTP fixed header field
  layout) — registry id `rfc-3550-rtp`
- `local/standards/rfc-6184-rtp-h264/rfc6184.txt` §5.6 (single NAL unit
  packet), §5.7 (aggregation packets, cut), §5.8 (FU-A/FU-B) — registry id
  `rfc-6184-rtp-h264`
- `local/standards/rfc-7798-rtp-hevc/rfc7798.txt` §1.1.4 (NAL unit header),
  §4.4.1 (single NAL unit packets), §4.4.2 (AP, cut), §4.4.3 (FU), §4.4.4
  (PACI, cut) — registry id `rfc-7798-rtp-hevc`
- `docs/standards/registry.toml` — BLAKE3 digests pinning all three RFCs above
- `crates/mpeg-ts-core/adr/0001-mpeg-ts-freestanding-core.md` — closest
  structural precedent: new unprefixed freestanding sans-io crate, no
  Mediaway dependency, "frame already-encoded data" boundary, cites RFC/spec
  sections for field-layout decisions
- `crates/rtmp/adr/0001-rtmp-freestanding-core.md` — sibling protocol-core
  ADR (not container-format); same sans-io byte-slice-in/out shape
- ADR-0012 (workspace) — unprefixed freestanding-core naming
- `docs/conventions/error-handling.md` — `thiserror`, `#[non_exhaustive]`
- `docs/spec/sans-io.md` — byte-slice-in/out core boundary this crate's
  `RtpHeader`/`RtpPacket`/`Packetizer`/`Depacketizer` all follow
