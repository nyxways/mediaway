/**
 * capture-microphone.ts — microphone capture quick start (raw PCM).
 *
 * Status: ✅ real ABI under it.
 * The C ABI's microphone capture (raw interleaved f32le PCM,
 * mediaway_audio_capture_*) is implemented; this example runs against it via
 * @mediaway/device. Mirrors examples/device/capture_microphone.rs: open the
 * default mic, poll ~2 s of PCM frames, print the negotiated format, close.
 * No encoding — there is no audio encoder in the ABI.
 *
 * Run: npx tsx examples/device/capture-microphone.ts
 */

import { openMicrophone, CaptureUnavailableError } from "@mediaway/device";

const RECORD_MS = 2_000;
const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

async function main(): Promise<void> {
  let mic;
  try {
    mic = await openMicrophone({ sampleRate: 48_000, channels: 2 });
  } catch (err) {
    if (err instanceof CaptureUnavailableError) {
      console.log("no microphone on this machine; nothing to capture");
      return;
    }
    throw err;
  }

  try {
    let frames = 0;
    let totalBytes = 0;
    const startedAt = Date.now();
    while (Date.now() - startedAt < RECORD_MS) {
      const audio = mic.pollFrame();
      if (audio === null) {
        await sleep(2);
        continue;
      }
      if (frames === 0) {
        console.log(`mic negotiated: ${audio.sampleRate} Hz, ${audio.channels} ch`);
      }
      frames++;
      totalBytes += audio.data.length;
    }
    console.log(`captured ${frames} PCM frame(s), ${totalBytes} bytes in ~${RECORD_MS} ms`);
  } finally {
    await mic.close(); // joins the backend worker thread
  }
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
