/**
 * @mediaway/encoder — pipeline capability: auto video encode -> fragmented MP4.
 *
 * Implements the DX contract in bindings/nodejs/README.md over the
 * mediaway-pipeline-ffi C ABI (via @mediaway/ffi). `openAutoEncoder()` picks
 * the best available OS/GPU encoder for the config and returns an
 * `EncodeSession` directly; `finish()` is terminal. Video only — there is no
 * audio encoder in the ABI.
 */

import {
  MwAudioEncodeConfig,
  MwAudioFrameView,
  MwAudioPacket,
  MwAudioStreamInfo,
  MwPipelineFrame,
  MwRational,
  pipeline,
  copyBytes,
} from "@mediaway/ffi";
import { MediawayError, type Rational } from "@mediaway/container";

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

  private constructor(codec: VideoCodec, width: number, height: number, timeBase: Rational) {
    this.codec = codec;
    this.width = width;
    this.height = height;
    this.timeBase = timeBase;
  }

  /** The ABI's defaults (backend-default bitrate, NV12 input). */
  static defaults(codec: VideoCodec, width: number, height: number, timeBase: Rational): AutoVideoEncodeConfig {
    return new AutoVideoEncodeConfig(codec, width, height, timeBase);
  }

  toAbi(): typeof MwRational {
    const codecMap: Record<string, number> = { h264: 0, hevc: 1, av1: 2, vp9: 3 };
    const raw = pipeline.encConfigNew(
      codecMap[this.codec] ?? 0,
      this.width,
      this.height,
      { num: BigInt(this.timeBase.num), den: this.timeBase.den }
    );
    if (this.bitrateBps !== undefined) raw.bitrate_bps = this.bitrateBps;
    const fmtMap: Record<string, number> = { nv12: 0, i420: 1, bgra8: 2, rgba8: 3, yuyv: 4 };
    if (this.pixelFormat !== undefined) raw.pixel_format = fmtMap[this.pixelFormat] ?? 0;
    return raw;
  }
}

function checkPipeline(status: number): void {
  if (status === 0) return;
  if (status === 3) throw new EncoderUnavailableError(status, "no encode backend compiled in or openable");
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
    const raw: typeof MwPipelineFrame = {
      pts: BigInt(frame.pts),
      duration: BigInt(frame.duration),
      width: frame.width,
      height: frame.height,
      pixel_format: { nv12: 0, i420: 1, bgra8: 2, rgba8: 3, yuyv: 4 }[frame.pixelFormat] ?? 0,
      storage_kind: 0, // CPU
      raw_bytes: frame.data,
      raw_bytes_len: frame.data.length,
      gpu_buffer: { kind: 255, native_a: 0, native_b: 0, subresource: 0, webgpu_texture_id: 0 },
    };
    checkPipeline(pipeline.sessionWriteFrame(this.handle, raw));
  }

  /** Flush the encoder + muxer and return the complete fMP4 bytes. Terminal. */
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
    const raw: typeof MwAudioEncodeConfig = {
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
    const raw: typeof MwAudioFrameView = {
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
    const raw: typeof MwAudioPacket = {};
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
    const raw: typeof MwAudioStreamInfo = {};
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
