/**
 * @mediaway/device — device capability: camera / screen / microphone capture,
 * plus the GPU device factory (mediaway-device ADR-0007).
 *
 * Implements the DX contract in bindings/nodejs/README.md over the
 * mediaway-ffi C ABI (via @mediaway/ffi). The ABI is domain-split
 * (`adr/0004-domain-feature-split.md`): this package folds it back into DX
 * functions. Camera delivers CPU frames; Screen capture is GPU-only, Zero-Copy
 * — it needs a live GPU device handle with no CPU fallback
 * (`adr/0003-gpu-handle-c-abi.md`), which `openScreenCapture()` now gets from
 * `GpuDevice` (auto-created, or caller-supplied for explicit adapter control
 * / sharing one device across capture + encode). Audio capture delivers raw
 * interleaved f32le PCM — there is no audio encoder in the ABI.
 */

import {
  MwAudioFrame,
  MwAudioConfig,
  MwCameraConfig,
  MwCameraFrame,
  MwGpuAdapterInfo,
  device,
  copyBytes,
  decodeArray,
  type RawAudioFrame,
  type RawCameraFrame,
  type RawDesktopFrame,
  type RawGpuAdapterInfo,
  type RawGpuDeviceHandle,
} from "@mediaway/ffi";
import { MediawayError, type Rational } from "@mediaway/container";

export type { Rational } from "@mediaway/container";

/**
 * @internal Key for the opaque native handle behind a capture/GPU-device
 * session, consumed only by @mediaway/encoder's capture-to-encode bridge
 * (`EncodeSession.writeFrameFromCameraCapture`/`writeFrameFromDesktopCapture`)
 * and by `AutoVideoEncodeConfig.gpuDevice`. Not part of the public DX
 * contract — never read this directly.
 */
export const NATIVE_HANDLE: unique symbol = Symbol("mediaway.device.nativeHandle");

export interface VideoFrame {
  pts: number;
  duration: number;
  width: number;
  height: number;
  pixelFormat: "nv12" | "bgra8";
  data: Buffer;
}

export interface AudioFrame {
  pts: number;
  sampleRate: number;
  channels: number;
  data: Buffer; // raw interleaved f32le PCM
}

/** No capture device/backend is available, or the ABI rejects this
 * configuration as unsupported (today: Screen capture) — expected outcomes,
 * not hard failures. */
export class CaptureUnavailableError extends MediawayError {}

function checkDevice(status: number): void {
  if (status === 0) return;
  if (status === 4 || status === 6 || status === 8) {
    throw new CaptureUnavailableError(status, "no capture backend or device available");
  }
  if (status === 3) {
    throw new CaptureUnavailableError(status, "this capture configuration is unsupported by the ABI");
  }
  const names: Record<number, string> = {
    1: "invalid argument",
    2: "handle poisoned by an earlier panic",
    5: "bad capture config",
    7: "session already closed or not open",
    9: "unknown error",
    10: "internal panic (handle poisoned)",
    11: "callback already registered",
    12: "callback mode active (poll disabled)",
    13: "timed out waiting for a frame",
  };
  throw new MediawayError(status, names[status] ?? "unknown device error");
}

// ── Camera ─────────────────────────────────────────────────────────────────────

/**
 * A Camera capture session. Negotiated geometry/format are authoritative over
 * the request — read them from the session properties.
 */
export class CameraSession {
  readonly width: number;
  readonly height: number;
  readonly pixelFormat: "nv12";
  readonly timeBase: Rational;

  private handle: unknown;

  /** Wrap a native camera capture handle. Prefer the openCamera() factory. */
  constructor(handle: unknown, width: number, height: number, timeBase: Rational) {
    this.handle = handle;
    this.width = width;
    this.height = height;
    this.pixelFormat = "nv12";
    this.timeBase = timeBase;
  }

