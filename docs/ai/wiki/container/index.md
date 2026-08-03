# Container (Mux / Demux)

| Doc | Summary |
|-----|---------|
| [sans-io](sans-io.md) | `iso-bmff` core + `mediaway-container` facade |
| [scaffold](scaffold.md) | Crate map for MP4 + WebM |
| [mp4-sample-entries](mp4-sample-entries.md) | Codec coverage per sample entry — H.264/VP9/HEVC/AV1 all real |
| [audio-containers](audio-containers.md) | `riff-wave-core`/`adts-core`/`mpeg-audio`/`ogg` — audio-only formats, all facade-wired |
| [general-containers](general-containers.md) | `flv-core`/`mpeg-ts-core` — general containers, all facade-wired |
| [rtmp](rtmp.md) | `rtmp` — publish-client protocol core (implemented, handshake unverified against a real server), reuses FLV tag-body byte shapes |
| [webm](webm.md) | `ebml-webm` core + `mediaway-container::webm` — mux + demux, deferred items |
| [cli-tools](cli-tools.md) | `mediaway-avprobe` / `mediaway-avcli` flag subsets over the facade |
| [ffi-c-abi](ffi-c-abi.md) | `mediaway-ffi` — C ABI over MP4 mux/demux; opaque handles, panic-catch |
