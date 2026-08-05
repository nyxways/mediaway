# rtp-core — roadmap

Sans-IO RTP payloadization for H.264/HEVC (unprefixed). Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — RTP header + H.264/HEVC single-NAL + FU packetize/depacketize (this session)

- [x] Crate + naming (ADR-0012) + [`adr/0001`](../adr/0001-rtp-freestanding-core.md)
- [x] `RtpHeader`/`RtpPacket`: minimal 12-byte fixed header (RFC 3550 §5.1)
      build + parse, CSRC list skipped (not exposed), extension/padding bits
      rejected rather than silently mis-parsed
- [x] `h264::Packetizer`/`h264::Depacketizer`: single-NAL-unit packets +
      FU-A fragmentation (RFC 6184 §5.6/§5.8)
- [x] `hevc::Packetizer`/`hevc::Depacketizer`: single-NAL-unit packets +
      FU fragmentation (RFC 7798 §4.4.1/§4.4.3)
- [x] Marker bit set on the last RTP packet of an access unit only
- [x] 90 kHz RTP video clock rate as a crate constant (`RTP_VIDEO_CLOCK_RATE_HZ`)
- [x] Round-trip tests: header, both codecs' single-NAL and FU paths (small
      and MTU-forcing-fragmentation NAL sizes), full wire-byte round trip
      (`packetize` → `RtpPacket::write` → `RtpPacket::parse` → `depacketize`)

### Deferred (tracked, not silently dropped)

- [ ] Aggregation packets — H.264 STAP-A/STAP-B/MTAP16/MTAP24 (RFC 6184 §5.7),
      HEVC AP (RFC 7798 §4.4.2). `depacketize` reports
      `Error::AggregationPacketUnsupported` rather than silently dropping
      aggregated NAL units.
- [ ] H.264 interleaved mode (FU-B, type 29) — `Error::InterleavedFragmentUnsupported`.
- [ ] HEVC PACI (type 50) — `Error::PaciPacketUnsupported`.
- [ ] HEVC DONL/DOND (decoding-order-number, `sprop-max-don-diff > 0` only) —
      never written or expected; this crate is in-order-only.
- [ ] RTCP (sender/receiver reports, retransmission, congestion control) —
      separate, larger scope.
- [ ] SRTP (encryption) — separate scope.
- [ ] Other codecs over RTP (AAC RFC 3640, Opus RFC 7587) — real future work,
      not started.
- [ ] Loss/reorder-tolerant depacketize (jitter buffer, sequence-number-keyed
      reassembly) — current `Depacketizer` assumes in-order arrival and
      returns `Error::MissingFuStart`/`Error::UnexpectedFuStart` on violations
      rather than silently discarding or reordering.
- [ ] `mediaway-*` facade wiring (a WebRTC/RTSP/SIP session/signaling adapter
      that actually opens a socket and drives `Packetizer`/`Depacketizer`) —
      freestanding core only so far, no product-level streaming session exists
      yet anywhere in the workspace to wire this into.
