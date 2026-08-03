/**
 * capture-screen.ts — screen capture quick start.
 *
 * Status: NOT AVAILABLE — the C ABI rejects screen capture today: it needs a
 * live GPU device handle (ID3D11Device*) with no CPU fallback, and its C
 * representation is deferred (see bindings/README.md's capability truth
 * table). `openScreenCapture()` therefore throws CaptureUnavailableError.
 * This is the capture-only analog of pipeline/screen-record.ts: it
 * demonstrates the honest gap and exits cleanly.
 *
 * Run: npx tsx examples/device/capture-screen.ts
 */

import { openScreenCapture, CaptureUnavailableError } from "@mediaway/device";

async function main(): Promise<void> {
  try {
    const screen = await openScreenCapture({ timeBase: { num: 1, den: 30 }, monitorIndex: 0 });
    await screen.close(); // unreachable today — the ABI changed if we get here
    console.log("unexpected: screen capture opened — the ABI changed");
  } catch (err) {
    if (err instanceof CaptureUnavailableError) {
      console.log("Screen capture is NOT available from this binding today:");
      console.log("  it needs a live GPU device handle (ID3D11Device*) with no");
      console.log("  CPU fallback, and its C representation is deferred.");
      console.log(`  (${err.message})`);
      return;
    }
    throw err;
  }
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
