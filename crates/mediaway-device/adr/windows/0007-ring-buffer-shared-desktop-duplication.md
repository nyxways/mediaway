# ADR-0007: Ring-buffered frame fan-out for shared DXGI Desktop Duplication

- **Status**: Accepted (2026-08-13 — approved by @dev-nyxie; implemented in
  `dxgi_shared.rs`). **Verification**: compiles and passes `cargo clippy
  --all-targets -- -D warnings` on real Windows/hardware (this is genuine
  native code, not cross-compiled); the two pre-existing hardware-gated
  tests (`attach_twice_to_same_output_both_succeed_or_skip`,
  `attach_with_mismatched_device_is_invalid_input_or_skip`) pass unchanged.
  **Not verified**: actual frame delivery through the new ring end-to-end —
  a manual multi-consumer scratch test was attempted on real hardware but
  the dev session's desktop was locked at the time
  (`AcquireNextFrame` returns `DXGI_ERROR_WAIT_TIMEOUT` unconditionally on a
  locked/secure desktop, a documented DXGI Desktop Duplication API
  limitation, confirmed via direct HRESULT tracing — not a bug in this
  change). The publish/reclaim logic has not had a real screen-content
  change flow through it on this hardware. Re-verify with an unlocked
  session before upgrading any related README Zero-Copy mark.
