/**
 * Auto video encode -> fragmented MP4 — aspirational quick-start example.
 *
 * ASPIRATIONAL EXAMPLE: no `@mediaway/encoder` npm package exists yet. This
 * file shows the target ergonomics for a future Node.js binding over
 * Mediaway's C ABI (napi-rs native addon under the hood, wrapped in
 * idiomatic Node/TypeScript: typed configs, `Error` subclasses, explicit
 * `close()`). This is the Node.js host reaching native code through the C
 * ABI — not the browser/WASM host. See ../README.md and docs/spec/c-ffi.md.
 *
 * Mirrors examples/encode_to_mp4.rs: pick the best available OS/GPU H.264
 * encoder automatically (Zero-Copy GPU path preferred, CPU-upload fallback),
 * wire its output packets into a fragmented MP4 muxer, push 90 synthetic
 * grey NV12 frames (3 s @ 30 fps), and write the finished MP4 to disk.
 *
 * Run (once the real package exists):
 *   npx tsx encode-to-mp4.ts
 */

import { writeFile } from "node:fs/promises";
import {
  AutoVideoEncodeConfig,
  EncodeSession,
  EncoderUnavailableError,
  openAutoEncoder,
  type VideoFrame,
} from "@mediaway/encoder";

const WIDTH = 640;
const HEIGHT = 480;
const FPS = 30;
const SECONDS = 3;
const FRAME_COUNT = FPS * SECONDS;

/** Build one grey NV12 frame: width*height Y bytes, then width*height/2 interleaved UV bytes. */
function makeGreyNv12Frame(pts: number): VideoFrame {
  const ySize = WIDTH * HEIGHT;
  const uvSize = ySize / 2;
  const data = Buffer.alloc(ySize + uvSize, 0x80); // Y=128, U=128, V=128 everywhere

  return {
    pts,
    duration: 1,
    width: WIDTH,
    height: HEIGHT,
    pixelFormat: "nv12",
    data,
  };
}

async function main(): Promise<void> {
  // Defaults for H.264 at this resolution/framerate, then override bitrate.
  const config: AutoVideoEncodeConfig = {
    ...AutoVideoEncodeConfig.defaults("h264", WIDTH, HEIGHT, { num: 1, den: FPS }),
    bitrateBps: 2_000_000,
  };

  let encoder;
  try {
    // Tries the best available backend on this machine (GPU Zero-Copy first,
    // CPU-upload fallback) — throws if none exists here.
    encoder = await openAutoEncoder(config);
  } catch (err) {
    if (err instanceof EncoderUnavailableError) {
      console.error(`encode-to-mp4: no encoder backend available on this platform (${err.message})`);
      return;
    }
    throw err;
  }

  console.log("encode-to-mp4: running on this platform");

  const session = new EncodeSession(encoder);
  try {
    for (let pts = 0; pts < FRAME_COUNT; pts++) {
      await session.writeFrame(makeGreyNv12Frame(pts));
    }

    // Flushes the encoder, finalizes the muxer, and returns the complete MP4 bytes.
    const mp4Bytes = await session.finish();

    await writeFile("out.mp4", mp4Bytes);

    console.log(`encode-to-mp4: ${FRAME_COUNT} frames -> out.mp4 (${mp4Bytes.length} bytes)`);
  } finally {
    session.close();
  }
}

await main();