  /** Poll the next frame; null when nothing is ready yet. Sync, never blocks. */
  pollFrame(): VideoFrame | null {
    const raw = {} as RawCameraFrame;
    const has: [boolean] = [false];
    checkDevice(device.cameraPollFrame(this.handle, raw, has));
    if (!has[0]) return null;
    const data = copyBytes(raw.data, raw.data_len);
    device.cameraFrameFree(raw);
    return {
      pts: Number(raw.pts),
      duration: Number(raw.duration),
      width: raw.width,
      height: raw.height,
      pixelFormat: "nv12",
      data,
    };
  }

  /** Joins the backend worker thread — can block up to one frame interval. */
  async close(): Promise<void> {
    if (this.handle) {
      checkDevice(device.cameraClose(this.handle));
      this.handle = null;
    }
  }

  /** @internal */
  [NATIVE_HANDLE](): unknown {
    return this.handle;
  }
}

/** Open camera `index` at `timeBase`; throws CaptureUnavailableError when no
 * camera exists on this machine. */
export async function openCamera(options: { index: number; timeBase: Rational }): Promise<CameraSession> {
  const config = device.cameraConfigDefault(
    options.index,
    { num: BigInt(options.timeBase.num), den: options.timeBase.den }
  );
  const out: [unknown] = [null];
  checkDevice(device.cameraOpen(config, out));
  if (!out[0]) throw new MediawayError(9, "capture open returned no handle");
  const width: [number] = [0];
  const height: [number] = [0];
  checkDevice(device.cameraGeometry(out[0], width, height));
  return new CameraSession(out[0], width[0], height[0], options.timeBase);
}

// ── Microphone ─────────────────────────────────────────────────────────────────

/** A Microphone capture session (raw interleaved f32le PCM). */
export class MicSession {
  readonly sampleRate: number;
  readonly channels: number;

  private handle: unknown;

  /** Wrap a native mic capture handle. Prefer the openMicrophone() factory. */
  constructor(handle: unknown, sampleRate: number, channels: number) {
    this.handle = handle;
    this.sampleRate = sampleRate;
    this.channels = channels;
  }

  /** Poll the next PCM chunk; null when nothing is ready yet. */
  pollFrame(): AudioFrame | null {
    const raw = {} as RawAudioFrame;
    const has: [boolean] = [false];
    checkDevice(device.audioPollFrame(this.handle, raw, has));
    if (!has[0]) return null;
    const data = copyBytes(raw.data, raw.data_len);
    device.audioFrameFree(raw);
    return { pts: Number(raw.pts), sampleRate: raw.sample_rate, channels: raw.channels, data };
  }

  /** Joins the backend worker thread — can block up to one period interval. */
  async close(): Promise<void> {
    if (this.handle) {
      checkDevice(device.audioClose(this.handle));
      this.handle = null;
    }
  }
}

/** Open the microphone at `sampleRate` Hz; throws CaptureUnavailableError
 * when no mic/backend exists. */
export async function openMicrophone(options: { sampleRate: number; channels?: number }): Promise<MicSession> {
  const config = device.audioConfigMicrophone({ num: 1n, den: options.sampleRate });
  const out: [unknown] = [null];
  checkDevice(device.audioOpen(config, out));
  if (!out[0]) throw new MediawayError(9, "capture open returned no handle");
  const rate: [number] = [0];
  const channels: [number] = [0];
  checkDevice(device.audioFormat(out[0], rate, channels));
  return new MicSession(out[0], rate[0], channels[0]);
}

// ── GPU device factory (mediaway-device ADR-0007) ──────────────────────────────
// The one place a caller with no pre-existing GPU device (every Node caller —
// there is no "bring your own device" path from JS) gets a real device for
// Screen capture or GPU-input encode (AutoVideoEncodeConfig.gpuDevice).

export interface GpuAdapterInfo {
  /** Pass to `GpuDevice.create({ adapterIndex })` to select this exact adapter. */
  index: number;
  name: string;
  vendorId: number;
  deviceId: number;
  dedicatedVideoMemoryBytes: number;
  /** `false` for a software rasterizer adapter (e.g. WARP). */
  isHardware: boolean;
}

