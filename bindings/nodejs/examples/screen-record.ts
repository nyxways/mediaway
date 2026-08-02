/**
 * Screen + mic capture -> encode -> fragmented MP4 — aspirational quick-start example.
 *
 * ASPIRATIONAL EXAMPLE: no `@mediaway/device` or `@mediaway/encoder` npm package
 * exists yet. This file shows the target ergonomics for a future Node.js binding
 * over Mediaway's C ABI (napi-rs native addon under the hood, wrapped in
 * idiomatic Node/TypeScript: typed configs, `Error` subclasses, explicit
 * `close()`). This is the Node.js host reaching native code through the C ABI —
 * Node runs on servers/desktops and calls OS screen/mic capture APIs through the
 * native addon; it does not use browser capture APIs. See ../README.md and
 * docs/spec/c-ffi.md.
 *
 * Builds a screen-recording pipeline from the same building blocks as the
 * auto-encode example (config -> open auto encoder -> open encode session ->
 * writeFrame -> finish), plus a device-capture layer, glued together by one
 * small, platform-agnostic `record()` function that only knows about the
 * `VideoCapture`/`AudioCapture` interfaces — never the concrete OS backend.
 *
 * Run (once the real packages exist):
 *   npx tsx screen-record.ts
 */

import { writeFile } from "node:fs/promises";
import type { Rational } from "@mediaway/common";
import {
  type AudioCapture,
  type VideoCapture,
  openMicrophoneCapture,
  openScreenCapture,
} from "@mediaway/device";
import {
  AutoVideoEncodeConfig,
  EncodeSession,
  EncoderUnavailableError,
  openAutoEncoder,
  type VideoFrame,
} from "@mediaway/encoder";

const FPS = 30;
const DURATION_MS = 3_000;
const OUTPUT_PATH = "out_screen.mp4";

/** Build one grey NV12 frame: width*height Y bytes, then width*height/2 interleaved UV bytes. */
function makeGreyNv12Frame(width: number, height: number, pts: number): VideoFrame {
  const ySize = width * height;
  const uvSize = ySize / 2;
  const data = Buffer.alloc(ySize + uvSize, 0x80); // Y=128, U=128, V=128 everywhere

  return {
    pts,
    duration: 1,
    width,
    height,
    pixelFormat: "nv12",
    data,
  };
}

/**
 * Poll video + (optional) audio capture and feed the encode session until
 * `durationMs` elapses.
 *
 * This function is the reusable "record loop": it only depends on the
 * `VideoCapture`/`AudioCapture` interfaces and an already-open `EncodeSession`,
 * so it works unchanged regardless of which OS backend opened the captures.
 */
async function record(
  video: VideoCapture,
  audio: AudioCapture | undefined,
  session: EncodeSession,
  durationMs: number,
): Promise<void> {
  const deadline = Date.now() + durationMs;
  let frameIndex = 0;

  while (Date.now() < deadline) {
    // A real capture frame may reference GPU-resident memory (Zero-Copy handle);
    // this example only writes a synthetic placeholder into the encoder instead
    // of the real pixels, but still models the release step below.
    const videoFrame = video.pollFrame();
    if (videoFrame !== undefined) {
      try {
        await session.writeFrame(makeGreyNv12Frame(video.width, video.height, frameIndex));
        frameIndex++;
      } finally {
        // Release the GPU-resident buffer back to the OS before polling again.
        videoFrame.release();
      }
    }

    // Drain any polled audio frames — this example doesn't wire audio into the
    // MP4 yet, just keeps the capture queue from backing up.
    if (audio !== undefined) {
      while (audio.pollFrame() !== undefined) {
        // intentionally discarded
      }
    }

    if (videoFrame === undefined) {
      // Nothing new yet — yield briefly instead of busy-spinning.
      await new Promise<void>((resolve) => setTimeout(resolve, 1));
    }
  }
}

async function main(): Promise<void> {
  let video: VideoCapture;
  try {
    // 0 = primary display; capture settles on its own actual stream geometry.
    video = await openScreenCapture({ displayIndex: 0, timeBase: { num: 1, den: FPS } satisfies Rational });
  } catch (err) {
    console.error(`screen-record: screen capture unavailable on this platform (${(err as Error).message})`);
    return;
  }

  let audio: AudioCapture | undefined;
  try {
    audio = await openMicrophoneCapture({ timeBase: { num: 1, den: 48_000 } satisfies Rational });
  } catch (err) {
    console.warn(`screen-record: microphone unavailable, continuing without audio (${(err as Error).message})`);
  }

  console.log(`screen-record: capturing at ${video.width}x${video.height}`);

  // Defaults for H.264 at the capture's real geometry @30fps, then override bitrate.
  const config: AutoVideoEncodeConfig = {
    ...AutoVideoEncodeConfig.defaults("h264", video.width, video.height, { num: 1, den: FPS }),
    bitrateBps: 8_000_000,
  };

  let encoder;
  try {
    // Tries the best available backend on this machine (GPU Zero-Copy first,
    // CPU-upload fallback) — throws if none exists here.
    encoder = await openAutoEncoder(config);
  } catch (err) {
    if (err instanceof EncoderUnavailableError) {
      console.error(`screen-record: no encoder backend available on this platform (${err.message})`);
      video.close();
      audio?.close();
      return;
    }
    throw err;
  }

  const session = new EncodeSession(encoder);
  try {
    try {
      await record(video, audio, session, DURATION_MS);
    } finally {
      video.close();
      audio?.close();
    }

    // Flushes the encoder, finalizes the muxer, and returns the complete MP4 bytes.
    const mp4Bytes = await session.finish();

    await writeFile(OUTPUT_PATH, mp4Bytes);

    console.log(
      `screen-record: ${(DURATION_MS / 1000).toFixed(0)}s @ ${video.width}x${video.height} -> ${OUTPUT_PATH} (${mp4Bytes.length} bytes)`,
    );
  } finally {
    session.close();
  }
}

await main();
