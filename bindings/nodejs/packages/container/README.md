# @mediaway/container

Sans-IO **fragmented MP4 (fMP4) mux and demux** over Mediaway's C ABI. The
muxer/demuxer never touch files or sockets — you push packets in and poll
bytes out, and own every byte of I/O.

Backed by the `iso-bmff` core crate (sans-io, unit-tested) and the native
`mediaway_ffi.dll` (shipped by `@mediaway/ffi`).

## Install

```bash
npm install @mediaway/container
```

Windows x64 (native DLL). The `@mediaway/ffi` dependency is installed
automatically.

## Mux example

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

## Demux example

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

## API

| Class | Methods | Notes |
| --- | --- | --- |
| `Muxer` | `addVideoTrack`, `addAudioTrack`, `begin`, `push`, `flush`, `pollBytes`, `close` | sans-io: never blocks, never touches I/O |
| `Demuxer` | `pushBytes`, `streams`, `pollPacket`, `close` | streaming — feed repeatedly |

Errors surface as `MediawayError` (with an ABI-level code).

## License

MIT OR Apache-2.0. Part of the [Mediaway](https://github.com/nyxways/mediaway)
project — pre-1.0, APIs may change.
