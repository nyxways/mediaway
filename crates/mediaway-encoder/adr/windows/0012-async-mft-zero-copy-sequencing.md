# ADR-0012: Async MFT sequencing for the DX11 Zero-Copy encode path

- **Status**: Accepted
- **Date**: 2026-09-18
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`windows::wmf`)

## Context

The DX11 Zero-Copy encode path (`VideoInputPreference::ZeroCopyGpu`, ADR-0003) never worked
on NVIDIA hardware. `WindowsVideoEncoder::open` returned `EncodeError::Backend`, and
`tests/screen_mic_av_smoke.rs` skipped with `skip: video encoder unavailable (Backend)`.

This was attributed to the environment. `docs/ai/wiki/pipeline/screen-record-av.md`
§ Known environment gap recorded it as "on some execution sessions (e.g. certain
automated/background job contexts), Media Foundation hardware transforms fail to activate,"
and several tests were written to skip on it.

**That attribution was wrong.** Measured on an RTX 4090 + Intel UHD 770, in both an
interactive desktop session and a background one:

- `MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE)` returns 3 entries for
  H.264 and 3 for HEVC; `NVIDIA H.264 Encoder MFT` and `NVIDIA HEVC Encoder MFT` both
  `ActivateObject` successfully.
- `MF_SA_D3D11_AWARE` is `1`.
- `MFCreateDXGIDeviceManager` and `ResetDevice` both succeed.

The hardware, the driver and the session were all fine. Three separate defects in this
crate's async-MFT sequencing were not. Each one masked the next, so each was only
observable after the previous was fixed:

1. **`open` failed.** `MFT_MESSAGE_SET_D3D_MANAGER` was sent *before* the async unlock.
   A locked async MFT rejects that message with `MF_E_TRANSFORM_ASYNC_LOCKED`
   (`0xC00D6D77`, "The caller does not appear to support this transform's asynchronous
   capabilities").
2. **The first `push_frame` failed.** `drain_output` called `ProcessOutput`
   unconditionally. `METransformHaveOutput` was received by `drain_events` and
   *discarded* — the branch was an empty `// ProcessOutput drained by caller.` comment,
   and `Dx11Session` had no field to count it in. An async MFT rejects `ProcessOutput`
   it has not announced.
3. **The second `push_frame` deadlocked.** `wait_need_input` polled for
   `METransformNeedInput` for 500 ms and then returned `EncodeError::Backend`. An async
   MFT stops requesting input until the output it has already produced is collected, so
   the encoder was waiting for the caller while the caller waited for the encoder.

Why this looked environment-dependent: **every NVIDIA encoder MFT is async, and Intel
QuickSync's are sync.** A sync MFT has no unlock, no `HaveOutput` event, and reports
"nothing ready" through `MF_E_TRANSFORM_NEED_MORE_INPUT` instead — so it sails through all
three defects. `MFT_ENUM_FLAG_SORTANDFILTER` puts the NVIDIA MFT first on a machine that
has one. The path worked exactly on the machines that did not select an async MFT.

## Decision

> We sequence the DX11 session the way the async MFT contract requires, and make the async
> bookkeeping explicit instead of implicit.

1. **Unlock first.** `unlock_async_if_needed` moves ahead of `SET_D3D_MANAGER` in
   `open_hw_encoder`. Unlocking must precede every other call on an async MFT.
2. **Count `HaveOutput`.** `Dx11Session` gains `have_output: u32`, incremented in
   `drain_events`. `take_have_output` claims one before `drain_output` calls
   `ProcessOutput`, and returns `true` unconditionally for sync sessions so their polling
   behaviour is byte-for-byte unchanged.
3. **Service output while waiting for input.** `wait_need_input` is replaced by
   `WmfVideoEncoder::await_need_input`, which lives in `video.rs` because it needs the
   transform and the pending-packet queue, not just the session. Its loop drains output
   each iteration. `dx11::has_need_input` is the read-only predicate it polls.

The 500 ms deadline is kept as a liveness backstop, not as the mechanism. It should now
never be reached; if it is, that is a real stall worth surfacing as `Backend`.

### Scope

`VideoInputPreference::ZeroCopyGpu` only. The CPU-upload path never used an async MFT
(`open_cpu` takes the inbox sync H.264 MFT or enumerates without the hardware filter) and is
untouched — `has_need_input`/`take_have_output` both short-circuit to "proceed" when
`events` is `None`.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Event-callback driving via `IMFAsyncCallback` (`BeginGetEvent`/`Invoke`) | The canonical async-MFT shape, but it inverts control flow into a callback thread and forces a thread-safe packet queue on a crate whose `VideoEncoder` trait is explicitly poll-shaped and `&mut self`. The polling loop satisfies the same contract while keeping the trait sync, per `docs/spec/async-and-streaming.md`. |
| Keep `wait_need_input` in `dx11.rs` and pass in a drain callback | A closure over the transform *and* the pending queue fights the borrow checker for no gain; the loop belongs where its state lives. |
| Treat `ProcessOutput` failure as "nothing ready" and continue | Hides real backend failures behind the same path as a normal empty poll. Defect 2 was precisely a case where an error meant "you asked wrong," not "nothing yet." |
| Prefer sync MFTs (filter NVIDIA out) | Picks the slower encoder on the machines with the best one, to avoid implementing a contract the code already half-implemented. |

## Consequences

### Positive

- DX11 Zero-Copy encode works on NVIDIA hardware for the first time. Verified on an RTX
  4090: 5-frame and 30-frame sessions, H.264 and HEVC, at 1920×1080.
- **BGRA input is confirmed working end to end**, not just accepted by `SetInputType` — so
  a WGC/DDA `Bgra8` surface reaches the encoder with no colour conversion, which is what the
  "live-recorder pattern" in `configure_types` was always aiming at.
- The wiki's "known environment gap" is retired, and the tests that skip on it can be
  believed again.
- The async state is now named and counted, so the next reader does not have to infer it
  from an empty `else if` branch.

### Negative / Trade-offs

- `await_need_input` polls with a 1 ms sleep. Cheap relative to a frame at any realistic
  rate, but it is a sleep in the submit path; an `IMFAsyncCallback` design would not have
  one. Revisit if a profile ever shows it.
- Output is now drained inside the input wait, so `push_frame` can enqueue packets before
  its own frame is submitted. Callers already had to treat `poll_packet` as independent of
  `push_frame`, so no contract changes — but the timing is less obvious than it was.
- Verified on one vendor's async MFT. AMD's is also async and is untested here.

## References

- [ADR-0003](0003-dx11-zero-copy.md) — the Zero-Copy DX11 path this repairs
- `docs/ai/wiki/encode/async-mft.md` — the contract, the failure signatures, and the
  sync/async split
- `src/windows/wmf/video_tests.rs::dx11_zero_copy_encodes_multiple_frames_or_skip` — the
  regression test; it pushes several frames precisely because each defect surfaced at a
  different frame index
- Microsoft, "Asynchronous MFTs" — `MF_TRANSFORM_ASYNC_UNLOCK`, `METransformNeedInput`,
  `METransformHaveOutput` ordering contract
