/**
 * @mediaway/device — device capability: camera / screen / microphone capture.
 *
 * Implements the DX contract in bindings/nodejs/README.md over the
 * mediaway-ffi C ABI (via @mediaway/ffi). The ABI is domain-split
 * (`adr/0004-domain-feature-split.md`): this package folds it back into the
 * three DX functions. CPU-storage only: Camera delivers CPU frames; Screen
 * capture raises CaptureUnavailableError from the C ABI today (it needs a
 * live GPU device handle with no CPU fallback). Audio capture delivers raw
 * interleaved f32le PCM — there is no audio encoder in the ABI.
 */

import {
  MwAudioFrame,
  MwAudioConfig,
  MwCameraConfig,
  MwCameraFrame,
  device,
  copyBytes,
  type RawAudioFrame,
  type RawCameraFrame,
} from "@mediaway/ffi";
import { MediawayError, type Rational } from "@mediaway/container";

export type { Rational } from "@mediaway/container";

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

// ── Screen (ideal only) ────────────────────────────────────────────────────────

/**
 * A Screen capture session — NOT available from the C ABI today. Screen needs
 * a live GPU device handle (ID3D11Device*) with no CPU fallback, and its C
 * representation is deferred (crates/mediaway-ffi/adr/0001 § Deferred),
 * so openScreenCapture always throws CaptureUnavailableError. The ideal DX
 * (BGRA8 CPU frames at the display's native geometry) is what the aspirational
 * screen-record example targets.
 */
export class ScreenSession {
  readonly width = 0;
  readonly height = 0;
  readonly pixelFormat = "bgra8" as const;
  readonly timeBase: Rational;

  /** Screen session (always throws — no C-ABI screen path yet). */
  constructor(timeBase: Rational) {
    this.timeBase = timeBase;
  }

  pollFrame(): VideoFrame | null {
    throw new CaptureUnavailableError(3, "Screen capture is not available from this binding today");
  }

  async close(): Promise<void> {}
}

export async function openScreenCapture(options: { timeBase: Rational; monitorIndex?: number }): Promise<ScreenSession> {
  void options;
  throw new CaptureUnavailableError(
    3,
    "Screen capture needs a live GPU device handle with no CPU fallback, and its C representation is deferred — not available from this binding today"
  );
}
