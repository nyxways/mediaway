# @mediaway/container

Sans-IO mux and demux over Mediaway's C ABI, covering all 8
`mediaway-container` formats. Nothing here touches files or sockets — you
push packets in and poll (or pull) bytes out, and own every byte of I/O.

| Format | API | Notes |
| --- | --- | --- |
| MP4 | `Muxer`, `Demuxer` (default) | fragmented MP4 (fMP4); typestated via `begin()` |
| WebM | `new Muxer("webm")`, `new Demuxer("webm")` | same `Muxer`/`Demuxer` shape as MP4 |
| Ogg | `OggMuxer`, `OggDemuxer` | no track registration — `push()` is ready immediately |
| ADTS (AAC) | `AdtsMuxer`, `AdtsDemuxer` | raw ADTS frame stream |
| FLV | `FlvMuxer`, `FlvDemuxer` | tag-based, `writeHeader()` before any tag |
| MPEG-TS | `TsMuxer`, `TsDemuxer` | PAT/PMT + access units |
| MP3 | `Mp3Muxer`, `Mp3Demuxer` | fixed Layer III frame header |
| WAV | `WavMuxer`, `parseWav()` | mux via `finish()`; demux is a one-shot function, no class |

MP4/WebM share `Muxer`/`Demuxer`; the other 6 formats have dedicated classes because
their C ABI shape genuinely differs (no track registration, out-buffer-per-call mux, or
a construction-time stream list — see each module's own top comment:
`ogg.ts`/`adts.ts`/`flv.ts`/`ts.ts`/`mp3.ts`/`wav.ts`).

Backed by the `iso-bmff`/`mediaway-container` core crates (sans-io, unit-tested) and the
native `mediaway_ffi.dll` (shipped by `@mediaway/ffi`).

## Install

```bash
npm install @mediaway/container
```

Windows x64 (native DLL). The `@mediaway/ffi` dependency is installed
automatically.

## MP4 mux example

```ts
import { Muxer, Demuxer, type VideoTrackInfo, type AudioTrackInfo, type Packet } from "@mediaway/container";

const muxer = new Muxer();

const videoTrack: VideoTrackInfo = {
  codec: "h264",
  width: 640,
  height: 480,
  pixelFormat: "nv12",
  timeBase: { num: 1, den: 30 },
};
const audioTrack: AudioTrackInfo = {
  codec: "aac",
  sampleRate: 48_000,
  channels: 2,
  timeBase: { num: 1, den: 48_000 },
};

const v = muxer.addVideoTrack(videoTrack);
const a = muxer.addAudioTrack(audioTrack);
const chunks: Buffer[] = [muxer.begin()]; // init segment (ftyp + moov)

for (let i = 0; i < 90; i++) {
  muxer.push({
    trackIndex: v,
    data: fakeH264Packet(i),       // your encoded bytes
    pts: i,                        // ticks of the track timeBase
    duration: 1,
    key: i % 30 === 0,
  } satisfies Packet);
}
muxer.flush();                     // finalize the last fragment
chunks.push(muxer.pollBytes());    // drain everything queued
muxer.close();
// chunks -> concatenate and write to a file / stream
```

## MP4 demux example

```ts
const demuxer = new Demuxer();
demuxer.pushBytes(buffer);          // feed chunks as they arrive
const streams = demuxer.streams();  // TrackInfo[] once moov parsed
for (;;) {
  const packet = demuxer.pollPacket(); // Packet | null
  if (!packet) break;
  // packet.trackIndex, packet.data, packet.pts, packet.duration, packet.key
}
demuxer.close();
```

WebM works the same way — pass `"webm"` to the `Muxer`/`Demuxer` constructor:
`new Muxer("webm")` / `new Demuxer("webm")`.

## Other formats

Ogg/ADTS/FLV/MPEG-TS/MP3/WAV are exported from the same package but have their own
classes and functions (see the format table above), not `Muxer`/`Demuxer`:

```ts
import { OggMuxer, OggDemuxer } from "@mediaway/container";
import { AdtsMuxer, AdtsDemuxer } from "@mediaway/container";
import { FlvMuxer, FlvDemuxer } from "@mediaway/container";
import { TsMuxer, TsDemuxer } from "@mediaway/container";
import { Mp3Muxer, Mp3Demuxer } from "@mediaway/container";
import { WavMuxer, parseWav } from "@mediaway/container";
```

Each has its own DX contract because the underlying C ABI shape differs (e.g. Ogg/ADTS
have no track registration; FLV needs `writeHeader()`; WAV mux uses a terminal
`finish()` and WAV demux is the one-shot `parseWav(buffer)` function, not a class) — see
`src/ogg.ts`, `src/adts.ts`, `src/flv.ts`, `src/ts.ts`, `src/mp3.ts`, `src/wav.ts` for
each format's full API.

## API (MP4 / WebM)

| Class | Methods | Notes |
| --- | --- | --- |
| `Muxer` | `addVideoTrack`, `addAudioTrack`, `begin`, `push`, `flush`, `pollBytes`, `close` | sans-io: never blocks, never touches I/O |
| `Demuxer` | `pushBytes`, `streams`, `pollPacket`, `close` | streaming — feed repeatedly |

Errors surface as `MediawayError` (with an ABI-level code) across every format.

## License

MIT OR Apache-2.0. Part of the [Mediaway](https://github.com/nyxways/mediaway)
project — pre-1.0, APIs may change.
