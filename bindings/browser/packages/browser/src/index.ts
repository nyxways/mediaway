/**
 * @mediaway/browser — browser (WASM + WebCodecs) binding for Mediaway.
 *
 * ADR-0020 labor split: the WASM module owns the container (fMP4 mux/demux),
 * the browser's native WebCodecs owns codecs, native Web APIs own capture.
 * No C ABI in this host, ever.
 *
 * Usage:
 *   import { init, Muxer, EncodeSession } from "@mediaway/browser";
 *   await init();                      // fetch + instantiate the WASM module
 *   const muxer = new Muxer(1);
 *   const session = new EncodeSession(muxer);
 *   const enc = await session.video({ codec: "avc1.42E01E", width: 640, height: 360 });
 *   enc.encode(new VideoFrame(nv12, { format: "NV12", ... }));
 *   const mp4 = await session.finish(); // Uint8Array
 */

import initWasm, {
  type InitInput,
  Demuxer as WasmDemuxer,
  JsSample as WasmSample,
  JsTrack as WasmTrack,
  Muxer as WasmMuxer,
} from "../pkg/iso_bmff_wasm.js";

// ── Public types ─────────────────────────────────────────────────────────────

/** Lowercase codec name (mirror of `iso_bmff::Codec`). */
export type Codec = "h264" | "hevc" | "av1" | "vp9" | "aac" | "opus" | "webvtt" | "tx3g";

/** Media timebase rational. */
export interface Rational {
  num: number;
  den: number;
}

/** Track / stream description (mirror of `iso_bmff::Track`). */
export interface Track {
  id: number;
  codec: Codec;
  timeBase: Rational;
  /** Video width (0 for audio). */
  width: number;
  /** Video height (0 for audio). */
  height: number;
  /** Codec config (avcC / AudioSpecificConfig). */
  extraData: Uint8Array;
}

/** One compressed sample (mirror of `iso_bmff::Sample`). */
export interface Sample {
  streamId: number;
  pts: number;
  dts: number;
  duration: number;
  isKeyframe: boolean;
  isDiscard: boolean;
  payload: Uint8Array;
}

/** Expected failure: the browser/device has no usable WebCodecs encoder. */
export class EncoderUnavailableError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "EncoderUnavailableError";
  }
}

// ── init ─────────────────────────────────────────────────────────────────────

let initialized = false;

/**
 * Fetch + instantiate the WASM module. Idempotent; must be called before any
 * other API. `input` defaults to the packaged `iso_bmff_wasm_bg.wasm` (resolved
 * relative to this module); pass an explicit `URL`/`Response`/`Uint8Array` to
 * override (e.g. a CDN URL).
 */
export async function init(input?: InitInput | Promise<InitInput>): Promise<void> {
  if (initialized) return;
  await initWasm(input ?? new URL("../pkg/iso_bmff_wasm_bg.wasm", import.meta.url));
  initialized = true;
}

function requireInit(): void {
  if (!initialized) throw new Error("call await init() before using @mediaway/browser");
}

// ── Muxer ────────────────────────────────────────────────────────────────────

/**
 * Fragmented-MP4 muxer (WASM). Typestate mirror of the Rust core:
 * `new()` → `addTrack` × N → `begin()` → `pushPacket` × N → `flush()` →
 * `pollBytes()`. Call `free()` when done (wasm memory is invisible to JS GC).
 */
export class Muxer {
  private inner: WasmMuxer;

  /** `fragmentBatch` — samples per fMP4 fragment (>= 1). */
  constructor(fragmentBatch = 1) {
    requireInit();
    this.inner = new WasmMuxer(fragmentBatch);
  }

  /** Register a track. Must be called before `begin()`. Returns the track id. */
  addTrack(track: Track): number {
    const wasm = new WasmTrack(
      track.id,
      track.codec,
      BigInt(track.timeBase.num),
      track.timeBase.den,
      track.width,
      track.height,
      track.extraData,
    );
    return this.inner.add_track(wasm);
  }

