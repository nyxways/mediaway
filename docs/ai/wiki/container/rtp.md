# `rtp-core` — RTP payloadization for H.264/HEVC (implemented)

Crate-local [ADR-0001](../../../../crates/rtp-core/adr/0001-rtp-freestanding-core.md).
Unprefixed freestanding core (ADR-0012): RTP fixed header (RFC 3550 §5.1, 12-byte
case) plus H.264 (RFC 6184) and HEVC (RFC 7798) single-NAL-unit + FU-A/FU
fragmentation packetize/depacketize. Sans-io — byte slices/`Bytes` in/out, no
`std::net`, no socket anywhere in the crate. `#![forbid(unsafe_code)]`.

## Why this exists

The workspace ships real, hardware-verified H.264/HEVC encoders (WMF, Vulkan,
D3D12) but had no RTP layer — no standard-interop path (WebRTC/RTSP/SIP) to stream
their output; a custom UDP protocol was the only alternative. This crate closes
that gap at the marshalling layer only (no session/signaling/socket adapter yet).

## Public shape

```text
RtpHeader { marker, payload_type, sequence_number, timestamp, ssrc }
RtpHeader::write(&self, &mut Vec<u8>) -> Result<(), Error>
RtpHeader::parse(&[u8]) -> Result<(Self, usize), Error>   // (header, bytes consumed)

RtpPacket { header: RtpHeader, payload: Bytes }
RtpPacket::write / RtpPacket::parse                        // whole-packet wire round trip

h264::Packetizer::new(max_payload_size, payload_type, ssrc, initial_sequence_number)
h264::Packetizer::packetize(&mut self, nal: &[u8], timestamp: u32, marker: bool)
  -> Result<Vec<RtpPacket>, Error>
h264::Depacketizer::new() / ::depacketize(&mut self, payload: &[u8])
  -> Result<Option<Bytes>, Error>

hevc::Packetizer / hevc::Depacketizer — same shape as h264's
```

`Packetizer` owns the sequence-number counter (increments per packet emitted,
`wrapping_add`) — sans-io, but a streaming caller needs consistent numbering
across calls without re-threading state itself.

## Scope: what's real vs. deliberately cut

| Real | Cut (see ADR-0001 § Scope cuts) |
|------|----------------------------------|
| Single-NAL-unit packets (both codecs) | Aggregation packets (H.264 STAP-A/STAP-B/MTAP, HEVC AP) |
| FU-A (H.264) / FU (HEVC) fragmentation | H.264 interleaved mode (FU-B) |
| Marker bit on last packet of an access unit | HEVC PACI |
| 90 kHz clock rate constant (`RTP_VIDEO_CLOCK_RATE_HZ`) | RTCP, SRTP |
| In-order depacketize reassembly | Loss/reorder-tolerant depacketize (jitter buffer) |
|  | Any codec other than H.264/HEVC |

Every cut surfaces as a named `Error` variant on `depacketize` (e.g.
`AggregationPacketUnsupported`, `InterleavedFragmentUnsupported`,
`PaciPacketUnsupported`) rather than silently dropping bytes or panicking.

## Two footguns worth knowing

- **`max_payload_size` is not the raw network MTU.** This crate has no
  network-layer knowledge (no IP version/options/UDP awareness), so the caller
  must pass the already-reduced per-packet payload budget (e.g. `1460` for
  standard Ethernet/IPv4, not `1500`). Named explicitly in `Packetizer`'s field
  rustdoc and in ADR-0001 § Decision — passing a raw link MTU would silently
  overflow real network packets.
- **HEVC's NAL header is 2 bytes, bit-packed non-byte-aligned** (`LayerId`
  splits 1 bit in the first byte + 5 bits in the second, RFC 7798 §1.1.4) —
  `hevc.rs`'s `decode_nal_header`/`encode_nal_header` isolate that math and are
  round-trip-tested directly, not just exercised indirectly through
  packetize/depacketize.

## No `mediaway-*` facade wiring yet

Freestanding core only — no product-level streaming session (WebRTC/RTSP/SIP)
exists anywhere in the workspace yet to consume `Packetizer`/`Depacketizer`
against a real socket. That adapter is future work, not designed here.
