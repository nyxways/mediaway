# Pipeline

| Doc | Summary |
|-----|---------|
| [overview](overview.md) | device/demux → decode → frames → encode → mux |
| [api-layers](api-layers.md) | Low-level APIs first-class; convenience is composition |
| [async-streaming](async-streaming.md) | Streaming-first · async without mandatory runtime |
| [frame-filter-hook](frame-filter-hook.md) | Mid-pipeline `FrameFilter` chain on `EncodeSession` (implemented, ADR-0001) |
| [audio-track-and-apm](audio-track-and-apm.md) | Optional audio track + `mediaway-audio-apm` (AEC/NS/AGC2/VAD) on `EncodeSession` (implemented, ADR-0003) |
| [c-ffi](c-ffi.md) | Per-capability `*-ffi` + optional feature umbrella |
| [ffi-c-abi](ffi-c-abi.md) | `mediaway-ffi` — auto-encode → fMP4 C ABI, GPU frame input reachable from C (ADR-0001/0002) |
| [screen-record-av](screen-record-av.md) | Screen + mic → H.264 + AAC → two-track fMP4 (test-level; not yet migrated onto `EncodeSession::open_with_audio`) |
| [trim-and-splice](trim-and-splice.md) | Decode → trim → splice → re-encode; found the AVCC/Annex-B mux gap |
