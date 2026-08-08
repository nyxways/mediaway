/**
 * @mediaway/encoder — pipeline capability: auto video decode + Opus audio decode.
 *
 * Wraps the mediaway-ffi C ABI's decode sessions (adr/0004-auto-decode-c-abi.md,
 * adr/pipeline/0006-audio-decode-c-abi.md) — the same "C ABI real, no
 * language binding wired" gap the container format series closed for
 * mux/demux, closed here for decode. Both sessions mirror
 * `openAutoEncoder()`/`AudioEncoder`'s single-step shape (the handle IS the
 * decoder, no consumption trap); `NO_BACKEND` throws
 * `DecoderUnavailableError`, an expected/graceful outcome.
 */

import {
  pipeline,
  copyBytes,
  type RawAudioDecodeConfig,
  type RawAutoVideoDecodeConfig,
  type RawDecodedAudioFrame,
  type RawDecodedVideoFrame,
  type RawDecodePacketView,
  type RawRational,
} from "@mediaway/ffi";
import { MediawayError, type Rational } from "@mediaway/container";
import { checkPipeline, type PixelFormat, type VideoCodec } from "./index.js";

/** No decoder backend is openable for this config — expected on machines
 * without a usable decoder; catch it and exit gracefully. */
export class DecoderUnavailableError extends MediawayError {}

export interface VideoDecodeConfig {
  codec: VideoCodec;
  /** Expected; may be refined from the bitstream. */
  width: number;
  height: number;
  timeBase: Rational;
  /** Preferred output format when the backend converts. */
  pixelFormat?: PixelFormat;
  /** AVCC / SPS-PPS codec config, required at open time — not supplied via
   * the first pushed packet (adr/0004 §1: the muxer-track analogy does not
   * hold for the wrapped decoder). */
  extraData?: Buffer;
}

/** Input to `DecodeSession.pushPacket`/`AudioDecodeSession.pushPacket` — a
 * pipeline-scoped packet view, distinct from `@mediaway/container`'s
 * `Packet` (adr/pipeline/0006 §4: shared by both video and audio decode). */
export interface DecodePacket {
  pts: number;
  dts?: number;
  duration?: number;
  keyframe?: boolean;
  /** For audio: an empty payload is Opus's packet-loss-concealment hint for
   * a lost frame, not an error — pass it whenever a frame is known lost. */
  payload: Buffer;
}

export interface DecodedVideoFrame {
  pts: number;
  duration: number;
  width: number;
  height: number;
  pixelFormat: PixelFormat;
  data: Buffer;
}

export interface DecodedAudioFrame {
  pts: number;
  duration: number;
  sampleRate: number;
  channels: number;
  data: Buffer;
}

const codecMap: Record<VideoCodec, number> = { h264: 0, hevc: 1, av1: 2, vp9: 3 };
const pixelFmtMap: Record<PixelFormat, number> = { nv12: 0, i420: 1, bgra8: 2, rgba8: 3, yuy2: 4 };
const pixelFmtRev: PixelFormat[] = ["nv12", "i420", "bgra8", "rgba8", "yuy2"];

/**
 * The best available video decode session for a config — the handle IS the
 * decoder (single-step open, no consumption trap, mirrors `openAutoEncoder`'s
 * `NO_BACKEND` handling). CPU output only (GPU decode output is deferred,
 * adr/0004 §1/§5).
 */
export class DecodeSession {
  private handle: unknown;

  private constructor(handle: unknown) {
    this.handle = handle;
  }

  /** Open the best available video decoder for `config`. Throws
   * DecoderUnavailableError when no decode backend exists on this machine. */
  static async open(config: VideoDecodeConfig): Promise<DecodeSession> {
    const extraData = config.extraData ?? Buffer.alloc(0);
    const raw: RawAutoVideoDecodeConfig = pipeline.decodeConfigNew(
      codecMap[config.codec] ?? 0,
      config.width,
      config.height,
      { num: BigInt(config.timeBase.num), den: config.timeBase.den } satisfies RawRational,
      extraData.length > 0 ? extraData : null,
      extraData.length
    );
    if (config.pixelFormat !== undefined) raw.pixel_format = pixelFmtMap[config.pixelFormat] ?? 0;
    const out: [unknown] = [null];
    checkPipeline(pipeline.decodeSessionOpen(raw, out), DecoderUnavailableError);
    if (!out[0]) throw new MediawayError(11, "decode session open returned no handle");
    return new DecodeSession(out[0]);
  }

