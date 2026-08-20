# ADR-0008: Opt-in exclusive DXGI Desktop Duplication — true Zero-Copy

- **Status**: Accepted
- **Date**: 2026-08-20
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-windows`

## Context

[ADR-0006](0006-shared-desktop-duplication.md) made every `CaptureSource::Screen` session —
including a lone consumer — go through a shared driver thread that pays one mandatory
`AcquireNextFrame` → ring-slot `CopyResource` per frame, because the DDA-owned resource
becomes invalid the instant `ReleaseFrame` runs and the driver thread ticks on its own timer
independent of consumer activity. [ADR-0001](0001-dxgi-desktop-duplication.md)'s original
design avoided this copy entirely for a single consumer by handing out the DDA-acquired
texture directly and deferring `ReleaseFrame` until the consumer's own `release_frame()` call —
but that design was removed when ADR-0006 required **universal registration** so a second
consumer could discover and join an already-open session for the same output.
[ADR-0007](0007-ring-buffer-shared-desktop-duplication.md) explicitly considered reintroducing
a solo-consumer-only skip-copy path and declined it, reasoning it "would not even be materially
simpler than the general [ring] design... while providing none of the benefit for N>1."

Re-examined 2026-08-20 at the user's request, now that real hardware verification work
(`screen_capture_delivers_zero_copy_frame_or_skip`) has confirmed the ring path works
end-to-end: the majority of real callers open exactly one screen-capture session per output
(the multi-consumer fan-out ADR-0007 optimizes for is the less common case), and for that
majority, the ring's mandatory copy is pure overhead with no shareability benefit actually
used. This ADR reintroduces the copy-free path — narrower than ADR-0001's original (which was
the *only* mode), and explicitly **opt-in** rather than automatic, so ADR-0006's shareability
guarantee stays the unconditional default for every caller who doesn't ask otherwise.

## Decision

> Add `CaptureSharing` (new enum, `desktop/video.rs`) as a field on
> `DesktopVideoCaptureConfig`, default `Shared` (today's behavior, unchanged):
>
> ```rust
> #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
> #[non_exhaustive]
> pub enum CaptureSharing {
>     /// Discoverable/joinable by a later `open()` for the same output (current
>     /// behavior) — shared driver thread + ring, one mandatory `CopyResource`/frame.
>     #[default]
>     Shared,
>     /// Caller asserts it is the only consumer for this output and will not need
>     /// another session to join. True Zero-Copy: the DDA-acquired texture is
>     /// handed out directly every `poll_frame`, no ring, no per-tick copy,
>     /// no driver thread — `release_frame` calls `ReleaseFrame` directly on the
>     /// calling thread. A concurrent `open()` for the same output — `Shared` or
>     /// `Exclusive` — while this session is alive fails with
>     /// `CaptureError::AccessDenied`: DXGI itself allows only one live
>     /// duplication per output per process, so this is enforced by the OS, not
>     /// by any bookkeeping this crate adds.
>     Exclusive,
> }
> ```
>
> Only meaningful for `DesktopCaptureSource::Screen` on Windows (`Window` never shares —
> WGC is already per-`HWND`; other platforms ignore the field). Added to
> `DesktopVideoCaptureConfig` rather than `DesktopCaptureSource::Screen` itself, so
> `DesktopCaptureSource`'s cross-platform enum shape (read by every platform crate) does not
> need a Windows-only concept — `DesktopVideoCaptureConfig::screen`/`::window`'s existing
> signatures are unchanged (field defaults to `Shared` inside both), so all 5 existing call
> sites in this workspace keep compiling with no changes.
>
> New module `windows_desktop/dxgi_exclusive.rs` (`ExclusiveDuplication`): owns one
> `IDXGIOutputDuplication` directly, no driver thread, no `Arc`/ring. `poll_frame` calls
> `AcquireNextFrame` on the calling thread and hands out the resulting `ID3D11Texture2D*`
> directly (cast from the returned `IDXGIResource`, mirroring `dxgi_shared.rs`'s existing cast);
> `release_frame` calls `ReleaseFrame`. Rejects a second `poll_frame` before `release_frame`
> with `CaptureError::Backend` (mirrors `dxgi_shared`'s `ConsumerRecord.held` check).
> `WindowsScreenCapture` gains `enum Backing { Shared(Session), Exclusive(ExclusiveSession) }`
> (reviving the name ADR-0007 § Context noted the shipped code never actually had), dispatching
> in `open()` on `config.sharing`.

### Why no registry entry for `Exclusive`

`dxgi_shared`'s registry exists so a second `attach()` for the same output can *join* the
first instead of calling `DuplicateOutput` again (which DXGI would reject). An `Exclusive`
session, by construction, promises no second consumer will ever attach — so there is nothing
to let a second caller join, and skipping registration entirely is simpler than adding a
"registered but not joinable" bookkeeping entry solely to produce a marginally clearer error.
DXGI's own single-duplication-per-output rule is the real correctness backstop either way: a
second `open()` (in either mode) while an `Exclusive` session is alive gets a real
`DuplicateOutput` failure, mapped to `CaptureError::AccessDenied` — an honest failure, not a
silently-ignored conflict.

### Lifetime / trust model — identical to the already-verified ring path

`poll_frame`'s returned texture is the DDA's own real backing resource, invalidated the
instant `ReleaseFrame` runs — so a caller must finish **issuing** (not necessarily completing)
any GPU work that reads it before calling `release_frame`, exactly the "issue-then-drop, never
drop-then-issue-later" contract ADR-0007 § Q2 already documents for the ring's `Arc`-drop
recycle signal, itself matching the already-hardware-verified WMF Zero-Copy encode trust
model. No new risk category — same caveat, same rustdoc treatment.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Auto-detect "exactly one consumer" and skip the copy transparently inside the existing shared/ring path | Requires the driver thread to know upfront whether a second consumer might attach later (it can't) — the whole reason `Exclusive` must be an explicit caller assertion, not an automatic optimization, is that "am I really alone" is caller-known information, not driver-observable information at open() time. |
| Live conversion: start `Shared`, transparently upgrade a solo session to skip-copy, downgrade back to `Shared` the moment a second consumer attaches | A materially more complex state machine (mid-flight driver-thread teardown/handoff, or a hybrid copy/no-copy driver loop) for a case (`Exclusive` opened, then a second consumer wants the same output) that is rare and — per this ADR — simply fails cleanly instead. Revisit only if a real caller needs both properties simultaneously. |
| Reintroduce `Exclusive` as the only mode again (ADR-0001's original scope) | Would silently break every current caller relying on ADR-0006's shareability default (e.g., a future feature needing two consumers on one output) — this ADR is additive/opt-in specifically to avoid re-litigating that default. |
| Register `Exclusive` sessions in the same registry, marked non-joinable, for a clearer "already exclusively open" error | Extra bookkeeping (a registry entry that exists only to be rejected) for a marginal error-message improvement over DXGI's own `AccessDenied` — not worth the complexity per AGENTS.md simplicity-first. |

## Consequences

### Positive

- The common single-consumer case (the majority of real callers) reaches genuine
  `EncodePathClass`/Zero-Copy-equivalent screen capture with zero payload copies at all — no
  ring, no per-tick `CopyResource`, no driver thread.
- Fully additive: `Shared` stays the default, every existing caller's behavior is unchanged.
- No changes to `dxgi_shared.rs`'s ring/driver-thread logic at all — `Exclusive` is a
  self-contained parallel path, so the already-hardware-verified ring code is not at risk of
  regression from this change.

### Negative / Trade-offs

- Two code paths to maintain in `windows_desktop` for screen capture now, not one — the exact
  trade-off ADR-0007 flagged when it declined a narrower version of this same idea. Accepted
  this time because the earlier "not materially simpler" reasoning was about avoiding a
  *third*, in-between path bolted onto the ring; a fully separate, driver-thread-free module is
  simpler in isolation than that would have been.
- A caller that opens `Exclusive` and then genuinely needs a second consumer later must close
  and reopen as `Shared` — no live upgrade path (see § Alternatives).
- `CaptureSharing` on `DesktopVideoCaptureConfig` is a dead field for every non-Windows-screen
  case (`Window`, other platforms) — acceptable, matches `gpu_device`'s existing precedent of a
  config field not every backend consumes.

## Addendum (2026-08-20): implementation + hardware verification

Implemented exactly per § Decision — `CaptureSharing` on `DesktopVideoCaptureConfig` (not
`DesktopCaptureSource::Screen`, to avoid a Windows-only concept touching the cross-platform
source enum), `dxgi_exclusive.rs`'s `ExclusiveDuplication`, and `WindowsScreenCapture`'s
`Backing::Shared | Exclusive` dispatch. Both existing constructors (`::screen`/`::window`)
default the new field to `Shared` — all 5 pre-existing call sites across the workspace needed
only a added `sharing: CaptureSharing::Shared` field (a mechanical, non-behavioral fixup caught
by the compiler, not a design change).

**Hardware-verified on the reference RTX 4090**: `exclusive_screen_capture_delivers_zero_copy_
frame_or_skip` (`lib_tests.rs`) bounded-polls until a real frame is delivered and hard-asserts
a genuine `GpuBufferHandle::DirectX11` — no `CopyResource` call exists anywhere in
`dxgi_exclusive.rs`'s `poll_frame`, confirmed by direct source inspection, not just passing
tests. `exclusive_screen_capture_blocks_second_open_or_skip` confirms the concurrent-open
backstop: a second `open()` (`Exclusive`, same output) while the first is alive fails with
`CaptureError::AccessDenied` — printed `second open correctly failed: AccessDenied`, exactly
the DXGI-enforced behavior § "Why no registry entry for Exclusive" predicted, no extra
bookkeeping needed. `cargo check`/`clippy --all-targets --all-features -- -D warnings`/`fmt
--check` clean across the whole workspace; the pre-existing `Shared`-path tests
(`screen_capture_delivers_zero_copy_frame_or_skip`, `open_screen_zero_copy_poll_release_or_
skip`, the dual-attach hardware tests) all still pass unchanged — `dxgi_shared.rs` itself was
not touched by this change.

## References

- [ADR-0001](0001-dxgi-desktop-duplication.md) — original exclusive Zero-Copy design this
  revives (narrower: opt-in, not the only mode).
- [ADR-0006](0006-shared-desktop-duplication.md) — universal registration; the default this
  ADR does not change.
- [ADR-0007](0007-ring-buffer-shared-desktop-duplication.md) — ring-buffered fan-out; declined
  a narrower version of this same idea, reconsidered here at user request post-verification.
- [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) — the
  issue-then-drop lifetime caveat this ADR's `poll_frame`/`release_frame` inherit unchanged.

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../docs/adr/).