  /** Lock tracks and enter the live streaming state. */
  begin(): void {
    this.inner.begin();
  }

  /** Push one compressed sample (H.264 Annex-B is auto-converted to AVCC). */
  pushPacket(sample: Sample): void {
    const wasm = new WasmSample(
      sample.streamId,
      BigInt(sample.pts),
      BigInt(sample.dts),
      BigInt(sample.duration),
      sample.isKeyframe,
      sample.isDiscard,
      sample.payload,
    );
    this.inner.push_packet(wasm);
  }

  /** Finalize the current fragment. */
  flush(): void {
    this.inner.flush();
  }

  /** Full accumulated fMP4 output — fresh `Uint8Array` copy each call. */
  pollBytes(): Uint8Array {
    return this.inner.poll_bytes();
  }

  /** Release wasm memory. Safe to call multiple times. */
  free(): void {
    this.inner.free();
  }
}

// ── Demuxer ──────────────────────────────────────────────────────────────────

/**
 * Streaming fragmented-MP4 demuxer (WASM). Call `free()` when done.
 */
export class Demuxer {
  private inner: WasmDemuxer;

  constructor() {
    requireInit();
    this.inner = new WasmDemuxer();
  }

  /** Feed a chunk of fMP4 bytes (streaming — call repeatedly). */
  pushBytes(bytes: Uint8Array): void {
    this.inner.push_bytes(bytes);
  }

  /** Demuxed track descriptions (from `moov`, available once the header is in). */
  streams(): Track[] {
    return this.inner.streams().map((t) => ({
      id: t.id,
      codec: t.codec as Codec,
      timeBase: { num: Number(t.time_base_num), den: t.time_base_den },
      width: t.width,
      height: t.height,
      extraData: t.extra_data,
    }));
  }

  /** Next packet, or `null` when the input is exhausted. */
  pollPacket(): Sample | null {
    const s = this.inner.poll_packet();
    return s === undefined
      ? null
      : {
          streamId: s.stream_id,
          pts: Number(s.pts),
          dts: Number(s.dts),
          duration: Number(s.duration),
          isKeyframe: s.is_keyframe,
          isDiscard: s.is_discard,
          payload: s.payload,
        };
  }

  /** Release wasm memory. Safe to call multiple times. */
  free(): void {
    this.inner.free();
  }
}

// ── WebCodecs glue ───────────────────────────────────────────────────────────

/** Video encoder configuration (WebCodecs `VideoEncoderConfig` + session bits). */
export interface VideoEncodeConfig {
  /** WebCodecs codec string, e.g. `"avc1.42E01E"`, `"vp8"`, `"av01.0.04M.08"`. */
  codec: string;
  width: number;
  height: number;
  /** Bits per second. */
  bitrate?: number;
  /** Frames per second (default 30) — drives sample durations. */
  framerate?: number;
  avc?: { format: "avc" | "annexb" };
}

/** Audio encoder configuration (WebCodecs `AudioEncoderConfig` + session bits). */
export interface AudioEncodeConfig {
  /** WebCodecs codec string, e.g. `"mp4a.40.2"` (AAC-LC) or `"opus"`. */
  codec: string;
  sampleRate: number;
  numberOfChannels: number;
  /** Bits per second. */
  bitrate?: number;
}

interface PendingEncoder {
  trackId: number;
  codec: Codec;
  timeBase: Rational;
  width: number;
  height: number;
  extraData: Uint8Array;
}

/**
 * Encode-to-MP4 session: owns the muxer, one or two WebCodecs encoders, and the
 * track-registration/begun handshake.
 *
 * The WASM muxer writes `moov` (with per-track avcC/AudioSpecificConfig) on the
 * FIRST pushed sample, and WebCodecs only exposes that config in the first
 * output's metadata — so `begin()` is deferred until every planned track has
 * produced its first chunk (samples are queued until then). Call order mirrors
 * the C ABI's push → stream_info → mux (ADR-0003).
 *
 * Usage:
 *   const session = new EncodeSession(new Muxer(1));
 *   const video = await session.video({ codec: "avc1.42E01E", ... });
 *   video.encode(frame);
 *   const mp4 = await session.finish();
 */
