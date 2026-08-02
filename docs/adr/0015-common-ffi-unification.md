# ADR-0015: `mediaway-common-ffi` — partial unification (value-type mirrors + buffer helper only)

- **Status**: Proposed
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)

## Context

Two `mediaway-*-ffi` crates exist today, both real, hardware-link-verified, independent
`cdylib`/`staticlib`/`rlib` targets: `mediaway-container-ffi` (ADR:
[`crates/mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md`](../../crates/mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md))
and `mediaway-pipeline-ffi` (ADR:
[`crates/mediaway-pipeline-ffi/adr/0001-auto-encode-c-abi.md`](../../crates/mediaway-pipeline-ffi/adr/0001-auto-encode-c-abi.md)).
Both ADRs explicitly deferred "start `mediaway-common-ffi` now?" to a later ADR with both
precedents in hand ([`docs/spec/c-ffi.md`](../spec/c-ffi.md) already lists it as an optional,
unbuilt crate; [`docs/spec/crate-packaging.md`](../spec/crate-packaging.md) line 31 reserves the
name). This is that ADR. A third crate, `mediaway-device-ffi`, is being designed in parallel (its
own ADR, not yet merged) and is expected to face the same duplication points — part of why this
is being decided now with two real data points instead of speculatively at n=1.

Verified concrete duplication (read from source, not assumed):

| Item | `mediaway-container-ffi` | `mediaway-pipeline-ffi` | Actually identical? |
|---|---|---|---|
| Status enum | `MediawayStatus` (`src/status.rs`, 9 variants) | `MediawayPipelineStatus` (`src/status.rs`, 13 variants) | **No** — only 3 concepts coincide (`InvalidArgument`/`InternalPanic`/`HandlePoisoned`); mux-error and encoder-error variants are per-crate and deliberately not numerically aligned (both source ADRs reasoned this through explicitly) |
| Buffer free helper | `mediaway_buffer_free` (`src/buffer.rs`) | `mediaway_pipeline_ffi_buffer_free` (`src/buffer.rs`) | **Yes, verbatim** — `borrow_slice`/`leak_boxed_slice`/`reclaim_boxed_slice` bodies are byte-for-byte identical private helpers; only the exported `#[no_mangle]` free function's *name* differs (deliberately, to avoid a duplicate-symbol link error if both crates' `staticlib` outputs are linked into one consumer) |
| Codec-kind enum | `MediawayCodecKind` (`src/types.rs`, 12 variants) | `MediawayPipelineCodecKind` (`src/types.rs`, 12 variants) | **Yes, numerically** — distinct C type names, but both wrap `mediaway_common::CodecKind` end-to-end and mirror it 1:1 by construction; the 1:1 mapping is currently guaranteed only by a doc comment and by both authors independently transcribing the same source enum, not by a shared implementation |
| Rational struct | `MediawayRational { num: u64, den: u32 }` | `MediawayRational { num: u64, den: u32 }` | **Yes, verbatim** — independently re-derived from `mediaway_common::Rational`, same fields, same conversions |

Both crates today build `crate-type = ["rlib", "cdylib", "staticlib"]` (verified in each
`Cargo.toml`) and hand-write their own header (`include/mediaway/container.h`,
`include/mediaway/pipeline.h`) — no shared header exists.

## Decision

> Create `mediaway-common-ffi` now, as an **`rlib`-only, header-less internal helper crate with
> zero `#[no_mangle]`/`extern "C"` symbols of its own**. It unifies exactly two things — the
> `Rational`/`CodecKind` `#[repr(C)]` value-type mirrors (+ their conversions to/from
> `mediaway_common`) and the private `borrow_slice` / `leak_boxed_slice` / `reclaim_boxed_slice`
> buffer-ownership helper implementation. It does **not** unify the per-crate status enums, and it
> does **not** define any shared exported free function — each `-ffi` crate keeps its own
> distinctly-named `#[no_mangle]` symbol, now implemented by calling into the shared helper.

### What moves, what doesn't

