# Container sans-IO

Canonical: [`docs/spec/sans-io.md`](../../../docs/spec/sans-io.md). Naming v1: [ADR-0012](../../../adr/0012-unprefixed-reusable-cores.md).

- **Core:** `iso-bmff` — freestanding ISOBMFF mux + demux (own types).
- **Facade:** `mediaway-container` — traits + Mediaway-typed `mp4` over `iso-bmff`.
- **WebM (mux + demux, 2026-07-29):** `ebml-webm` — freestanding EBML VINT +
  WebM Segment/Tracks/Cluster/SimpleBlock walk (own types) + writer, +
  `mediaway-container::webm`. See [webm.md](webm.md) for the exact subset and
  deferred items.
- Apps/CLIs depend on the facade (or `iso-bmff` when Mediaway types are not needed).
- Pure sans-io — callers own I/O. No in-crate file adapters. No `Box` on hot paths.
- ZCA: `FourCc` / paired box files, typestate mux — [zca](../meta/zca.md).
- `smallvec`: tracks ≤4 inline, fragment sample rows ≤32; byte sinks stay `Vec`.
- Errors: `thiserror` in `iso-bmff` ([errors](../meta/errors.md)).
- Demux: fMP4 `moof`/`mdat`, unfragmented `stbl`, and `edts`/`elst` sample expansion.
- Sample-entry codec coverage (`avc1`/`vp09`/`mp4a`, HEVC/AV1 still mislabeled): [mp4-sample-entries](mp4-sample-entries.md).
- Mux timing: sample durations derive from consecutive `dts` deltas (no silent zero-duration trun — players stutter on it); `Sample::duration` optional, trusted only for the last sample of a fragment — `iso-bmff/adr/0004` (2026-08-04).
- Edit-list remap: `dts' = dts - media_time + base` (signed); out-of-window samples set `is_discard`.
- ClearKey: `tenc`/`senc` + `DemuxDecrypt` → [`iso-cenc`](../meta/crypto.md).
- Conformance + FATE `oracle_compare` (`nb_read_packets`) — [testing](../meta/testing.md). Every container crate now carries its own `fate_manifest.txt`/`demux_exceptions.rs` (2026-07-29), not just `iso-bmff` — see [webm.md](webm.md), [audio-containers.md](audio-containers.md), [general-containers.md](general-containers.md).
- **Cargo features** (ADR `iso-bmff/adr/0001`): `audio`, `video`, `mux`, `demux`, `full` (default). Slim audio mux: `default-features = false`, `features = ["audio", "mux"]`. `mediaway-container` forwards the same flags.
