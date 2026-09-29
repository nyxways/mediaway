/**
 * window-capture.test.ts — Window capture over the C ABI (mediaway-ffi ADR-0005):
 * the grown `mediaway_desktop_capture_config_t` layout, the zero-means-previous-behaviour
 * defaults, the refusals that need no GPU, the status-14 mapping, and — opt-in — a real
 * WGC session against a real window.
 *
 * Mirrors crates/mediaway-ffi/tests/window_capture_smoke.rs. Assertion-based with
 * node:assert (no test framework); any failed assertion exits nonzero.
 *
 * The hardware section pops a real window onto the desktop, so it only runs with
 * MEDIAWAY_RUN_WINDOW_TESTS=1 (Windows 11 build 22000+ for the borderless assertion):
 *
 *   MEDIAWAY_RUN_WINDOW_TESTS=1 node node_modules/tsx/dist/cli.mjs test/window-capture.test.ts
 */

import assert from "node:assert/strict";
import koffi from "koffi";
import { device, MwDesktopConfig, type RawDesktopConfig } from "@mediaway/ffi";
import {
  CaptureUnavailableError,
  GpuDevice,
  RegionOutOfBoundsError,
  deviceStatusError,
  openWindowCapture,
  type WindowSession,
} from "@mediaway/device";
import { MediawayError } from "@mediaway/container";

const isWindows = process.platform === "win32";
const NO_GPU = { kind: 0, native: 0, webgpu_device_id: 0 };
const TIME_BASE = { num: 1n, den: 30 };

// mediaway_device_status_t
const UNSUPPORTED = 3;
const NO_BACKEND = 4;
const INVALID_INPUT = 5;

function layoutPin(): void {
  // Hand-derived from device.h's mediaway_desktop_capture_config_t (rational 16 B/align 8,
  // gpu handle 24 B/align 8, enums 4 B, bool 1 B). A reordered or resized field fails here
  // instead of corrupting a config the native side then reads.
  const expected: Record<string, number> = {
    source_kind: 0,
    source_index: 4,
    time_base: 8,
    gpu_device: 24,
    window_handle: 48,
    cursor: 56,
    border: 60,
    dimensions: 64,
    region_x: 68,
    region_y: 72,
    region_width: 76,
    region_height: 80,
    region_enabled: 84,
  };
  for (const [field, offset] of Object.entries(expected)) {
    assert.equal(koffi.offsetof(MwDesktopConfig, field), offset, `offsetof ${field}`);
  }
  assert.equal(koffi.sizeof(MwDesktopConfig), 88);
}

function defaultsAreZero(): void {
  const window: RawDesktopConfig = device.desktopConfigWindow(0x1_0000_0001n, TIME_BASE, NO_GPU);
  const screen: RawDesktopConfig = device.desktopConfigScreen(0, TIME_BASE, NO_GPU);
  assert.equal(window.source_kind, 1);
  assert.equal(BigInt(window.window_handle), 0x1_0000_0001n, "hwnd survives a native round trip");
  assert.equal(window.source_index, 0);
  for (const config of [window, screen]) {
    assert.equal(config.cursor, 0);
    assert.equal(config.border, 0);
    assert.equal(config.dimensions, 0);
    assert.equal(config.region_enabled, false);
    assert.deepEqual(
      [config.region_x, config.region_y, config.region_width, config.region_height],
      [0, 0, 0, 0]
    );
  }
  assert.equal(screen.source_kind, 0);
}

function open(config: RawDesktopConfig): number {
  const out: [unknown] = [null];
  const status = device.desktopOpen(config, out);
  assert.equal(out[0], null, "a failed open must not hand back a handle");
  return status;
}

function refusalsNeedNoGpu(): void {
  assert.equal(device.abiVersion(), 2, "the device ABI grew: a stale native library was loaded");

  // An HWND of 0 names no window; off Windows there is no HWND source at all.
  const zero = device.desktopConfigWindow(0n, TIME_BASE, NO_GPU);
  assert.equal(open(zero), isWindows ? INVALID_INPUT : UNSUPPORTED);

  // A zero-sized region is never valid — checked before any platform dispatch.
  const region = device.desktopConfigScreen(0, TIME_BASE, NO_GPU) as RawDesktopConfig;
  region.region_enabled = true;
  region.region_width = 0;
  region.region_height = 64;
  assert.equal(open(region), INVALID_INPUT);

  // Window-only options on a Screen config are a caller mistake, not silently dropped.
  const border = device.desktopConfigScreen(0, TIME_BASE, NO_GPU) as RawDesktopConfig;
  border.border = 1;
  assert.equal(open(border), INVALID_INPUT);
  const dims = device.desktopConfigScreen(0, TIME_BASE, NO_GPU) as RawDesktopConfig;
  dims.dimensions = 1;
  assert.equal(open(dims), INVALID_INPUT);

  // DXGI cannot draw the pointer or crop: UNSUPPORTED, refused before any device call.
  const cursor = device.desktopConfigScreen(0, TIME_BASE, NO_GPU) as RawDesktopConfig;
  cursor.cursor = 1;
  assert.equal(open(cursor), isWindows ? UNSUPPORTED : NO_BACKEND);
  const crop = device.desktopConfigScreen(0, TIME_BASE, NO_GPU) as RawDesktopConfig;
  crop.region_enabled = true;
  crop.region_width = 64;
  crop.region_height = 64;
  assert.equal(open(crop), isWindows ? UNSUPPORTED : NO_BACKEND);
}

