# ADR-0013: WMF timestamps — an exact `hns` round trip

- **Status**: Accepted — hardware-verified (corrected 2026-09-18, see § Correction)
- **Date**: 2026-09-18
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`windows::wmf`), mirrored in `mediaway-decoder` (`windows::wmf`)

## Context

Every HEVC recording made through this backend carried colliding presentation timestamps.
The files played, which is why it went unnoticed. A real recording taken 2026-09-18 — a live
Unity editor window at `time_base = 1/60` — held **279 video packets and only 215 distinct
presentation timestamps**, from a caller that had pushed strictly increasing ticks.

Decoding that file, ffmpeg warned:

```text
Application provided invalid, non monotonically increasing dts to muxer in stream 0: 6 >= 6
```

That warning is about ffmpeg's *decoded output*, which inherits the duplicate presentation
times. **The file's own decode timestamps were monotonic** — measured, zero non-increasing
steps in either track. The warning appears only on files with an audio track as well; a
video-only file with the same duplicates decodes without it.

### `to_hns` / `from_hns` were not inverses

A timestamp makes the trip `tick → hns → tick` on its way through the MFT: written onto the
input `IMFSample`, read back off the output one. **Both directions truncated.** Since
10 000 000 is not divisible by most timebase denominators, distinct ticks collapsed onto one:

| tick | `to_hns` | truncating back |
|---|---|---|
| 6 | 1 000 000 | 6 |
| 7 | 1 166 666 | **6** |
| 8 | 1 333 333 | **7** |

At `1/60` only every third tick survived. `1/30`, `1/24` and `1001/30000` are affected the
same way, audio included (`aac.rs` shares `to_hns`), and `mediaway-decoder` had a
byte-identical copy of the pair. The two halves lived in different files — `runtime.rs` and
`shared.rs` — which is how a pair that is only correct together drifted apart unread.

### The two codecs broke differently

Measured on an RTX 4090 host by pushing 30 frames at ticks 0..29 and reading the raw hns the
MFT returned:

| | raw sample times | truncating back | effect |
|---|---|---|---|
| **HEVC** | exactly what `to_hns` wrote | `0, 0, 1, 3, 3, 4, …` — **20 distinct** | collapse |
| **H.264** (inbox) | recomputed; a hair below each whole tick, reordered (B-frames), one tick of reorder delay | `0, 2, 1, 4, 3, …` — distinct | the delay is shaved off, and B-frames get **`dts 2 > pts 1`**: presented before decoded |

qarec records HEVC by default, which is why its recordings showed the collapse.

## Decision

> `to_hns` keeps truncating; `from_hns` **rounds to the nearest tick**. The pair lives in one
> file per crate, and both crates are fixed.

### Scope

- `mediaway-encoder::windows::wmf::runtime` owns both directions; `shared.rs` imports
  `from_hns` rather than keeping a second copy.
- `mediaway-decoder::windows::wmf::runtime` gets the identical rounding change.
- **Decode timestamps are unchanged.** `WmfVideoEncoder::drain_output` already assigned `dts`
  from a decode-order counter, and that was correct for the constant-rate input tested here.
  See § Correction for what this ADR first claimed, and § Open question for where the counter
  stops being correct.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Round `from_hns` **away from zero** | Also an exact inverse of a truncating `to_hns`, and **indistinguishable on every value measured** — both MFTs returned sample times at or below whole ticks, never above. Nearest is preferred only for the unobserved case of an MFT rounding *up*, where away-from-zero would jump a tick. A design argument, not a measurement. |
| Round `to_hns` instead, leave `from_hns` truncating | Fixes the round trip equally well and breaks the outward direction: the hns handed to MF would no longer be the nearest representation of the tick. The lossy conversion is the one crossing into a foreign unit, so the correction belongs on the way back. |
| Demand a timebase that divides 10 000 000 | Rejects `1/60`, `1/30`, `1/24` and NTSC — nearly every real caller — to avoid arithmetic the backend can simply get right. |

## Consequences

### Positive

- `from_hns(to_hns(t)) == t` for every realistic timebase, both signs, pinned by
  `runtime_tests.rs` in both crates.
