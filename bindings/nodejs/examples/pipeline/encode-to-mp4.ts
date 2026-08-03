/**
 * encode-to-mp4.ts — auto video encode → fragmented MP4
 * Mirrors: examples/pipeline/encode_to_mp4.rs
 *
 * Status: ✅ real ABI under it.
 * The C ABI's pipeline capability (auto video encode → fMP4, mediaway_pipeline_ffi)
 * is implemented: one call picks the best available OS/GPU encoder for the config and
 * wires it into an internal MP4 muxer. Video only — no audio encoder in the ABI.
 *
 * Flow: encode 90 synthetic grey NV12 frames with the auto-selected H.264 encoder and
 * write the resulting MP4 bytes to out.mp4. When no encoder backend exists on this
 * machine we catch EncoderUnavailableError and exit cleanly.
 *
 * Run (once @mediaway/* packages exist): npx tsx examples/encode-to-mp4.ts
 */

import { writeFileSync } from "node:fs";

import {
  AutoVideoEncodeConfig,
  EncodeSession,
  VideoFrame,
  openAutoEncoder,
  EncoderUnavailableError,
  MediawayError,
} from "@mediaway/encoder";
import { Rational } from "@mediaway/container";

const CODEC = "h264";
const WIDTH = 640;
const HEIGHT = 480;
const TIME_BASE: Rational = { num: 1, den: 30 };
const FRAME_COUNT = 90; // 3 s at 30 fps
const OUT = "out.mp4";

async function main(): Promise<void> {
  const config = AutoVideoEncodeConfig.defaults(CODEC, WIDTH, HEIGHT, TIME_BASE);
  config.bitrateBps = 1_500_000; // override the backend default for this resolution

  let session: EncodeSession;
  try {
    session = await openAutoEncoder(config);
  } catch (err) {
    if (err instanceof EncoderUnavailableError) {
      console.log(`no ${CODEC} encoder backend on this machine; nothing to do`);
      return;
    }
    throw err;
  }

  try {
    // Grey NV12: Y plane and both chroma planes at 128 → neutral grey, no gradients.
    const greyFrame = Buffer.alloc((WIDTH * HEIGHT * 3) / 2, 0x80);

    for (let i = 0; i < FRAME_COUNT; i++) {
      const frame: VideoFrame = {
        pts: i, // units of the config's timeBase (1/30 s)
        duration: 1,
        width: WIDTH,
        height: HEIGHT,
        pixelFormat: "nv12",
        data: greyFrame, // safe to reuse: the wrapper copies per writeFrame call
      };
      await session.writeFrame(frame);
    }

    // finish() is terminal: it consumes the session and returns the complete MP4.
    const mp4 = await session.finish();
    writeFileSync(OUT, mp4);
    console.log(`encoded ${FRAME_COUNT} frames (${CODEC} ${WIDTH}x${HEIGHT}) -> ${OUT} (${mp4.length} bytes)`);
  } finally {
    // Idempotent; a no-op after finish() consumed the session, but required to free
    // the native handle when an error aborted the encode mid-way.
    session.close();
  }
}

main().catch((err) => {
  if (err instanceof MediawayError) {
    console.error(`mediaway error (status ${err.status}): ${err.message}`);
  } else {
    console.error(err);
  }
  process.exitCode = 1;
});
