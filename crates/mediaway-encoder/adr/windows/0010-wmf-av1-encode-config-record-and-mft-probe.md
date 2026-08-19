# ADR-0010: WMF AV1 encode — codec-generic dispatch already covers it; real gaps are `av1C` correctness + a real encoder-MFT probe

- **Status**: Accepted
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder-windows`

## Context

### Scope correction — the originally briefed premise was wrong

This ADR was requested to "wire up WMF AV1 encode" on the premise that
[`wmf/video.rs`](../../src/windows/wmf/video.rs) gates encode to H.264 only via a hardcoded
`if config.codec == CodecKind::H264` check. Reading the actual code (this session) shows
that premise does not hold:

- [`wmf/codec.rs`](../../src/windows/wmf/codec.rs) already maps `CodecKind::Av1 →
  MFVideoFormat_AV1` in `video_subtype`, and `is_supported_video_codec` already accepts
  `H264 | Hevc | Av1 | Vp9`.
- [`wmf/video.rs::open_cpu`](../../src/windows/wmf/video.rs)'s `if config.codec ==
  CodecKind::H264` branch only chooses **which transform to open** — the well-known inbox
  `CLSID_MSH264EncoderMFT` for H.264, vs. `dx11::activate_encoder_mft(&output_subtype,
  false)` (a real `MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, …)` enumeration, **no hardcoded
  CLSID**) for HEVC/AV1/VP9. It does not reject AV1.
- [`wmf/video.rs::open_dx11`](../../src/windows/wmf/video.rs) (Zero-Copy) always goes
  through the same enumeration-based `dx11::activate_hw_encoder` →
  `dx11::activate_encoder_mft(&output_subtype, true)` regardless of codec.
- [`windows/mod.rs`](../../src/windows/mod.rs) already has real (soft-skip-on-absence)
  hardware tests exercising AV1 through **both** paths: `open_hevc_av1_vp9_cpu_or_skip` and
  `open_hevc_av1_vp9_dx11_or_skip` (both loop over `[Hevc, Av1, Vp9]`).
- This is exactly what [ADR-0004](0004-multi-codec-wmf.md) (Accepted, 2026-07-28) already
  decided and landed: *"CPU: H.264 keeps the inbox sync MFT; HEVC/AV1/VP9 use `MFTEnumEx`
  (any match) … Zero-Copy: hardware `MFTEnumEx` + DXGI for all four codecs."*

So **"enumerate an AV1 encoder MFT instead of hardcoding a CLSID" is already done** —
mirroring `mediaway-decoder-windows`'s own decode-side enumeration pattern was the
originally-requested design goal, and it was already met by ADR-0004 for encode too, before
this ADR was drafted. No new discovery/negotiation code is proposed here.

### The real gaps, found by reading the code

1. **`refresh_extradata()` is codec-blind and AVC-specific — a real correctness bug for
   AV1.** [`wmf/video.rs::refresh_extradata`](../../src/windows/wmf/video.rs)
   unconditionally reads `MF_MT_MPEG_SEQUENCE_HEADER` and runs it through
   `iso_bmff::bitstream::avc::to_avcc`, which is H.264 Annex-B-specific (`is_annex_b`, a
   NAL-type switch on SPS=7/PPS=8). For an AV1 blob, `is_annex_b` will not recognize an
   Annex-B start code, so `to_avcc` returns `avcc: None`, and `refresh_extradata` falls back
   to storing the **raw WMF blob bytes verbatim** as `StreamInfo::Video::extra_data`.
   Downstream, `iso-bmff`'s `write_av01` ([ADR iso-bmff/0003](../../../iso-bmff/adr/0003-hevc-av1-sample-entry.md))
   writes `track.extra_data` **verbatim** into the `av1C` box whenever it is non-empty ("a
   real demuxed config always wins" over `AV1C_PLACEHOLDER`) — so this raw, non-`av1C`-shaped
   blob would be written into the container as if it were a real
   `AV1CodecConfigurationRecord`. This silently corrupts AV1 MP4 output the moment any AV1
   encoder MFT exists — a real, not hypothetical, correctness gap.
2. **No `iso-bmff` helper builds a real `av1C` from Sequence Header OBU bytes today.**
   `iso-bmff/src/bitstream/` has `avc.rs` (Annex-B ↔ AVCC + `annex_b_sequence_header` for
   WMF's own decoder-side reverse conversion) and `aac.rs`; there is no `av1.rs` sibling.
   `sample_entry.rs`'s `av1C` handling is either a caller-supplied verbatim payload or
   `AV1C_PLACEHOLDER` (`marker=1, version=1`, everything else zero, no `configOBUs`) — a
   real AV1 encoder needs the former to actually be correct, and nothing produces it yet.
3. **Zero real-machine evidence, in either direction, that an AV1 encoder MFT exists to
   target.** `open_hevc_av1_vp9_cpu_or_skip` / `_dx11_or_skip` both `eprintln!`-skip on
   `Err` from `WindowsVideoEncoder::open`, so they have only ever proven the honest-failure
   path works, never a real AV1 encode. Decode has a documented diagnostic for exactly this
   question — `mediaway-decoder-windows`'s
   [`video_cpu_tests.rs::list_decoder_mfts_for_each_codec`](../../../mediaway-decoder/src/windows/wmf/video_cpu_tests.rs)
   calls `MFTEnumEx(MFT_CATEGORY_VIDEO_DECODER, …)` per codec and logs friendly names,
   informationally (asserts nothing about presence/absence). No encoder-side equivalent
   exists.
4. **Indirect real evidence this session found, suggesting "no AV1 encoder MFT" is the more
   likely outcome on this workspace's known verification host** (not a certainty):
   [`docs/ai/wiki/platform/windows-encode.md`](../../../../docs/ai/wiki/platform/windows-encode.md)
   already records that on that host (RTX 4090 + Intel UHD 770), **neither GPU registered a
   working Media Foundation encode HW MFT for H.264** — "NVENC exists but isn't exposed as
   an `IMFTransform` on that driver." Separately, this crate's D3D12-native-video-encode AV1
   path ([ADR-0007](0007-d3d12-native-video-encode.md), 2026-08-07 addenda) confirms NVIDIA's
   driver on the *same* GPU supports AV1 encode through the narrower native D3D12 Video
   Encode API (`EncodeFrame` succeeds, output not yet decodable — a different, non-WMF
   surface) while `mediaway-encoder::nvenc` already hardware-verifies AV1 through NVIDIA's
   own SDK directly (also non-WMF). A driver that does not expose *any* MFT-wrapped H.264
   encoder is unlikely to expose a rarer MFT-wrapped AV1 one — but this is inference, not a
   real `MFTEnumEx` result, and a different GPU/driver/Windows build could differ.

## Decision

> Treat AV1 encode **dispatch** as already correct (ADR-0004) and out of scope to touch.
> Scope this ADR narrowly to the two real gaps: (a) codec-aware, non-corrupting extradata
> production for AV1, and (b) a real, honest `MFTEnumEx` encoder-MFT probe mirroring
> decode's own, so "does a machine actually have one" stops being unanswered.

### (a) `av1C` correctness

- Add `iso_bmff::bitstream::av1` (new file, same crate/folder as `avc.rs`, no new
  dependency): a function shaped like `to_avcc`, e.g. `to_av1c(data: &[u8]) -> Av1cOut`,
  that locates the Sequence Header OBU (`obu_type == 1`, low 3 bits after the header's
  forbidden bit) in the byte stream WMF hands back via `MF_MT_MPEG_SEQUENCE_HEADER` and
  builds a real `AV1CodecConfigurationRecord`: `marker = 1`, `version = 1`, `configOBUs` =
  the Sequence Header OBU bytes verbatim (mirroring `to_avcc`'s "concatenate the raw
  parameter-set NALs, don't re-encode them" approach). Falls back to `Av1cOut { av1c: None,
  .. }` (same shape `AvccOut` uses for "not recognized") when no Sequence Header OBU is
  found, so callers keep their existing not-a-real-config fallback behavior instead of
  fabricating one.
- **Open design question, deliberately left for implementation time, not resolved here**:
  whether to also parse and populate the record's own `seq_profile`/`seq_level_idx_0`/
  `seq_tier_0`/`high_bitdepth`/`mono_chrome`/`chroma_subsampling_{x,y}` bitfields from the
  OBU (real values) or leave them zero like the existing `AV1C_PLACEHOLDER` (spec allows
  readers to parse `configOBUs` themselves for authoritative values; many real muxers do
  exactly that). Recommend starting with the zero-fields-but-real-`configOBUs` shape —
  simplest, matches this crate's existing placeholder posture, and avoids hand-writing an
  OBU bitfield parser for a code path with no confirmed backing MFT yet (see (b)). Populate
  the fields for real only once an AV1 encoder MFT is confirmed to exist and end-to-end
  output can actually be verified against a real decoder — writing that parser blind, against
  a spec section instead of a real bitstream, risks the same category of bug ADR-0007's D3D12
  AV1 addenda repeatedly found the hard way.
- `wmf/video.rs::refresh_extradata()` becomes codec-aware: keep calling
  `iso_bmff::bitstream::avc::to_avcc` only when the session's codec is `CodecKind::H264`;
  call the new `av1::to_av1c` when it is `CodecKind::Av1`; **HEVC/VP9 keep today's existing
  raw-bytes-verbatim fallback unchanged** — the same class of bug likely exists for HEVC's
  `hvcC` too, but fixing it is out of scope here (task is AV1-specific); flagged as a
  follow-up ADR, not silently left undocumented.

### (b) Real encoder-MFT probe

- Add an encoder-side mirror of `list_decoder_mfts_for_each_codec`: a new
  `list_encoder_mfts_for_each_codec` test (new `wmf` test file, e.g.
  `video_tests.rs` — no such file exists yet in `wmf/`) that calls
  `MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, …)` for HEVC/AV1/VP9 output subtypes, both
  unfiltered (`MFT_ENUM_FLAG_SORTANDFILTER` only) and `MFT_ENUM_FLAG_HARDWARE`-filtered
  (mirroring `activate_encoder_mft`'s own two call shapes for CPU vs. DX11 open), logging
  friendly names via `eprintln!`. Informational only — asserts nothing about presence or
  absence, since which encoder MFTs are registered is a property of the OS/driver install,
  not this crate, exactly the stance `list_decoder_mfts_for_each_codec`'s own doc comment
  takes.
- Once run on a real machine, record the actual findings in this crate's
  [`docs/roadmap.md`](../../docs/roadmap.md) and cross-link from
  [`docs/ai/wiki/platform/windows-encode.md`](../../../../docs/ai/wiki/platform/windows-encode.md)
  — same convention the decode module's own doc comment already claims for itself.
- Extend `open_hevc_av1_vp9_cpu_or_skip` / `open_hevc_av1_vp9_dx11_or_skip`: when a session
  *does* open and produce ≥1 packet for `CodecKind::Av1` specifically, additionally assert
  `stream_info().extra_data()` is non-empty and its first byte has the `av1C` marker/version
  pattern (`0x81`) — so if a real AV1 encoder MFT is ever found on some future host, this
  test starts proving the (a) fix landed, not just "some bytes came out."

### Honest outcome if no AV1 encoder MFT is found

No behavior change is proposed here: `activate_encoder_mft` already returns
`EncodeError::Unsupported` when `MFTEnumEx` finds zero matching transforms (`activates.is_null()
|| count == 0` or every `ActivateObject` call failing), which `open_cpu`/`open_dx11` already
propagate as-is. This is the expected, legitimate outcome on any machine without a
registered AV1 encoder MFT — not a failure of this design, and not something this ADR's test
plan tries to force past. `mediaway-encoder::nvenc` / `mediaway-encoder-windows`'s own
D3D12-native path ([ADR-0007](0007-d3d12-native-video-encode.md)) remain the real,
independently-verified ways to reach AV1 hardware encode on this same GPU when WMF has
nothing registered.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Hardcode a specific vendor's AV1 encoder MFT CLSID once one is found on one machine | Same reason ADR-0004 rejected this for HEVC/VP9 — ties correctness to one vendor/driver/Windows-build combination instead of the portable `MFTEnumEx` contract; breaks silently on any other machine |
| Leave `refresh_extradata` reusing `avc::to_avcc` unmodified for AV1 (do nothing) | Confirmed above to silently write a non-conformant `av1C` the moment any AV1 encoder MFT exists — an honesty violation this crate's own `caveats-and-clarity.md` posture rejects, not merely a hypothetical edge case |
| Fully parse AV1 Sequence Header OBU bitfields into `av1C` now, not deferred | Speculative complexity for a code path with no confirmed backing encoder MFT on this workspace's known hosts; ADR-0007's AV1 addenda show hand-writing AV1 header-field logic against the spec alone (no real bitstream to check against) repeatedly produced real, hard-to-find bugs — defer bitfield population until a real MFT exists and output can be checked against a real decoder |
| Build the `av1C` helper inside `mediaway-encoder-windows` instead of `iso-bmff` | Violates crate packaging: `iso-bmff` already owns `avc`/`aac` bitstream helpers plus the `av1C` sample-entry writer/placeholder (ADR iso-bmff/0003); an AV1 config-record builder belongs beside its sibling, reusable by any future AV1-producing backend (Vulkan, D3D12 native, NVENC), not duplicated per-backend |
| Skip the `MFTEnumEx` encoder probe test; rely on `open_hevc_av1_vp9_cpu_or_skip`'s existing skip-on-`Err` as "verification" | That test cannot distinguish "no MFT registered" from any other `Unsupported`/`Backend` failure reason, and produces no record of what MFTs (if any) actually exist — the same reasoning that justified decode's own dedicated enumeration diagnostic applies symmetrically here |

## Consequences

### Positive

- Closes a real, silent AV1 `av1C` corruption bug the moment any AV1 encoder MFT becomes
  available on any machine — without that fix, the container output would look valid
  (non-empty `extra_data`) while actually being wrong.
- Gives this crate the same honest, recorded answer to "does an AV1 encoder MFT exist here"
  that decode already has for decoder MFTs, instead of inferring it indirectly from an
  unrelated H.264 finding.
- New `iso_bmff::bitstream::av1` module is reusable by any future AV1-producing backend in
  this workspace (Vulkan, D3D12 native, NVENC, QuickSync), not WMF-specific.
- No churn to the already-correct, already-tested codec-generic dispatch/negotiation code
  (`video_subtype`, `is_supported_video_codec`, `activate_encoder_mft`, `configure_types`).

### Negative / Trade-offs

- The `av1C` fix cannot be end-to-end hardware-verified this pass if (as indirect evidence
  suggests is likely) no AV1 encoder MFT is registered on the available verification
  host — it will land as sans-io-unit-tested-only (real OBU bytes in, real `av1C` bytes out)
  plus a graceful `Unsupported` at the `open()` layer, same honesty posture as this crate's
  own D3D12/Vulkan AV1 addenda when hardware didn't cooperate.
- HEVC's matching `hvcC`-correctness gap is intentionally left unfixed here (task scope is
  AV1-specific) — a real, already-identified follow-up, not silently dropped.
- Profile/level/tier bitfields inside the produced `av1C` stay zero (deferred, see Decision)
  until a real MFT exists to verify against — stricter readers that trust those fields
  without parsing `configOBUs` would still see zeros, same limitation the existing
  `AV1C_PLACEHOLDER` already has.

## Test Plan

- **Sans-io, no hardware, always runs**: new `iso_bmff::bitstream::av1::to_av1c` unit tests
  (sibling `av1_tests.rs`, per this workspace's testing convention) — real Sequence Header
  OBU bytes in (synthesized or captured from a real `ffmpeg`/`libaom-av1` bitstream, same
  oracle this workspace's decode-side AV1 test already uses) → non-empty `av1c` with correct
  `marker`/`version`/`configOBUs`; non-OBU/garbage input → `av1c: None`, no panic.
- **Hardware-gated, soft-skip on absence (default suite must pass without it)**:
  - `list_encoder_mfts_for_each_codec` — informational `MFTEnumEx` dump, never fails.
  - Extended `open_hevc_av1_vp9_cpu_or_skip` / `open_hevc_av1_vp9_dx11_or_skip` — additional
    `av1C`-shape assertion **only** in the branch where AV1 actually opened and produced a
    packet; every other branch/codec keeps today's skip-on-`Err` behavior unchanged.
- **Legitimate, expected outcome, not a failure to chase further**: if
  `list_encoder_mfts_for_each_codec` finds zero AV1 entries on every available host this
  pass, this ADR's (a) work stays sans-io-verified-only and (b) stays a documented "no MFT
  found here" result — mirroring `mediaway-decoder-windows`'s own "`Unsupported` when no
  matching decoder MFT is registered" contract, now confirmed symmetrically true for encode.

## References

- [ADR-0004](0004-multi-codec-wmf.md) — the already-landed codec-generic dispatch decision
  this ADR builds on, not replaces
- [ADR-0007](0007-d3d12-native-video-encode.md) — this crate's independent, non-WMF D3D12
  native AV1 encode path (real hardware findings, still not fully decodable)
- `mediaway-encoder::nvenc` ADR-0001 — hardware-verified AV1 via NVIDIA's SDK directly, the
  third independent AV1 encode path on this same GPU
- [`mediaway-decoder-windows` `video_cpu.rs`](../../../mediaway-decoder/src/windows/wmf/video_cpu.rs) /
  [`video_cpu_tests.rs`](../../../mediaway-decoder/src/windows/wmf/video_cpu_tests.rs) — the
  decode-side `MFTEnumEx` enumeration + friendly-name probe pattern this ADR mirrors on the
  encode side
- [`iso-bmff` ADR-0003](../../../iso-bmff/adr/0003-hevc-av1-sample-entry.md) — `av1C`/`hvcC`
  sample-entry writer + placeholder posture this ADR's `av1C` producer must feed correctly
- [`docs/standards/registry.toml`](../../../../docs/standards/registry.toml) id
  `av1-isobmff-binding` — AOMedia AV1 Codec ISO Media File Format Binding (free, pinned, not
  yet cached locally) — primary source for `AV1CodecConfigurationRecord`'s exact field
  layout when implementing `to_av1c`
- [`docs/ai/wiki/platform/windows-encode.md`](../../../../docs/ai/wiki/platform/windows-encode.md) —
  records the H.264 "no encode HW MFT on either GPU" finding this ADR's indirect-evidence
  reasoning relies on
