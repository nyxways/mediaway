/**
 * screen-record.ts — screen + mic capture → encode → MP4
 * Mirrors: examples/pipeline/screen_record.rs
 *
 * Status: 🚧 ASPIRATIONAL — the native C ABI returns UNSUPPORTED for screen capture
 * today. Screen needs a live GPU device handle (ID3D11Device*) with no CPU fallback,
 * and its C representation is deferred (crates/mediaway-device-ffi/adr/0001, §
 * Deferred) — an honest gap, not a bug. The flow below is the ideal DX the
 * @mediaway/device + @mediaway/encoder packages target; run against the current ABI,
 * openScreenCapture() throws CaptureUnavailableError with status UNSUPPORTED and the
 * example exits with the explanation. It becomes runnable once the ABI lands a screen
 * source.
 *
 * Run (future): npx tsx examples/screen-record.ts
 */

import { writeFileSync } from "node:fs";

import {
  AutoVideoEncodeConfig,
  EncodeSession,
  openAutoEncoder,
  EncoderUnavailableError,
  MediawayError,
} from "@mediaway/encoder";
import {
  openScreenCapture,
  openMicrophone,
  ScreenSession,
  MicSession,
  CaptureUnavailableError,
} from "@mediaway/device";

const RECORD_MS = 5_000; // ~5 s of screen video at the negotiated capture rate
const SCREEN_TIME_BASE = { num: 1, den: 60 };
const MIC_SAMPLE_RATE = 48_000;
const OUT = "out.mp4";

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

async function main(): Promise<void> {
  let screen: ScreenSession | undefined;
  let mic: MicSession | undefined;
  try {
    // Today this always throws CaptureUnavailableError (status UNSUPPORTED).
    try {
      screen = await openScreenCapture({ timeBase: SCREEN_TIME_BASE, monitorIndex: 0 });
    } catch (err) {
      if (err instanceof CaptureUnavailableError) {
        console.log(
          `screen capture is UNSUPPORTED in the current C ABI (status ${err.status}); this example is aspirational`
        );
        return;
      }
      throw err;
    }

    // Mic is optional: fall back to a screen-only recording when it is missing.
    try {
      mic = await openMicrophone({ sampleRate: MIC_SAMPLE_RATE, channels: 2 });
    } catch (err) {
      if (err instanceof CaptureUnavailableError) {
        console.log("no microphone; recording screen only");
      } else {
        throw err;
      }
    }

    // Negotiated geometry is authoritative — the encoder must use it, not our request.
    const { width, height, pixelFormat, timeBase } = screen;
    console.log(`screen negotiated: ${width}x${height} ${pixelFormat} @ ${timeBase.den}/${timeBase.num} fps`);

    let session: EncodeSession;
    try {
      const config = AutoVideoEncodeConfig.defaults("h264", width, height, timeBase);
      config.bitrateBps = 4_000_000; // screen content benefits from more headroom
      session = await openAutoEncoder(config);
    } catch (err) {
      if (err instanceof EncoderUnavailableError) {
        console.log("no encoder backend on this machine; nothing to do");
        return;
      }
      throw err;
    }

    let recorded = 0;
    let drained = 0;
    try {
      const startedAt = Date.now();
      while (Date.now() - startedAt < RECORD_MS) {
        const frame = screen.pollFrame();
        if (frame === null) {
          await sleep(5); // no frame pending yet — poll again shortly
          continue;
        }
        await session.writeFrame(frame);
        recorded++;

        // Audio note: the screen path is blocked before audio matters — Screen
        // capture needs a live GPU device handle from C (mediaway-device-ffi
        // adr/0001, § Deferred), so the mic PCM is drained, not muxed.
        while (mic !== undefined) {
          const audio = mic.pollFrame();
          if (audio === null) break;
          drained++; // PCM frame deliberately dropped
        }
      }

      const mp4 = await session.finish();
      writeFileSync(OUT, mp4);
      console.log(
        `recorded ${recorded} frames in ~${RECORD_MS} ms -> ${OUT} (${mp4.length} bytes; mic frames drained: ${drained})`
      );
    } finally {
      session.close(); // no-op after finish(); frees the handle on early-error paths
    }
  } finally {
    // Capture sessions join their worker thread on close.
    await screen?.close();
    await mic?.close();
  }
}

main().catch((err) => {
  if (err instanceof MediawayError) {
    console.error(`mediaway error (status ${err.status}): ${err.message}`);
  } else {
    console.error(err);
  }
  process.exitCode = 1;
});
