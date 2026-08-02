// screen-record.ts — Mediaway browser (WASM) screen recording quick start
// (ASPIRATIONAL EXAMPLE).
//
// No `@mediaway/browser` package exists yet — this file shows the target
// ergonomics for a future browser binding covering screen + mic recording.
// See ../README.md and docs/spec/c-ffi.md Tier C.
//
// Mirrors examples/screen_record.rs, but the "capture" half is plain
// browser-native code, not a Mediaway wrapper: the browser already exposes
// `getDisplayMedia()` / `getUserMedia()` for screen + mic, so Mediaway's WASM
// module only owns the "encode + mux" half — the SAME `AutoVideoEncoder` /
// `EncodeSession` building blocks used in encode-to-mp4.ts.
//
// Browser-specific note: raw frames come off the captured video track via a
// `MediaStreamTrackProcessor` (WebCodecs Insertable Streams), which yields a
// `ReadableStream<VideoFrame>` of *native* WebCodecs frames — not the same
// `VideoFrame` type Mediaway's encode session expects. That name collides
// with Mediaway's own frame shape, so Mediaway's type is imported under an
// alias (`MediawayVideoFrame`) below.

import init, { AutoVideoEncoder, EncodeSession } from "@mediaway/browser";
import type { VideoEncodeConfig, VideoFrame as MediawayVideoFrame } from "@mediaway/browser";

// `MediaStreamTrackProcessor` (WebCodecs Insertable Streams for
// `MediaStreamTrack`) is not yet part of TypeScript's bundled `dom` lib —
// declare the minimal shape actually used here. Support: Chromium-based
// browsers only, behind the "Insertable Streams for MediaStreamTrack" flag.
declare global {
  interface MediaStreamTrackProcessorInit {
    track: MediaStreamTrack;
  }

  class MediaStreamTrackProcessor {
    constructor(init: MediaStreamTrackProcessorInit);
    readonly readable: ReadableStream<VideoFrame>;
  }
}

const FPS = 30;
const RECORD_MS = 3_000;

async function main(): Promise<void> {
  // The WASM module must be instantiated (fetched + compiled) once before any
  // other call. Every call after this resolves is synchronous-feeling.
  await init();

  // 1. Screen video — plain browser-native capture, no Mediaway wrapper.
  const displayStream = await navigator.mediaDevices.getDisplayMedia({ video: true });
  const videoTrack = displayStream.getVideoTracks()[0];
  const { width = 0, height = 0 } = videoTrack.getSettings();

  if (width === 0 || height === 0) {
    console.error("screen-record: could not read the captured display resolution");
    displayStream.getTracks().forEach((track) => track.stop());
    return;
  }

  // 2. Microphone audio — also plain browser-native capture. Failure here is
  // non-fatal: log it and continue recording screen-only, matching the other
  // language examples.
  let micStream: MediaStream | null = null;
  try {
    micStream = await navigator.mediaDevices.getUserMedia({ audio: true });
  } catch (err) {
    console.warn(`screen-record: mic unavailable (${err}) — continuing without audio`);
  }
  const micTrack = micStream?.getAudioTracks()[0] ?? null;

  console.log(`screen-record: ${width}x${height} display${micTrack ? ", mic ready" : ", no mic"}`);

  // 3. Turn the captured video track into a ReadableStream<VideoFrame> the
  // record loop below can pull from — this is the only place WebCodecs
  // Insertable Streams appears; everything past it is generic frame plumbing.
  const frameStream = new MediaStreamTrackProcessor({ track: videoTrack }).readable;

  // 4. Same encode building blocks as the plain auto-encode example: config
  // -> open auto encoder -> open encode session -> writeFrame -> finish.
  const config: VideoEncodeConfig = {
    codec: "h264",
    width,
    height,
    frameRate: { num: 1, den: FPS },
    bitrateBps: 8_000_000,
  };

  let encoder: AutoVideoEncoder;
  try {
    encoder = await AutoVideoEncoder.open(config);
  } catch (err) {
    console.error(`screen-record: no supported H.264 encode path in this browser (${err})`);
    displayStream.getTracks().forEach((track) => track.stop());
    micStream?.getTracks().forEach((track) => track.stop());
    return;
  }

  try {
    const session = new EncodeSession(encoder);
    try {
      const frameCount = await record(frameStream, micTrack, session, width, height, RECORD_MS);

      // Stop capture before finishing the encode — no more frames incoming.
      displayStream.getTracks().forEach((track) => track.stop());
      micStream?.getTracks().forEach((track) => track.stop());

      // Flushes the encoder, finalizes the muxer, and returns the complete MP4 bytes.
      const mp4Bytes = await session.finish();

      console.log(
        `screen-record: ${(RECORD_MS / 1000).toFixed(1)}s, ${frameCount} frame(s) -> ` +
          `out_screen.mp4 (${mp4Bytes.byteLength} bytes)`,
      );
      downloadAsFile(mp4Bytes, "out_screen.mp4");
    } finally {
      session.free(); // release the WASM-side session; JS GC can't reach into WASM memory
    }
  } finally {
    encoder.free(); // release the WASM-side encoder handle
  }
}

/**
 * Record loop, decoupled from where frames come from: pulls `VideoFrame`s off
 * `frameStream` and writes them into `session` until `durationMs` elapses.
 * Returns the number of frames written.
 *
 * `micTrack` is accepted for a future second (audio) encoded track — not
 * wired into the muxer in this example, mirroring examples/screen_record.rs.
 */
async function record(
  frameStream: ReadableStream<VideoFrame>,
  micTrack: MediaStreamTrack | null,
  session: EncodeSession,
  width: number,
  height: number,
  durationMs: number,
): Promise<number> {
  const reader = frameStream.getReader();
  const deadline = performance.now() + durationMs;

  // Synthetic NV12 placeholder (real BGRA/WebCodecs -> NV12 conversion is not
  // implemented here, to keep the example focused on the API shape): grey
  // Y=128 everywhere, U/V=128 everywhere. Layout is width*height Y bytes
  // followed by width*height/2 interleaved UV bytes.
  const ySize = width * height;
  const uvSize = ySize / 2;
  const greyNv12 = new Uint8Array(ySize + uvSize).fill(128);

  let pts = 0;
  try {
    while (performance.now() < deadline) {
      const { value: capturedFrame, done } = await reader.read();
      if (done) break;

      try {
        const frame: MediawayVideoFrame = {
          pts,
          duration: 1,
          width,
          height,
          pixelFormat: "nv12",
          data: greyNv12,
        };
        await session.writeFrame(frame);
        pts++;
      } finally {
        capturedFrame.close(); // release the underlying WebCodecs VideoFrame
      }
    }
  } finally {
    reader.releaseLock();
  }

  if (micTrack) {
    // Not wired yet: encode mic audio into a second track and mux it
    // alongside video.
  }

  return pts;
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
