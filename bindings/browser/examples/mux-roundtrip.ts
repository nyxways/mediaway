// ASPIRATIONAL EXAMPLE — no such binding package exists yet.
//
// This file shows the target ergonomics for a future browser/WASM binding of
// Mediaway's container mux/demux capability. The browser host goes through
// `wasm-bindgen` (never the C ABI) — see `docs/spec/c-ffi.md` Tier C and
// `../README.md`. Written as if `@mediaway/browser` already existed on npm.
//
// Mirrors `examples/mux_roundtrip.rs`: build a fragmented MP4 with one H.264
// video track and one AAC audio track, then demux the same bytes back.

import init, { Muxer, Demuxer } from "@mediaway/browser";
import type { Packet } from "@mediaway/browser";

async function main(): Promise<void> {
  // The WASM module must be instantiated (fetched + compiled) once before any
  // other call. Every call after this resolves is synchronous-feeling.
  await init();

  const videoTimeBase = { num: 1, den: 30 }; // 30 fps
  const audioTimeBase = { num: 1, den: 48_000 }; // 48 kHz
  const frameCount = 90; // 3 s at 30 fps

  // 1. Create a muxer — starts "open": tracks can still be registered.
  const muxer = new Muxer();

  const videoTrackId = muxer.addTrack({
    kind: "video",
    codec: "h264",
    timeBase: videoTimeBase,
    width: 1920,
    height: 1080,
    extraData: new Uint8Array(0),
  });

  const audioTrackId = muxer.addTrack({
    kind: "audio",
    codec: "aac",
    timeBase: audioTimeBase,
    extraData: new Uint8Array(0),
    sampleRate: 48_000,
    channels: 2,
  });

  // 2. begin() closes track registration and switches the muxer to "live".
  muxer.begin();

  const videoPayload = new Uint8Array([0x00, 0x00, 0x00, 0x01]); // placeholder NAL
  const audioPayload = new Uint8Array([0xff, 0xf1]); // placeholder AAC frame

  for (let frame = 0; frame < frameCount; frame++) {
    muxer.pushPacket({
      streamId: videoTrackId,
      pts: frame,
      dts: frame,
      duration: 1,
      isKeyframe: frame % 30 === 0,
      isDiscard: false,
      payload: videoPayload,
    });

    muxer.pushPacket({
      streamId: audioTrackId,
      pts: frame * 1_600,
      dts: frame * 1_600,
      duration: 1_600,
      isKeyframe: true,
      isDiscard: false,
      payload: audioPayload,
    });
  }

  muxer.flush();

  // 3. Pull out the muxed bytes — the muxer never touches storage/network
  // itself; the caller decides where they go (upload, MediaSource, disk, ...).
  const mp4Bytes = muxer.pollBytes();
  muxer.free(); // release the WASM-side muxer; JS GC can't reach into WASM memory

  console.log(`mux-roundtrip: ${frameCount} frames -> ${mp4Bytes.byteLength} bytes of fMP4`);

  // 4. Demux the same bytes back, streaming them in as if freshly received.
  const demuxer = new Demuxer();
  demuxer.pushBytes(mp4Bytes);

  console.log(`mux-roundtrip: demuxer sees ${demuxer.streams.length} stream(s)`);
  for (const stream of demuxer.streams) {
    if (stream.kind === "video") {
      console.log(`  stream ${stream.id} - ${stream.codec} ${stream.width}x${stream.height}`);
    } else {
      console.log(`  stream ${stream.id} - ${stream.codec}`);
    }
  }

  let videoPackets = 0;
  let audioPackets = 0;
  let packet: Packet | undefined;
  while ((packet = demuxer.pollPacket()) !== undefined) {
    if (packet.streamId === videoTrackId) {
      videoPackets++;
    } else {
      audioPackets++;
    }
  }
  demuxer.free();

  console.log(`mux-roundtrip: recovered ${videoPackets} video + ${audioPackets} audio packets`);
}

main().catch((err: unknown) => {
  console.error(err);
});
