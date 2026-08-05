# ADR-0016: Adopt `cbindgen` for `mediaway-*-ffi` C header generation

- **Status**: Accepted (tooling; per-header migration tracked separately — see
  2026-08-05 addendum)
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)

## Context

ADR-0004 / [`docs/spec/c-ffi.md`](../spec/c-ffi.md) established per-capability
`mediaway-*-ffi` crates. Two now exist, each with a **hand-written** header:
`crates/mediaway-container-ffi/include/mediaway/container.h` and
`crates/mediaway-pipeline-ffi/include/mediaway/pipeline.h`. Both crates'
first-pass ADRs explicitly declined `cbindgen` **for that pass only** and
deferred the tooling question:

- `mediaway-container-ffi/adr/0001` §8: hand-write "for this first pass"; a new
  dev-dependency isn't "worth adding for one still-being-designed header on the
  very first `-ffi` crate"; "revisit ... once a second/third `-ffi` crate
  exists".
- `mediaway-pipeline-ffi/adr/0001` §8 / § Deferred: "now that **two**
  hand-written `-ffi` headers exist, a follow-up ADR should evaluate
  `cbindgen`/shared tooling across both — not ... this ADR's call to make
  unilaterally for a sibling crate."

A third crate, `mediaway-device-ffi`, is being designed in parallel (its own
ADR, not yet merged) and will need a header too. Both prior ADRs' own stated
trigger ("once ≥2/3 `-ffi` crates exist") is met now, before a third
hand-written header accumulates the same burden.

**Audit of the actual drift risk** (read in full before this ADR): every
`#[repr(C)]` struct/enum in `types.rs`/`status.rs` and every `extern "C" fn`
signature in `muxer.rs`/`demuxer.rs`/`buffer.rs` (container) and
`config.rs`/`encoder.rs`/`session.rs`/`buffer.rs` (pipeline) matches its header
declaration **exactly**, today. That diligence is real, but it is an
unenforced manual invariant — nothing today catches a signature edited in one
place and not the other before a reviewer notices by eye.

Two concrete findings from that audit, cited below because they bear directly
on whether `cbindgen` fits this codebase's actual patterns (not just the
abstract idea of it):

- The input/output packet split cited in `mediaway-container-ffi/adr/0001` §4c
  as "`cbindgen` has no way to know to make that split" was true only of the
  **pre-implementation aspirational sketch**. The real, implemented Rust
  already has two distinct `#[repr(C)]` structs — `MediawayPacketView`
  (`payload: *const u8`) and `MediawayPacket` (`payload: *mut u8`) — so a
  mechanical tool translates each correctly today; this is no longer an
  argument against `cbindgen`.
- `mediaway-pipeline-ffi/src/encoder.rs` defines
  `pub type AutoEncoderHandle = Box<dyn VideoEncoder>;` and exposes
  `*mut AutoEncoderHandle` in `extern "C"` signatures. This is **not** a plain
  `#[repr(C)]` struct with private fields (the well-documented cbindgen
  opaque-pointer case that `MuxerHandle`/`DemuxerHandle`/`EncodeSessionHandle`
  already fit) — it is a type alias to a boxed trait object. Whether
  `cbindgen` can forward-declare this as an opaque struct out of the box is
  **not confirmed** from documented behavior known to this ADR's author;
  flagged as needing a spike, not asserted either way.

## Decision

> Adopt `cbindgen` for all `mediaway-*-ffi` header generation, starting with
> `mediaway-device-ffi`. Generation is an explicit maintainer/CI command, never
> `build.rs`. The two existing hand-written headers are migrated as tracked,
> non-blocking follow-ups per crate — not required before this ADR takes
> effect, and not left permanently hand-written either.

