# ADR-0013: WMF timestamps — an exact `hns` round trip, and a real decode timestamp

- **Status**: Accepted — hardware-verified
- **Date**: 2026-09-18
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`windows::wmf`), mirrored in `mediaway-decoder` (`windows::wmf`)

## Context

Every video file this backend has produced carried a malformed timestamp track. The file
played, which is why it went unnoticed; ffmpeg named it on sight:

```text
Application provided invalid, non monotonically increasing dts to muxer
```

A real recording measured 2026-09-18 — a live Unity editor window at `time_base = 1/60` —
contained **279 video packets and 215 distinct presentation timestamps**. The caller had
pushed strictly increasing ticks. Two independent defects, both in `sample_to_packet`'s
timestamp handling, were behind it.

### 1. `to_hns` / `from_hns` were not inverses

A timestamp makes the trip `tick → hns → tick` on its way through the MFT: written onto the
input `IMFSample`, read back off the output one. **Both directions truncated.** Since
10 000 000 is not divisible by most timebase denominators, distinct ticks collapsed onto one:

| tick | `to_hns` | truncating back |
|---|---|---|
| 6 | 1 000 000 | 6 |
| 7 | 1 166 666 | **6** |
| 8 | 1 333 333 | **7** |

At `1/60` only every third tick survived, which is exactly where the duplicates sat in the
recording. `1/30`, `1/24` and `1001/30000` are all affected, audio included (`aac.rs` shares
`to_hns`), and `mediaway-decoder` had a byte-identical copy of the same pair.

The two halves lived in different files — `runtime.rs` and `shared.rs` — which is how a pair
that is only correct together drifted apart without anyone reading them side by side.

### 2. `dts` was reported as a copy of `pts`

`Packet { pts, dts: pts, … }`. That is true only for an encoder that does not reorder. The
inbox H.264 MFT **does**: measured on this host, 30 frames pushed at ticks 0..29 came back
in decode order with presentation timestamps `1, 3, 2, 5, 4, … 29, 28, 30`. Reporting those
as decode timestamps produced a sequence that marched backwards every other packet — the
literal complaint in ffmpeg's message.

The container was never the problem. `iso_bmff::Muxer::push_packet` already derives the
composition offset from `pts - dts` and writes it; with `dts == pts` that offset was
permanently zero, so B-frame files were mistimed as well as non-monotonic.

## Decision

> `to_hns` keeps truncating; `from_hns` **rounds to the nearest tick**. The pair lives in one
> file. `dts` is read from `MFSampleExtension_DecodeTimestamp` when the MFT provides it.

### Scope

- `mediaway-encoder::windows::wmf::runtime` owns both directions; `shared.rs` imports
  `from_hns` rather than keeping a second copy.
- `mediaway-decoder::windows::wmf::runtime` gets the identical rounding change.
- Absent `MFSampleExtension_DecodeTimestamp`, `dts` falls back to `pts` — that attribute is
  present only when the MFT reorders, and when it does not, `pts` *is* the decode time.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Round `from_hns` **away from zero** | Also an exact inverse of a truncating `to_hns`, and the tidier argument. **Measured wrong on the real path**: the MFT does not echo the hns value written to it, it recomputes sample times with rounding of its own, landing a fraction of a tick *above* the exact value as often as below. Away-from-zero pushed every such value into the next tick — a whole 30-frame sequence displaced by one frame, carrying a timestamp never submitted. Caught only because the fix was tested against a real encoder rather than the unit round trip alone. |
| Round `to_hns` instead, leave `from_hns` truncating | Fixes the round trip equally well and breaks the outward direction: the hns handed to MF would no longer be the nearest representation of the tick. The lossy conversion is the one crossing into a foreign unit, so the correction belongs on the way back. |
| Make the tick→hns conversion exact by demanding a timebase that divides 10 000 000 | Rejects `1/60`, `1/30`, `1/24` and NTSC — i.e. nearly every real caller — to avoid arithmetic the backend can simply get right. |
| Leave `dts = pts` and disable B-frames | Would need a knob the MFT does not reliably honour (`gop_size: 1` did not stop it reordering here), and would trade a correctness fix for a compression loss on every caller. |
| Drop `pts`/`dts` apart in the muxer instead | The muxer cannot recover a decode order the backend already discarded. |

## Consequences

### Positive

- Timestamps survive the encoder: `from_hns(to_hns(t)) == t` for every realistic timebase,
  both signs, pinned by `runtime_tests.rs` in both crates.
- Reordered streams are now described correctly. Measured after the change: pts
  `1, 3, 2, 5, 4, …` over dts `0, 1, 2, … 29` — strictly increasing decode times, every
  `dts <= pts`, and the muxer writes real composition offsets instead of zeros.
- The `+1` displacement on presentation timestamps is the encoder's reorder delay, which is
  what lets the first decode timestamp be zero. Correct, and now understood rather than
  mistaken for a bug.

### Negative / Trade-offs

- An hns value that did not come from `to_hns` is snapped to the nearest tick rather than
  carried verbatim. That is the intended behaviour for MFT-generated times and there is no
  representation for sub-tick precision in the packet anyway.
- Timebases finer than 5 MHz (`time_base_den > num * 5 * 10^6`) still lose precision. The
  loss happens inside `to_hns`, before any rounding rule on the way back could act on it;
  `runtime_tests.rs` records it rather than implying it was fixed.
- The two crates carry duplicate copies of the pair. Folding them into a shared crate is a
  packaging decision (ADR-0003) larger than this fix; the tests are duplicated with them so
  the copies cannot drift apart silently again.

## References

- `crates/mediaway-encoder/src/windows/wmf/runtime.rs` — both directions, with the rationale
- `crates/mediaway-encoder/tests/wmf_timestamp_round_trip.rs` — the real-encoder regression test
- `crates/iso-bmff/src/mux/mod.rs` — composition offset from `pts - dts`
- [`docs/ai/wiki/platform/windows-timestamps.md`](../../../../docs/ai/wiki/platform/windows-timestamps.md)
- ADR-0012 — the previous case of this path being wrong and the cause being attributed elsewhere