- **Date**: 2026-08-13
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-windows`

## Context

[ADR-0006](0006-shared-desktop-duplication.md) (Accepted, implemented in
[`dxgi_shared.rs`](../../src/windows_desktop/dxgi_shared.rs)) made every
`CaptureSource::Screen` session — including a lone consumer — go through a
shared driver thread that performs one GPU-side `CopyResource` **per attached
consumer per frame**, on top of the already-necessary DDA→scratch copy
(`AcquireNextFrame` → `CopyResource` into a scratch texture → `ReleaseFrame`,
required because the DDA-owned resource becomes invalid the instant
`ReleaseFrame` runs). ADR-0006 named, but explicitly deferred, a
"solo-consumer skip-copy" optimization and rejected general session pooling,
cross-device sharing, and backpressure-on-slow-consumer as out of scope.

This ADR was scoped by the user, after being offered the narrower
solo-consumer-only option, to the **full** replacement: a ring/rotating
buffer that **all** attached consumers read directly, eliminating the
per-consumer copy entirely so only the one already-necessary DDA→scratch
copy remains. The two invariants ADR-0006 established must still hold: the
driver thread never blocks on any consumer, and a consumer never observes a
torn/partially-written frame.

**Correction to ADR-0006's own sketch, found while reading the real
code for this ADR (not a re-litigation, a factual correction):** ADR-0006's
"Type shape" sketch proposed `Backing::Exclusive(CaptureSession) |
Shared { .. }` so the pre-existing zero-copy single-consumer path would
stay a distinct, unchanged variant. The shipped `dxgi.rs` never implements
that split — per ADR-0006's own "Correction versus this ADR's first draft"
paragraph, **every** `WindowsScreenCapture::open` on `CaptureSource::Screen`
routes through `dxgi_shared::attach` unconditionally, because an
unregistered fast-path session can never be discovered or joined later.
`Backing::Exclusive`/`enum Backing` do not exist in the current source —
`WindowsScreenCapture` is a single struct holding `Option<Session>`
(`dxgi.rs`). This matters for Q4 below: there is no live "exclusive" code
path today to keep or subsume; the question is only whether the new ring
design needs a special-cased "solo" branch.

`sample_from_dx11_texture`
([`mediaway-encoder/src/windows/wmf/dx11.rs`](../../../mediaway-encoder/src/windows/wmf/dx11.rs))
confirms the WMF hardware encoder already receives a **caller-owned**
`ID3D11Texture2D*` directly (`MFCreateDXGISurfaceBuffer(..., fCopy = false)`,
a genuine Zero-Copy wrap, no CPU copy), and `push_dx11_frame`
([`mediaway-encoder/src/windows/wmf/video.rs`](../../../mediaway-encoder/src/windows/wmf/video.rs))
calls `ProcessInput` synchronously and returns without waiting for GPU
completion — the only synchronization is `wait_need_input` gating the
*next* `ProcessInput` call on the `METransformNeedInput` event, not a
completion fence on the texture just submitted. This is the existing,
already-hardware-verified trust model this ADR's recycle signal must match
(see Decision § Recycle signal).

## Decision

> Replace the shared driver thread's O(N) per-consumer `CopyResource`
> fan-out with a fixed-depth ring of GPU textures that consumers read via
> cheap `Arc` clones. The driver only overwrites a ring slot once nothing
> external still references it; when every slot is referenced (a
> pathological, bounded case), it falls back to a one-off transient copy
> instead of blocking or corrupting a slot a consumer is still viewing. The
> mandatory DDA→scratch copy is unchanged (still required — see Context).

### Q1 — Ring depth and the "ring exhausted while a straggler holds a slot" case

**Correctness is decoupled from ring depth.** A fixed depth alone cannot
*guarantee* no read-during-write for an arbitrarily slow consumer (the
user's option (a) is rightly rejected as unacceptable). This ADR adopts a
refinement of option (b): each ring slot is wrapped in `Arc<RingSlot>`; the
driver thread only writes into a slot whose `Arc::strong_count == 1` (i.e.
only the ring's own bookkeeping array holds it — no consumer clone
outstanding). If **no** slot in the fixed-size ring is free this tick (every
slot is referenced by at least one straggling consumer), the driver does
**not** grow the ring permanently and does **not** touch any referenced
slot. Instead it publishes this tick's frame via a one-off **transient**
`Arc`-wrapped texture, allocated on demand and freed automatically once its
own `Arc` refcount drops to zero. This is bounded, self-cleaning, and never
touches a slot a consumer is mid-read on.

Ring depth (`RING_DEPTH`, default **3**) is therefore a throughput/latency
tuning knob, not a correctness parameter: correctness holds for any depth
≥ 1 because of the transient fallback. Depth 3 gives headroom for "a
consumer viewing frame T−1 while the driver just produced frame T" (2, the
double-buffering minimum) plus one slot of slack so a consumer mid-`Arc`-clone
at the moment the driver ticks again does not immediately force a transient
copy. In the steady state where consumers stay roughly caught up, the
transient-fallback path is never exercised and every attached consumer's
`poll_frame` is a cheap `Arc` clone against shared GPU memory — a **real**
Zero-Copy fan-out for any number of caught-up consumers, unlike ADR-0006's
always-copy design. A consumer that falls behind by more than `RING_DEPTH`
frames degrades gracefully to the ADR-0006 always-copy cost for *its own*
lagging frames only — other consumers and the driver thread are unaffected.

### Q2 — Recycle signal: `Arc` drop, not a GPU fence

The recycle signal is **`Arc` drop** (a consumer's `release_frame` dropping
its held `Arc<RingSlot>`), not GPU-side completion. This matches, rather
than weakens, the trust model the WMF Zero-Copy path already relies on and
has hardware-verified (see Context): `ProcessInput` is synchronous only in
the sense that it has *issued* GPU work by the time it returns, not that the
GPU has *finished* that work; safety instead comes from D3D11 immediate-context
command ordering (`ID3D11Multithread::SetMultithreadProtected`, already set
in `dx11.rs::enable_multithread` on the **same** `ID3D11Device` instance
ADR-0006 already requires every attached consumer to share via its
`device_raw` check) — commands submitted on one shared context execute in
submission order, so a driver-thread `CopyResource` overwrite submitted
*after* a consumer's `ProcessInput` read is guaranteed to execute after it,
even though both are asynchronous relative to their calling CPU threads.

This is an **inherited assumption**, not a new risk this ADR introduces:
today's single-consumer path already hands a live DXGI texture straight to
WMF with no fence and is hardware-verified. The ring design's risk profile
on this axis is identical to what already ships. The one new obligation the
ring design adds: a consumer must finish **issuing** (not necessarily
completing) any GPU work that reads its held slot **before** calling
`release_frame` — issue-then-drop, never drop-then-issue-later. A future
consumer that defers GPU work past its own call stack (e.g. queues work and
returns immediately) would break this and must add its own real GPU fence;
this must be documented as a caveat on the ring API
([`caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md)),
not silently assumed.

### Q3 — Type shape (ZCA sketch)

```rust
// mediaway-device-windows/src/windows_desktop/dxgi_shared.rs

const RING_DEPTH: usize = 3;

/// One ring slot: a GPU-resident texture allocated once and reused in place
/// for the ring's whole lifetime — only its *content* changes per tick, via
/// `CopyResource`. Reclaimable by the driver once `Arc::strong_count(&self)
/// == 1` (only the ring's own `Ring::slots` array holds it).
struct RingSlot {
    /// Raw `ID3D11Texture2D*` bit pattern — same "never dereferenced off the
    /// driver thread" discipline as today's `ConsumerRecord::raw_texture_ptr`.
    raw_texture_ptr: usize,
    /// Bumped by the driver each time this slot's content is refreshed;
    /// lets a consumer detect "nothing new since my last poll" (replaces
    /// today's per-consumer `SlotState::Empty`) without an extra flag.
    generation: AtomicU64,
}

