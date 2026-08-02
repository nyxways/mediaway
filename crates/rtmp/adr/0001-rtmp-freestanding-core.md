# ADR-0001: `rtmp` — freestanding RTMP publish-client handshake, chunk stream, AMF0 command mux

- **Status**: Accepted
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `rtmp`

**2026-07-31 addendum**: `hmac`/`sha2 = "0.13"/"0.11"` added to `[workspace.dependencies]`
and this crate's `Cargo.toml`. `cargo deny check` run for real against the resolved
graph — clean, no new exceptions needed. The digest-offset (scheme 0 vs 1) sub-problem
remains a real-server-interop-gated correctness item at implementation time (§ Negative /
Trade-offs) — Accepted here refers to the design/dependency decision, not a claim that
handshake correctness is pre-verified.

## Context

A prior research pass (not repeated here) recommended RTMP as the first live-streaming
egress protocol: it reaches every major platform (YouTube, Twitch, Kick, Facebook, X), and
Enhanced RTMP v2 (Apache-2.0 spec, final) is what Twitch's 2026 "Enhanced Broadcasting"
feature runs on. This ADR is step 1: a freestanding, sans-io `rtmp` core with no
Enhanced-RTMP/HEVC/AV1 signaling yet (see Non-Goals).

No AMF encoder and no RTMP implementation exist anywhere in this workspace today (confirmed
by search across `crates/`). Two adjacent, already-real pieces exist and shape this design:

- [`crates/flv/src/mux.rs`](../../flv/src/mux.rs) — `Muxer::write_header(has_audio, has_video,
  &mut Vec<u8>)` + `Muxer::write_tag(&Tag, &mut Vec<u8>)`, appending to a caller-owned buffer,
  no `finish()`. `rtmp`'s own mux surface (§ Decision, item 4) uses the same push-append shape.