| Duplication point | Move to `mediaway-common-ffi`? | Why |
|---|---|---|
| `Rational`/`CodecKind` `#[repr(C)]` mirrors + conversions | **Yes** | Both already wrap the exact same underlying `mediaway_common` type 1:1; unifying converts a currently-informal "we keep these in sync by convention + a doc comment" invariant into a structurally-guaranteed one (one implementation, not two independently-transcribed copies that could silently drift, e.g. if a new `CodecKind` variant is added and only one crate's transcription is updated) |
| `borrow_slice`/`leak_boxed_slice`/`reclaim_boxed_slice` implementation | **Yes** | Verbatim-duplicate, `unsafe`-bearing pointer-reconstruction logic; a bug fix applied to one copy and forgotten in the other is a real risk class this removes. Both crates keep their own distinctly-named exported `_free` function(s) — only the body they call into is shared |
| `MediawayStatus` / `MediawayPipelineStatus` | **No** | The two enums share only 3 of 9/13 variants by coincidence (both crates independently inventing similar FFI-layer concepts), not because of a shared underlying Rust error type. `MediawayPipelineStatus`'s 5 encoder-error and 3 mux-error variants have no container-ffi analog; a hypothetical `mediaway-device-ffi` status enum would differ again (permission/capability errors, no "mux" concept at all). Forcing a shared enum today would either expose crate-irrelevant variants to callers who never see that capability, or require an "extensible common core + per-crate additions" C idiom with no clean precedent anywhere in this codebase — premature generalization from n=2 samples for a shape that should wait for real evidence, if it should exist at all (see Alternatives) |
| Per-crate exported free-function **name** (`mediaway_buffer_free` vs. `mediaway_pipeline_ffi_buffer_free`) | **No** | This distinctness is the *fix*, not the problem — see below |

### `mediaway-common-ffi` never gets its own `cdylib`/`staticlib` or public C symbols

```toml
[lib]
crate-type = ["rlib"]  # explicit, not the implicit default — states the "no C symbols" intent
```

It is consumed as an ordinary Rust path dependency by `mediaway-container-ffi`,
`mediaway-pipeline-ffi`, and (later) `mediaway-device-ffi`. It ships **no** `include/` header and
is never itself linked into a C consumer. This is deliberate: the crate exists purely to remove
*Rust-internal* duplication; it must not introduce a *third* set of exported C symbols that would
need its own duplicate-symbol-avoidance story. `#[repr(C)]` types are usable across a normal Rust
crate boundary without any `extern "C"` involved — sharing the type does not require sharing a
symbol.

### Per-crate re-export, not a per-crate rename of the public C type

Each consuming crate keeps its own existing Rust (and therefore C-header) type name, re-exported
under a local alias:

```rust
// mediaway-container-ffi/src/types.rs
pub use mediaway_common_ffi::{CodecKind as MediawayCodecKind, Rational as MediawayRational};

// mediaway-pipeline-ffi/src/types.rs
pub use mediaway_common_ffi::{CodecKind as MediawayPipelineCodecKind, Rational as MediawayRational};
```

Both crates' hand-written headers (`container.h` declares `mediaway_codec_kind_t`; `pipeline.h`
declares `mediaway_pipeline_codec_kind_t`) are **untouched** — the Rust-side rename is invisible at
the C boundary. This preserves the exact property both source ADRs' own reasoning relied on
(distinct C type names avoid a `typedef` redefinition hazard if a consumer ever includes both
headers in one translation unit); unifying the *implementation* does not force unifying the
*C-facing name*. Going further — one shared `mediaway_common.h` with one canonical C type name
`#include`d by both headers — is a larger, separate move (§ Deferred), not decided here.

### Migration scope (concrete)

`crates/mediaway-container-ffi/`:

- `Cargo.toml` — add `mediaway-common-ffi = { path = "../mediaway-common-ffi", version = "0.1.0" }`.
- `src/types.rs` — delete the local `MediawayRational` struct + its two `From` impls, and the local
  `MediawayCodecKind` enum + its two `From` impls (24 match arms); replace with the two-line
  `pub use` above.
- `src/buffer.rs` — delete the local `borrow_slice`/`leak_boxed_slice`/`reclaim_boxed_slice`
  bodies; `use mediaway_common_ffi::buffer::{borrow_slice, leak_boxed_slice, reclaim_boxed_slice};`.
  The three exported `#[no_mangle] extern "C"` functions (`mediaway_buffer_free`,
  `mediaway_packet_free`, `mediaway_stream_info_free`) are **unchanged** in name/signature — only
  their bodies now call the shared `reclaim_boxed_slice`.
