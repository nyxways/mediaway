# Frame filter hook

`mediaway` ADR-0001 (crate-local) adds a mid-pipeline hook on
`EncodeSession::write_frame`, between the caller's frame and
`encoder.push_frame`. **Implemented** — `FrameFilter`/`FilterError` live in
`src/filter.rs`, the chain + `push_filter` in `src/session.rs`; see
`src/session_tests.rs` for the empty-chain / stateful-filter / rejection /
`Gpu`-frame-rejected coverage.

```text
caller → write_frame(&VideoFrame)
              │
   filters empty? ──yes──► encoder.push_frame(frame)   (zero clone, unchanged today)
              │no
              ▼
   Gpu-backed frame? ──yes──► Err(FilterError::GpuFrameUnsupported)  (no silent readback)
              │no (Cpu-backed)
              ▼
   clone once → run ordered FrameFilter chain → encoder.push_frame(&result)
```

- **`FrameFilter` trait**, not a bare closure — matches `VideoEncoder`/`VideoDecoder`'s
  existing "caller supplies an implementation" idiom; a blanket `FnMut` impl can be
  added later without breaking this shape.
- **Ordered chain** (`SmallVec<[Box<dyn FrameFilter>; 4]>`), not a single filter slot —
  a single slot would just push composition onto every multi-step caller.
- **v1 scope: CPU-backed frames only** (`VideoFrameStorage::Cpu`). A filter chain
  attached to a session that receives a `Gpu`-backed frame fails loudly
  (`FilterError::GpuFrameUnsupported`) — never a silent GPU→CPU readback.
- Attached via `EncodeSession::push_filter(filter)` — additive, `open`/`write_frame`
  signatures unchanged.
- Errors propagate through `write_frame`'s existing `Result` (`PipelineError::Filter`),
  same as encoder/mux failures — no swallowed/logged-only filter errors.

**Explicitly deferred** (same design conversation, not in this ADR): a GPU shader-pass /
render-graph framework at this same seam. Would need its own future ADR if revisited —
do not fold it into `FrameFilter` retroactively.

See [ADR-0001](../../../../crates/mediaway/adr/0001-frame-filter-hook.md) for
the full trait/error shapes and reasoning.
