/**
 * Record ~3 s of the screen -> H.264 -> fragmented MP4.
 *
 * Status: ✅ REAL (browser) — @mediaway/browser (ADR-0020) + native Web APIs.
 * getDisplayMedia captures the screen; frames reach the encode session via a
 * canvas bridge; WebCodecs VideoEncoder does the codec work; the WASM module
 * muxes. The browser is AHEAD of the native stack here: the C ABI hosts cannot
 * screen-capture from C at all today (a live GPU device handle with no CPU
 * fallback, mediaway-device-ffi adr/0001 § Deferred) — this example is the
 * working browser path for the same scenario.
 *
 * Mic path: requested but drained, not muxed — see device/camera-record.ts for
 * the rationale; audio encode is demonstrated by pipeline/encode-audio.ts.
 */
import { init, Muxer, Demuxer, EncodeSession, type Sample } from "@mediaway/browser";

const WIDTH = 1280;
const HEIGHT = 720;
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

  const stream = await navigator.mediaDevices.getDisplayMedia({
    video: { width: { ideal: WIDTH }, height: { ideal: HEIGHT }, frameRate: { ideal: FPS } },
    audio: false,
  });

  // Hidden <video> element as the capture sink; canvas bridges into WebCodecs
  // VideoFrame (universal — no Insertable-Streams flag needed).
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
    bitrate: 4_000_000,
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
  console.log(`screen-record: ${frames} frames -> ${mp4.length} B fMP4`);
  muxer.free();
  download("screen.mp4", mp4, "video/mp4");

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
    `screen-record: ${streams.map((s) => `${s.codec} ${s.width}x${s.height} extraData=${s.extraData.length}B`).join(", ")} — ${recovered} packets`,
  );
  if (streams.length !== 1 || streams[0].codec !== "h264" || recovered === 0) {
    throw new Error("screen-record: unexpected roundtrip result");
  }
}

main().catch((err) => {
  console.error("screen-record failed:", err);
});