- `src/status.rs` — **unchanged**.
- `include/mediaway/container.h` — **unchanged**.
- `adr/0001-mp4-mux-demux-c-abi.md` — append a resolution note to its own § Deferred (its
  `mediaway-common-ffi` mention there is about a *different*, still-open idea — a borrowed/
  Zero-Copy output buffer-handle type — not this ADR's scope; see § Deferred below).

`crates/mediaway-pipeline-ffi/`:

- `Cargo.toml` — same added path dependency.
- `src/types.rs` — delete the local `MediawayRational` duplicate and `MediawayPipelineCodecKind`'s
  field list/derives/24 match arms; replace with the second `pub use` line above.
  `MediawayPixelFormat` and `MediawayAutoVideoEncodeConfig`/`MediawayVideoFrame` are **unchanged**
  — no container-ffi analog exists for any of them, nothing to unify.
- `src/buffer.rs` — delete the local helper bodies; import the shared ones. The one exported
  function (`mediaway_pipeline_ffi_buffer_free`) is **unchanged** in name/signature.
- `src/status.rs` — **unchanged**.
- `include/mediaway/pipeline.h` — **unchanged**.
- `adr/0001-auto-encode-c-abi.md` — resolve its own § Deferred `mediaway-common-ffi` bullet by
  pointing at this ADR.

New `crates/mediaway-common-ffi/`:

- `Cargo.toml` (`crate-type = ["rlib"]`, depends on `mediaway-common` only), `src/lib.rs`,
  `src/types.rs` (`Rational`, `CodecKind` + conversions), `src/buffer.rs` (the three helpers, now
  `pub` instead of `pub(crate)` since they cross a crate boundary). No `include/` directory. A
  short crate-local `adr/` + `docs/roadmap.md` at implementation time, per
  [`docs/conventions/docs-layout.md`](../conventions/docs-layout.md), pointing back to this ADR
  rather than re-deriving the same reasoning.

This ADR is a design decision only; the migration above is not implemented by this document.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Keep both crates fully independent, no `mediaway-common-ffi` at all | Genuine option, not obviously wrong — but the buffer-helper and value-type duplication is real (not hypothetical), already at 2 verbatim copies with a documented 3rd (`mediaway-device-ffi`) imminent, and the codec-kind/rational 1:1 mapping is currently enforced only by convention, not structurally |
| Fully unify **including** the status enums into one shared `mediaway_status_t` | The two enums overlap in only 3 of 9/13 variants by coincidence, not a shared error hierarchy; forcing a shared type would expose crate-irrelevant variants or need an unproven "common core + extension" C idiom; premature at n=2 samples, and a third crate's real shape isn't known yet |
| Ship `mediaway-common-ffi` as its own `cdylib`/`staticlib` with public C symbols | No consumer needs to link it standalone; every real use today is "one more Rust crate two `-ffi` crates depend on," which a plain `rlib` provides with strictly less surface area and zero new duplicate-symbol risk |
| Also unify the C header text now (one shared `mediaway_common.h` both headers `#include`) | Bigger, riskier move — would require changing at least one crate's public C-facing type name (e.g. `mediaway_pipeline_codec_kind_t` → `mediaway_codec_kind_t`) to actually share text, and is entangled with the still-open `cbindgen`-adoption question both source ADRs already deferred; better revisited together, once ≥3 `-ffi` crates exist |

## Consequences

### Positive

- The `unsafe`-bearing buffer leak/reclaim logic has one canonical, single-audited implementation
  instead of two copies that could silently drift.
- The `CodecKind`/`Rational` 1:1 numeric mirroring — previously a doc-comment promise — becomes
  structurally guaranteed by construction (one implementation, two re-exports).
- `mediaway-device-ffi` can depend on `mediaway-common-ffi` from its first commit instead of
  copy-pasting a third time.
- Zero new public C surface and zero new duplicate-symbol risk: the shared crate never exports
  `#[no_mangle]` functions itself.
- Each existing crate's independently-named status enum and independently-named free function —
  the property that lets a consumer link both crates with zero coordination — is preserved exactly
  as-is; this ADR treats that independence as a feature to keep, not a bug to fix.

### Negative / Trade-offs

- One more workspace crate (Cargo.toml, CI matrix entry, doc surface) for roughly 150–200 lines of
  shared code.
- Status-enum fragmentation across `-ffi` crates remains, unresolved by design: a consumer linking
  both `mediaway-container-ffi` and `mediaway-pipeline-ffi` still juggles two distinct status
  types with no shared constants, even for the 3 conceptually-overlapping cases. This ADR argues
  that is the correct state pre-1.0, not a residual problem, but it does make
  `docs/spec/c-ffi.md`'s current "error codes, `Rational`, buffer view only" description of
  `mediaway-common-ffi` stale — updated alongside this ADR (§ References).
- Migration touches two already-shipped, hardware-link-verified crates' internals
  (`types.rs`/`buffer.rs`/`Cargo.toml` in both). Low risk — private-implementation rename, no
  public C ABI change — but still requires rebuilding/relinking both crates' `cdylib`/`staticlib`
  targets and re-running their existing smoke tests before merging, not a free refactor.
- Hand-written headers stay textually duplicated per crate (§ Alternatives, last row) — this ADR
  only removes Rust-side duplication, not C-header text duplication.

## Deferred to a later ADR / explicit open questions

- **Shared C header text** (one `mediaway_common.h` both `-ffi` headers `#include`) — deferred
  until the `cbindgen`-adoption question (already open in both source ADRs' § Deferred) is
  revisited, and until ≥3 hand-written headers exist to judge the pattern against.
  **Update:** `mediaway-device-ffi/adr/0001-capture-c-abi.md` §7 confirmed this is no longer
  hypothetical — co-including `device.h` and `pipeline.h` in one C translation unit is a real,
  demonstrated `gcc -std=c11` redefinition error (both independently `typedef struct {...}`
  the identical `mediaway_rational_t`/`mediaway_pixel_format_t` shape), not merely an
  unconfirmed C++ risk as that crate's ADR originally assumed. Raises the priority of this
  deferral; still not solved by this ADR.
- **Status-enum unification** — not attempted here; revisit only if a real 3rd/4th crate's error
  surface turns out to overlap meaningfully with an existing one, not preemptively.
- **Borrowed/Zero-Copy CPU buffer-handle ABI type** (mentioned in
  `mediaway-container-ffi/adr/0001`'s own § Deferred, "mirroring `GpuBufferHandle`, but CPU-side")
  — a **different, still fully open** question from this ADR's scope. This ADR only shares the
  existing *owned-copy* leak/reclaim mechanism; it does not add a new Zero-Copy ABI type or change
  any function's ownership semantics.
- **GPU-handle-across-C-boundary types** in `mediaway-common-ffi`
  ([`docs/spec/gpu-interop.md`](../spec/gpu-interop.md)) — no such type exists in any `-ffi` crate
  today; nothing to move yet.

## References

- [`crates/mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md`](../../crates/mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md) — first `-ffi` crate; § Deferred logged the Zero-Copy buffer-handle question this ADR does not resolve
- [`crates/mediaway-pipeline-ffi/adr/0001-auto-encode-c-abi.md`](../../crates/mediaway-pipeline-ffi/adr/0001-auto-encode-c-abi.md) — second `-ffi` crate; § Deferred explicitly asked this ADR's question
- [`crates/mediaway-container-ffi/src/{status,types,buffer}.rs`](../../crates/mediaway-container-ffi/src/status.rs), [`crates/mediaway-pipeline-ffi/src/{status,types,buffer}.rs`](../../crates/mediaway-pipeline-ffi/src/status.rs) — verified duplication points
- [`docs/spec/c-ffi.md`](../spec/c-ffi.md), [`docs/spec/crate-packaging.md`](../spec/crate-packaging.md) — reserved `mediaway-common-ffi` naming/role, refined by this ADR
- [`docs/adr/0004-c-ffi.md`](0004-c-ffi.md) — workspace C-FFI policy this ADR concretizes further
- [`docs/conventions/deps-policy.md`](../conventions/deps-policy.md) — new-crate justification (internal split, not a new external dependency)
- [`docs/conventions/docs-layout.md`](../conventions/docs-layout.md) — crate-local `adr/`/`docs/roadmap.md` placement for the new crate

ADRs are **English**. Numbering is per `docs/adr/` folder.
