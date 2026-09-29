/**
 * @mediaway/decoder — pipeline capability: auto video decode + Opus and AAC audio decode.
 *
 * Implements the DX contract in bindings/nodejs/README.md over the
 * mediaway-ffi C ABI (via @mediaway/ffi). Split out of @mediaway/encoder into
 * its own package (previously buried as encoder/src/decode.ts, undiscoverable
 * and undocumented) — decode and encode are peer capabilities, mirroring the
 * Rust crate split (`mediaway-decoder`/`mediaway-encoder`), not one depending
 * on the other. Both sessions mirror `openAutoEncoder()`/`AudioEncoder`'s
 * single-step shape (the handle IS the decoder, no consumption trap);
 * `NO_BACKEND` throws `DecoderUnavailableError`, an expected/graceful outcome.
 */

import {
  PIPELINE_CODEC,
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

export type { Rational } from "@mediaway/container";
export { MediawayError } from "@mediaway/container";

export type PixelFormat = "nv12" | "bgra8" | "rgba8" | "i420" | "yuy2";
export type VideoCodec = "h264" | "hevc" | "av1" | "vp9";

/** No decoder backend is openable for this config — expected on machines
 * without a usable decoder; catch it and exit gracefully. */
export class DecoderUnavailableError extends MediawayError {}

function checkPipeline(
  status: number,
  noBackendError: new (status: number, message: string) => MediawayError = DecoderUnavailableError
): void {
  if (status === 0) return;
  if (status === 3) throw new noBackendError(status, "no backend compiled in or openable");
  const names: Record<number, string> = {
    1: "invalid argument",
    2: "handle poisoned by an earlier panic",
    4: "codec/pixel-format/geometry not supported",
    5: "bad dimensions, rates, codec config (e.g. an empty AAC AudioSpecificConfig) or frame metadata",
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
  /** For Opus audio: an empty payload is the packet-loss-concealment hint for
   * a lost frame, not an error — pass it whenever a frame is known lost. For
   * AAC an empty payload is refused (`MediawayError` status 5): AAC has no
   * such convention. */
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

/** Audio codecs `AudioDecodeSession` opens. */
export type AudioDecodeCodec = "opus" | "aac";

/**
 * Config for `AudioDecodeSession.open()`.
 *
 * **Opus** is the software decoder: identical output on every host, no `extraData`, and
 * `timeBase` (the frame duration, also the per-frame sample cap) is required.
 *
 * **AAC** is the *operating system's* decoder (Media Foundation on Windows, AudioToolbox on
 * macOS/iOS; `DecoderUnavailableError` elsewhere), so its samples **can differ between hosts and
 * OS versions**. `extraData` — the raw AudioSpecificConfig, e.g. an MP4's `esds`
 * DecoderSpecificInfo — is **required**: an empty one is refused (`MediawayError` status 5)
 * because a synthesised default would decode SBR/PS streams to quietly wrong output. Raw AAC
 * only; strip ADTS headers first (`@mediaway/container`'s ADTS demuxer). `timeBase` defaults to
 * `1 / sampleRate`, so packet and frame `pts`/`duration` are sample counts. The Apple decode path
 * is compile-checked but has not been run yet.
 */
export interface AudioDecodeConfig {
  codec: AudioDecodeCodec;
  sampleRate: number;
  channels: number;
  timeBase?: Rational;
  /** AAC: the AudioSpecificConfig. Ignored for Opus. */
  extraData?: Buffer;
}

/**
 * An audio decode session (Opus or AAC) — the handle IS the decoder (adr/pipeline/0006 and
 * adr/pipeline/0007, mirrors `DecodeSession`'s video shape; no muxer to wire, no consumption
 * trap). Opus is cross-platform (mediaway-sw, no OS dependency); AAC needs the OS decoder — see
 * `AudioDecodeConfig`, and `decoderSupport("aac")` to learn whether it exists here.
 */
export class AudioDecodeSession {
  readonly codec: AudioDecodeCodec;
  readonly sampleRate: number;
  readonly channels: number;

  private handle: unknown;

  private constructor(handle: unknown, codec: AudioDecodeCodec, sampleRate: number, channels: number) {
    this.handle = handle;
    this.codec = codec;
    this.sampleRate = sampleRate;
    this.channels = channels;
  }

  /** Open an audio decode session. Throws DecoderUnavailableError when no
   * decode backend exists on this machine. */
  static async open(config: AudioDecodeConfig): Promise<AudioDecodeSession>;
  /** Open an Opus decode session (the original positional form). */
  static async open(sampleRate: number, channels: number, timeBase: Rational): Promise<AudioDecodeSession>;
  static async open(
    first: AudioDecodeConfig | number,
    channels?: number,
    timeBase?: Rational
  ): Promise<AudioDecodeSession> {
    const config: AudioDecodeConfig =
      typeof first === "number"
        ? { codec: "opus", sampleRate: first, channels: channels ?? 0, timeBase }
        : first;
    const tb = config.timeBase ?? (config.codec === "aac" ? { num: 1, den: config.sampleRate } : undefined);
    if (!tb) throw new MediawayError(5, "Opus decode needs a timeBase (the frame duration)");
    const rawTimeBase = { num: BigInt(tb.num), den: tb.den } satisfies RawRational;

    const raw: RawAudioDecodeConfig =
      config.codec === "aac"
        ? pipeline.audioDecodeConfigAac(config.sampleRate, config.channels, rawTimeBase, null, 0)
        : pipeline.audioDecodeConfigOpus(config.sampleRate, config.channels, rawTimeBase);
    if (config.codec === "aac" && config.extraData && config.extraData.length > 0) {
      // The ASC is BORROWED by native code for the open() call only. Assigning the Buffer here
      // (rather than passing it to the constructor) lets koffi pin it for exactly that call.
      raw.extra_data = config.extraData;
      raw.extra_data_len = config.extraData.length;
    }
    const out: [unknown] = [null];
    checkPipeline(pipeline.audioDecodeSessionOpen(raw, out), DecoderUnavailableError);
    if (!out[0]) throw new MediawayError(11, "audio decode session open returned no handle");
    return new AudioDecodeSession(out[0], config.codec, config.sampleRate, config.channels);
  }

  /** Push one compressed packet. For Opus an empty payload is the
   * packet-loss-concealment hint for a lost frame, not an error; for AAC an
   * empty payload is refused (status 5). May produce zero or more frames
   * (drain via pollFrame()). */
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

// ── Capability probe (ABI v7, adr/pipeline/0007 §3) ─────────────────────────────

/** A codec `decoderSupport()` accepts. */
export type DecodeProbeCodec = VideoCodec | "aac" | "opus";

/** Whether decoding a codec is usable right now. */
export type DecodeSupportState = "supported" | "not-implemented" | "no-device" | "unknown";

const SUPPORT_STATES: Record<number, DecodeSupportState> = {
  0: "supported",
  1: "not-implemented",
  2: "no-device",
};

/**
 * Whether decoding `codec` is usable on this machine right now.
 *
 * Decode has one implementation per platform, so this is one state, not a list. It is how a
 * caller learns whether AAC decode exists here (`"aac"`) without opening a session.
 *
 * **Costly:** it opens a throwaway session. Call it when a settings screen opens, never per
 * frame or in a loop.
 */
export async function decoderSupport(codec: DecodeProbeCodec): Promise<DecodeSupportState> {
  const out: [number] = [255];
  checkPipeline(pipeline.decoderSupport(PIPELINE_CODEC[codec], out));
  return SUPPORT_STATES[out[0]] ?? "unknown";
}