- Measured end to end, before (`318813a`) vs after, same harness — 300 frames at 1/60 through
  WMF, muxed to fMP4 with an AAC track, judged by ffmpeg:

  | | distinct pts | ffmpeg decode warning |
  |---|---|---|
  | HEVC before | 200 / 300 | *non monotonically increasing dts* |
  | HEVC after | 300 / 300 | none |
  | H.264 before / after | 300 / 300 both | none either way |

- `tests/wmf_timestamp_round_trip.rs` covers both failure shapes, and **both cases were shown
  to fail when the truncation is reinstated.**

### Negative / Trade-offs

- An hns value that did not come from `to_hns` is snapped to the nearest tick rather than
  carried verbatim — intended for MFT-generated times, and a packet has no sub-tick precision
  to carry it in anyway.
- Timebases finer than 5 MHz (`time_base_den > num * 5 * 10^6`) still lose precision, inside
  `to_hns`, before any rounding rule on the way back could act. `runtime_tests.rs` records it.
- The two crates carry duplicate copies of the pair. Folding them into a shared crate is a
  packaging decision (ADR-0003) larger than this fix; the tests are duplicated with them so the
  copies cannot drift apart silently again.

## Open question — the decode-order counter assumes a constant frame rate

`drain_output`'s counter advances **one tick per emitted frame**. Measured with irregular input
(ticks `0, 4, 6, 10, 12, …`), the inbox H.264 MFT's own `MFSampleExtension_DecodeTimestamp`
tracked the real times (`0, 4, 6, 10, 12, …` ticks) while the counter produced
`0, 1, 2, 3, 4, …`. Under variable-rate input the two diverge by every skipped tick, so the
composition offset grows over the recording. Event-driven screen capture is variable-rate by
nature.

Tracked as #101. Not decided here, because switching `dts` to the attribute alone would
leave packet `duration` (from the MFT's nominal frame duration) inconsistent with the new
`dts` deltas.
Needs its own measurement of what a long variable-rate recording does in real players.

## Correction (2026-09-18)

The first version of this ADR, as merged in #100, made three claims that were wrong. Measured
the same day, while verifying the merge end to end:

1. **"`dts` was reported as a copy of `pts`, and that caused ffmpeg's warning."** False.
   `sample_to_packet` wrote `dts: pts`, but `drain_output` overwrote it with the decode-order
   counter for every video packet — the original recording's container `dts` was monotonic.
   #100 also added a read of `MFSampleExtension_DecodeTimestamp` in `sample_to_packet`. That
   same override discarded it, so it was dead code and has been removed. The ffmpeg warning
   comes from the duplicate *presentation* times.
2. **"Away-from-zero was measured wrong: it displaced a 30-frame sequence by one frame."**
   False. The H.264 output was `1, 3, 2, …, 30` under nearest rounding as well. The one-tick
   displacement is the MFT's reorder delay, not the rounding rule. The rules have never been
   observed to disagree (§ Alternatives).
3. **The integration test was described as the regression test for the collapse.** It
   tested H.264, which never collapsed. It would still have failed against the old code, but
   on `dts > pts` — a defect the text did not know about — not on the collapse it claimed to
   catch. It now covers HEVC as well, and both cases were verified against the reinstated
   truncation.

Each came from reasoning about code without reading the caller (`drain_output`), or from
attributing an effect to the change just made. The corrective change is the same PR that
adds this section.

## References

- `crates/mediaway-encoder/src/windows/wmf/runtime.rs` — both directions, with the rationale
- `crates/mediaway-encoder/src/windows/wmf/video.rs` — `drain_output`'s decode-order counter
- `crates/mediaway-encoder/tests/wmf_timestamp_round_trip.rs` — real-encoder regression test
- `crates/iso-bmff/src/mux/mod.rs` — composition offset from `pts - dts`
- [`docs/ai/wiki/platform/windows-timestamps.md`](../../../../docs/ai/wiki/platform/windows-timestamps.md)
- ADR-0012 — the previous case of this path being wrong and the cause being attributed elsewhere
