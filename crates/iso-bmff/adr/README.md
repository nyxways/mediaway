# iso-bmff — ADRs

| ADR | Title |
|-----|-------|
| [0001](0001-paired-sans-io-zca.md) | Paired sans-io mux/demux + ZCA |
| [0002](0002-vp9-sample-entry.md) | VP9 (`vp09`/`vpcC`) sample-entry mux + demux |
| [0003](0003-hevc-av1-sample-entry.md) | HEVC (`hvc1`/`hvcC`) + AV1 (`av01`/`av1C`) sample-entry mux + demux; honest `ftyp` brands |
| [0004](0004-mux-duration-dts-delta.md) | Mux sample durations from `dts` deltas |
| [0005](0005-opus-sample-entry.md) | Opus (`Opus`/`dOps`) sample-entry mux + demux; replaces an `mp4a`/`esds` AAC mislabel |
| [0006](0006-hevc-in-mp4.md) | HEVC in MP4: unescape the SPS, length-prefix the samples; every HEVC file this workspace wrote decoded zero frames |
| [0007](0007-mux-payload-placements.md) | Opt-in mux payload placements: absolute output offset + length of every sample written |
| [0008](0008-progressive-mp4-head.md) | A non-fragmented MP4 head with edit lists (`write_progressive`): FFmpeg applies an `elst` to sample tables only, not to fragmented MP4 |
| [0009](0009-indexed-moov-for-existing-samples.md) | A `moov` with sample tables for samples already in a file (`write_indexed_moov`): the second half of a hybrid MP4; FFmpeg needs the fragments inside an `mdat` |