function statusMapping(): void {
  assert.equal(deviceStatusError(0), undefined);
  const region = deviceStatusError(14);
  assert.ok(region instanceof RegionOutOfBoundsError);
  assert.equal(region.status, 14);
  assert.ok(region instanceof MediawayError, "stays catchable as a MediawayError");
  // 14 must not collapse into the bad-config or unavailable classes.
  const bad = deviceStatusError(INVALID_INPUT);
  assert.ok(bad instanceof MediawayError && !(bad instanceof RegionOutOfBoundsError));
  assert.ok(!(bad instanceof CaptureUnavailableError));
  assert.ok(deviceStatusError(UNSUPPORTED) instanceof CaptureUnavailableError);
}

// ── Real hardware (opt-in) ────────────────────────────────────────────────────

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/** Poll until one frame arrives (WGC delivery is async), or fail after 3 s. */
async function nextFrame(session: WindowSession): Promise<{ width: number; height: number }> {
  const deadline = Date.now() + 3000;
  while (Date.now() < deadline) {
    const frame = session.pollFrame();
    if (frame !== null) return frame;
    await sleep(20);
  }
  assert.fail("no frame within 3 s");
}

async function realWindow(): Promise<void> {
  const user32 = koffi.load("user32.dll");
  const CreateWindowExW = user32.func(
    "void *CreateWindowExW(uint32_t ex, str16 cls, str16 title, uint32_t style, int x, int y, int w, int h, void *parent, void *menu, void *inst, void *param)"
  );
  const ShowWindow = user32.func("bool ShowWindow(void *hwnd, int cmd)");
  const DestroyWindow = user32.func("bool DestroyWindow(void *hwnd)");

  const WS_OVERLAPPEDWINDOW = 0x00cf0000;
  const CW_USEDEFAULT = -2147483648;
  const SW_SHOWNORMAL = 1;
  // 335x250 makes WGC's capture size odd on both axes (it excludes the invisible resize
  // borders), so even-cropping is observable. Checked below rather than trusted.
  const hwndPtr = CreateWindowExW(
    0, "Static", "mediaway node window capture", WS_OVERLAPPEDWINDOW,
    CW_USEDEFAULT, CW_USEDEFAULT, 335, 250, null, null, null, null
  );
  assert.ok(hwndPtr, "could not create a real test window");
  ShowWindow(hwndPtr, SW_SHOWNORMAL);
  const hwnd = koffi.address(hwndPtr);

  const gpu = await GpuDevice.create();
  const timeBase = { num: 1, den: 30 };
  try {
    // Native size, from a plain session; a Shown border is never "hidden".
    const plain = await openWindowCapture({ hwnd, timeBase, gpuDevice: gpu });
    const native = { width: plain.width, height: plain.height };
    assert.ok(
      native.width % 2 === 1 || native.height % 2 === 1,
      `the window captured at ${native.width}x${native.height}, even on both axes, so this ` +
        "proves nothing about cropping; change the window size in this test"
    );
    assert.equal(plain.borderHidden, false);
    await nextFrame(plain);
    await plain.close();

    // Pointer + hidden border + even crop, all through the options object.
    const cropped = await openWindowCapture({
      hwnd, timeBase, gpuDevice: gpu,
      cursor: "included", border: "hidden", dimensions: "even-cropped",
    });
    assert.equal(cropped.width % 2, 0);
    assert.equal(cropped.height % 2, 0);
    assert.equal(cropped.width, native.width & ~1);
    assert.equal(cropped.height, native.height & ~1);
    // Needs Windows 11 build 22000+. Asserted, not skipped: a skip is how the border once
    // went unhidden with nobody noticing.
    assert.equal(cropped.borderHidden, true, "the OS refused to hide the border");
    const croppedFrame = await nextFrame(cropped);
    assert.deepEqual([croppedFrame.width, croppedFrame.height], [cropped.width, cropped.height]);
    await cropped.close();

    // A region at the origin (free) and one away from it (one GPU copy per frame): frames
    // are exactly the region's size either way.
    for (const [x, y] of [[0, 0], [10, 8]] as const) {
      const region = await openWindowCapture({
        hwnd, timeBase, gpuDevice: gpu, region: { x, y, width: 64, height: 48 },
      });
      assert.deepEqual([region.width, region.height], [64, 48], `region (${x}, ${y}) geometry`);
      const frame = await nextFrame(region);
      assert.deepEqual([frame.width, frame.height], [64, 48], `region (${x}, ${y}) frame`);
      await region.close();
    }

    // A region larger than the window is its own error, not the bad-config one.
    await assert.rejects(
      openWindowCapture({
        hwnd, timeBase, gpuDevice: gpu,
        region: { x: 0, y: 0, width: native.width + 100, height: native.height + 100 },
      }),
      (err: unknown) => err instanceof RegionOutOfBoundsError && err.status === 14
    );
    console.log(
      `window capture hardware: native ${native.width}x${native.height}, ` +
        `cropped ${cropped.width}x${cropped.height}, border hidden, regions ok`
    );
  } finally {
    gpu.close();
    DestroyWindow(hwndPtr);
  }
}

async function main(): Promise<void> {
  layoutPin();
  defaultsAreZero();
  refusalsNeedNoGpu();
  statusMapping();
  console.log("window capture: layout, defaults, refusals, status mapping ok");

  if (process.env.MEDIAWAY_RUN_WINDOW_TESTS !== "1") {
    console.log("window capture hardware test skipped (set MEDIAWAY_RUN_WINDOW_TESTS=1; it shows a window)");
    return;
  }
  assert.ok(isWindows, "the hardware test needs Windows");
  await realWindow();
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
