/**
 * @mediaway/encoder — pipeline capability: auto video encode -> fragmented MP4.
 *
 * Implements the DX contract in bindings/nodejs/README.md over the
 * mediaway-ffi C ABI (via @mediaway/ffi). `openAutoEncoder()` picks
 * the best available OS/GPU encoder for the config and returns an
 * `EncodeSession` directly; `finish()` is terminal. Video only — there is no
 * audio encoder in the ABI.
 */

import {
  MwAudioEncodeConfig,
  MwAudioFrameView,
  MwAudioPacket,
  MwAudioStreamInfo,
  MwEncoderCapability,
  MwPipelineFrame,
  MwRational,
  PIPELINE_CODEC,
  pipeline,
  copyBytes,
  decodeArray,
  type RawAudioEncodeConfig,
  type RawAudioFrameView,
  type RawAudioPacket,
  type RawAudioStreamInfo,
  type RawEncoderCapability,
  type RawPipelineFrame,
  type RawRational,
} from "@mediaway/ffi";
import { MediawayError, type Rational } from "@mediaway/container";
import { NATIVE_HANDLE, type CameraSession, type DesktopCaptureSession, type GpuDevice } from "@mediaway/device";

export type { Rational } from "@mediaway/container";
export { MediawayError } from "@mediaway/container";

export type PixelFormat = "nv12" | "bgra8" | "rgba8" | "i420" | "yuy2";
export type VideoCodec = "h264" | "hevc" | "av1" | "vp9";

export interface VideoFrame {
  /** Ticks of the session's timeBase. */
  pts: number;
  duration: number;
  width: number;
  height: number;
  pixelFormat: PixelFormat;
  data: Buffer;
}

/** No encode backend is openable for this config — expected on machines
 * without a usable encoder; catch it and exit gracefully. */
export class EncoderUnavailableError extends MediawayError {}

export class AutoVideoEncodeConfig {
  codec: VideoCodec;
  width: number;
  height: number;
  timeBase: Rational;
  bitrateBps?: number; // 0 / undefined = backend default
  pixelFormat?: PixelFormat;
  /** Zero-Copy GPU input: the same device a Screen capture session was opened
   * with (or any `GpuDevice`) — set together with `pixelFormat: "bgra8"` for
   * `writeFrameFromDesktopCapture()`'s GPU frames (DXGI delivers BGRA8, not
   * the NV12 default). Leave unset for CPU input (Camera, or manual
   * `writeFrame()` with CPU-storage frames). */
  gpuDevice?: GpuDevice;

  private constructor(codec: VideoCodec, width: number, height: number, timeBase: Rational) {
    this.codec = codec;
    this.width = width;
    this.height = height;
    this.timeBase = timeBase;
  }

  /** The ABI's defaults (backend-default bitrate, NV12 input, CPU-only). */
  static defaults(codec: VideoCodec, width: number, height: number, timeBase: Rational): AutoVideoEncodeConfig {
    return new AutoVideoEncodeConfig(codec, width, height, timeBase);
  }

  toAbi(): RawRational {
    const codecMap: Record<string, number> = { h264: 0, hevc: 1, av1: 2, vp9: 3 };
    const raw = pipeline.encConfigNew(
      codecMap[this.codec] ?? 0,
      this.width,
      this.height,
      { num: BigInt(this.timeBase.num), den: this.timeBase.den }
    );
    if (this.bitrateBps !== undefined) raw.bitrate_bps = this.bitrateBps;
    const fmtMap: Record<string, number> = { nv12: 0, i420: 1, bgra8: 2, rgba8: 3, yuy2: 4 };
    if (this.pixelFormat !== undefined) raw.pixel_format = fmtMap[this.pixelFormat] ?? 0;
    if (this.gpuDevice !== undefined) raw.gpu_device = this.gpuDevice[NATIVE_HANDLE]();
    return raw;
  }
}

export function checkPipeline(
  status: number,
  noBackendError: new (status: number, message: string) => MediawayError = EncoderUnavailableError
): void {
  if (status === 0) return;
  if (status === 3) throw new noBackendError(status, "no backend compiled in or openable");
  const names: Record<number, string> = {
    1: "invalid argument",
    2: "handle poisoned by an earlier panic",
    4: "codec/pixel-format/geometry not supported",
    5: "bad dimensions, rates, or frame metadata",
    6: "encoder backend OS/API failure",
    7: "session already finished or not open",
    8: "muxer rejected the encoder's stream info",
    9: "packet does not match the registered track",
    10: "malformed container data",
    11: "unknown error",
    12: "internal panic (handle poisoned)",
    13: "decoder backend OS/API failure",
    14: "decode session already finished or not open",
  };
  throw new MediawayError(status, names[status] ?? "unknown pipeline error");
}

