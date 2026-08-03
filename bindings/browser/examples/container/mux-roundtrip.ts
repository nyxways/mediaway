/**
 * Mux -> demux roundtrip (fragmented MP4).
 *
 * Status: ✅ REAL — the core capability is proven in-WASM today: the iso-bmff-wasm
 * smoke crate already runs this same sans-io mux/demux logic in-browser. The
 * @mediaway/browser wrapper package itself is still 📐 design, but this scenario is
 * the one that already works.
 *
 * Muxes 90 synthetic H.264 packets + 90 synthetic AAC packets into a fragmented
 * MP4, then demuxes those exact bytes back and counts the recovered packets.
 * Pure computation — no capture, no codecs, no WebCodecs involved.
 */
import {
  init,
  Muxer,
  Demuxer,
  type Rational,
  type VideoTrackInfo,
  type AudioTrackInfo,
  type EncodedPacket,
  type DemuxedPacket,
  type DemuxedStream,
} from "@mediaway/browser";

const VIDEO_COUNT = 90;
const AUDIO_COUNT = 90;

// One 30 fps video frame, in microseconds.
const VIDEO_FRAME_US = Math.round(1_000_000 / 30); // 33_333 µs
// One AAC frame = 1024 samples at 48 kHz, in microseconds.
const AUDIO_FRAME_US = Math.round((1024 * 1_000_000) / 48_000); // 21_333 µs

function makeVideoPacket(i: number): EncodedPacket {
  const ptsUs = i * VIDEO_FRAME_US;
  return {
    // Fake Annex-B access unit: start code + NAL header (0x65 IDR / 0x41 slice).
    data: new Uint8Array([0x00, 0x00, 0x00, 0x01, i === 0 ? 0x65 : 0x41, i & 0xff]),
    ptsUs,
    dtsUs: ptsUs, // synthetic stream has no B-frames
    durationUs: VIDEO_FRAME_US,
    keyframe: i === 0,
  };
}

function makeAudioPacket(i: number): EncodedPacket {
  const ptsUs = i * AUDIO_FRAME_US;
  return {
    data: new Uint8Array([0x21, 0x10, i & 0xff]), // fake AAC access unit
    ptsUs,
    dtsUs: ptsUs,
    durationUs: AUDIO_FRAME_US,
    keyframe: true, // every AAC frame is independently decodable
  };
}

async function main(): Promise<void> {
  // init() fetches and instantiates the WASM module; nothing else may run before it.
  await init();

  // --- Mux ---
  const muxer = new Muxer();

  const videoTrackInfo: VideoTrackInfo = {
    codec: "h264",
    width: 640,
    height: 480,
    frameRate: { num: 30, den: 1 }, // 30 fps
  };
  const audioTrackInfo: AudioTrackInfo = {
    codec: "aac",
    sampleRate: 48_000,
    channelCount: 2,
  };

  const videoTrack = muxer.addVideoTrack(videoTrackInfo);
  const audioTrack = muxer.addAudioTrack(audioTrackInfo);

  muxer.begin(); // live: packets may now be pushed

  for (let i = 0; i < VIDEO_COUNT; i += 1) {
    muxer.pushVideo(videoTrack, makeVideoPacket(i));
  }
  for (let i = 0; i < AUDIO_COUNT; i += 1) {
    muxer.pushAudio(audioTrack, makeAudioPacket(i));
  }

  muxer.flush(); // finalize the trailing fragment(s)
  const mp4: Uint8Array = muxer.pollBytes(); // bytes are copied into JS-owned memory
  muxer.free(); // WASM-side handle: JS GC cannot see into WASM memory
  console.log(`muxed ${mp4.length} bytes`);

  // --- Demux ---
  const demuxer = new Demuxer();
  demuxer.pushBytes(mp4);

  const streams: DemuxedStream[] = demuxer.streams();
  console.log(`demuxer reports ${streams.length} stream(s)`);
  for (const stream of streams) {
    console.log(
      `  stream ${stream.index}: ${stream.kind} ${stream.codec}` +
        ` ${stream.width ?? "-"}x${stream.height ?? "-"} @ ${stream.sampleRate ?? "-"} Hz`,
    );
  }

  let recoveredVideo = 0;
  let recoveredAudio = 0;
  for (let packet: DemuxedPacket | null = demuxer.pollPacket(); packet !== null; packet = demuxer.pollPacket()) {
    if (streams[packet.stream].kind === "video") {
      recoveredVideo += 1;
    } else {
      recoveredAudio += 1;
    }
  }
  demuxer.free();

  console.log(`recovered ${recoveredVideo} video / ${recoveredAudio} audio packets`);
  if (recoveredVideo !== VIDEO_COUNT || recoveredAudio !== AUDIO_COUNT) {
    throw new Error("roundtrip lost packets");
  }
}

main().catch((err) => console.error("mux-roundtrip failed:", err));