export class EncodeSession {
  readonly muxer: Muxer;
  private videoTrack: PendingEncoder | null = null;
  private audioTrack: PendingEncoder | null = null;
  private videoEncoder: VideoEncoder | null = null;
  private audioEncoder: AudioEncoder | null = null;
  private begun = false;
  private queued: Sample[] = [];
  private videoStarted = false;
  private audioStarted = false;

  /** `muxer` — the target WASM muxer (not yet begun; the session begins it). */
  constructor(muxer: Muxer) {
    requireInit();
    this.muxer = muxer;
  }

  /** Create and configure a WebCodecs video encoder wired into this session. */
  async video(config: VideoEncodeConfig): Promise<VideoEncoder> {
    if (this.videoTrack !== null) throw new Error("EncodeSession: video encoder already created");
    if (typeof VideoEncoder === "undefined") {
      throw new EncoderUnavailableError("VideoEncoder is not available in this browser");
    }
    const supported = await VideoEncoder.isConfigSupported(config);
    if (!supported.supported) {
      throw new EncoderUnavailableError(`unsupported video codec config: ${JSON.stringify(config)}`);
    }
    const framerate = config.framerate ?? 30;
    const timeBase: Rational = { num: 1, den: framerate };
    this.videoTrack = {
      trackId: 0,
      codec: "h264",
      timeBase,
      width: config.width,
      height: config.height,
      extraData: new Uint8Array(0),
    };
    const encoder = new VideoEncoder({
      output: (chunk, metadata) => this.onVideoOutput(chunk, metadata),
      error: (err) => {
        throw new EncoderUnavailableError(`video encoder error: ${err.message}`);
      },
    });
    encoder.configure(config);
    this.videoEncoder = encoder;
    return encoder;
  }

  /** Create and configure a WebCodecs audio encoder wired into this session. */
  async audio(config: AudioEncodeConfig): Promise<AudioEncoder> {
    if (this.audioTrack !== null) throw new Error("EncodeSession: audio encoder already created");
    if (typeof AudioEncoder === "undefined") {
      throw new EncoderUnavailableError("AudioEncoder is not available in this browser");
    }
    const supported = await AudioEncoder.isConfigSupported(config);
    if (!supported.supported) {
      throw new EncoderUnavailableError(`unsupported audio codec config: ${JSON.stringify(config)}`);
    }
    const timeBase: Rational = { num: 1, den: config.sampleRate };
    this.audioTrack = {
      trackId: 1,
      codec: "aac",
      timeBase,
      width: 0,
      height: 0,
      extraData: new Uint8Array(0),
    };
    const encoder = new AudioEncoder({
      output: (chunk, metadata) => this.onAudioOutput(chunk, metadata),
      error: (err) => {
        throw new EncoderUnavailableError(`audio encoder error: ${err.message}`);
      },
    });
    encoder.configure(config);
    this.audioEncoder = encoder;
    return encoder;
  }

  /** True once every planned encoder has produced its first chunk. */
  private readyToBegin(): boolean {
    if (this.videoTrack !== null && !this.videoStarted) return false;
    if (this.audioTrack !== null && !this.audioStarted) return false;
    return this.videoTrack !== null || this.audioTrack !== null;
  }