/**
 * A single-use encode session. `finish()` is terminal — it consumes the
 * session; `close()` after it is a no-op (idempotent), but is required to free
 * the native handle when an error aborts the encode mid-way.
 */
export class EncodeSession {
  readonly codec: VideoCodec;
  readonly width: number;
  readonly height: number;
  readonly timeBase: Rational;

  private handle: unknown;
  private finished = false;

  constructor(config: AutoVideoEncodeConfig, encoderHandle: unknown) {
    this.codec = config.codec;
    this.width = config.width;
    this.height = config.height;
    this.timeBase = config.timeBase;
    // mediaway_encode_session_open consumes the encoder unconditionally.
    const out: [unknown] = [null];
    checkPipeline(pipeline.sessionOpen(encoderHandle, out));
    this.handle = out[0];
  }

  async writeFrame(frame: VideoFrame): Promise<void> {
    const raw: RawPipelineFrame = {
      pts: BigInt(frame.pts),
      duration: BigInt(frame.duration),
      width: frame.width,
      height: frame.height,
      pixel_format: { nv12: 0, i420: 1, bgra8: 2, rgba8: 3, yuy2: 4 }[frame.pixelFormat] ?? 0,
      storage_kind: 0, // CPU
      raw_bytes: frame.data,
      raw_bytes_len: frame.data.length,
      gpu_buffer: { kind: 255, native_a: 0, native_b: 0, subresource: 0, webgpu_texture_id: 0 },
    };
    checkPipeline(pipeline.sessionWriteFrame(this.handle, raw));
  }

  /**
   * Poll one frame from `capture` (a `CameraSession`, `@mediaway/device`) and
   * push it straight into this session — no intermediate `VideoFrame`, no
   * extra copy (`adr/pipeline/0005-capture-encode-bridge-c-abi.md`). Returns
   * `false` when nothing was ready yet (not an error) — mirrors
   * `CameraSession.pollFrame()`'s own null-vs-error shape.
   */
  async writeFrameFromCameraCapture(capture: CameraSession): Promise<boolean> {
    const wrote: [boolean] = [false];
    checkPipeline(
      pipeline.sessionWriteFrameFromCameraCapture(this.handle, capture[NATIVE_HANDLE](), wrote)
    );
    return wrote[0];
  }

  /**
   * Same shape as `writeFrameFromCameraCapture`, for a `ScreenSession` or `WindowSession`
   * (`@mediaway/device`) instead of Camera. GPU frames pass through
   * Zero-Copy: the polled frame's GPU handle moves straight into the encoder
   * with no CPU copy — this is the real way to consume Screen frames (see
   * `ScreenSession.pollFrame()`'s own doc: it never copies pixels out).
   */
  async writeFrameFromDesktopCapture(capture: DesktopCaptureSession): Promise<boolean> {
    const wrote: [boolean] = [false];
    checkPipeline(
      pipeline.sessionWriteFrameFromDesktopCapture(this.handle, capture[NATIVE_HANDLE](), wrote)
    );
    return wrote[0];
  }

  /**
   * Take the fMP4 bytes that are ready now, without ending the session — the streaming exit
   * (`adr/pipeline/0007` §1). Call it as often as you like (after every `writeFrame`, on a timer,
   * or never) and the session's memory stays bounded by the poll cadence instead of growing with
   * the recording: a long capture polled into a file holds a fragment, not the whole file.
   *
   * Resolves to an empty buffer when nothing is ready, which is indistinguishable from "already
   * drained" — track your own running total if you need the difference. Polling never finishes
   * the stream: the last fragments only appear in `finish()`'s result.
   */
  async pollBytes(): Promise<Buffer> {
    if (!this.handle) throw new MediawayError(7, "session already finished or not open");
    const outData: [unknown] = [null];
    const outLen: [number] = [0];
    checkPipeline(pipeline.sessionPollBytes(this.handle, outData, outLen));
    if (outLen[0] === 0) return Buffer.alloc(0);
    const data = copyBytes(outData[0], outLen[0]);
    pipeline.bufferFree(outData[0], outLen[0]);
    return data;
  }

  /**
   * Flush the encoder + muxer and return the fMP4 bytes **not yet taken by `pollBytes()`**:
   * the complete file for a session that was never polled, only its tail for one that was.
   * A streaming caller appends this to what it already wrote. Terminal.
   */
  async finish(): Promise<Buffer> {
    const outData: [unknown] = [null];
    const outLen: [number] = [0];
    // finish consumes the session UNCONDITIONALLY (even on failure) — null
    // the handle before checking so close() cannot double-free it.
    const status = pipeline.sessionFinish(this.handle, outData, outLen);
    this.handle = null;
    this.finished = true;
    checkPipeline(status);
    const data = copyBytes(outData[0], outLen[0]);
    if (outLen[0] > 0) pipeline.bufferFree(outData[0], outLen[0]);
    return data;
  }

