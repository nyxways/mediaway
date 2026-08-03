/**
 * Record ~3 s of the screen (+ mic) -> H.264 -> fragmented MP4.
 *
 * Status: 🚧 ASPIRATIONAL — Chromium-only for now: feeding captured frames into
 * WebCodecs via MediaStreamTrackProcessor needs the "Insertable Streams for
 * MediaStreamTrack" flag. And unlike every other scenario, the native C ABI on the
 * other hosts returns UNSUPPORTED for screen capture today (a live GPU device handle
 * with no CPU fallback, deferred in mediaway-device-ffi ADR-0001) — so this example
 * sketches the browser path ahead of the native stack.
 *
 * Capture is native Web APIs (getDisplayMedia for video, getUserMedia for mic).
 * The mic track is drained, not muxed: no audio encode exists in the ABI yet.
 */
import {
  init,
  AutoVideoEncoder,
  EncoderUnavailableError,
  VideoFrame as MediawayVideoFrame,
  type VideoEncodeConfig,
} from "@mediaway/browser";

// MediaStreamTrackProcessor is not in the TS dom lib yet — minimal shape only.
// Its `readable` yields the *global* WebCodecs VideoFrame type (see import alias).
declare global {
  class MediaStreamTrackProcessor {
    constructor(init: { track: MediaStreamTrack; maxBufferSize?: number });
    readonly readable: ReadableStream<VideoFrame>;
  }
}

const DURATION_MS = 3_000;

/** Trigger a browser download from bytes via an object URL + synthetic anchor click. */
function download(filename: string, bytes: Uint8Array, mimeType: string): void {
  const blob = new Blob([bytes], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click(); // synthetic click: no DOM attachment needed in modern browsers
  URL.revokeObjectURL(url);
}

async function main(): Promise<void> {
  // init() fetches and instantiates the WASM module; nothing else may run before it.
  await init();

  // Native capture — the host provides devices, Mediaway never wraps them.
  const displayStream = await navigator.mediaDevices.getDisplayMedia({ video: true });
  const videoTrack = displayStream.getVideoTracks()[0];
  const micStream = await navigator.mediaDevices.getUserMedia({ audio: true });
  const micTrack = micStream.getAudioTracks()[0]; // drained, not muxed (see header)

  const settings = videoTrack.getSettings();
  const width = settings.width ?? 1920;
  const height = settings.height ?? 1080;

  const config: VideoEncodeConfig = {
    codec: "h264",
    width,
    height,
    frameRate: { num: 30, den: 1 }, // hint; the pipeline paces on frame timestamps
  };

  let encoder: AutoVideoEncoder;
  try {
    encoder = new AutoVideoEncoder(config);
  } catch (err) {
    videoTrack.stop();
    micTrack.stop();
    if (err instanceof EncoderUnavailableError) {
      console.error(`no ${config.codec} encoder on this browser: ${err.message}`);
      return;
    }
    throw err;
  }

  const session = encoder.createSession();
  encoder.free(); // the session owns the encode->mux pipeline now

  // Insertable Streams: pull native VideoFrames straight off the screen track.
  // Screen tracks commonly run at 60 fps; the pipeline drops frames as needed.
  const processor = new MediaStreamTrackProcessor({ track: videoTrack });
  const reader = processor.readable.getReader();

  const deadline = performance.now() + DURATION_MS;
  let frames = 0;
  while (performance.now() < deadline) {
    const { value, done } = await reader.read();
    if (done) break; // user ended sharing via the browser's "Stop sharing" UI
    // Convert the native WebCodecs frame into Mediaway's frame type (pixels are
    // copied into JS-owned memory). The native frame is still ours to close.
    const mediawayFrame = await MediawayVideoFrame.fromWebCodecs(value);
    session.writeFrame(mediawayFrame);
    value.close(); // release the WebCodecs CPU/GPU backing
    frames += 1;
  }
  await reader.cancel();

  // finish() is terminal: encodes the tail, flushes the muxer, returns the fMP4
  // bytes, and consumes the session handle.
  const mp4: Uint8Array = await session.finish();

  videoTrack.stop();
  micTrack.stop(); // mic was drained, not muxed

  console.log(`captured ${frames} frames over ${DURATION_MS} ms -> ${mp4.length} bytes`);
  download("screen-record.mp4", mp4, "video/mp4");
}

main().catch((err) => console.error("screen-record failed:", err));
