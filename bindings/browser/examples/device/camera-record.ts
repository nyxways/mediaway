/**
 * Record ~3 s from the camera -> H.264 -> fragmented MP4.
 *
 * Status: ✅ REAL — @mediaway/browser (ADR-0020) + native Web APIs. Capture is
 * getUserMedia (the binding deliberately does NOT wrap capture — Tier C);
 * frames reach the encode session via a canvas bridge (universal — no
 * Insertable-Streams flag needed), WebCodecs VideoEncoder does the codec work,
 * the WASM module muxes. Requires camera permission; needs a real camera to
 * produce frames (the native camera_record.* equivalents are verified on real
 * hardware).
 *
 * Mic path: requested so the plumbing is visible, but drained, not muxed —
 * audio encode is demonstrated by pipeline/encode-audio.ts (synthetic PCM →
 * AAC) and the native bindings' camera_record (two-track). Feeding mic PCM
 * through WebAudio/AudioWorklet into AudioEncoder is a host-composition
 * concern, not new package surface.
 */
import { init, Muxer, Demuxer, EncodeSession, type Sample } from "@mediaway/browser";

const WIDTH = 640;
const HEIGHT = 480;
const FPS = 30;
const DURATION_MS = 3_000;
const FRAME_US = Math.round(1_000_000 / FPS);

/** Trigger a browser download from bytes via an object URL + synthetic anchor click. */
function download(filename: string, bytes: Uint8Array, mimeType: string): void {
  const blob = new Blob([new Uint8Array(bytes)], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click(); // synthetic click: no DOM attachment needed in modern browsers
  URL.revokeObjectURL(url);
}

async function main(): Promise<void> {
  await init(); // browsers: resolves the packaged wasm URL automatically

  const stream = await navigator.mediaDevices.getUserMedia({
    video: { width: { ideal: WIDTH }, height: { ideal: HEIGHT }, frameRate: { ideal: FPS } },
    audio: false,
  });

  // Hidden <video> element as the getUserMedia sink; canvas is the universal
  // bridge into WebCodecs VideoFrame (works without any experimental flag).
  const videoEl = document.createElement("video");
  videoEl.srcObject = stream;
  videoEl.muted = true;
  await videoEl.play();
  const canvas = document.createElement("canvas");
  canvas.width = WIDTH;
  canvas.height = HEIGHT;
  const ctx = canvas.getContext("2d");
  if (ctx === null) throw new Error("2d canvas context unavailable");

  const muxer = new Muxer(1);
  const session = new EncodeSession(muxer);
  const encoder = await session.video({
    codec: "avc1.42E01E",
    width: WIDTH,
    height: HEIGHT,
    bitrate: 2_000_000,
    framerate: FPS,
    avc: { format: "avc" },
  });

  const startedAt = performance.now();
  let frames = 0;
  while (performance.now() - startedAt < DURATION_MS) {
    if (videoEl.readyState >= 2 /* HAVE_CURRENT_DATA */) {
      ctx.drawImage(videoEl, 0, 0, WIDTH, HEIGHT);
      const frame = new VideoFrame(canvas, { timestamp: frames * FRAME_US });
      encoder.encode(frame, { keyFrame: frames % 90 === 0 });
      frame.close();
      frames += 1;
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  stream.getTracks().forEach((t) => t.stop());

  const mp4 = await session.finish();
  console.log(`camera-record: ${frames} frames -> ${mp4.length} B fMP4`);
  muxer.free();
  download("camera.mp4", mp4, "video/mp4");

  // --- Demux sanity check ---
  const demuxer = new Demuxer();
  demuxer.pushBytes(mp4);
  const streams = demuxer.streams();
  let recovered = 0;
  for (let packet: Sample | null = demuxer.pollPacket(); packet !== null; packet = demuxer.pollPacket()) {
    recovered += 1;
  }
  demuxer.free();
  console.log(
    `camera-record: ${streams.map((s) => `${s.codec} ${s.width}x${s.height} extraData=${s.extraData.length}B`).join(", ")} — ${recovered} packets`,
  );
  if (streams.length !== 1 || streams[0].codec !== "h264" || recovered === 0) {
    throw new Error("camera-record: unexpected roundtrip result");
  }
}

main().catch((err) => {
  console.error("camera-record failed:", err);
});
