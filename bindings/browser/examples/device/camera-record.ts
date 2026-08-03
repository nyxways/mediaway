/**
 * Record ~3 s from the camera (+ mic) -> H.264 -> fragmented MP4.
 *
 * Status: 🚧 ASPIRATIONAL — depends on @mediaway/browser and on WebCodecs
 * availability in the browser. Capture itself is native Web APIs (getUserMedia),
 * which the binding deliberately does NOT wrap; frames reach the encode session
 * through WebCodecs Insertable Streams (MediaStreamTrackProcessor).
 *
 * Mic path: per the capability truth table, the native ABI has no audio encode yet —
 * mic frames are drained, not muxed. This example requests the mic so the plumbing
 * is visible, but only the video track feeds the encode session; the audio track is
 * stopped without being encoded.
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
  const stream = await navigator.mediaDevices.getUserMedia({ video: true, audio: true });
  const videoTrack = stream.getVideoTracks()[0];
  const audioTrack = stream.getAudioTracks()[0]; // drained, not muxed (see header)

  const settings = videoTrack.getSettings();
  const width = settings.width ?? 1280;
  const height = settings.height ?? 720;

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
    if (err instanceof EncoderUnavailableError) {
      console.error(`no ${config.codec} encoder on this browser: ${err.message}`);
      return;
    }
    throw err;
  }

  const session = encoder.createSession();
  encoder.free(); // the session owns the encode->mux pipeline now

  // Insertable Streams: pull native VideoFrames straight off the live track.
  const processor = new MediaStreamTrackProcessor({ track: videoTrack });
  const reader = processor.readable.getReader();

  const deadline = performance.now() + DURATION_MS;
  let frames = 0;
  while (performance.now() < deadline) {
    const { value, done } = await reader.read();
    if (done) break; // track ended (camera unplugged, permission revoked)
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
  if (audioTrack) audioTrack.stop(); // mic was drained, not muxed

  console.log(`captured ${frames} frames over ${DURATION_MS} ms -> ${mp4.length} bytes`);
  download("camera-record.mp4", mp4, "video/mp4");
}

main().catch((err) => console.error("camera-record failed:", err));
