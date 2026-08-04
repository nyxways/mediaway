# ADR-0004: Mux sample durations from dts deltas

- **Status**: Accepted
- **Date**: 2026-08-04
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `iso-bmff`

## Context

`Muxer<Live>` wrote `Sample::duration` verbatim into `trun.sample_duration`
for every sample. Callers that time streams from `pts`/`dts` only — the
normal case for compressed media — left `duration` at 0, producing files
where every sample in a fragment shared one timestamp (all-zero trun
durations). Players receive no frame pacing, so playback stutters; zero
durations are also non-conformant MP4. The playback verification harness
hit this exactly.

## Decision

Sample durations are derived at fragment flush from consecutive `dts`
deltas: `durations[i] = dts[i + 1] - dts[i]`. `Sample::duration` becomes
optional and is trusted only for the **last** sample of a fragment; when it
is zero, the last sample's duration is estimated from the previous sample's
delta, and a lone-sample fragment defaults to one media tick. `dts` must be
monotonically non-decreasing per track.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Require callers to always supply `duration` | Hostile to pts/dts-only callers; every caller must know per-sample durations; the harness proved this bites |
| Reject `duration == 0` as invalid input | Legitimate callers time streams purely from pts/dts; the muxer can do better for free |
| Trust `duration` verbatim (status quo) | Silently emits invalid files (all-zero durations) |

## Consequences

### Positive

- Callers may omit `duration` entirely; constant-rate streams get correct
  durations automatically; last-sample estimation covers fragment ends.

### Negative / Trade-offs

- The last sample of a fragment is an estimate when the caller supplies no
  duration — callers with VFR endings should set it explicitly.
- Out-of-order `dts` degrades to a 1-tick duration (never a zero or `u32::MAX`
  sample); callers should keep `dts` non-decreasing (documented on
  `push_packet`).
