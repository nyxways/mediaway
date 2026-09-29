/**
 * capture-window.ts — single-window capture quick start (Windows, WGC).
 *
 * Captures the window given as an HWND on the command line (decimal or 0x-hex), or the
 * foreground window when none is given. Frames are GPU-only and Zero-Copy: this polls a few
 * to show real timestamps and sizes; to record them, feed the session to
 * `EncodeSession.writeFrameFromDesktopCapture()` (`@mediaway/encoder`).
 *
 * Options shown: even-cropped frames (any hardware encoder accepts them), the pointer, and a
 * hidden capture border (Windows 11 build 22000+; a refusal is reported, not fatal).
 *
 * Run: npx tsx examples/device/capture-window.ts [hwnd]
 */

import koffi from "koffi";
import { openWindowCapture, CaptureUnavailableError, RegionOutOfBoundsError } from "@mediaway/device";

const POLL_FRAMES = 5;
const POLL_TIMEOUT_MS = 5_000;
const POLL_INTERVAL_MS = 20;

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

function targetWindow(): bigint {
  const arg = process.argv[2];
  if (arg !== undefined) return BigInt(arg);
  const GetForegroundWindow = koffi.load("user32.dll").func("void *GetForegroundWindow()");
  return koffi.address(GetForegroundWindow());
}

async function main(): Promise<void> {
  if (process.platform !== "win32") {
    console.log("window capture is Windows-only (WGC by HWND) — exiting gracefully");
    return;
  }
  const hwnd = targetWindow();
  let window;
  try {
    window = await openWindowCapture({
      hwnd,
      timeBase: { num: 1, den: 30 },
      cursor: "included",
      border: "hidden",
      dimensions: "even-cropped",
    });
  } catch (err) {
    if (err instanceof CaptureUnavailableError || err instanceof RegionOutOfBoundsError) {
      console.log(`cannot capture window 0x${hwnd.toString(16)} (${err.message}) — exiting gracefully`);
      return;
    }
    throw err;
  }
  console.log(
    `window 0x${hwnd.toString(16)} negotiated: ${window.width}x${window.height} ${window.pixelFormat}` +
      ` (border ${window.borderHidden ? "hidden" : "shown"})`
  );

  let polled = 0;
  const startedAt = Date.now();
  while (polled < POLL_FRAMES && Date.now() - startedAt < POLL_TIMEOUT_MS) {
    const frame = window.pollFrame();
    if (frame === null) {
      await sleep(POLL_INTERVAL_MS);
      continue;
    }
    console.log(`polled frame ${polled + 1}: pts=${frame.pts} ${frame.width}x${frame.height}`);
    polled++;
  }
  console.log(`polled ${polled} real window frame(s)`);

  await window.close();
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
