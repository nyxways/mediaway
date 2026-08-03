// capture-screen.ts — browser screen capture quick start.
//
// REAL in this host — capture is a native browser capability (Tier C: WASM +
// Web APIs; the C ABI is never involved, and unlike the C-ABI hosts there is
// no UNSUPPORTED gap: getDisplayMedia() is the platform's own screen
// capture). This is the browser analog of examples/device/capture_screen.rs:
// share the screen, count ~2 s of captured frames via a
// MediaStreamTrackProcessor, stop. Mediaway's WASM module would only be
// involved for the encode+mux half (see pipeline/screen-record.ts).
//
// Note: MediaStreamTrackProcessor (WebCodecs Insertable Streams) is not yet in
// TypeScript's bundled dom lib — the minimal shape is declared below.
// Run: open in Chromium-based browsers with the Insertable-Streams flag.

declare global {
  class MediaStreamTrackProcessor {
    constructor(init: { track: MediaStreamTrack; maxBufferSize?: number });
    readonly readable: ReadableStream<VideoFrame>;
  }
}

const CAPTURE_MS = 2_000;

async function main(): Promise<void> {
  const stream = await navigator.mediaDevices.getDisplayMedia({
    video: true,
    audio: false,
  });

  const [track] = stream.getVideoTracks();
  const processor = new MediaStreamTrackProcessor({ track });
  const reader = processor.readable.getReader();

  let frames = 0;
  const startedAt = performance.now();
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    frames++;
    value.close(); // native WebCodecs frame — always close it
    if (performance.now() - startedAt >= CAPTURE_MS) break;
  }
  reader.releaseLock();

  console.log(`captured ${frames} screen frame(s) in ~${CAPTURE_MS} ms`);

  stream.getTracks().forEach((t) => t.stop());
}

main().catch((err) => {
  console.error(err);
});