  /** Idempotent; no-op after finish(). Frees the native handle on error paths. */
  close(): void {
    if (this.handle) {
      pipeline.sessionClose(this.handle);
      this.handle = null;
    }
  }
}

/** Pick the best available encoder for `config` and open a session on it.
 * Throws EncoderUnavailableError when no backend exists on this machine. */
export async function openAutoEncoder(config: AutoVideoEncodeConfig): Promise<EncodeSession> {
  const raw = config.toAbi();
  const outEncoder: [unknown] = [null];
  checkPipeline(pipeline.autoEncoderOpen(raw, outEncoder));
  if (!outEncoder[0]) throw new MediawayError(11, "encoder open returned no handle");
  return new EncodeSession(config, outEncoder[0]);
}

// ── Capability probe (ABI v7, adr/pipeline/0007 §3) ─────────────────────────────

/** A codec the encoder probe accepts. */
export type ProbeCodec = VideoCodec | "aac" | "opus";

/** Which encode backend a probe row describes. */
export type EncodeBackend = "os" | "nvenc" | "quicksync" | "amf" | "vulkan" | "software" | "unknown";

/** Whether a backend or codec is usable right now. */
export type SupportState = "supported" | "not-implemented" | "no-device" | "unknown";

/** The cheapest data path a supported encoder reached. */
export type EncodePathClass = "zero-copy" | "gpu-copy" | "cpu-upload" | "readback" | "software" | "unknown";

/** One backend row of `encoderSupport()`. */
export interface EncoderCapability {
  backend: EncodeBackend;
  state: SupportState;
  /** The data path cost — `null` unless `state === "supported"`. */
  pathClass: EncodePathClass | null;
}

const BACKENDS: Record<number, EncodeBackend> = {
  0: "os",
  1: "nvenc",
  2: "quicksync",
  3: "amf",
  4: "vulkan",
  5: "software",
};
const PATH_CLASSES: Record<number, EncodePathClass> = {
  1: "zero-copy",
  2: "gpu-copy",
  3: "cpu-upload",
  4: "readback",
  5: "software",
};
const SUPPORT_STATES: Record<number, SupportState> = {
  0: "supported",
  1: "not-implemented",
  2: "no-device",
};

/** Map a `mediaway_support_state_t` value to its name (shared with `@mediaway/decoder`). */
export function supportStateName(state: number): SupportState {
  return SUPPORT_STATES[state] ?? "unknown";
}

/**
 * Probe every encode backend for `codec` **at `width` x `height`**.
 *
 * Encoder support is resolution-dependent — a hardware encoder has minimum and maximum
 * dimensions, so a backend that works at one size says nothing about another — which is why there
 * is deliberately no resolution-free form. Pass the size you will encode.
 *
 * **Costly:** it opens a throwaway session per backend (a real MFT / VA-API / VideoToolbox
 * session each). Call it when a settings screen opens, never per frame or in a loop. A platform
 * with no per-backend selection reports an empty list.
 *
 * @throws MediawayError status 5 for a zero width or height.
 */
export async function encoderSupport(codec: ProbeCodec, width: number, height: number): Promise<EncoderCapability[]> {
  const outRows: [unknown] = [null];
  const outCount: [number] = [0];
  checkPipeline(pipeline.encoderSupportAt(PIPELINE_CODEC[codec], width, height, outRows, outCount));
  const count = outCount[0];
  if (count === 0) return [];
  const rows = decodeArray<RawEncoderCapability>(outRows[0], MwEncoderCapability, count);
  const result = rows.map((row): EncoderCapability => ({
    backend: BACKENDS[row.backend] ?? "unknown",
    state: supportStateName(row.state),
    pathClass: row.state === 0 ? (PATH_CLASSES[row.path_class] ?? "unknown") : null,
  }));
  pipeline.encoderSupportFree(outRows[0], count);
  return result;
}

// ── Audio encode (ABI v2, adr/0003) ────────────────────────────────────────────

export type AudioCodec = "aac";
export type SampleFormat = "s16" | "s32" | "f32";

export interface AudioEncodeConfig {
  /** Output codec — "aac" today (the only real backend codec). */
  codec?: AudioCodec;
  /** Input sample rate in Hz — must match the pushed PCM frames. */
  sampleRate: number;
  /** Input channel count — must match the pushed PCM frames (a mono mic is
   * not the AAC sugar's default stereo). */
  channels: number;
  /** Input PCM format — "f32" today. */
  sampleFormat?: SampleFormat;
  /** Sample clock: { num: 1, den: sampleRate } = tick per sample. */
  timeBase?: Rational;
  /** Target bitrate; 0 / undefined = backend default (128 kbps). */
  bitrateBps?: number;
}

