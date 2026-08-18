# ADR-0003: AMD AMF (`shiguredo_amf`) — HEVC + AV1 codec dispatch (H.264-only → tri-codec)

- **Status**: Accepted (decision: extend the codec dispatch; the `.rs` edits are the immediate
  follow-up PR this ADR authorizes — same posture as [ADR-0002](0002-amf-linux-shiguredo-amf-h264-cpu-upload.md))
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (module `mediaway-encoder::amf::linux` — `x86_64-unknown-linux-gnu`
  only, per ADR-0002; not a new module, not a new crate)

## Context

[ADR-0002](0002-amf-linux-shiguredo-amf-h264-cpu-upload.md) shipped a real, working
`mediaway-encoder::amf::linux` backend, but scoped it to **H.264 only**
(`crates/mediaway-encoder/src/amf/linux/codec.rs`):

```rust
pub(super) const fn is_supported_video_codec(codec: CodecKind) -> bool {
    matches!(codec, CodecKind::H264)
}
```

with a test (`codec_tests.rs::is_supported_video_codec_accepts_only_h264`) that explicitly asserts
`Hevc`/`Av1`/`Vp9` are **not** supported, and `session.rs::open_cpu` hardcoding

```rust
let codec_config = CodecConfig::H264(H264EncoderConfig { profile: None });
```

ADR-0002's own "Scope → Out" section listed HEVC/AV1 as deferred, noting only that
`HevcEncoderConfig`/`Av1EncoderConfig` "exist in the crate but are out of scope this stage,
matching the H.264-first pattern every other backend followed" — it did not read their real field
shapes. This ADR closes that gap.

### `shiguredo_amf` 2026.3.0 real source — direct read, this session

