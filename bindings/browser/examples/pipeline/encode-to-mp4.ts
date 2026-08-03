/**
 * WebCodecs H.264 encode -> fMP4 (fragmented MP4).
 *
 * Status: ✅ REAL — @mediaway/browser (ADR-0020). The WASM module owns the
 * container; the browser's native WebCodecs `VideoEncoder` owns the codec.
 * Encodes 90 synthetic NV12 frames (64x64, 30 fps) to H.264 and muxes them
 * through `EncodeSession`, then demuxes the result and counts packets.
 *
 * BROWSER ONLY — WebCodecs `VideoEncoder` is not available in Node. Serve the
 * package and open this example in a browser:
 *   cd bindings/browser && bunx --bun serve .   (or any static server)
 * The native bindings' video-encode equivalent is `pipeline/encode_to_mp4.*`.
 */
import { init, Muxer, Demuxer, EncodeSession, type Sample } from "@mediaway/browser";

const WIDTH = 64;
const HEIGHT = 64;
const FRAMES = 90;
const FPS = 30;
const FRAME_US = Math.round(1_000_000 / FPS);

/** One NV12 frame: Y plane + interleaved UV. Solid grey (Y=128, U=V=128). */
function makeNv12Frame(i: number): { data: Uint8Array; frame: VideoFrame } {
  const yLen = WIDTH * HEIGHT;
  const uvLen = yLen / 2;
  const data = new Uint8Array(yLen + uvLen);
  data.fill(128);
  // Vary the grey level per frame so the encoder sees motion (keyframe-safe).
  const shade = 64 + ((i * 7) % 64);
  data.fill(shade, 0, yLen);
  const frame = new VideoFrame(data, {
    format: "NV12",
    codedWidth: WIDTH,
    codedHeight: HEIGHT,
    timestamp: i * FRAME_US,
  });
  return { data, frame };
}

async function main(): Promise<void> {
  // Browsers: `await init()` resolves the packaged wasm URL automatically.
  await init();

  const muxer = new Muxer(1); // one frame per fragment
  const session = new EncodeSession(muxer);
  const encoder = await session.video({
    codec: "avc1.42E01E", // H.264 baseline
    width: WIDTH,
    height: HEIGHT,
    bitrate: 500_000,
    framerate: FPS,
    avc: { format: "avc" },
  });

  for (let i = 0; i < FRAMES; i += 1) {
    const { frame } = makeNv12Frame(i);
    encoder.encode(frame, { keyFrame: i % 30 === 0 });
    frame.close();
  }

  const mp4 = await session.finish();
  console.log(`encode-to-mp4: muxed ${mp4.length} bytes (${FRAMES} frames)`);
  muxer.free();

  // --- Demux roundtrip ---
  const demuxer = new Demuxer();
  demuxer.pushBytes(mp4);
  const streams = demuxer.streams();
  let recovered = 0;
  for (let packet: Sample | null = demuxer.pollPacket(); packet !== null; packet = demuxer.pollPacket()) {
    recovered += 1;
  }
  demuxer.free();
  console.log(
    `encode-to-mp4: ${streams.length} stream(s) ` +
      `(${streams.map((s) => `${s.codec} ${s.width}x${s.height} extraData=${s.extraData.length}B`).join(", ")}), ` +
      `${recovered} packets recovered`,
  );
  if (streams.length !== 1 || streams[0].codec !== "h264" || recovered === 0) {
    throw new Error("encode-to-mp4: unexpected roundtrip result");
  }
}

main().catch((err) => {
  console.error("encode-to-mp4 failed:", err);
});