  private ensureBegun(): void {
    if (this.begun) return;
    if (!this.readyToBegin()) return;
    if (this.videoTrack !== null && this.videoTrack.extraData.length === 0) return;
    if (this.audioTrack !== null && this.audioTrack.extraData.length === 0) return;
    if (this.videoTrack !== null) {
      this.muxer.addTrack({
        id: this.videoTrack.trackId,
        codec: this.videoTrack.codec,
        timeBase: this.videoTrack.timeBase,
        width: this.videoTrack.width,
        height: this.videoTrack.height,
        extraData: this.videoTrack.extraData,
      });
    }
    if (this.audioTrack !== null) {
      this.muxer.addTrack({
        id: this.audioTrack.trackId,
        codec: this.audioTrack.codec,
        timeBase: this.audioTrack.timeBase,
        width: 0,
        height: 0,
        extraData: this.audioTrack.extraData,
      });
    }
    this.muxer.begin();
    this.begun = true;
    for (const sample of this.queued) {
      this.muxer.pushPacket(sample);
    }
    this.queued = [];
  }

  private push(sample: Sample): void {
    if (!this.begun) {
      this.queued.push(sample);
      return;
    }
    this.muxer.pushPacket(sample);
  }

  private onVideoOutput(chunk: EncodedVideoChunk, metadata?: EncodedVideoChunkMetadata): void {
    const track = this.videoTrack;
    if (track === null) return; // no session wiring — cannot happen
    if (!this.videoStarted) {
      const description = metadata?.decoderConfig?.description;
      if (description !== undefined) {
        track.extraData = copyBytes(description);
      }
      this.videoStarted = true;
      this.ensureBegun();
    }
    const payload = new Uint8Array(chunk.byteLength);
    chunk.copyTo(payload);
    const pts = Math.round((chunk.timestamp * track.timeBase.den) / 1e6);
    this.push({
      streamId: track.trackId,
      pts,
      dts: pts,
      duration: Math.max(1, Math.round(track.timeBase.den / track.timeBase.num)),
      isKeyframe: chunk.type === "key",
      isDiscard: false,
      payload,
    });
  }

  private onAudioOutput(chunk: EncodedAudioChunk, metadata?: EncodedAudioChunkMetadata): void {
    const track = this.audioTrack;
    if (track === null) return; // no session wiring — cannot happen
    if (!this.audioStarted) {
      const description = metadata?.decoderConfig?.description;
      if (description !== undefined) {
        track.extraData = copyBytes(description);
      }
      this.audioStarted = true;
      this.ensureBegun();
    }
    const payload = new Uint8Array(chunk.byteLength);
    chunk.copyTo(payload);
    const pts = Math.round((chunk.timestamp * track.timeBase.den) / 1e6);
    const duration = chunk.duration === undefined || chunk.duration === null
      ? 1024
      : Math.max(1, Math.round((chunk.duration * track.timeBase.den) / 1e6));
    this.push({
      streamId: track.trackId,
      pts,
      dts: pts,
      duration,
      isKeyframe: chunk.type === "key",
      isDiscard: false,
      payload,
    });
  }

  /**
   * Flush both encoders + the muxer and return the complete fMP4 bytes.
   * Terminal: the session's encoders and muxer are consumed.
   */
  async finish(): Promise<Uint8Array> {
    if (this.videoEncoder !== null) {
      await this.videoEncoder.flush();
      this.videoEncoder.close();
      this.videoEncoder = null;
    }
    if (this.audioEncoder !== null) {
      await this.audioEncoder.flush();
      this.audioEncoder.close();
      this.audioEncoder = null;
    }
    this.muxer.flush();
    return this.muxer.pollBytes();
  }
}

function copyBytes(source: AllowSharedBufferSource): Uint8Array {
  if (source instanceof Uint8Array) return new Uint8Array(source);
  if (source instanceof ArrayBuffer) return new Uint8Array(source);
  // SharedArrayBuffer / ArrayBufferView
  const view = source as ArrayBufferView;
  if (view.byteLength !== undefined) {
    return new Uint8Array(view.buffer as ArrayBuffer, view.byteOffset, view.byteLength);
  }
  return new Uint8Array(source as unknown as ArrayBuffer);
}