Read directly from the vendored crate source (not docs.rs, not carried forward from ADR-0002's
network-fetch pass):
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/shiguredo_amf-2026.3.0/src/encode.rs`
(confirmed present via `Glob`; same crate/version this workspace already depends on).

Confirmed facts, with exact line numbers from that file:

- **`CodecConfig` is a real 3-variant enum today**, not H.264-only (lines 184–190):
  ```rust
  pub enum CodecConfig {
      H264(H264EncoderConfig),
      Hevc(HevcEncoderConfig),
      Av1(Av1EncoderConfig),
  }
  ```
- **`HevcEncoderConfig`/`HevcProfile`** (lines 159–170): `HevcProfile { Main, Main10 }`,
  `HevcEncoderConfig { pub profile: Option<HevcProfile> }` — same one-field shape as the
  already-shipped `H264EncoderConfig`.
- **`Av1EncoderConfig`/`Av1Profile`** (lines 172–182): `Av1Profile { Main }` (a single variant —
  AMF's AV1 encoder only exposes one profile today), `Av1EncoderConfig { pub profile:
  Option<Av1Profile> }` — same shape again.
- **No VP9 variant anywhere in `CodecConfig`, `sys::str::AMFVideoEncoder*` selection, or
  `FrameFormat`.** `Encoder::new`'s component-ID selection (lines 425–430) matches exactly
  `CodecConfig::{H264, Hevc, Av1}` → `AMFVideoEncoderVCE_AVC` / `AMFVideoEncoder_HEVC` /
  `AMFVideoEncoder_AV1`, with no fourth arm. **`shiguredo_amf` itself has no VP9 ceiling to reach**
  — this is not a Mediaway limitation to work around, it is the crate's own real scope.
- **All AMF-specific property plumbing is already implemented per codec, inside this crate**:
  `set_properties`/`set_h264_properties`/`set_hevc_properties`/`set_av1_properties` (lines
  483–814, each codec dispatch), `reconfigure`'s dynamic-property dispatch (lines 501–524,
  `set_h264_dynamic_properties`/`set_hevc_dynamic_properties`/`set_av1_dynamic_properties`, lines
  550–610), `force_picture_type`'s per-codec IDR/KEY-frame forcing (lines 892–923,
  `AMF_VIDEO_ENCODER_FORCE_PICTURE_TYPE` / `AMF_VIDEO_ENCODER_HEVC_FORCE_PICTURE_TYPE` /
  `AMF_VIDEO_ENCODER_AV1_FORCE_FRAME_TYPE`), and output picture-type decoding
  (`get_output_picture_type`, lines 1098–1138, per-codec `AMF_VIDEO_ENCODER_*_OUTPUT_*` property
  name **and** per-codec value-to-`PictureType` mapping). **Mediaway's `session.rs` never touches
  any `AMF_VIDEO_ENCODER_HEVC_*`/`AMF_VIDEO_ENCODER_AV1_*` constant directly** — those live
  entirely inside `shiguredo_amf`, reached automatically once `session.rs` passes the right
  `CodecConfig` variant into `EncoderConfig::codec`.
- **`gop_pic_size` stays codec-uniform on the `shiguredo_amf` side**: H.264 reads it via
  `set_h264_dynamic_properties` (line 700–704, called from `set_h264_properties`), HEVC sets
  `AMF_VIDEO_ENCODER_HEVC_GOP_SIZE` directly from `config.gop_pic_size` at construction (lines
  760–762), AV1 reads it via `set_av1_dynamic_properties`'s `gop_pic_size_name` argument (line
  608, `AMF_VIDEO_ENCODER_AV1_GOP_SIZE`). Mediaway's existing
  `encoder_config.gop_pic_size = u16::try_from(config.gop_size).ok();` (`session.rs` line 169)
  already reaches all three codecs unchanged — no session.rs edit needed for GOP.
- **Key-frame detection stays correct unchanged**: `get_output_picture_type` (lines 1118–1136)
  maps H.264's `type_val == 0` → `Idr`, HEVC's `type_val == 0` → `Idr`, and AV1's `type_val == 0`
  (KEY) → `Idr` — all three codecs' "this is a keyframe" case normalizes to the same
  `PictureType::Idr` this crate's `packet_from_encoded_frame`
  (`matches!(frame.picture_type(), PictureType::Idr)`) already checks. No session.rs change needed
  there either.

### Known asymmetries inside `shiguredo_amf` itself (not Mediaway bugs — see § Open questions)

- **HEVC has no B-frame QP property**: `set_hevc_dynamic_properties` (lines 570–590) passes
  `qp_b_name: None` unconditionally, with the crate's own comment (lines 585–587, translated):
  *"AMF's HEVC encoder has no B-frame QP property, so `qpb` is not set (no HEVC equivalent of
  H.264's `AMF_VIDEO_ENCODER_QP_B`)."* — the exact fact the task description flagged as a hint to
  verify; confirmed, not guessed.
- **AV1's initial setup suppresses `qpb` even though AV1's *dynamic* reconfigure path supports
  it**: `set_av1_properties` (lines 767–814) hardcodes `qpb: None` in the `ReconfigureParams` it
  builds for construction-time setup (line 802–803, comment: *"AV1's initial configuration does
  not set qpb, as before"*), while `set_av1_dynamic_properties` (line 593–610) **does** pass
  `Some(sys::str::AMF_VIDEO_ENCODER_AV1_Q_INDEX_INTER_B)` as its `qp_b_name` argument — meaning a
  later `Encoder::reconfigure` call **can** set AV1 B-frame QP even though the initial `Encoder::
  new` call cannot.
- **HEVC's output picture-type decoding has only 3 branches** (lines 1126–1131): `0 → Idr`,
  `1 → I`, `2 → P`, **everything else (including any real B-frame `type_val`) → `Unknown`** — HEVC
  never reports `PictureType::B`, unlike H.264 (line 1122, 4 branches including `B`) or AV1's
  binary Idr/P split (lines 1132–1135).

### Workspace context

This session already extended two other encoder backends the same H.264-only → HEVC/AV1 way:
Vulkan (hardware-verified) and Linux VA-API (`mediaway-encoder::linux::vaapi`, confirmed via
`crates/mediaway-encoder/src/linux/vaapi/codec.rs`'s `is_supported_video_codec` and
`video_tests.rs` already exercising `CodecKind::Hevc` — WSL2-compile-verified, no dedicated ADR
number yet at the time of this ADR). AMD AMF inherits the same **zero real AMD GPU/driver
hardware verification** posture as ADR-0002 documented — unchanged by this ADR. No AMD silicon
exists on any OS available to this workspace's sessions; whatever the follow-up implementation PR
writes will be, at best, compile-verified via WSL2 Ubuntu, never run against a real
`AmfLibrary::load()` / `Encoder::new()` call on real hardware. This backend's README/status marker
stays **🆗** (compiles, structurally complete, zero hardware verification) after HEVC/AV1 land —
never ✅ — exactly as it is today for H.264.

## Decision

> Extend `mediaway-encoder::amf::linux` to accept **H.264, HEVC, and AV1** by dispatching
> `VideoEncoderConfig::codec` to the matching `shiguredo_amf::CodecConfig` variant, instead of
> hardcoding `CodecConfig::H264(...)`. **VP9 stays out of scope** — `shiguredo_amf`'s own
> `CodecConfig` has no VP9 variant to dispatch to (confirmed above), so this is not a Mediaway
> restriction to lift later, it is the real ceiling of the dependency. This is a small, surgical
> dispatch change inside the already-shipped callback→poll bridge (ADR-0002) — **not** a redesign,
> not a new typestate, not a new module.

### Exact `codec.rs` / `session.rs` changes (design, not code — implementation PR writes these)

1. **`codec.rs::is_supported_video_codec`** — widen the `matches!`:
   ```rust
   matches!(codec, CodecKind::H264 | CodecKind::Hevc | CodecKind::Av1)
   ```
   `codec.rs`'s own doc comment ("No `shiguredo_amf` types here so these stay testable independent
   of any real AMF library / AMD driver being present") is preserved — this function stays pure,
   only its `CodecKind` set changes. `Vp9` (and every audio `CodecKind` variant) stays `false`.

2. **`session.rs::open_cpu`** — replace the hardcoded `CodecConfig::H264(...)` with a small
   private dispatch helper **in `session.rs`** (not `codec.rs` — it must reference
   `shiguredo_amf::{CodecConfig, H264EncoderConfig, HevcEncoderConfig, Av1EncoderConfig}`, which
   `codec.rs` deliberately does not import):
   ```rust
   fn codec_config_for(codec: CodecKind) -> Result<CodecConfig, EncodeError> {
       match codec {
           CodecKind::H264 => Ok(CodecConfig::H264(H264EncoderConfig { profile: None })),
           CodecKind::Hevc => Ok(CodecConfig::Hevc(HevcEncoderConfig { profile: None })),
           CodecKind::Av1 => Ok(CodecConfig::Av1(Av1EncoderConfig { profile: None })),
           _ => Err(EncodeError::Unsupported),
       }
   }
   ```
   Called from `open_cpu` after `validate()` already ran (`validate()` calls
   `codec::is_supported_video_codec` first, so the `_ => Err(Unsupported)` arm is unreachable in
   practice) — an honest `Result` fallback, **not** `unreachable!()`/`panic!()`, per this
   workspace's "no new `unwrap`/`expect`/`panic!`" rule. This mirrors the same
   defensive-but-non-panicking shape `validate()` itself already uses.
3. **`session.rs::stream_info_from`** — currently hardcodes `codec: CodecKind::H264,`. This is a
   **real gap this ADR must fix**, not a cosmetic detail: today it is harmless only because
   `validate()` guarantees nothing but H.264 ever reaches this function; once HEVC/AV1 configs can
   open a session, the hardcode would silently mislabel every HEVC/AV1 stream's `StreamInfo` as
   H.264 (a container/downstream-facing correctness bug, not just an internal detail). Change to
   `codec: config.codec,` (direct passthrough — `config: &VideoEncoderConfig` is already the
   function's sole parameter).
4. **No change needed** to `push_frame`/`upload_cpu_nv12`/`write_plane_rows` (CPU NV12 upload is
   codec-agnostic — `FrameFormat::Nv12` is a property of `EncoderConfig`, not `CodecConfig`),
   `poll_packet`/`flush` (queue draining is codec-agnostic), `PacketSink`/`FrameMeta`/
   `packet_from_encoded_frame` (codec-agnostic `EncodedFrame<FrameMeta>` shape), or `set_bitrate`
   (its `ReconfigureParams { target_kbps, ..Default::default() }` construction is already
   codec-agnostic on the Mediaway side — `Encoder::reconfigure` dispatches internally on its own
   stored `codec_config`, confirmed above).
5. **`validate()`** stays unchanged in shape (still calls `codec::is_supported_video_codec`,
   still requires `PixelFormat::Nv12`) — see § Open questions for why 10-bit HEVC is deliberately
   not addressed here.

### Test changes (design intent — implementation PR writes these)

- `codec_tests.rs::is_supported_video_codec_accepts_only_h264` → rename/update: assert `H264`,
  `Hevc`, `Av1` all `true`; `Vp9` stays `false`.
- `session_tests.rs::validate_rejects_non_h264_codec` → its premise (`Hevc` rejected) flips now
  that HEVC is supported; replace with an equivalent "rejects a genuinely unsupported codec" test
  using `CodecKind::Vp9`.
- New `stream_info_from` test asserting the returned `codec` matches `config.codec` for `Hevc`/
  `Av1` inputs, not just `H264` — this is the regression test for item 3 above (the bug this ADR
  is fixing, not merely a new-feature test).
- `lib_tests.rs::open_unsupported_codec_returns_unsupported_without_hardware` currently sets
  `cfg.codec = CodecKind::Av1` expecting `Unsupported` — must change to `CodecKind::Vp9` (or
  another genuinely out-of-scope codec) since AV1 becomes supported by this ADR.
- Hardware-gated smoke tests (`session_tests.rs::amf_open_and_encode_or_skip_without_hw`,
  `lib_tests.rs::open_h264_cpu_upload_or_skip_without_hw`) gain HEVC/AV1 counterparts (same
  "expected to skip, no AMD hardware in this workspace" honesty posture ADR-0002 established —
  not a new hardware-verified claim).

## Open questions

1. **`HevcProfile`/`Av1Profile` defaults** — should `codec_config_for` pass `profile: None`
   (encoder auto-picks, matching today's `H264Profile: None` convention) for both, or commit to an
   explicit `HevcProfile::Main` / `Av1Profile::Main`? This ADR's design above defaults to `None`
   for symmetry with the shipped H.264 path (`Av1Profile` has only one variant anyway, so `None`
   vs `Some(Main)` is close to moot for AV1 specifically) — flagged as open because it is a real,
   deliberate choice a maintainer may want to override, not something this ADR wants to silently
   lock in.
2. **HEVC `Main10` (10-bit) stays unreachable** — `HevcProfile::Main10` exists in `shiguredo_amf`,
   but `validate()` still hard-requires `PixelFormat::Nv12` (8-bit) for every codec uniformly.
   Whether 10-bit HEVC input (`FrameFormat::P010`, confirmed to exist at line 44) is ever wired
   is **out of scope for this ADR** — a separate future scope decision, not decided here. This ADR
   only reaches HEVC's 8-bit `Main` ceiling under the current `PixelFormat::Nv12`-only `validate()`
   gate.
3. **HEVC's missing B-frame QP property** (§ Context, confirmed via `shiguredo_amf`'s own source
   comment) has **no current effect** on Mediaway — `session.rs::set_bitrate` only ever sets
   `target_kbps` in `ReconfigureParams`, never `qpi`/`qpp`/`qpb`. Flagged only so a future
   contributor who plumbs per-frame-type QP through `RateControlConfig` (not planned by this ADR)
   knows HEVC silently drops B-frame QP requests **inside `shiguredo_amf` itself** — not a
   Mediaway-side bug to chase if it's ever noticed.
4. **AV1's initial-setup-vs-reconfigure `qpb` asymmetry** (§ Context) is similarly inert today
   (Mediaway never sets `qpb`) but is a real, confirmed quirk worth knowing before anyone wires
   AV1 CQP-style B-frame QP through this backend later.
5. **HEVC's picture-type reporting collapses non-Idr/I/P values to `Unknown`** (§ Context) — does
   not affect this crate's `is_keyframe` detection today (still correctly `Idr`-gated for all
   three codecs), but means any future Mediaway feature that wants to distinguish HEVC B-frames
   specifically cannot, without a `shiguredo_amf` upstream change. Not addressed by this ADR.
6. **Whether `Backend::Amf` should become reachable from any `auto` dispatcher for HEVC/AV1
   specifically** — out of scope. ADR-0002 already deferred all `auto`-wiring for AMF regardless
   of codec (`Backend::Amf` only exists today in `mediaway-encoder::windows::auto`, where it always
   fails with `EncodeError::NoBackend` since AMF has no Windows binding); this ADR does not change
   that.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Keep H.264-only, defer HEVC/AV1 further | `shiguredo_amf` already implements the hard part (all AMF property-name plumbing, per codec) — unlike ADR-0001's original MSRV/hardware/naming blockers, there is no real prerequisite left to wait on here; deferring buys nothing. |
| Hand-roll HEVC/AV1 `AMF_VIDEO_ENCODER_HEVC_*`/`AMF_VIDEO_ENCODER_AV1_*` property names directly in Mediaway, bypassing `shiguredo_amf`'s `CodecConfig` dispatch | Strictly more owned property-name/`unsafe` surface for zero benefit — `shiguredo_amf`'s dispatch (confirmed via direct source read, not guessed) already does this correctly and is exercised by its own upstream tests; duplicating it in Mediaway would be pure risk with no upside. |
| A new `mediaway-encoder-amf-hevc`-style sibling module or crate per codec | Contradicts ADR-0002/ADR-0021's established single-module-per-vendor-backend shape. The actual difference between codecs here is a three-arm enum-variant `match`, not a structurally different backend — a new module/crate would be needless fragmentation. |
| A Mediaway-side generic/typestate `AmfSession<C: Codec>` | No such split is warranted — every AMF component/property difference between H.264/HEVC/AV1 is entirely internal to `shiguredo_amf`. Adding Mediaway-side generics over a config-value dispatch would be premature complexity this workspace's ZCA guidance (`docs/spec/zero-cost-abstractions.md`) does not call for — a plain `match` returning a value is already zero-cost. |
| Add VP9 too, working around `shiguredo_amf`'s missing variant with hand-written FFI | Rejected outright — `shiguredo_amf` genuinely has no VP9 AMF component/property support to bind to (confirmed: no `AMFVideoEncoder*VP9*` constant referenced anywhere in `encode.rs`'s component-ID selection); this would mean owning a second, unrelated `unsafe` FFI surface against AMD's raw headers for a codec this dependency was never built to support — completely out of proportion to the request. |

## Consequences

### Positive

- Small, surgical change confined to `codec.rs`'s one `matches!` line and `session.rs`'s codec-
  config construction + `stream_info_from` — no new module, no new type, no new `unsafe` surface
  (the existing `upload_cpu_nv12`/`write_plane_rows` `unsafe` blocks are untouched and stay
  codec-agnostic).
- VP9 stays honestly excluded to `shiguredo_amf`'s own real ceiling, not an arbitrary Mediaway
  restriction — verified by direct source read, not assumed.
- Catches and fixes a real, if currently-latent, `stream_info_from` codec-hardcode bug as a
  natural side effect of this same change, before it could ever manifest for a real HEVC/AV1
  caller.
- Every per-codec AMF property-name difference (including two confirmed asymmetries — HEVC's
  missing B-frame QP, AV1's init-vs-reconfigure `qpb` split) is now a documented, cited fact for
  this backend rather than an unknown risk for a future implementer to rediscover.
- Test premise flips (`Hevc`/`Av1` no longer "unsupported") are all deterministic, hardware-free
  (`validate()`/`is_supported_video_codec` need no AMD driver) — the zero-hardware honesty posture
  of this backend's test suite is unaffected.

### Negative / Trade-offs

- Still zero real AMD hardware verification — unchanged, inherited from ADR-0002; this backend's
  README/status marker stays 🆗, never ✅, after this lands.
- Three confirmed `shiguredo_amf`-internal quirks (HEVC no B-QP, AV1 init-vs-reconfigure `qpb`
  asymmetry, HEVC's `Unknown`-collapsing picture-type reporting) are inherited limitations Mediaway
  cannot fix from its own side — only document and flag for future contributors.
- HEVC `Main10`/10-bit stays unreachable under this ADR (validate() still forces
  `PixelFormat::Nv12`) — a real scope boundary, not an oversight, but one a future request may
  reasonably want lifted later.

## References

- [ADR-0002](0002-amf-linux-shiguredo-amf-h264-cpu-upload.md) — the H.264-only design/
  implementation this ADR extends; binding choice, callback→poll bridge, zero-hardware-
  verification posture (all unchanged, inherited here)
- [ADR-0001](0001-amf-deferred-no-hardware.md) — original deferral research (superseded by
  ADR-0002)
- `crates/mediaway-encoder/src/amf/linux/{codec.rs,session.rs,mod.rs}` — the real, shipped H.264
  implementation this ADR's design edits apply to
- `shiguredo_amf` 2026.3.0 vendored source,
  `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/shiguredo_amf-2026.3.0/src/encode.rs`
  — every codec-dispatch fact cited in § Context read directly from this file, this session
- `crates/mediaway-encoder/src/linux/vaapi/{codec.rs,video_tests.rs}` — this session's
  same-pattern HEVC extension for the VA-API sibling backend (no dedicated ADR number yet at the
  time of this ADR)
- [`docs/spec/zero-cost-abstractions.md`](../../../../docs/spec/zero-cost-abstractions.md) — why
  a plain `match` dispatch, not a generic/typestate split, is the right shape here
- [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) — basis for
  documenting (not silently absorbing) the confirmed `shiguredo_amf`-internal asymmetries above
- [`docs/conventions/error-handling.md`](../../../../docs/conventions/error-handling.md) — basis
  for `codec_config_for` returning `Result<_, EncodeError>` instead of `unreachable!()`/`panic!()`
