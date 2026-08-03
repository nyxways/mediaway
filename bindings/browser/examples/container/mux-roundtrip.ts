/**
 * Mux -> demux roundtrip (fragmented MP4).
 *
 * Status: ✅ REAL — @mediaway/browser (ADR-0020). Muxes 90 synthetic H.264
 * packets + 90 synthetic AAC packets into a fragmented MP4 through the WASM
 * muxer, then demuxes those exact bytes back and counts the recovered packets.
 * Pure computation — no capture, no codecs, no WebCodecs involved.
 *
 * Run (Node): npx tsx mux-roundtrip.ts
 */
import { init, Muxer, Demuxer, type Rational, type Track, type Sample } from "@mediaway/browser";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const VIDEO_COUNT = 90;
const AUDIO_COUNT = 90;

// One 30 fps video frame, in timescale ticks (timebase 1/30).
const VIDEO_TICKS = 1;
// One AAC frame = 1024 samples at 48 kHz, in timescale ticks (timebase 1/48000).
const AUDIO_TICKS = 1024;

function makeVideoPacket(i: number): Sample {
  return {
    streamId: 0,
    pts: i * VIDEO_TICKS,
    dts: i * VIDEO_TICKS,
    duration: VIDEO_TICKS,
    isKeyframe: i % 30 === 0,
    isDiscard: false,
    // Fake AVCC-format payload (length-prefixed): 2-byte len + minimal data.
    payload: new Uint8Array([0, 2, 0x65, 0x88]),
  };
}

function makeAudioPacket(i: number): Sample {
  return {
    streamId: 1,
    pts: i * AUDIO_TICKS,
    dts: i * AUDIO_TICKS,
    duration: AUDIO_TICKS,
    isKeyframe: true,
    isDiscard: false,
    payload: new Uint8Array([0x21, 0x10, 0x04, 0x60, 0x8c, 0x1c]),
  };
}

async function main(): Promise<void> {
  // init() fetches and instantiates the WASM module; nothing else may run before it.
  // Browsers: `await init()` alone resolves the packaged wasm URL. Under Node
  // (this dev run) file: fetch is unavailable, so pass the wasm bytes explicitly.
  const wasmPath = path.join(
    path.dirname(fileURLToPath(import.meta.url)),
    "..", "..", "packages", "browser", "pkg", "iso_bmff_wasm_bg.wasm",
  );
  await init(new Uint8Array(readFileSync(wasmPath)));

  // --- Mux ---
  const muxer = new Muxer(1); // one sample per fragment

  const timeBaseVideo: Rational = { num: 1, den: 30 };
  const timeBaseAudio: Rational = { num: 1, den: 48_000 };

  const videoTrack: Track = {
    id: 0,
    codec: "h264",
    timeBase: timeBaseVideo,
    width: 64,
    height: 64,
    // Minimal avcC so the muxer can write the avc1 sample entry.
    extraData: new Uint8Array([
      1, 0x42, 0, 0x1e, 0xff, 0xe1, 0, 4, 0x67, 0x42, 0x00, 0x1e, 1, 0, 4, 0x68, 0xce, 0x06, 0xe2,
    ]),
  };
  const audioTrack: Track = {
    id: 1,
    codec: "aac",
    timeBase: timeBaseAudio,
    width: 0,
    height: 0,
    extraData: new Uint8Array([0x11, 0x90]), // AudioSpecificConfig, 48 kHz stereo
  };

  muxer.addTrack(videoTrack);
  muxer.addTrack(audioTrack);
  muxer.begin(); // live: packets may now be pushed

  for (let i = 0; i < VIDEO_COUNT; i += 1) {
    muxer.pushPacket(makeVideoPacket(i));
  }
  for (let i = 0; i < AUDIO_COUNT; i += 1) {
    muxer.pushPacket(makeAudioPacket(i));
  }

  muxer.flush(); // finalize the trailing fragment(s)
  const mp4: Uint8Array = muxer.pollBytes(); // fresh copy into JS-owned memory
  muxer.free(); // WASM-side handle: JS GC cannot see into WASM memory
  console.log(`muxed ${mp4.length} bytes`);

  // --- Demux ---
  const demuxer = new Demuxer();
  demuxer.pushBytes(mp4);

  const streams: Track[] = demuxer.streams();
  console.log(`demuxer reports ${streams.length} stream(s)`);
  for (const stream of streams) {
    console.log(
      `  stream ${stream.id}: ${stream.codec} ${stream.timeBase.num}/${stream.timeBase.den} ` +
        `${stream.width}x${stream.height} extraData=${stream.extraData.length} B`,
    );
  }

  let recoveredVideo = 0;
  let recoveredAudio = 0;
  for (let packet: Sample | null = demuxer.pollPacket(); packet !== null; packet = demuxer.pollPacket()) {
    if (packet.streamId === 0) recoveredVideo += 1;
    else recoveredAudio += 1;
  }
  demuxer.free();

  console.log(`recovered ${recoveredVideo} video / ${recoveredAudio} audio packets`);
  if (recoveredVideo !== VIDEO_COUNT || recoveredAudio !== AUDIO_COUNT) {
    throw new Error("roundtrip lost packets");
  }
  if (streams.length !== 2) {
    throw new Error("expected 2 demuxed streams");
  }
}

main().catch((err) => {
  console.error("mux-roundtrip failed:", err);
  process.exitCode = 1;
});