/** List every GPU adapter this machine's DXGI factory reports. */
export function listGpuAdapters(): GpuAdapterInfo[] {
  const outAdapters: [unknown] = [null];
  const outCount: [number] = [0];
  checkDevice(device.gpuAdapterList(outAdapters, outCount));
  const count = outCount[0];
  const decoded = decodeArray<RawGpuAdapterInfo>(outAdapters[0], MwGpuAdapterInfo, count);
  const result = decoded.map((raw) => ({
    index: raw.index,
    name: raw.name,
    vendorId: raw.vendor_id,
    deviceId: raw.device_id,
    dedicatedVideoMemoryBytes: Number(raw.dedicated_video_memory),
    isHardware: raw.is_hardware,
  }));
  if (count > 0) device.gpuAdapterListFree(outAdapters[0], count);
  return result;
}

export interface GpuDeviceOptions {
  /** Explicit adapter (from `listGpuAdapters()`'s `index`); omit for the first
   * hardware adapter DXGI reports. */
  adapterIndex?: number;
  /** D3D11_CREATE_DEVICE_VIDEO_SUPPORT. Default true. */
  videoSupport?: boolean;
  /** D3D11_CREATE_DEVICE_DEBUG. Default false. */
  debugLayer?: boolean;
}

/**
 * A real GPU device, created via the factory (no "bring your own device" path
 * exists from JS). Pass into `openScreenCapture({ gpuDevice })` and/or
 * `AutoVideoEncodeConfig.gpuDevice` (`@mediaway/encoder`) to share one device
 * across capture and encode — Zero-Copy end to end. Close it after every
 * session built from it is closed.
 */
export class GpuDevice {
  private handle: unknown;

  /** Wrap a native GPU device handle. Prefer the create() factory. */
  private constructor(handle: unknown) {
    this.handle = handle;
  }

  /** Create a real GPU device. Throws CaptureUnavailableError when no usable
   * adapter exists (e.g. headless CI without even WARP). */
  static async create(options: GpuDeviceOptions = {}): Promise<GpuDevice> {
    const raw = {
      adapter: {
        kind: options.adapterIndex === undefined ? 0 : 1,
        index: options.adapterIndex ?? 0,
      },
      video_support: options.videoSupport ?? true,
      debug_layer: options.debugLayer ?? false,
    };
    const out: [unknown] = [null];
    checkDevice(device.gpuDeviceCreate(raw, out));
    if (!out[0]) throw new MediawayError(9, "GPU device create returned no handle");
    return new GpuDevice(out[0]);
  }

  /** Release the underlying GPU device. Every session built from it (Screen
   * capture, encode config) becomes invalid the moment this returns. */
  close(): void {
    if (this.handle) {
      device.gpuDeviceClose(this.handle);
      this.handle = null;
    }
  }

  /** @internal — the `mediaway_gpu_device_handle_t` value bits, read once at
   * create() time. Consumed by openScreenCapture() and
   * AutoVideoEncodeConfig.gpuDevice (@mediaway/encoder). Not part of the
   * public DX contract. */
  [NATIVE_HANDLE](): RawGpuDeviceHandle {
    const out = {} as RawGpuDeviceHandle;
    checkDevice(device.gpuDeviceHandle(this.handle, out));
    return out;
  }
}

// ── Screen ───────────────────────────────────────────────────────────────────

/**
 * A Screen capture session — GPU-only, Zero-Copy (`adr/0003-gpu-handle-c-abi.md`
 * §4: no CPU fallback exists in the wrapped Rust backend, so unlike Camera
 * there is no CPU pixel readback path here either). `pollFrame()` proves
 * frames are genuinely arriving (real `pts`/geometry) but its `VideoFrame.data`
 * is always empty — it does not copy pixels out. For real pixel data, feed
 * the session straight into the encoder with
 * `EncodeSession.writeFrameFromDesktopCapture()` (`@mediaway/encoder`), which
 * moves the GPU texture Zero-Copy with no CPU round trip at all.
 */