struct Ring {
    /// Fixed-size array, allocated once at driver startup — no per-tick Vec
    /// churn (ZCA: SmallVec/Vec avoided entirely here, plain array).
    slots: [Arc<RingSlot>; RING_DEPTH],
    /// Currently-published slot; consumers `Arc::clone` this on `poll_frame`.
    latest: Mutex<Arc<RingSlot>>,
}

/// Cross-thread-visible consumer bookkeeping — plain data + one `Arc`, no
/// COM types, still trivially `Send + Sync`.
struct ConsumerRecord {
    id: u64,
    /// Generation last handed to this consumer, so a second `poll_frame`
    /// before `release_frame` still errors the same way it does today, and
    /// "nothing new" doesn't require touching the ring.
    last_seen_generation: u64,
    /// `Some` between `poll_frame` and `release_frame` — the consumer's own
    /// strong reference. Its `Drop` (in `release_shared_frame`) *is* the
    /// recycle signal (Q2).
    held: Option<Arc<RingSlot>>,
}
```

Driver per-tick loop (unchanged steps kept, only the fan-out step
changes):

```rust
// unchanged: drain control_rx, AcquireNextFrame, cast, ReleaseFrame around
// the copy step below.
fn publish_tick(device: &ID3D11Device, context: &ID3D11DeviceContext,
                 source: &ID3D11Texture2D, ring: &mut Ring) {
    if let Some(slot) = ring.slots.iter().find(|s| Arc::strong_count(s) == 1) {
        // SAFETY: same-device CopyResource; ordering guarantee per Q2.
        unsafe { context.CopyResource(slot_texture(slot), source) };
        slot.generation.fetch_add(1, Ordering::AcqRel);
        *ring.latest.lock().unwrap_or_else(PoisonError::into_inner) = Arc::clone(slot);
    } else {
        publish_transient(device, context, source, ring); // Q1 fallback
    }
}
```

Consumer calls map directly onto "which slot am I viewing":

```rust
// poll_frame: Arc::clone(&*ring.latest.lock()) if its generation differs
// from record.last_seen_generation; store it in record.held; build the
// VideoFrame from its raw_texture_ptr exactly as today.
//
// release_frame: record.held = None — the Arc's Drop is the only action.
```

### Q4 — No distinct "exclusive" path to keep or remove

As established in Context, `Backing::Exclusive` does not exist in the
shipped code today — ADR-0006's own implementation correction already
replaced it with universal registration. So this is not "collapse two
paths into one"; there is only one path, and the question is whether the
ring needs a special-cased branch for a single attached consumer. It does
not: at any `RING_DEPTH ≥ 1` a solo, reasonably-prompt consumer almost
always finds its own previous slot free (`strong_count == 1` the instant it
calls `release_frame`), so it transparently gets the same "cheap `Arc`
clone, no per-consumer copy" treatment as any other caught-up consumer —
recovering ADR-0006's deferred "solo skip-copy" goal as a natural
consequence of the general design, not a separate code path. One real,
unavoidable difference from the *original* ADR-0001 exclusive path remains
and must stay documented: the ring still pays the one mandatory DDA→scratch
`CopyResource` that ADR-0001's now-removed direct-handoff design did not
pay (it handed out the DDA-acquired texture itself). That cost was already
accepted workspace-wide in ADR-0006's Decision (universal registration) and
is unchanged by this ADR.

### Q5 — Keyed per output, unchanged

`Ring` is a field of `SharedDuplication`, which is spawned once per unique
`DeviceId` key in `spawn_driver` (one instance per live output, matching
`REGISTRY: HashMap<DeviceId, Weak<SharedDuplication>>`, unchanged by this
ADR). Multi-monitor capture is unaffected: each output gets its own driver
thread and its own independent ring; a slow consumer or a saturated ring on
one output never touches another output's ring.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Solo-consumer-only skip-copy (narrower scope, offered before this task's scope was set) | User explicitly chose the full ring scope. Per Q4, a solo-only special case would not even be materially simpler than the general design — it would still need the same `Arc`-refcounted-slot machinery, just artificially gated to exactly one consumer, while providing none of the benefit for N>1 caught-up consumers. |
| Unbounded ring growth (permanently add a slot whenever the ring is saturated) | Ties standing GPU memory to the worst historical straggler and never shrinks without extra bookkeeping — exactly what this ADR's bounded ring + one-off transient-copy fallback avoids while still never blocking or corrupting data. |
| GPU fence-based sync (`ID3D11Fence`/`ID3D11Query`) instead of `Arc` refcounting for the recycle signal | More precise (ties recycling to actual GPU completion, not CPU-side issuance order) but adds real complexity — fence creation, cross-thread wait/poll plumbing — for a guarantee the existing WMF Zero-Copy path already gets for free from D3D11 immediate-context ordering + `SetMultithreadProtected` (already relied on, hardware-verified). Revisit only if real corruption is observed, or a future consumer needs to defer GPU work past its own call stack. |
| Steal the oldest referenced slot and force a private copy for its lagging holder | More complex than parking the new frame in a transient buffer and leaving the ring untouched: it requires reaching into an already-attached consumer's live view after the fact, versus this ADR's fallback which only ever affects the *new* frame being published. |
| Cross-device sharing via `OpenSharedResource` | Out of scope, unchanged from ADR-0006 — no new caller need surfaced by this ADR. |

## Consequences

### Positive

- Any number of caught-up consumers share GPU-resident frames via cheap
  `Arc` clones — a real Zero-Copy fan-out, replacing ADR-0006's O(N)
  `CopyResource`-per-consumer cost for the common case.
- Recovers ADR-0006's deferred "solo consumer skip-copy" goal as a natural
  consequence of the general design, with no separate code path to
  maintain (Q4).
- Correctness (no torn reads, driver never blocks) does not depend on
  choosing the "right" ring depth — depth is a pure throughput/latency
  tuning knob (Q1).
- No new standing GPU/memory cost beyond `RING_DEPTH` fixed textures per
  output — the pathological all-slots-held case degrades to (not below)
  ADR-0006's existing always-copy cost, only for the lagging consumer.
- No new Cargo dependency — `std::sync::{Arc, Mutex, atomic::AtomicU64}`
  only.

### Negative / Trade-offs

- Real added complexity versus ADR-0006's always-copy design: per-slot
  refcount-gated write selection, generation tracking, and a transient-copy
  fallback path that must itself be tested (including the "ring fully
  saturated" case, which is hard to exercise without an artificially slow
  consumer in tests).
- The recycle-signal trust model (Q2) is inherited, not newly proven: it
  rests on D3D11 immediate-context ordering across the driver thread and
  any consumer's own GPU work (e.g. WMF encode), which is real and
  hardware-verified for today's usage pattern but would silently break for
  a future consumer that defers GPU submission past `release_frame`. This
  must ship as an explicit rustdoc caveat, not just this ADR's prose.
- A straggling consumer's own frames still cost a real, undocumented-until-now
  transient `CopyResource` once it falls behind by more than `RING_DEPTH`
  frames — a genuine cost that must be named in rustdoc, not hidden as
  "the ring is always zero-copy."
- `RING_DEPTH` is a compile-time constant in this pass, not runtime
  configurable — acceptable for v1 (matches ADR-0006's own "ship the
  simpler version first" precedent) but a real limitation if a caller needs
  more slack than 3 slots' worth of straggling.

## References

- [ADR-0006](0006-shared-desktop-duplication.md) — shared driver thread,
  registry keying, and the always-copy fan-out this ADR replaces; its
  "Deferred optimization" section is what this ADR fulfills, generalized
  beyond the solo-consumer case
- [ADR-0001](0001-dxgi-desktop-duplication.md) — original exclusive-path
  Zero-Copy decision; the direct-handoff variant it specified no longer
  exists in `dxgi.rs` (see Context's correction)
- [`mediaway-encoder/src/windows/wmf/dx11.rs`](../../../mediaway-encoder/src/windows/wmf/dx11.rs),
  [`video.rs`](../../../mediaway-encoder/src/windows/wmf/video.rs) — the
  existing Zero-Copy consumption pattern this ADR's recycle signal (Q2)
  must stay consistent with
- [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md)
  — honest-cost documentation this ADR's transient-fallback and
  issue-then-drop obligations must carry into rustdoc
- [`docs/spec/zero-cost-abstractions.md`](../../../../docs/spec/zero-cost-abstractions.md)
  — fixed-size array over `Vec`, `Arc` used for a genuine shared-GPU-resource
  case (not habitual clone)
- [`docs/spec/gpu-interop.md`](../../../../docs/spec/gpu-interop.md) —
  `GpuBufferHandle` vocabulary this ADR does not add a new variant to

ADRs are **English**. Numbering is local to this `adr/` folder.