- [`crates/mediaway-container/src/flv.rs`](../../mediaway-container/src/flv.rs) — builds the
  exact `VideoTagHeader`/`AudioTagHeader` + payload byte shapes (`avc_seq_header_data`,
  `avc_nalu_data`, `aac_seq_header_data`, `aac_raw_data`, `mp3_data`) that an RTMP video/audio
  message body *is*, byte-for-byte (RTMP's `Message Type ID 9` video / `8` audio payload is an
  FLV tag's `data` field — no FLV file signature, 11-byte tag header, or `PreviousTagSize`).
  These functions are `mediaway-container`-facade-private and take `mediaway_common::Packet`/
  `Bytes` — this crate cannot depend on them (see § Payload boundary below).

### Candidates surveyed (independently re-verified this session, not trusted secondhand)

| Crate | crates.io state (checked live) | Deps | Notes |
|-------|-------------------------------|------|-------|
| `shiguredo_rtmp` | Apache-2.0, `2026.1.0-canary.6` (canary-only versioning — no stable release exists), repo `shiguredo/rtmp-rs`, 106 commits | **Zero** runtime dependencies, `no_std` | Sans-io by design (`feed_recv_buf`/`send_buf`/`advance_send_buf`, byte-slice in/out, no socket). Public API includes `RtmpPublishClientConnection`, `MediaFrame`/`AudioFrame`/`VideoFrame`, `AvcSequenceHeader` — i.e. it interprets FLV-tag-shaped codec framing itself, one layer higher than this ADR's scope. **Disqualifying finding**: its own `rtmp_handshake.rs` deliberately hardcodes the smallest RTMP version value specifically *to avoid* ever needing "the digest format introduced later" (source comment, translated) — it implements **only the legacy simple handshake**, never HMAC-SHA256 digest. Repo README: "We will not respond to PRs or issues that have not been discussed on Discord" — closed contribution model; a codebase we could not easily upstream-patch for the digest handshake ourselves. |
| `rml_rtmp` | MIT, `0.8.0`, **last published 2023-04-29** (confirmed via crates.io `updated_at`), repo `KallDrexx/rust-media-libs` | `byteorder ^1.3`, `bytes ^1`, `hmac ^0.10`, `rand ^0.8`, `rml_amf0 ^0.3.0`, `sha2 ^0.9`, `thiserror ^1.0` | Real digest handshake support (this is the crate `E:\Personal\live-recorder`'s `src/sink/rtmp/mod.rs` wraps and runs against real servers including YouTube Enhanced RTMP HEVC — mined for protocol facts only, not code). But dead upstream for 3+ years, and its own dependency pins (`hmac 0.10`, `sha2 0.9`, `rand 0.8`) are all pre-1.0-and-superseded majors (current: `hmac 0.13`, `sha2 0.11` — checked live below) — a real transitive-graph staleness/advisory-exposure risk this workspace would inherit, not just an abstract concern. |
| `amf` (`sile/amf`) | MIT OR Apache-2.0, `1.0.0`, last published 2022-02-03, repo `sile/amf` | `byteorder ^1` only | Encodes **and** decodes AMF0 + AMF3 — far more surface than the publish-only command subset this crate needs (`connect`/`createStream`/`publish`/`onMetaData` encode; no decode at all, see § AMF0 scope). Stable 1.0 API, single small dependency, license-clean — a legitimate candidate in isolation, evaluated in Alternatives Considered. |
| `hmac` (RustCrypto) | MIT OR Apache-2.0, `0.13.0`, updated 2026-03-29, repo `RustCrypto/MACs` | — | Adopted (§ Decision) — the one genuinely crypto-primitive piece of this design; not hand-rolled. |
| `sha2` (RustCrypto) | MIT OR Apache-2.0, `0.11.0`, updated 2026-03-25, repo `RustCrypto/hashes` | — | Adopted alongside `hmac` for the same reason. |

## Decision

> **Hand-roll** the RTMP protocol core (handshake, chunk stream, and a narrow AMF0 **encode**
> subset) in a new unprefixed freestanding crate **`rtmp`** (ADR-0012 naming), sans-io, zero
> dependency on any `mediaway-*` crate or type. The **only** new runtime dependencies are
> **`hmac`** and **`sha2`** (RustCrypto, MIT OR Apache-2.0) for the HMAC-SHA256 digest
> handshake — genuine cryptographic primitives, not something this ADR proposes hand-rolling
> (see § Why hand-roll the protocol but not the crypto). **Do not** adopt `shiguredo_rtmp`,
> `rml_rtmp`, or `amf` as dependencies of this crate.

### Why hand-roll, not adopt — the consequential call

This mirrors the *exact* pattern this workspace already applies to `flv`/`mpeg-ts`/`adts`/
`ogg`/`mpeg-audio`: hand-rolled freestanding sans-io cores, zero non-essential dependencies,
full control over the sans-io boundary and public shape. RTMP's candidates make that default
even stronger here, not weaker:

1. **`shiguredo_rtmp` is architecturally the best fit and still disqualified on the one fact
   that matters most.** Zero deps, `no_std`, genuinely sans-io, Apache-2.0 — every box ticked
   except the single one this whole effort exists for: it structurally **cannot** do the
   HMAC-SHA256 digest handshake real servers require (its own source comment states the
   version-value trick exists specifically to dodge it). Depending on it would mean silently
   failing exactly the target platforms (YouTube, Twitch) this project is being built for,
   discoverable only at a real ingest attempt, not at compile time. Its closed,
   Discord-gated contribution policy also means we could not cleanly upstream a digest-handshake
   patch — we would be forking a canary-versioned crate to fix the one piece we most needed,
   which erases most of the "adopt, don't hand-roll" benefit for that piece anyway.
2. **`rml_rtmp` has the right handshake but is a genuinely dead, stale-pinned dependency.**
   Three-plus years with no release, and its transitive `hmac 0.10`/`sha2 0.9`/`rand 0.8` are
   two-plus majors behind current RustCrypto releases — a real, not hypothetical, source of
   future RUSTSEC exposure this workspace would import wholesale rather than pin freshly
   itself. Its own API is also message/event-shaped around a `ClientSession`/`ServerSession`
   abstraction with an internal packet queue, not the byte-slice-in/byte-slice-out sans-io
   shape `docs/spec/sans-io.md` requires of a core crate.
3. **AMF0 for this crate's actual need is small and stable enough to not need a dependency at
   all.** `connect`/`createStream`/`publish` command messages and one `onMetaData` data message
   use exactly five AMF0 type markers (Number `0x00`, Boolean `0x01`, String `0x02`, Object
   `0x03`, Null `0x05`, ECMA Array `0x08`, Object-End marker `0x00 0x00 0x09`) — the encode
   direction only (§ AMF0 scope). AMF0 has been frozen for ~20 years; this is squarely
   [`deps-policy.md`](../../../docs/conventions/deps-policy.md)'s "can std / ~20-50 lines of
   local code cover it?" territory, and matches this workspace's own established posture of
   hand-rolling exactly this class of small, stable, spec-frozen byte framing (see `flv`'s own
   tag/header framing, `mpeg-ts`'s CRC-32/PES bit-packing). Using `amf`'s general AMF0+AMF3
   codec for a five-type, encode-only subset would be strictly more surface than needed for a
   crate this workspace otherwise keeps deliberately narrow.
4. **Crypto is the one place "hand-roll it ourselves" is the wrong instinct.** HMAC-SHA256 is
   exactly the kind of primitive this workspace already defers to a reviewed dependency for
   elsewhere (`aes` for `iso-cenc`'s CTR mode, not a hand-rolled AES) rather than reimplementing.
   `hmac`/`sha2` from RustCrypto are the standard, actively maintained (`0.13.0`/`0.11.0`,
   both updated within the last several months), MIT OR Apache-2.0, already-idiomatic choice —
   adopted here as the two narrow crypto-primitive dependencies this design needs, distinct
   from "adopt an RTMP crate."

### 1. Handshake — sans-io, digest (complex) variant only

`Handshake` (client role) drives C0/C1/C2 ⇄ S0/S1/S2 as pure byte buffers — caller owns the
socket, matching `shiguredo_rtmp`'s own proven shape (studied as a design reference, not
depended on): `feed_recv_bytes(&mut self, &[u8])`, `pending_send(&self) -> &[u8]` +
`advance_send(&mut self, n: usize)`, `is_complete(&self) -> bool`.

- **HMAC-SHA256 digest handshake only** (the "complex" handshake) — matches the ground-truth
  finding that real servers (YouTube, Twitch) require it. The legacy all-zero "simple"
  handshake is an explicit, named scope cut (§ Non-Goals), not a silent gap: a server that
  only accepts the simple handshake is out of scope for v1.
- **Known-hard sub-problem, flagged honestly:** the digest's placement inside the 1536-byte
  C1 block depends on a **scheme** (0 or 1) whose exact offset formula is not in any officially
  redistributable Adobe document — it is reverse-engineered community knowledge (same posture
  `flv`'s own ADR-0001 already takes toward Adobe's non-redistributable FLV spec). Real
  implementations (librtmp, OBS's fork, `rml_rtmp`) compute an offset from a sum-of-bytes
  formula over a fixed C1 window and try scheme 1, falling back to scheme 0. This is the
  single riskiest piece of hand-rolling this crate and the primary reason implementation
  should cross-check the offset formula against multiple independent community references
  (and, if available, capture a real handshake against a target server) before calling this
  ADR's handshake module correctness-verified — not assumed correct from spec reading alone.
- Adobe's RTMP specification itself is not freely redistributable; field layouts come from
  widely-documented community references (e.g. the community `rtmp-spec` reverse-engineering
  docs, public librtmp/OBS source comments), cross-checked via this crate's own tests, same
  citation posture as `flv`/`mpeg-ts`.

### 2. Chunk stream — sans-io encode + decode

`ChunkEncoder`/`ChunkDecoder`: basic header (1/2/3-byte chunk-stream-ID encoding), message
header types 0-3 (full / same-stream / same-length-and-type / continuation, each reusing the
prior chunk's cached header fields per RTMP's own rules), extended timestamp (`0xFFFFFF`
escape), and payload re-assembly across `chunk_size`-bounded fragments (default 128 bytes,
negotiated via `Set Chunk Size` control message). Same push-append-to-`&mut Vec<u8>` shape as
`flv::Muxer`; decode is a `push_bytes`/`poll_message` incremental reader (mirrors
`flv::Demuxer`'s `push_bytes`/`poll_tag`), producing `(message_type_id, timestamp_ms,
payload: Vec<u8>)` — **not** AMF-interpreted (see § 3).

### 3. AMF0 — encode subset only; decode is an explicit, named scope cut

Encodes only what `connect`/`createStream`/`publish` command messages and one `onMetaData`
data message need: Number (f64 BE), Boolean, String (UTF-8, 16-bit length), Object (ordered
key/value pairs, `0x00 0x00 0x09` terminator), Null, ECMA Array. **No AMF0 decoder** — a
publish-only client does not need to parse `_result`/`onStatus` command *contents* to make
progress.

This does not leave the caller blind to server responses: § 2's chunk-stream decode still
unchunks incoming bytes into raw `(message_type_id, timestamp_ms, payload)` tuples regardless
of whether `message_type_id == 20` (AMF0 command). A caller can treat "any message arrived on
control chunk stream 3" as a coarse "server responded" signal, or parse the handful of bytes
itself for a specific string, without this crate providing a general AMF0 reader. AMF3, AMF0
object/array **decode**, and RPC-style request/response correlation (transaction IDs mapped
back to pending calls) are all out of scope for v1 — named here so they are a documented cut,
not a silently dropped feature.

### 4. Public mux surface — raw bytes in, sans-io, no socket

```text
Handshake            — see § 1 (byte-slice in/out, no socket)

Muxer
  new(chunk_size: u32) -> Self
  write_connect(&mut self, app: &str, tc_url: &str, out: &mut Vec<u8>)      // AMF0 command
  write_create_stream(&mut self, out: &mut Vec<u8>)                         // AMF0 command
  write_publish(&mut self, stream_key: &str, out: &mut Vec<u8>)             // AMF0 command
  write_metadata(&mut self, meta: &OnMetaData, out: &mut Vec<u8>)           // AMF0 data
  push_video_data(&mut self, data: &[u8], timestamp_ms: u32, out: &mut Vec<u8>)
  push_audio_data(&mut self, data: &[u8], timestamp_ms: u32, out: &mut Vec<u8>)

Demuxer (message-boundary only, no AMF decode — see § 3)
  push_bytes(&mut self, chunk: &[u8])
  poll_message(&mut self) -> Option<(u8, u32, Vec<u8>)>   // (message_type_id, timestamp_ms, payload)
```

No `std::net`/socket type anywhere in this crate — every method takes/returns byte
slices/buffers, matching [`sans-io.md`](../../../docs/spec/sans-io.md) and
[`api-layers.md`](../../../docs/spec/api-layers.md)'s "usable without opening a file/socket"
bar. `Handshake` and `Muxer`/`Demuxer` are separate types (handshake is a one-shot state
machine that finishes before any chunk-stream traffic starts) rather than one god-object,
matching this workspace's `Muxer`-vs-`Demuxer`-as-separate-types convention.

### 5. Payload boundary decision: raw `&[u8]`, zero `mediaway-*`/`flv`/`mediaway-container` dependency

`push_video_data`/`push_audio_data` take **already-built** `VideoTagHeader+NALU` /
`AudioTagHeader+AAC-or-MP3` byte payloads — the exact shapes
`mediaway-container::flv`'s private `avc_seq_header_data`/`avc_nalu_data`/`aac_seq_header_data`/
`aac_raw_data`/`mp3_data` already build, but this crate does **not** call into them or into the
`flv` core crate:

- `mediaway-container::flv`'s builder functions take `mediaway_common::{Bytes, Packet}` —
  depending on them would pull `mediaway-common` into `rtmp`, breaking the ADR-0012 unprefixed
  boundary this crate must keep (per the task scoping: zero dependency on any `mediaway-*`
  crate/type, to stay consumable outside Mediaway).
- Depending on the `flv` **core** crate instead would only buy `Tag`/`TagType`, whose `Muxer`
  writes full FLV file/tag *framing* (9-byte header, 11-byte tag header, `PreviousTagSize`) —
  none of which RTMP wants; RTMP wants only the tag *body* bytes, framed by its own completely
  different chunk-stream layer (§ 2). Reusing `flv` for an unrelated framing layer would be
  more misleading than helpful.
- So `rtmp` stays genuinely codec-and-container agnostic: it frames whatever bytes it is given
  under RTMP's own message/chunk rules, the same "frame, don't encode" boundary `flv` itself
  documents relative to `AudioTagHeader`/`VideoTagHeader` sub-framing.

**Consequence, named not silently dropped**: a future adapter (not designed or scoped by this
ADR — likely either a new small facade or an addition alongside `mediaway-container::flv`'s
existing builder functions) is responsible for turning a `mediaway_common::Packet` into the
right FLV-tag-body bytes and calling `rtmp::Muxer::push_video_data`/`push_audio_data`. That
adapter is genuinely Mediaway-typed glue, not a freestanding-core concern, and is deferred to
its own follow-up ADR when RTMP egress is wired into `mediaway-pipeline` or a live-streaming
facade.

## Non-Goals (explicit v1 scope cuts)

| Cut | Why deferred |
|-----|---------------|
| Sockets / TCP transport | Sans-io core — caller owns I/O, matches every other mux/demux crate in this workspace |
| RTMPS / TLS | A real, separately-scoped adapter concern; **flagged as a genuine open risk** discovered this session — some current guidance suggests major platforms increasingly recommend/require RTMPS for public ingest in 2026, which this ADR does not resolve or block on (see § Consequences) |
| Server role / play (subscribe) role | This ADR is publish-client only, per the task's own framing |
| Enhanced RTMP v2 (FourCC, HEVC/AV1 signaling) | Explicitly named as a phase-3 follow-up ADR by the prior research pass; this crate's `push_video_data` takes opaque bytes so it does not structurally block that follow-up |
| Reconnect / backoff policy | Application-level concern, not protocol framing |
| AMF0 decode, AMF3 (either direction) | § 3 — a publish-only client does not need it; named cut, not an oversight |
| Legacy "simple" (non-digest) handshake | § 1 — real target servers require the digest variant; simple-handshake-only servers are out of scope for v1 |
| Any dependency on `mediaway-*` types/crates | Required by the task scoping and by ADR-0012 (freestanding core must be consumable outside Mediaway) |

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Adopt `shiguredo_rtmp` as the base dependency | Zero-dep/sans-io/`no_std`/Apache-2.0 is an excellent fit, but its handshake module structurally excludes the HMAC-SHA256 digest variant real target servers require, and its closed Discord-gated contribution policy makes that gap hard to fix upstream without forking a canary-versioned crate — see § Why hand-roll |
| Adopt `rml_rtmp` as the base dependency | Real digest handshake, proven against YouTube in a sibling project, but dead upstream since 2023-04 with stale pre-1.0 `hmac`/`sha2`/`rand` transitive pins two-plus majors behind current; not sans-io in the byte-slice-in/out sense this workspace requires of a core crate |
| Adopt `amf` (sile) for AMF0 encode | License-clean, stable, small — but decode+AMF3 surface is unneeded for this crate's five-type, encode-only subset; hand-rolling stays inside this workspace's existing "small, spec-frozen framing → hand-roll" pattern (`flv`, `mpeg-ts`) rather than adding a dependency for less than it would cover |
| Hand-roll HMAC-SHA256 too (fully zero-dependency crate) | Rolling your own crypto primitive is a real anti-pattern this workspace does not otherwise practice (`aes` is a dependency for `iso-cenc`, not hand-rolled) — `hmac`/`sha2` are reviewed, standard, actively maintained RustCrypto crates; the benefit of avoiding two small, license-clean deps does not outweigh the correctness/audit risk of a hand-rolled MAC |
| Implement the legacy simple handshake as a fallback alongside digest | Real target servers (YouTube, Twitch, per this ADR's ground truth) require digest; adding a second handshake path this session for servers not in scope would be speculative scope creep with no current caller |
| Fold AMF0/chunk/handshake into `mediaway-container::flv` or a `mediaway-*` facade directly | RTMP is a network protocol with no container-framing analog to MP4/WebM/FLV in this workspace's existing facade set, and the task requires zero `mediaway-*` dependency so this stays reusable outside Mediaway — matches why `flv`/`mpeg-ts` are unprefixed too |

## Consequences

### Positive

- Full control over the sans-io boundary and public shape from day one, consistent with every
  other container/protocol core in this workspace (no adopted dependency's API mismatch to
  work around later).
- No inherited stale-dependency risk (`rml_rtmp`'s pre-1.0 transitive pins) and no inherited
  protocol gap (`shiguredo_rtmp`'s missing digest handshake) — both real, concrete problems
  this decision avoids rather than defers.
- Only two new dependencies, both narrowly scoped to the one place hand-rolling is the wrong
  instinct (crypto), both currently maintained and license-clean.
- `push_video_data`/`push_audio_data` taking opaque bytes keeps this crate structurally open to
  the Enhanced RTMP v2 follow-up (FourCC-tagged HEVC/AV1 payloads are still "just bytes" to
  this layer) without a breaking change.

### Negative / Trade-offs

- More implementation work than adopting a ready-made crate — hand-rolling handshake + chunk
  stream + AMF0 from spec/community references is a real, non-trivial slice of protocol work,
  not a thin wrapper.
- The digest-offset (scheme 0 vs 1) sub-problem is genuinely hard to get right from
  documentation alone and is not verified by this ADR — real-server interop testing is a
  blocking correctness gate before this crate can be considered done, not merely "Accepted."
- No AMF0 decode means a caller who wants to robustly detect `connect`/`publish` failure
  reasons (server's `onStatus`/`_error` payload) gets raw bytes, not a parsed reason — a real,
  named limitation for production-grade error reporting, deferred rather than solved here.
- RTMPS/TLS is out of scope, and this session surfaced (not resolved) a real open risk: current
  guidance suggests major platforms are moving toward requiring/recommending RTMPS for public
  ingest — this v1 crate's plain-RTMP-only scope may need revisiting sooner than "phase 3" if
  that turns out to block real publish targets. Not blocking this ADR, but must not be
  silently forgotten.
- `hmac`/`sha2` are new dependencies (not yet added to `Cargo.toml` — see crate `Cargo.toml`
  comment); a real `cargo deny check` against them is a prerequisite for Accepted status, same
  posture as `mediaway-sw-opus`/`mediaway-audio-apm` this session.

## References

- `docs/spec/sans-io.md` — byte-slice-in/out core boundary this crate's `Handshake`/
  `Muxer`/`Demuxer` all follow
- `docs/spec/api-layers.md` — low-level surface must stay usable without a socket
- `docs/adr/0012-unprefixed-reusable-cores.md` — naming rationale for staying unprefixed
- `docs/conventions/error-handling.md` — `thiserror`, `#[non_exhaustive]` (this crate's
  `Error` enum follows the same shape as `flv::Error`/`mpeg_ts::Error`)
- `docs/conventions/deps-policy.md` — review checklist applied to `hmac`/`sha2` above
- `docs/conventions/security.md` — BSD/MIT/Apache-2.0 allow-list; `hmac`/`sha2` are MIT OR
  Apache-2.0, already covered
- `crates/flv/adr/0001-flv-freestanding-core.md` — closest structural precedent: unprefixed
  freestanding core, "frame, don't encode" boundary, non-redistributable-spec citation posture
- `crates/mpeg-ts/adr/0001-mpeg-ts-freestanding-core.md` — sibling general-container core,
  same push/poll incremental decode shape
- `crates/flv/src/mux.rs`, `crates/mediaway-container/src/flv.rs` — read this session; the
  exact byte shapes RTMP video/audio message payloads reuse (§ Payload boundary)
- `crates/mediaway-sw-opus/adr/0001-unsafe-libopus-encode-decode.md`,
  `crates/mediaway-audio-apm/adr/0001-sonora-audio-processing-adoption.md` — Proposed-status,
  no-dependency-added-yet scaffold pattern mirrored here
- crates.io API (fetched live this session): `shiguredo_rtmp` `2026.1.0-canary.6`, `rml_rtmp`
  `0.8.0`, `amf` `1.0.0`, `hmac` `0.13.0`, `sha2` `0.11.0`
- `github.com/shiguredo/rtmp-rs` — studied as a design reference for the sans-io handshake
  shape (`feed_recv_buf`/`send_buf`/`advance_send_buf`); not a dependency
- `E:\Personal\live-recorder\src\sink\rtmp\mod.rs` (sibling personal project, not part of this
  workspace) — mined for the real-world fact that `rml_rtmp`'s digest handshake works against
  YouTube Enhanced RTMP HEVC in production; not copied
- Adobe RTMP specification — not freely redistributable; not pinned via the standards registry,
  same posture as `flv`'s own ADR-0001. Field layouts to come from widely-documented community
  references (reverse-engineered `rtmp-spec` docs, public librtmp/OBS source comments) at
  implementation time, cross-checked via this crate's own tests plus real-server interop

ADRs are written in **English**.

## 2026-07-31 implementation addendum: digest-offset formula sourcing + confidence

Implemented per this ADR's design (§ Decision items 1-5; AMF0 → chunk stream → handshake →
`Muxer`/`Demuxer`, per the process this ADR implies). Recording the digest-offset sourcing
this ADR flagged as the highest-risk item (§ 1, § Consequences):

**Sources cross-checked** (three independently authored, publicly available implementations,
not one codebase read three times):

1. FFmpeg `rtmpproto.c` digest handshake, via an annotated community gist
   (`gist.github.com/gyk/967af2aae2f1455d6d40779678aefde5`).
2. `librtmp`/`rtmpdump`'s `librtmp/handshake.h` (`GetDigestOffset1`/`GetDigestOffset2`),
   fetched directly from `github.com/j0sh/rtmpdump`.
3. SRS (`github.com/ossrs/srs`) `trunk/src/protocol/srs_protocol_rtmp_handshake.cpp` — an
   independent, from-scratch C++ RTMP server, not derived from `librtmp`/FFmpeg's codebase.

**Finding**: all three agree **exactly** on both offset formulas (digest-first layout: `12 +
(sum of 4 bytes at absolute offset 8..12, mod 728)`; key-first layout: `776 + (sum of 4 bytes
at absolute offset 772..776, mod 728)`), and on the HMAC-SHA256 key material (`GenuineFPKey`
62 bytes / `GenuineFMSKey` 68 bytes, byte-for-byte identical across `librtmp` and SRS) and the
`C2`/`S2` digest derivation. Community sources number the two layouts "scheme 0"/"scheme 1"
*inconsistently* (SRS's own `SrsSchema0`/`SrsSchema1` naming turns out to label the opposite
layout from the `librtmp`-derived convention) — `crates/rtmp/src/handshake.rs` sidesteps this
by naming the two layouts by structure (`digest_offset_digest_first` /
`digest_offset_key_first`) instead of by a scheme number, documented in that module's rustdoc.

**Confidence: high** for the byte-level offset formula and key material — three independently
authored implementations agree exactly, not approximately. **Not independently confirmed**:
the exact `C1` `version` field bytes/thresholds some real servers branch on (this
implementation uses `librtmp`'s plaintext-client default, `10.0.45.2`, for maximum
realistic-client resemblance, but that specific choice is not cross-checked the same way).

**Not resolved by this addendum**: real-server interop. This handshake has been cross-checked
against reference source code and against its own math (self-consistency tests in
`crates/rtmp/src/handshake_tests.rs` — this crate's own `C1`-building math used to construct a
synthetic-but-compliant `S1`/`S2`, round-tripped through `Handshake` and accepted), **not**
against a real running RTMP server (YouTube/Twitch/nginx-rtmp/SRS live instance). Treat the
handshake as unverified for production interop until that gate is run — the same named risk
this ADR's § Consequences already flagged, still open, not silently dropped.

`ChunkEncoder`/`ChunkDecoder`/`amf0` landed as `pub` (not just internal to `Muxer`/`Demuxer`),
per the workspace's low-level-APIs-stay-public rule. `Handshake::feed_recv_bytes` and
`Demuxer::poll_message` return `Result<_, Error>` rather than this ADR's literal § 4
signatures (bare `()` / `Option<_>`) — both can genuinely fail on malformed input, and this
matches `flv::Demuxer::poll_tag`'s own `Result<Option<T>, Error>` idiom rather than silently
swallowing a real parse error. Documented in each module's own rustdoc.
