# ADR-0002: AMD AMF (`shiguredo_amf`) vendor encode — proceed, module + design (still zero real-hardware verification)

- **Status**: Accepted
- **Date**: 2026-08-18
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (module `mediaway-encoder::amf`, ADR-0021 `#[cfg]`-gated
  backend — **not** a separate `mediaway-encoder-amf` crate; supersedes the stale crate-name
  assumption in [ADR-0001](0001-amf-deferred-no-hardware.md), written before ADR-0021's
  workspace-wide crate consolidation)

## Context

[ADR-0001](0001-amf-deferred-no-hardware.md) researched `shiguredo_amf` and explicitly
**deferred** implementation on five prerequisites. This ADR resumes that work — per direct user
direction — and resolves each prerequisite:

| # | ADR-0001 prerequisite | Status this session |
|---|------------------------|----------------------|
| 1 | Real AMD GPU + driver available | **Still blocked.** No AMD GPU exists on any OS in this session either. This ADR ships with the same "zero real-hardware verification" honesty posture as `mediaway-encoder::linux::vaapi` ([adr/linux/0001](../linux/0001-vaapi-cros-libva-h264-cpu-upload.md)) and this workspace's Android/Apple decode/encode backends. |
| 2 | Workspace `rust-version` bumped past `1.93` | **Resolved.** [`docs/adr/0023-msrv-bump-1-96.md`](../../../../docs/adr/0023-msrv-bump-1-96.md) bumped workspace MSRV to `1.96` — confirmed by reading root `Cargo.toml`: `[workspace.package] rust-version = "1.96"`. `shiguredo_amf`'s `1.93` floor now fits. |
| 3 | VA-API real-hardware verification first | **Still not done** — VA-API itself remains hardware-unverified (its own ADR's caveat, unchanged). **The user has explicitly directed proceeding with AMD AMF anyway, overriding this sequencing preference.** This is a conscious waiver by explicit user direction, not a silent skip — recorded here so a future reader does not mistake it for an oversight. |
| 4 | Cross-cutting vendor-SDK crate naming ADR | **Moot.** Confirmed empirically by reading `crates/mediaway-encoder/src/lib.rs`: `nvenc`, `quicksync`, `vulkan`, `linux`, `windows`, `android`, `apple`, `web` are all `pub mod` entries inside the **single** `mediaway-encoder` crate (`// ── merged platform/domain modules (ADR-0021) ──`), not separate `mediaway-encoder-<vendor>` crates. ADR-0021's workspace-wide backend consolidation already answered the naming question before three agents could invent it independently — there is no `mediaway-encoder-nvenc` or `mediaway-encoder-quicksync` crate to reconcile against. This ADR places AMD's code the same way: `mediaway-encoder::amf`, a `pub mod` at crate root, mirroring `nvenc`/`quicksync`/`linux` exactly. No new naming-table bureaucracy is needed. |
| 5 | Direct review of `shiguredo_amf`'s real public API | **Materially improved, not fully closed.** See § Research below — network-fetched via `WebFetch` against docs.rs and GitHub this session (URLs cited), not a docs.rs prose paraphrase. Several details remain unconfirmed and are flagged explicitly rather than guessed. |

## Research: `shiguredo_amf`'s real API surface (this session)

**Method, stated plainly:** `local/vendor-ref/` has no `shiguredo_amf`/`amf-rs` checkout (checked
via glob — absent), and this task does not include a `git clone`. Network access **was**
available this session via the `WebFetch` tool, which fetches a page and relays it through a
summarization pass — a real improvement over ADR-0001's "docs.rs summary pass" self-admission
(that pass apparently never actually queried docs.rs; this one did, with URLs cited below), but
still short of a byte-for-byte source read (contrast the Apple backend's ADR-0001, grounded in a
local `objc2` clone). Every claim below is labeled by exactly what was fetched.

Pages fetched:
- `https://docs.rs/shiguredo_amf/latest/shiguredo_amf/` (crate root item list)
- `https://docs.rs/shiguredo_amf/latest/shiguredo_amf/struct.Encoder.html`
- `https://docs.rs/shiguredo_amf/latest/shiguredo_amf/struct.EncoderConfig.html`
- `https://github.com/shiguredo/amf-rs` (README)

### Confirmed this session (network-fetched, not carried forward)

- **Platform**: `x86_64-unknown-linux-gnu` only — re-confirmed independently of ADR-0001's prior
  finding, not merely repeated.
- **License**: Apache-2.0 — re-confirmed (crate root page + GitHub README).
- **Build hazard re-confirmed**: `build.rs` still fetches AMD's AMF headers from GitHub at build
  time (README, in Japanese: "ビルド時に GitHub から AMF ヘッダーを自動取得します" — "AMF headers
  are automatically retrieved from GitHub at build time"); runtime linking is `dlopen`-based, no
  build-time link. Same live-network-build hazard ADR-0001 flagged — unchanged, not re-litigated
  here beyond re-confirming it still applies.
- **Crate root item list** (struct/enum/trait/fn/const names only, from the docs.rs landing
  page): structs `AmfLibrary`, `Av1EncoderConfig`, `CodecInfo`, `DecodedFrame`, `Decoder`,
  `DecoderConfig`, `DecodingInfo`, `EncodeOptions`, `EncodedFrame`, `Encoder`, `EncoderConfig`,
  `EncodingInfo`, `Error`, `FnDecodeHandler`, `FnEncodeHandler`, `H264EncoderConfig`,
  `HevcEncoderConfig`, `ReconfigureParams`; enums `Av1EncodingProfile`, `Av1Profile`,
  `CodecConfig`, `DecoderCodec`, `EncodingProfiles`, `FrameFormat`, `H264EncodingProfile`,
  `H264Profile`, `HevcEncodingProfile`, `HevcProfile`, `PictureType`, `RateControlMode`,
  `VideoCodecType`; traits `DecodeHandler`, `EncodeHandler`; fn `supported_codecs()`; const
  `BUILD_VERSION`. **No DirectX/Vulkan/GPU-surface type appears in this list** — consistent with
  a CPU-upload-only scope for this stage, matching every other Stage-1 backend in this workspace.
  `supported_codecs()` looks like a real runtime capability probe (analogous to oneVPL's
  `MFXVideoENCODE_Query`, used by `mediaway-encoder::quicksync` to confirm AV1 is genuinely
  unsupported on its iGPU rather than a bindings gap) — worth using the same way here, once
  hardware exists to probe.
- **`Encoder<H: EncodeHandler>` real signatures** (from `struct.Encoder.html`):
  ```rust
  pub fn new(config: EncoderConfig, handler: H) -> Result<Self, Error>
  pub fn reconfigure(&mut self, params: ReconfigureParams) -> Result<(), Error>
  pub fn alloc_surface(&self) -> Result<Surface, Error>
  pub fn encode(&mut self, surface: Surface, options: &EncodeOptions, user_data: H::UserData) -> Result<(), Error>
  pub fn finish(&mut self) -> Result<(), Error>
  ```
  **This corrects a real wrong guess in ADR-0001.** ADR-0001 guessed the shape reads "closer to
  WMF's `IMFTransform` push/pull model than VA-API's typestate." The real shape is neither: it is
  a **callback/handler-driven** design (`H: EncodeHandler`, receiving `EncodedFrame<H::UserData>`
  through a callback as output becomes ready — not a poll method the caller drives), combined
  with a **surface-alloc-then-submit** input model (`alloc_surface()` then `encode(surface, ...)`)
  that is structurally closer to VA-API's `Surface`/`Picture` staging pattern than to a flat
  push-buffer call. `encode`'s `user_data: H::UserData` parameter is a per-call correlation slot
  (e.g. for carrying a PTS through to the matching `EncodedFrame` in the callback) — a real,
  useful hook for wiring this crate's timestamp-preserving contract.
  `finish()`, not `flush()`, drains remaining buffered output.
- **`EncoderConfig` fields** (from `struct.EncoderConfig.html`):
  ```rust
  pub struct EncoderConfig {
      pub codec: CodecConfig,
      pub width: u32,
      pub height: u32,
      pub frame_format: FrameFormat,
      pub framerate_num: u32,
      pub framerate_den: u32,
      pub rate_control_mode: RateControlMode,
      pub target_kbps: Option<u32>,
      pub max_kbps: Option<u32>,
      pub qpi: Option<u16>,
      pub qpp: Option<u16>,
      pub qpb: Option<u16>,
      pub gop_pic_size: Option<u16>,
  }
  impl EncoderConfig {
      pub fn new(codec: CodecConfig, width: u32, height: u32, frame_format: FrameFormat,
                 framerate_num: u32, framerate_den: u32, rate_control_mode: RateControlMode) -> Self;
  }
  ```
  Maps cleanly onto this crate's `VideoEncoderConfig`: `width`/`height` direct;
  `time_base` (seconds-per-tick) → `framerate_num = time_base.den, framerate_den = time_base.num`
  (framerate is the reciprocal of a seconds-per-tick time base); `gop_size` → `gop_pic_size`;
  `rate_control: Some(RateControlConfig { target_bitrate_bps, .. })` →
  `target_kbps = Some(target_bitrate_bps / 1000)` (+ `max_kbps` from `vbv_buffer_size_bytes` if a
  sensible conversion exists — TBD in the implementation PR). **`gop_pic_size` being a plain
  config field (not manual reference-list plumbing, unlike VA-API's ADR-0001) is a genuinely
  promising sign** that real P-frame GOP structure may be reachable "for free" via config, driver-
  managed — see § Scope below for why this ADR still does not commit to that without confirming
  `EncodeOptions`/`Surface` details first.

### `Surface`/`Plane` write API — resolved this session (follow-up fetch, closes the ADR's own flagged gap)

A first WebFetch pass against a guessed URL (`shiguredo_amf/struct.Surface.html`) 404'd; the
real path, confirmed by asking for `Encoder::alloc_surface`'s doc-generated hyperlink `href`
directly, is `shiguredo_amf::amf::Surface` (a `pub mod amf` submodule, not crate root). Fetched
from the correct URL:

```rust
impl Surface {
    pub fn set_pts(&self, pts: amf_pts);
    pub fn set_duration(&self, duration: amf_pts);
    pub fn get_plane(&self, plane_type: AMF_PLANE_TYPE) -> Result<Plane, Error>;
    pub fn get_plane_at(&self, index: amf_size) -> Result<Plane, Error>;
    // + from_raw/from_raw_acquired/as_ptr/into_raw/convert/property_storage — not needed this stage
}
impl Plane {
    pub fn get_native(&self) -> *mut c_void;   // raw plane base pointer — write target
    pub fn get_hpitch(&self) -> amf_int32;     // row pitch/stride, bytes
    pub fn get_vpitch(&self) -> amf_int32;     // row count (padded height)
    pub fn get_width(&self) -> amf_int32;
    pub fn get_height(&self) -> amf_int32;
    // + from_raw/from_raw_acquired/as_ptr/into_raw — not needed this stage
}
```

**This is a raw-pointer, `unsafe`-write API — structurally identical to VA-API's
`vaMapBuffer`/pitch-aware write pattern this crate's own `linux::vaapi` sibling already uses**,
not a safe slice/`Vec` API. `push_frame` must: `alloc_surface()` → `get_plane(Y)`/`get_plane(UV)`
(exact `AMF_PLANE_TYPE` variant names for NV12's two planes not resolved this session — the
`amf::AMF_PLANE_TYPE` docs.rs page 404'd; standard AMD AMF SDK naming is `AMF_PLANE_Y`/
`AMF_PLANE_UV`, carried forward as a plausible-not-confirmed guess for the implementation PR to
verify against real source) → for each plane, `unsafe { std::ptr::copy_nonoverlapping(...) }`
row-by-row using `get_hpitch()` as the destination stride, into `get_native()`'s pointer — the
exact same row-at-a-time, pitch-aware copy shape `linux::vaapi::nv12`'s **write** direction (not
its read-back sibling in this same ADR family) already implements safely behind one `unsafe`
block with a `// SAFETY:` comment. `Surface::set_pts`/`set_duration` are the real hook for this
crate's timestamp-preserving contract — `encode`'s separate `user_data: H::UserData` correlation
slot (§ above) may be redundant with this if `EncodedFrame` echoes `pts` back on output
(unconfirmed — see remaining gaps below); worth checking both before committing to which one
actually carries `Packet::pts` through.

`FrameFormat::Nv12` **confirmed to exist** (`enum.FrameFormat.html`, fetched: 15 variants total
— `Nv12`, `Yv12`, `I420`, `Bgra`, `Argb`, `Rgba`, `Yuy2`, `Uyvy`, `P010`, `P012`, `P016`, `Y210`,
`Ayuv`, `Y410`, `Y416`) — this crate's `PixelFormat::Nv12` maps directly, no format-mismatch
landmine here.

`RateControlMode` **confirmed** (`enum.RateControlMode.html`, fetched: `Cqp`, `Cbr`, `Vbr`,
`LatencyConstrainedVbr`, `QualityVbr`, `HighQualityVbr`, `HighQualityCbr`) — `Cbr` is the direct
match for this crate's `RateControlConfig` CBR convention.

`H264EncoderConfig` **confirmed** (`struct.H264EncoderConfig.html`, fetched): one field,
`profile: Option<H264Profile>` — `None` is a reasonable Stage-1 default (encoder picks a
profile), avoiding a premature `H264Profile` variant-name commitment.

`Error` **confirmed to be a struct, not an enum** (`struct.Error.html`, fetched):
`status(&self) -> Option<AMF_RESULT>`, plus `new_custom`/`from_amf`/`check` constructors and
`Debug`/`Display`/`std::error::Error`. There is **no variant-level match** to build a rich
`EncodeError` mapping from — every `shiguredo_amf::Error` maps to `EncodeError::Backend`
(matching this crate's existing convention of collapsing opaque backend failures, e.g.
`linux::vaapi`'s own `MediaError`→`EncodeError::Backend` mapping) except where this crate's own
pre-call validation catches a bad input first (`EncodeError::InvalidInput` before ever calling
into `shiguredo_amf`, same as every sibling backend's `validate()` step).

### Still not verified this session — flagged, not guessed

- Exact `AMF_PLANE_TYPE` variant names for NV12's Y/UV planes (`amf::AMF_PLANE_TYPE` docs.rs page
  404'd this session) — plausible-not-confirmed guess above (`AMF_PLANE_Y`/`AMF_PLANE_UV`,
  standard AMD AMF SDK naming), must be confirmed against real source before `push_frame` compiles.
- `EncodedFrame`'s fields — whether it echoes `pts`/`user_data` back, needed to resolve the
  redundant-correlation-slot question raised above.
- `HevcEncoderConfig` / `Av1EncoderConfig` field lists and `CodecConfig`'s exact enum shape (how
  a codec-specific config nests inside `EncoderConfig::codec`) — out of scope this stage (H.264
  only) but needed before any future HEVC/AV1 stage.
- `ReconfigureParams` fields — plausible candidate for backing `VideoEncoder::set_bitrate` live
  retargeting, but unconfirmed whether it carries a bitrate field. Speculative until confirmed.
- `EncodeHandler`'s exact trait bounds (is `H` required `'static`? `Send`? — affects whether the
  planned `Rc<RefCell<_>>` output-queue bridge below needs to be `Arc<Mutex<_>>` instead).
- Whether any DirectX/Vulkan GPU-surface import exists anywhere in the crate (not surfaced in the
  root item list fetched — absence there is suggestive but not conclusive; Zero-Copy input stays
  out of scope either way, see § Scope).

## Decision

> **Proceed.** Add `mediaway-encoder::amf` as a new `pub mod` at `mediaway-encoder` crate root
> (same tier as `nvenc`/`quicksync`/`vulkan`/`linux`/`windows`/`android`/`apple`/`web`, per
> ADR-0021 — **no** new `mediaway-encoder-amf` crate). Depend on `shiguredo_amf` **by its exact
> crates.io package name** — never `amf-rs`, which is a real, unrelated, GPL-3.0-or-later crate
> (Action Message Format / Flash AMF protocol, `F2077/amf-rs`) that `cargo add amf-rs` would
> silently pull instead. This hazard was flagged once already in ADR-0001; repeating it here
> because it is the single easiest mistake to make when scaffolding this module.
>
> This ADR is a **design + go-ahead decision, not an implementation**. Per this task's explicit
> scope, no `Cargo.toml` edit and no `.rs` file are added by this ADR — the `Cargo.toml`
> dependency addition and `src/amf/` module scaffold are the immediate follow-up PR this ADR
> authorizes.

### Module shape / ZCA sketch

Mirrors `mediaway-encoder::linux`'s exact wrapper shape (`Option<Inner>` closed-after-move
sentinel, no `Box<dyn _>`), with a `target_arch` gate `linux` does not need because `cros-libva`
is not architecture-restricted while `shiguredo_amf` is (`x86_64-unknown-linux-gnu` only):

```rust
// crate root: pub mod amf;

// amf/mod.rs
pub struct AmfVideoEncoder {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    inner: Option<linux::AmfSession>,
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    _priv: (),
}
// open()/VideoEncoder impl: identical shape to NvencVideoEncoder / QuickSyncVideoEncoder /
// LinuxVideoEncoder — stream_info() falls back to closed_stream_info() when inner is None,
// push_frame/poll_packet/flush delegate via .as_mut().ok_or(EncodeError::Closed).

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod linux; // amf/linux/{mod,session}.rs — the real shiguredo_amf-backed session
```

**Callback → poll bridge (the one genuinely new piece of machinery this backend needs, unlike
every sibling backend which is naturally poll/query-driven):** `shiguredo_amf::Encoder::new`
takes ownership of a handler value that receives output via callback, not a method the caller
polls. This crate's `VideoEncoder` trait is poll-based (`push_frame` / `poll_packet` /
`flush`). Plan: a small `PacketSink` struct implementing `EncodeHandler`, holding a shared,
single-threaded queue (`Rc<RefCell<VecDeque<Packet>>>`) — cloned once into `AmfSession` itself so
both the encoder's internal callback and `poll_packet` see the same queue. No `Box<dyn _>`, no
threading, no `unsafe`: this is an ordinary "shared owner, `RefCell` for the one mutation point"
pattern, the same shape Rust code reaches for whenever a callback API and a pull API must be
bridged without a self-referential struct. If `EncodeHandler` turns out to require `Send`
(unconfirmed — see § Not verified above), this becomes `Arc<Mutex<_>>` instead; either way, no
architectural surprise, just a `Rc`→`Arc` substitution decided in the implementation PR once the
real bound is confirmed.

- `push_frame`: `encoder.alloc_surface()` → write NV12 planes into the `Surface` (mechanism
  unconfirmed — see § Not verified) → `encoder.encode(surface, &options, user_data)`. Any
  callback firings synchronously during that call push into the shared queue — genuinely CPU
  upload, documented the same way as every other backend's `upload_cpu_nv12` (cost-disclosed,
  not silent).
- `poll_packet`: pop from the shared queue. No I/O, no blocking — matches this trait's contract.
- `flush`: `encoder.finish()`, then continue draining the queue via `poll_packet` (same two-step
  contract this trait already documents: "then `flush` and drain again").
- `set_bitrate`: tentatively `encoder.reconfigure(...)` if `ReconfigureParams` turns out to carry
  a bitrate field (unconfirmed) — otherwise the trait's own default (`EncodeError::Unsupported`)
  applies honestly, no invented capability.

### Scope (this stage)

**In:**
- H.264 only, [`VideoInputPreference::CpuUploadOk`] only — matches every other Stage-1 backend
  in this workspace (Windows WMF, Linux VA-API, NVENC, QuickSync, Android, Apple).
- Real GOP structure (`gop_pic_size`) and CBR-style rate control (`rate_control_mode` /
  `target_kbps`) are **config-field-reachable candidates**, not a commitment — the implementation
  PR confirms whether `EncodeOptions`/`Surface` require any manual reference-picture bookkeeping
  on this crate's side (unlike VA-API, which needed it) before deciding whether Stage 1 ships
  real inter-frame GOP or starts IDR-only like VA-API did. Either outcome is an honest, documented
  fallback per `caveats-and-clarity.md` — this ADR does not pre-decide it without the missing
  `Surface`/`EncodeOptions` detail.

**Out (deferred, tracked in `docs/roadmap.md`):**
- [`VideoInputPreference::ZeroCopyGpu`] — `EncodeError::Unsupported`; no GPU-surface-import type
  confirmed to exist in `shiguredo_amf` at all.
- HEVC / AV1 (`HevcEncoderConfig`/`Av1EncoderConfig` exist in the crate but are out of scope this
  stage, matching the H.264-first pattern every other backend followed).
- Decode (`Decoder`/`DecoderConfig` exist in the crate; this ADR is encode-only, matching the
  facade's current `mediaway-encoder` scope).
- Any Linux `auto`-style dispatcher reaching `Backend::Amf` — today `Backend::Amf` only exists in
  `mediaway-encoder::windows::auto`, where it always fails with `EncodeError::NoBackend` (honest:
  AMF has no Windows binding). `mediaway-encoder::amf` is reachable directly via
  `AmfVideoEncoder::open`, the same low-level-first-class pattern `LinuxVideoEncoder`/
  `NvencVideoEncoder`/`QuickSyncVideoEncoder` already use — no `auto` wiring is added by this ADR.

## Zero real-hardware verification (still true — read before relying on this backend)

Same posture as `mediaway-encoder::linux::vaapi` ([adr/linux/0001](../linux/0001-vaapi-cros-libva-h264-cpu-upload.md))
and the Android/Apple encode backends: **this ADR ships no code at all yet** (task scope), and
when the follow-up implementation PR lands, it will have **zero real AMD GPU/driver to run
against** — no AMD silicon exists on any OS available to this workspace's sessions. Whatever code
that PR writes will be, at best, compile-verified (this workspace's WSL2 Ubuntu instance can
build/test/clippy Linux-target crates — a real option unlike Android's missing-NDK or Apple's
cannot-legally-cross-compile walls) but **not run against a real `AmfLibrary::load()` /
`Encoder::new()` call succeeding on real hardware.** Treat every AMF call path as unverified
until run on a real AMD GPU + driver. Post-implementation README marking should be **🆗**
(compiles, structurally complete, zero hardware verification) — matching VA-API/Android/Apple —
**never ✅.**

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Keep deferring (do nothing this session) | The hard MSRV blocker is cleared and the user has explicitly directed resuming this work — nothing new would be learned by waiting further, and the naming-bureaucracy prerequisite turned out to already be moot. |
| Wait for VA-API real-hardware verification first (ADR-0001's prerequisite 3) | Explicitly overridden by direct user instruction this session — recorded as a conscious waiver, not silently skipped. This workspace will carry two zero-hardware-verified Linux vendor/HW encode paths at once as a result. |
| Hand-written `bindgen` FFI against AMD's headers directly in this workspace | Same live-GitHub-header-fetch-at-build-time cost either way (no vendored/cached header tarball upstream), strictly more owned `unsafe` surface than depending on `shiguredo_amf`'s existing safe wrapper, no upside identified — same conclusion as ADR-0001. |
| Add `amf-rs` (bare crates.io name) instead | Would pull the **wrong, GPL-3.0-or-later** crate — rejected outright; documented again here for the same reason ADR-0001 documented it. |
| Register a new `mediaway-encoder-amf` crate (ADR-0001's original placement plan) | Stale — ADR-0021 already merged every platform/vendor backend into `#[cfg]`-gated modules inside the single `mediaway-encoder` crate; a new crate would contradict that established, already-shipped pattern for no benefit. |

## Consequences

### Positive

- The MSRV blocker is cleared and confirmed by reading the actual `Cargo.toml`, not assumed.
- The vendor-SDK-naming prerequisite is resolved as moot — empirically confirmed, not
  re-litigated with a new naming-table ADR nobody needed.
- This session's `shiguredo_amf` research is materially better than ADR-0001's: real,
  network-fetched method signatures (`Encoder::new`, `alloc_surface`, `encode`, `finish`,
  `reconfigure`, `EncoderConfig`'s field list) replace a docs.rs prose paraphrase, and one real
  wrong assumption (the "closer to WMF's `IMFTransform`" API-shape guess) is corrected before any
  code is written against it. The `Surface`/`Plane` write API — flagged as the single most
  important open gap in this ADR's first draft — was tracked down (a wrong guessed URL 404'd;
  the real path was found via the doc-generated hyperlink) and fully resolved: it's a raw-pointer,
  pitch-aware write, structurally identical to `linux::vaapi`'s own NV12 write pattern already
  shipped in this crate, not a new shape.
- The callback→poll bridge design (`Rc<RefCell<VecDeque<Packet>>>` behind a small
  `EncodeHandler` impl) is sketched now, before implementation, catching the one piece of
  genuinely new machinery this backend needs ahead of time rather than mid-PR.

### Negative / Trade-offs

- Still zero real AMD hardware — this backend ships (once implemented) with the same honesty
  ceiling (🆗, never ✅) as VA-API/Android/Apple, and that ceiling is not expected to change until
  AMD silicon becomes available to this workspace.
- Several real API details remain unconfirmed even after this session's fetch (`Surface`'s
  plane-write API being the most important gap) — the implementation PR must close these before
  writing `push_frame`, either via further `WebFetch` calls or a local vendor-ref clone, not by
  guessing.
- The VA-API-first sequencing preference is waived, not resolved — a deliberate trade-off, not a
  free one: this workspace now has two Linux vendor/HW encode paths with zero real-hardware
  confirmation stacked at once, doubling the "verify when hardware becomes available" backlog.
- `shiguredo_amf`'s live-GitHub-header-fetch-at-build-time hazard (flagged in ADR-0001) is
  unchanged and still applies to any future build of the `amf` module on `x86_64` Linux.

## References

- [ADR-0001](0001-amf-deferred-no-hardware.md) — superseded/amended by this ADR (see its updated
  Status line)
- [`docs/adr/0023-msrv-bump-1-96.md`](../../../../docs/adr/0023-msrv-bump-1-96.md) — the MSRV
  bump this ADR's prerequisite 2 depended on
- `mediaway-encoder::linux::vaapi` [adr/linux/0001](../linux/0001-vaapi-cros-libva-h264-cpu-upload.md)
  — the zero-real-hardware-verification honesty precedent this ADR follows
- `mediaway-encoder::android` [adr/android/0001](../android/0001-ndk-amediacodec-h264-cpu-upload.md),
  `mediaway-encoder::apple` [adr/apple/0001](../apple/0001-videotoolbox-h264-cpu-upload.md) — the
  same-session zero-compile-verification precedent this workspace already shipped under
- [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md)
- [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md)
- [`shiguredo_amf` on crates.io](https://crates.io/crates/shiguredo_amf) ·
  [`docs.rs`](https://docs.rs/shiguredo_amf/latest/shiguredo_amf/) ·
  [GitHub](https://github.com/shiguredo/amf-rs) (Apache-2.0) — fetched this session
- Root README § [GPU — by vendor](../../../../README.md#gpu--by-vendor) · `docs/roadmap.md`
  platform order (Windows → Web → Linux → other)