1. **Per-crate `cbindgen.toml`**, not one shared/inherited config. Each
   `-ffi` crate is an independently versioned, independently compiled artifact
   (ADR-0004's per-capability packaging) with its own type list, status enum,
   and buffer-free name; a shared config would have almost nothing to actually
   share beyond style. House-style defaults (`documentation = true`, include
   guard naming, `extern "C"` wrapping, ABI-version macro shape) are recorded
   as a copy-paste template + wiki convention, not a literal shared file —
   **cbindgen.toml's support for cross-file config inheritance/include is not
   confirmed and needs verification** before assuming otherwise.
2. **Explicit command, not `build.rs`.** The header is a committed, versioned
   artifact consumed by non-Rust/non-Cargo builds; regenerating it on every
   `cargo build` would (a) force every downstream Rust build of a `-ffi` crate
   to have `cbindgen` installed even when it only needs to compile the
   `cdylib`, (b) risk non-reproducible diffs across `cbindgen` versions/
   machines, (c) silently overwrite a hand-authored preamble a maintainer just
   edited directly. Per [`docs/conventions/scripts.md`](../conventions/scripts.md),
   the orchestration (crate discovery, argument wiring, diff/verify mode) is a
   **Bun + TypeScript** wrapper under `tools/scripts/` invoking the `cbindgen`
   Rust CLI/crate as its underlying tool — the wrapper is the "script"; the
   `cbindgen` invocation itself is not required to be Bun.
3. **CI drift gate, regardless of migration state.** The same wrapper's
   "verify" mode regenerates each crate's header into a scratch path and diffs
   it against the committed file; CI fails on mismatch. This is required
   immediately for `mediaway-device-ffi` and becomes the ongoing discipline for
   any crate still on a hand-written header during its own migration window —
   answering the "if not adopted" question's diff-check idea as a permanent
   safety net, not a fallback used only in place of adoption.
4. **Migration of the two existing headers is tracked per crate, not gated on
   this ADR.** Verifying `cbindgen` output is faithful (correct
   `[export.rename]` entries for every `Mediaway*` type, correct enum-variant
   prefix truncation, preserved ownership/safety doc content, and a resolved
   `AutoEncoderHandle` opaque-handle shape) is real per-crate validation work.
   Blocking `mediaway-device-ffi`'s adoption on that work would penalize the
   crate that has no legacy header to reconcile. Each of
   `mediaway-container-ffi` and `mediaway-pipeline-ffi` gets its own follow-up
   entry in its `docs/roadmap.md`.
5. **Doc-comment passthrough is the load-bearing feature that makes this
   acceptable under ADR-0006** ("code carries the contract"): `cbindgen`'s
   `documentation = true` (default-on behavior, to this ADR's author's
   knowledge) copies `///` doc comments verbatim into the generated header.
   The existing `///`/`# Safety` prose in `types.rs`/`status.rs`/`muxer.rs`/
   `demuxer.rs`/`encoder.rs`/`session.rs` already carries the ownership and
   thread-safety contract almost word-for-word with today's header — this
   moves the SSOT for that prose to the Rust source (where ADR-0006 already
   says it belongs) instead of two independently hand-maintained copies.
   **Known cost:** passthrough is raw text, not markdown rendering — generated
   comments will show literal `# Safety` header lines and `[bracketed]`
   doc-links rather than today's hand-tuned prose. A visible, accepted
   cosmetic regression, not a hidden one.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Keep hand-writing (status quo) | The demonstrated risk (manual struct/signature transcription) has no safety net; both prior ADRs' own "revisit" trigger is already met |
| Hand-write `mediaway-device-ffi`'s header too, decide after 3 precedents | Waiting adds a third hand-maintained artifact carrying the identical burden the first two ADRs already flagged; this audit found no new information a third hand-written header would surface first |
| `cbindgen` via `build.rs` (regenerate every build) | Forces `cbindgen` onto every downstream Rust build; risks non-reproducible diffs; can clobber a hand-edited preamble silently |
| Start `mediaway-common-ffi` and unify status/buffer-free naming in this ADR | Explicitly out of scope; both crate ADRs already deferred that as its own future decision — conflating it here grows this ADR's blast radius past what it needs to decide |
| Migrate both existing headers in the same change that lands this ADR | Real per-crate validation work (renames, enum prefixes, doc fidelity, the `AutoEncoderHandle` gap) that would block `mediaway-device-ffi`'s adoption on unrelated legacy migration |

## Consequences

### Positive

- Removes the actual, demonstrated transcription-drift risk for all new FFI
  surface going forward, starting with `mediaway-device-ffi`.
- CI's regenerate-and-diff check is a hard gate usable both during the
  migration window and afterward as ongoing defense-in-depth (catches a
  committed header edited directly, or a Rust signature changed without
  re-running the generator).
- Doc-comment passthrough eliminates today's silent risk of the header's prose
  and the Rust doc comments' prose drifting apart in *wording*, not just
  signatures — a second, less obvious drift class this decision also closes.
- Establishes the per-crate `cbindgen.toml` + Bun/TS wrapper pattern once,
  before a third crate accumulates a third hand-written header.

### Negative / Trade-offs

- New dev-time dependency (`cbindgen` crate/CLI) needs its own
  [`docs/conventions/deps-policy.md`](../conventions/deps-policy.md) review
  and PR record per crate that adds it — not pre-approved by this ADR alone.
- Generated header prose is visibly rougher (raw rustdoc syntax) than today's
  hand-tuned comments unless a cleanup pass is done.
- Per-crate `cbindgen.toml` (rename tables, enum-prefix overrides) is itself a
  new hand-maintained artifact — smaller and differently shaped than a header,
  with a safer *visible* failure mode (a wrong/missing rename shows up as an
  ugly un-prefixed name in the generated header, not a silent ABI mismatch),
  but not zero maintenance.
- `AutoEncoderHandle` (`Box<dyn VideoEncoder>` type alias) needs a concrete,
  verified fix — most likely a one-line newtype wrapper
  (`pub struct AutoEncoderHandle(Box<dyn VideoEncoder>);`) — before
  `mediaway-pipeline-ffi`'s own migration can succeed with `cbindgen`. Flagged
  as a real, non-hypothetical follow-up, not a hidden unknown.
- Two hand-written headers coexist with the new generated workflow for an
  interim, per-crate-tracked period.

## Deferred / open questions

- **Exact `cbindgen.toml` keys for enum-variant prefix truncation.** The
  existing headers use a shortened prefix (`MEDIAWAY_CODEC_H264`, not
  `MEDIAWAY_CODEC_KIND_H264`) that plain automatic `prefix_with_name` (using
  the type's own renamed name) would not reproduce. Verify the installed
  `cbindgen` version's own config schema before finalizing any crate's
  `cbindgen.toml` — not asserted here without checking.
- **Whether `cbindgen.toml` supports config inheritance/include** for a shared
  house-style base. If not, house style stays a copy-paste template + wiki
  page, not literal shared config.
- **`AutoEncoderHandle`'s opaque-handle shape fix** — decide the exact form
  when `mediaway-pipeline-ffi`'s own migration is scheduled.
- **Migration order/timing for the two existing headers** — tracked against
  each crate's own `docs/roadmap.md`, not fixed by this ADR.
- **`mediaway-common-ffi` unification** (shared status enum / buffer-free
  function across `-ffi` crates) — unaffected by this ADR; remains its own
  future decision as already logged in both crates' first-pass ADRs. See also
  [ADR-0015](0015-common-ffi-unification.md) if accepted, which may change the
  type/status-enum surface these `-ffi` crates generate headers for.

## References

- spec: [`docs/spec/c-ffi.md`](../spec/c-ffi.md)
- related ADR: [ADR-0004](0004-c-ffi.md), [ADR-0006](0006-caveats-and-clarity.md)
- crate ADRs this supersedes-in-part: `crates/mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md`
  §8 / § Deferred, `crates/mediaway-pipeline-ffi/adr/0001-auto-encode-c-abi.md`
  §8 / § Deferred
- conventions: [`docs/conventions/deps-policy.md`](../conventions/deps-policy.md),
  [`docs/conventions/scripts.md`](../conventions/scripts.md)
- wiki: `docs/ai/wiki/container/ffi-c-abi.md`, `docs/ai/wiki/pipeline/ffi-c-abi.md`

## 2026-08-05 addendum: tooling validated, real findings

Written against [ADR-0021](0021-workspace-consolidation.md)'s merged `mediaway-ffi`
crate (`common`/`container`/`device`/`pipeline` modules, one header set) — the four
per-capability crates this ADR originally targeted no longer exist separately.
`cbindgen 0.29.4` installed and run against the real crate; `crates/mediaway-ffi/cbindgen.toml`
and `tools/scripts/cbindgen-headers.ts` (`generate`/`verify`) now exist and produce a
header that compiles cleanly (`gcc`/`g++`, `-Wall -Wextra`, both C and C++ modes) for
the crate's full default feature set. Resolves this ADR's own § Deferred/open questions:

- **`pub use X as Y` re-exports are invisible to `cbindgen`** (not documented anywhere
  in its own docs; discovered empirically) — it cannot resolve them to their
  underlying `#[repr(C)]` definition at all (`Can't find MediawayRational`-class
  warnings, then a missing/incomplete type in the generated header). `pub type Y = X;`
  **type aliases** resolve correctly and collapse to one typedef. Fixing this required
  converting every `pub use crate::common::{types,gpu}::X as MediawayY;` re-export in
  `container::types`/`device::types`/`pipeline::types` (~17 sites) to `pub type`
  aliases — a real, applied source change (this ADR's own affected files), not just a
  config knob.
- **`MediawayPixelFormat`/`MediawaySampleFormat`/`MediawayVideoFrameStorageKind` were
  independently duplicated Rust definitions** in `device::types` and `pipeline::types`
  (not merely re-exports) — a real name collision once `cbindgen` combines the whole
  crate into one header. Fixed by moving them into `common::types` as the single
  canonical definition (`adr/common/0001-shared-header-consolidation.md`'s Rust-side
  analog), consumed via the same `pub type` alias pattern.
- **`AutoEncoderHandle`'s opaque-handle shape**: confirmed the ADR's own suspicion —
  a bare `Box<dyn VideoEncoder>` type alias cannot be forward-declared as an opaque
  struct. Fixed with a `#[repr(transparent)]` newtype
  (`struct AutoEncoderHandle(Box<dyn VideoEncoder>)`) plus a delegating `VideoEncoder`
  impl; the crate's other trait-object handle, `AudioEncodeSessionHandle`
  (`pipeline::audio`), had the identical latent problem and got the identical fix.
- **`cbindgen --parse-expand` requires a nightly `rustc`** (`-Zunpretty=expanded`) —
  tried as an alternative to the `pub use` fix above; rejected as a project-wide
  tooling requirement (this workspace targets stable) once the `pub type` fix proved
  sufficient without it.
- **Enum variant prefixing leaks the renamed type's `_t` suffix**: neither
  `[enum] prefix_with_name = true` nor `rename_variants = "QualifiedScreamingSnakeCase"`
  derives the prefix from the original Rust name — both use the post-`[export.rename]`
  C name, producing `MEDIAWAY_STATUS_T_OK` instead of the hand-written headers'
  `MEDIAWAY_STATUS_OK`. No config fix found within this pass's time budget; accepted as
  a known, visible cosmetic divergence (already anticipated by § Decision 5's "known
  cost" framing), not a blocker.
- **`documentation_style = "doxy"` (`/** */`) is a real, compile-breaking hazard**, not
  just cosmetically rougher than hand-written prose: several existing doc comments
  contain markdown bold immediately followed by a slash (e.g. `` **buffer**/texture ``),
  which forms a `**/` sequence that prematurely closes a C block comment and corrupts
  every declaration after it. `documentation_style = "c99"` (`//` line comments) has no
  multi-line terminator sequence and cannot hit this class of bug — switched as the
  config default, not left as a discovered-but-unfixed landmine.
- **`style = "type"`**, not the config template's default `"both"`, matches every
  hand-written header's existing signature style exactly (`mediaway_muxer_t
  *mediaway_muxer_create(void);`, not `struct mediaway_muxer_t *...`).
- **`CodecKind` cannot be fully collapsed**: `MediawayCodecKind` (container) and
  `MediawayPipelineCodecKind` (pipeline) are deliberately distinct C types by an
  earlier, still-valid ADR decision, so the shared base `CodecKind` Rust enum is left
  unrenamed in `[export.rename]` — `cbindgen` emits it as a third, unprefixed,
  unused-by-name leftover definition alongside the two real ones. Accepted as a known
  cosmetic redundancy (source-code searchable, functionally inert), the alternative
  (renaming the base too) would silently re-merge the two intentionally-separate
  types.
- **`--verify`** is a built-in `cbindgen` flag (regenerate and diff against a
  committed file, failing on mismatch) — used directly by the wrapper script's
  `verify` mode rather than hand-rolling the diff logic §3 assumed would be
  hand-written.

**Not done in this pass, tracked as the real per-header migration §4 already
anticipated:** `include/mediaway/{common,container,device,pipeline}.h` are still
hand-written. `generate`'s default output goes to `target/cbindgen/` (gitignored,
scratch), not over the real headers — cutting any one of them over means updating
every `bindings/c/examples/**` file that includes it and re-running that example's
hardware verification, real per-header work this addendum does not shortcut.
`verify` is not yet wired into CI for the same reason: nothing real to diff against
until a header is actually migrated.

**2026-08-05, later same day — a second real finding while adding `pipeline`'s
decode + capture-bridge surfaces:** `cbindgen.toml`'s `[export.rename]` table needs
a manual entry per exported type, and drifts the moment new types are added without
a matching entry (found and fixed twice this same day — decode's three new types,
then this one). More seriously: once `pipeline::capture_bridge`'s two new functions
started referencing `device::{CameraCaptureHandle, DesktopCaptureHandle}` (the
crate's first types referenced by exported functions in **two different modules**),
`cbindgen` stopped emitting an opaque forward-declaration for either type at all —
the generated header uses `mediaway_camera_capture_t`/`mediaway_desktop_capture_t`
in the bridge functions' signatures but never declares either type anywhere,
failing to compile. Root cause not investigated further this pass (same
time-boxing as the enum-prefix `_T` finding above); the working, shipped
`include/mediaway/pipeline.h` (hand-written) is unaffected — this is a `cbindgen`
generation gap only, found via `tools/scripts/cbindgen-headers.ts generate` +
`gcc -fsyntax-only`, not a regression in any real header.

ADRs are written in **English**.