export interface AudioStreamInfo {
  codec: AudioCodec;
  sampleRate: number;
  channels: number;
  /** AudioSpecificConfig — register it on the muxer's audio track. */
  extraData: Buffer;
}

export interface AudioPcmFrame {
  /** Sample index in the stream timeBase (frame i starts at i * samplesPerFrame). */
  pts: number;
  /** Sample count; undefined = derived from the chunk length. */
  duration?: number;
  /** Interleaved f32le PCM bytes. */
  data: Buffer;
}

export interface EncodedAudioPacket {
  pts: number; // timeBase ticks
  dts: number;
  duration: number;
  keyframe: boolean;
  /** Owned AAC bytes; freed by this wrapper inside pollPacket(). */
  data: Buffer;
}

/**
 * An opened auto audio encoder — the session IS the encoder (ABI v2,
 * adr/0003): single-step open, no intermediate handle, no consumption trap;
 * `close()` is always safe (idempotent).
 */
export class AudioEncoder {
  readonly sampleRate: number;
  readonly channels: number;
  readonly timeBase: Rational;

  private handle: unknown;

  private constructor(handle: unknown, config: AudioEncodeConfig, timeBase: Rational) {
    this.handle = handle;
    this.sampleRate = config.sampleRate;
    this.channels = config.channels;
    this.timeBase = timeBase;
  }

  /** Open the best available audio encoder. Throws EncoderUnavailableError
   * when no audio backend exists on this machine. */
  static async open(config: AudioEncodeConfig): Promise<AudioEncoder> {
    const tb = config.timeBase ?? { num: 1, den: config.sampleRate };
    const raw: RawAudioEncodeConfig = {
      codec: { aac: 4 }[config.codec ?? "aac"] ?? 4,
      sample_rate: config.sampleRate,
      channels: config.channels,
      sample_format: { s16: 0, s32: 1, f32: 2 }[config.sampleFormat ?? "f32"] ?? 2,
      time_base: { num: BigInt(tb.num), den: tb.den },
      bitrate_bps: config.bitrateBps ?? 0,
    };
    const out: [unknown] = [null];
    checkPipeline(pipeline.audioEncoderOpen(raw, out));
    if (!out[0]) throw new MediawayError(11, "audio encoder open returned no handle");
    return new AudioEncoder(out[0], config, tb);
  }

  /** Push one interleaved f32le PCM chunk (borrowed — copied synchronously). */
  async pushPcm(frame: AudioPcmFrame): Promise<void> {
    const samples = frame.duration ?? frame.data.length / 4 / this.channels;
    const raw: RawAudioFrameView = {
      pts: BigInt(frame.pts),
      duration: BigInt(Math.round(samples)),
      sample_rate: this.sampleRate,
      channels: this.channels,
      sample_format: 2, // F32
      data: frame.data,
      data_len: frame.data.length,
    };
    checkPipeline(pipeline.audioPushPcm(this.handle, raw));
  }

  /** Pull the next encoded packet, if one is ready. null is a valid "nothing
   * ready" result, not an error. */
  async pollPacket(): Promise<EncodedAudioPacket | null> {
    const raw = {} as RawAudioPacket;
    const has: [boolean] = [false];
    checkPipeline(pipeline.audioPollPacket(this.handle, raw, has));
    if (!has[0]) return null;
    const data = copyBytes(raw.payload, raw.payload_len);
    pipeline.pipelinePacketFree(raw);
    return {
      pts: Number(raw.pts),
      dts: Number(raw.dts),
      duration: Number(raw.duration),
      keyframe: raw.is_keyframe,
      data,
    };
  }

  /** Signal end of input; drain the remaining packets with pollPacket(). */
  async flush(): Promise<void> {
    checkPipeline(pipeline.audioFlush(this.handle));
  }

  /** Codec config (AudioSpecificConfig) + negotiated rates — available after
   * the first pushed frame (adr/0003 call order: push, then streamInfo, then
   * mux). */
  async streamInfo(): Promise<AudioStreamInfo> {
    const raw = {} as RawAudioStreamInfo;
    checkPipeline(pipeline.audioStreamInfo(this.handle, raw));
    const extraData = copyBytes(raw.extra_data, raw.extra_data_len);
    pipeline.pipelineStreamInfoFree(raw);
    return {
      codec: (["aac"] as const)[raw.codec - 4] ?? "aac",
      sampleRate: raw.sample_rate,
      channels: raw.channels,
      extraData,
    };
  }

  /** Always safe — no handle-consumption trap on this surface (adr/0003). */
  close(): void {
    if (this.handle) {
      pipeline.audioSessionClose(this.handle);
      this.handle = null;
    }
  }
}