  /** Push one compressed packet. May produce zero or more frames (drain via
   * `pollFrame()`). */
  async pushPacket(packet: DecodePacket): Promise<void> {
    const raw: RawDecodePacketView = {
      stream_id: 0,
      pts: BigInt(packet.pts),
      dts: BigInt(packet.dts ?? packet.pts),
      duration: BigInt(packet.duration ?? 0),
      is_keyframe: packet.keyframe ?? false,
      is_discard: false,
      payload: packet.payload,
      payload_len: packet.payload.length,
    };
    checkPipeline(pipeline.decodeSessionPushPacket(this.handle, raw));
  }

  /** Pull the next decoded frame, if one is ready. null is a valid "nothing
   * ready" result, not an error. */
  async pollFrame(): Promise<DecodedVideoFrame | null> {
    const raw = {} as RawDecodedVideoFrame;
    const has: [boolean] = [false];
    checkPipeline(pipeline.decodeSessionPollFrame(this.handle, raw, has));
    if (!has[0]) return null;
    const data = copyBytes(raw.data, raw.data_len);
    pipeline.decodedVideoFrameFree(raw);
    return {
      pts: Number(raw.pts),
      duration: Number(raw.duration),
      width: raw.width,
      height: raw.height,
      pixelFormat: pixelFmtRev[raw.pixel_format] ?? "nv12",
      data,
    };
  }

  /** Signal end of input; drain the remaining frames with pollFrame(). */
  async flush(): Promise<void> {
    checkPipeline(pipeline.decodeSessionFlush(this.handle));
  }

  /** Always safe — no handle-consumption trap on this surface. */
  close(): void {
    if (this.handle) {
      pipeline.decodeSessionClose(this.handle);
      this.handle = null;
    }
  }
}

/**
 * An Opus audio decode session — the handle IS the decoder (adr/pipeline/0006,
 * mirrors `DecodeSession`'s video shape; no muxer to wire, no consumption
 * trap). Cross-platform (mediaway-sw, no OS dependency), unlike
 * `DecodeSession`'s Windows-only WMF backend.
 */
export class AudioDecodeSession {
  readonly sampleRate: number;
  readonly channels: number;

  private handle: unknown;

  private constructor(handle: unknown, sampleRate: number, channels: number) {
    this.handle = handle;
    this.sampleRate = sampleRate;
    this.channels = channels;
  }

  /** Open an Opus decode session. Throws DecoderUnavailableError when no
   * decode backend exists on this machine. */
  static async open(sampleRate: number, channels: number, timeBase: Rational): Promise<AudioDecodeSession> {
    const raw: RawAudioDecodeConfig = pipeline.audioDecodeConfigOpus(sampleRate, channels, {
      num: BigInt(timeBase.num),
      den: timeBase.den,
    } satisfies RawRational);
    const out: [unknown] = [null];
    checkPipeline(pipeline.audioDecodeSessionOpen(raw, out), DecoderUnavailableError);
    if (!out[0]) throw new MediawayError(11, "audio decode session open returned no handle");
    return new AudioDecodeSession(out[0], sampleRate, channels);
  }

  /** Push one compressed Opus packet. An empty payload is Opus's
   * packet-loss-concealment hint for a lost frame, not an error. May produce
   * zero or more frames (drain via pollFrame()). */
  async pushPacket(packet: DecodePacket): Promise<void> {
    const raw: RawDecodePacketView = {
      stream_id: 0,
      pts: BigInt(packet.pts),
      dts: BigInt(packet.dts ?? packet.pts),
      duration: BigInt(packet.duration ?? 0),
      is_keyframe: packet.keyframe ?? false,
      is_discard: false,
      payload: packet.payload,
      payload_len: packet.payload.length,
    };
    checkPipeline(pipeline.audioDecodeSessionPushPacket(this.handle, raw));
  }

  /** Pull the next decoded PCM frame, if one is ready. null is a valid
   * "nothing ready" result, not an error. */
  async pollFrame(): Promise<DecodedAudioFrame | null> {
    const raw = {} as RawDecodedAudioFrame;
    const has: [boolean] = [false];
    checkPipeline(pipeline.audioDecodeSessionPollFrame(this.handle, raw, has));
    if (!has[0]) return null;
    const data = copyBytes(raw.data, raw.data_len);
    pipeline.decodedAudioFrameFree(raw);
    return {
      pts: Number(raw.pts),
      duration: Number(raw.duration),
      sampleRate: raw.sample_rate,
      channels: raw.channels,
      data,
    };
  }

  /** Signal end of input; drain the remaining frames with pollFrame(). */
  async flush(): Promise<void> {
    checkPipeline(pipeline.audioDecodeSessionFlush(this.handle));
  }

  /** Always safe — no handle-consumption trap on this surface. */
  close(): void {
    if (this.handle) {
      pipeline.audioDecodeSessionClose(this.handle);
      this.handle = null;
    }
  }
}
