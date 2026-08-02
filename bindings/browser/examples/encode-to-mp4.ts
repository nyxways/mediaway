// encode-to-mp4.ts — Mediaway browser (WASM) quick start (ASPIRATIONAL EXAMPLE).
//
// No `@mediaway/browser` package exists yet — this file shows the target
// ergonomics for a future browser binding wrapping Mediaway's WASM module
// (`wasm-bindgen`), never the C ABI. See ../README.md and docs/spec/c-ffi.md
// Tier C.
//
// Mirrors examples/encode_to_mp4.rs: build a 640x480 @30fps H.264 config,
// open the best available auto encode path in this browser, push 90
// synthetic grey NV12 frames (3 s at 30 fps) through an encode session, and
// get back the resulting fragmented MP4 bytes.
//
// Browser-specific note: real H.264 hardware encode on the web goes through
// the browser's native `WebCodecs` `VideoEncoder`, not a software encoder
// reimplemented in WASM. `AutoVideoEncoder` is expected to drive WebCodecs
// under the hood and hand the resulting packets to Mediaway's WASM
// fragmented-MP4 muxer — callers still see the same "config -> open encoder
// -> encode session -> writeFrame -> finish" shape as every other language
// binding.

import init, { AutoVideoEncoder, EncodeSession } from "@mediaway/browser";
import type { VideoEncodeConfig, VideoFrame } from "@mediaway/browser";

const WIDTH = 640;
const HEIGHT = 480;
const FPS = 30;
const SECONDS = 3;
const FRAME_COUNT = FPS * SECONDS; // 90 frames = 3 s at 30 fps

async function main(): Promise<void> {
  // The WASM module must be instantiated (fetched + compiled) once before any
  // other call. Every call after this resolves is synchronous-feeling.
  await init();

  // Defaults for H264 at this resolution/framerate, then override bitrate.
  const config: VideoEncodeConfig = {
    codec: "h264",
    width: WIDTH,
    height: HEIGHT,
    frameRate: { num: 1, den: FPS },
    bitrateBps: 2_000_000,
  };

  // 1. Open the auto encoder — tries the best available encode path in this
  // browser (WebCodecs hardware/software H.264 under the hood) and throws
  // if none is available. This is a normal, expected outcome on
  // unsupported browsers, not a crash — catch it and bail out gracefully.
  let encoder: AutoVideoEncoder;
  try {
    encoder = await AutoVideoEncoder.open(config);
  } catch (err) {
    console.error(`encode-to-mp4: no supported H.264 encode path in this browser (${err})`);
    return;
  }

  console.log("encode-to-mp4: running in this browser");

  try {
    // 2. Wrap the opened encoder in an encode session — wires WebCodecs
    // output packets into the internal fragmented-MP4 muxer.
    const session = new EncodeSession(encoder);

    try {
      // Synthetic NV12 source (replace with real frames in your app): grey
      // Y=128 everywhere, U/V=128 everywhere. Layout is width*height Y bytes
      // followed by width*height/2 interleaved UV bytes.
      const ySize = WIDTH * HEIGHT;
      const uvSize = (WIDTH * HEIGHT) / 2;
      const nv12Frame = new Uint8Array(ySize + uvSize).fill(128);

      for (let pts = 0; pts < FRAME_COUNT; pts++) {
        const frame: VideoFrame = {
          pts,
          duration: 1,
          width: WIDTH,
          height: HEIGHT,
          pixelFormat: "nv12",
          data: nv12Frame,
        };
        await session.writeFrame(frame);
      }

      // 3. Flush the encoder, finalize the muxer, and get the complete MP4
      // file bytes back.
      const mp4Bytes = await session.finish();

      console.log(`encode-to-mp4: ${FRAME_COUNT} frames -> ${mp4Bytes.byteLength} bytes of fMP4`);

      // 4. Session/muxer never touch storage themselves — the caller decides
      // where the bytes go. Here: trigger a normal browser download.
      downloadAsFile(mp4Bytes, "out.mp4");
    } finally {
      session.free(); // release the WASM-side session; JS GC can't reach into WASM memory
    }
  } finally {
    encoder.free(); // release the WASM-side encoder handle
  }
}

// Trigger a browser download of the muxed bytes via an object URL + a
// synthetic anchor click.
function downloadAsFile(bytes: Uint8Array, filename: string): void {
  const blob = new Blob([bytes], { type: "video/mp4" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

main().catch((err: unknown) => {
  console.error(err);
});
