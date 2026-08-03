/**
 * Encode 90 synthetic grey NV12 frames to H.264 -> fragmented MP4.
 *
 * Status: 🚧 ASPIRATIONAL — depends on the @mediaway/browser package existing AND on
 * the browser shipping a WebCodecs H.264 encoder. The encode -> fMP4 pipeline itself
 * is real on the native stack; in-browser it needs both of those.
 *
 * Mediaway's WASM module owns the encode->mux wiring: AutoVideoEncoder picks the
 * platform codec (through native WebCodecs), EncodeSession holds the pipeline, and
 * finish() returns the complete file. No capture is involved — frames are synthetic.
 */
import {
  init,
  AutoVideoEncoder,
  EncoderUnavailableError,
  VideoFrame as MediawayVideoFrame,
  type VideoEncodeConfig,
} from "@mediaway/browser";

const WIDTH = 640;
const HEIGHT = 480;
const FRAME_COUNT = 90;
const FRAME_US = Math.round(1_000_000 / 30); // 33_333 µs at 30 fps

function makeGreyNv12Frame(i: number): MediawayVideoFrame {
  // NV12 layout: W*H bytes of Y, then W*H/2 bytes of interleaved UV.
  const data = new Uint8Array((WIDTH * HEIGHT * 3) / 2);
  data.fill(0x80); // 0x80 on every plane = mid-grey
  return new MediawayVideoFrame({
    format: "nv12",
    width: WIDTH,
    height: HEIGHT,
    timestampUs: Math.round((i * 1_000_000) / 30),
    durationUs: FRAME_US,
    data, // a fresh JS-owned copy per frame
  });
}

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

  const config: VideoEncodeConfig = {
    codec: "h264",
    width: WIDTH,
    height: HEIGHT,
    frameRate: { num: 30, den: 1 },
  };

  let encoder: AutoVideoEncoder;
  try {
    encoder = new AutoVideoEncoder(config);
  } catch (err) {
    // Expected failure when the platform lacks the codec (e.g. no H.264 encoder).
    if (err instanceof EncoderUnavailableError) {
      console.error(`no ${config.codec} encoder on this browser: ${err.message}`);
      return;
    }
    throw err;
  }

  const session = encoder.createSession();
  encoder.free(); // the session owns the encode->mux pipeline now

  for (let i = 0; i < FRAME_COUNT; i += 1) {
    session.writeFrame(makeGreyNv12Frame(i));
  }

  // finish() is terminal: encodes the tail, flushes the muxer, and returns the
  // complete fMP4 bytes. The session handle is consumed — no free() needed after.
  const mp4: Uint8Array = await session.finish();

  console.log(`encoded ${FRAME_COUNT} frames -> ${mp4.length} bytes`);
  download("encode-to-mp4.mp4", mp4, "video/mp4");
}

main().catch((err) => console.error("encode-to-mp4 failed:", err));