export class ScreenSession {
  readonly width: number;
  readonly height: number;
  readonly pixelFormat = "bgra8" as const;
  readonly timeBase: Rational;

  private handle: unknown;
  private readonly ownsGpuDevice: GpuDevice | undefined;

  /** Wrap a native desktop capture handle. Prefer the openScreenCapture() factory. */
  constructor(
    handle: unknown,
    width: number,
    height: number,
    timeBase: Rational,
    ownsGpuDevice: GpuDevice | undefined
  ) {
    this.handle = handle;
    this.width = width;
    this.height = height;
    this.timeBase = timeBase;
    this.ownsGpuDevice = ownsGpuDevice;
  }

  /** Poll the next frame; null when nothing is ready yet. Sync, never blocks.
   * `data` is always empty — see the class doc. */
  pollFrame(): VideoFrame | null {
    const raw = {} as RawDesktopFrame;
    const has: [boolean] = [false];
    checkDevice(device.desktopPollFrame(this.handle, raw, has));
    if (!has[0]) return null;
    // storage_kind is always GPU (1) for Screen — the frame lives in
    // gpu_buffer, a BORROWED handle released below (never freed via
    // desktopFrameFree, which would double-release it; there is nothing to
    // copy out of raw.data/data_len, which stay empty for GPU storage).
    checkDevice(device.desktopReleaseFrame(this.handle));
    return {
      pts: Number(raw.pts),
      duration: Number(raw.duration),
      width: raw.width,
      height: raw.height,
      pixelFormat: "bgra8",
      data: Buffer.alloc(0),
    };
  }

  /** Joins the backend worker thread — can block up to one frame interval.
   * Also closes the GPU device this session created internally (auto mode);
   * a caller-supplied device (`options.gpuDevice`) is left open — the caller
   * owns it and must close it themselves. */
  async close(): Promise<void> {
    if (this.handle) {
      checkDevice(device.desktopClose(this.handle));
      this.handle = null;
    }
    this.ownsGpuDevice?.close();
  }

  /** @internal */
  [NATIVE_HANDLE](): unknown {
    return this.handle;
  }
}

/**
 * Open Screen capture for output `monitorIndex` (default the primary
 * display). Creates a `GpuDevice` internally unless `options.gpuDevice` is
 * supplied — pass one in to pick an explicit adapter
 * (`GpuDevice.create({ adapterIndex })`) or to share one device across
 * capture and encode. Throws `CaptureUnavailableError` when no usable
 * GPU/Desktop-Duplication path exists.
 */
export async function openScreenCapture(options: {
  timeBase: Rational;
  monitorIndex?: number;
  gpuDevice?: GpuDevice;
}): Promise<ScreenSession> {
  const ownsGpuDevice = options.gpuDevice === undefined ? await GpuDevice.create() : undefined;
  const gpuDevice = options.gpuDevice ?? ownsGpuDevice;
  if (gpuDevice === undefined) throw new MediawayError(9, "no GPU device available");
  try {
    const config = device.desktopConfigScreen(
      options.monitorIndex ?? 0,
      { num: BigInt(options.timeBase.num), den: options.timeBase.den },
      gpuDevice[NATIVE_HANDLE]()
    );
    const out: [unknown] = [null];
    checkDevice(device.desktopOpen(config, out));
    if (!out[0]) throw new MediawayError(9, "capture open returned no handle");
    const width: [number] = [0];
    const height: [number] = [0];
    checkDevice(device.desktopGeometry(out[0], width, height));
    return new ScreenSession(out[0], width[0], height[0], options.timeBase, ownsGpuDevice);
  } catch (err) {
    ownsGpuDevice?.close();
    throw err;
  }
}
