/**
 * capture-screen.ts — screen capture quick start.
 *
 * Status: ✅ real ABI under it, as of the GPU device factory
 * (mediaway-device ADR-0007). Before it, a Node caller had no way to
 * construct or own a live GPU device handle (ID3D11Device*) — Screen capture
 * has no CPU fallback, so every Screen config was unreachable. `GpuDevice`
 * (`@mediaway/device`) closes that gap: `openScreenCapture()` now creates one
 * internally (or accepts a caller-supplied device) and drives real Screen
 * capture with it.
 *
 * Run: npx tsx examples/device/capture-screen.ts
 */

import { openScreenCapture, CaptureUnavailableError } from "@mediaway/device";

const POLL_FRAMES = 5;
const POLL_TIMEOUT_MS = 5_000;
const POLL_INTERVAL_MS = 20;

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

async function main(): Promise<void> {
  let screen;
  try {
    screen = await openScreenCapture({ timeBase: { num: 1, den: 30 }, monitorIndex: 0 });
  } catch (err) {
    if (err instanceof CaptureUnavailableError) {
      console.log(`no usable GPU/Desktop-Duplication path (${err.message}) — exiting gracefully`);
      return;
    }
    throw err;
  }
  console.log(`screen negotiated: ${screen.width}x${screen.height} ${screen.pixelFormat}`);

  let polled = 0;
  const startedAt = Date.now();
  while (polled < POLL_FRAMES && Date.now() - startedAt < POLL_TIMEOUT_MS) {
    const frame = screen.pollFrame();
    if (frame === null) {
      await sleep(POLL_INTERVAL_MS);
      continue;
    }
    console.log(`polled frame ${polled + 1}: pts=${frame.pts} ${frame.width}x${frame.height}`);
    polled++;
  }
  console.log(`polled ${polled} real screen frame(s)`);

  await screen.close();
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
