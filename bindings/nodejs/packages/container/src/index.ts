/**
 * @mediaway/container — container capability: sans-io fragmented-MP4 mux + demux.
 *
 * Implements the DX contract in bindings/nodejs/README.md over the
 * mediaway-ffi C ABI (via @mediaway/ffi). Typed structs in, typed
 * structs out; the muxer never touches files — the caller owns byte I/O.
 * `pts`/`duration` are integer ticks of each track's `timeBase`.
 */

import {
  MwPacket,
  MwPacketView,
  MwRational,
  MwStreamInfo,
  MwVideoTrackInfo,
  MwAudioTrackInfo,
  container,
  copyBytes,
  type RawAudioTrackInfo,
  type RawPacket,
  type RawPacketView,
  type RawStreamInfo,
  type RawVideoTrackInfo,
} from "@mediaway/ffi";

export interface Rational {
  num: number;
  den: number;
}

export type VideoCodec = "h264" | "hevc" | "av1" | "vp9" | "vp8";
export type AudioCodec = "aac" | "opus" | "mp3" | "raw_audio";
export type PixelFormat = "nv12" | "bgra8" | "rgba8" | "i420" | "yuy2";

/** Which format `Muxer`/`Demuxer` open — mirrors mediaway_container_format_t.
 * Only formats sharing MP4's multi-track, typestated shape are reachable
 * here; Ogg/ADTS/FLV/MPEG-TS/MP3/WAV have their own dedicated classes. */
export type ContainerFormat = "mp4" | "webm";
const FORMAT_TO_ABI: Record<ContainerFormat, number> = { mp4: 0, webm: 1 };

export interface VideoTrackInfo {
  codec: VideoCodec;
  width: number;
  height: number;
  timeBase: Rational;
  pixelFormat?: PixelFormat;
  /** Codec config (avcC etc.); empty = placeholder written. */
  extraData?: Buffer;
}

export interface AudioTrackInfo {
  codec: AudioCodec;
  sampleRate: number;
  channels: number;
  timeBase: Rational;
  bitDepth?: number;
  /** Codec config (AudioSpecificConfig); empty = placeholder written. */
  extraData?: Buffer;
}

export type TrackInfo =
  | ({ type: "video" } & VideoTrackInfo)
  | ({ type: "audio" } & AudioTrackInfo);

export interface Packet {
  /** Index into `Demuxer.streams()` (demux side) / track index (mux side). */
  trackIndex: number;
  data: Buffer;
  /** Ticks of the track's timeBase. */
  pts: number;
  /** Ticks of the track's timeBase. */
  duration: number;
  /** Sync sample / keyframe. */
  key?: boolean;
}

/** Raw per-crate ABI status from mediaway-ffi (mediaway_status_t). */
export class MediawayError extends Error {
  readonly status: number;
  constructor(status: number, message?: string) {
    super(message ?? `mediaway error (status ${status})`);
    this.status = status;
  }
}

export const CODEC_TO_ABI: Record<string, number> = {
  h264: 0,
  hevc: 1,
  av1: 2,
  vp9: 3,
  aac: 4,
  opus: 5,
  mp3: 6,
  vorbis: 7,
  raw_audio: 11,
  vp8: 12,
};
export const ABI_TO_CODEC: Record<number, string> = Object.fromEntries(
  Object.entries(CODEC_TO_ABI).map(([k, v]) => [v, k])
);

export function check(status: number): void {
  if (status === 0) return;
  const names: Record<number, string> = {
    1: "invalid argument",
    2: "invalid state (typestate violation)",
    3: "invalid or duplicate track id",
    4: "packet does not match a registered track",
    5: "truncated or malformed container data",
    6: "unknown error",
    7: "internal panic (handle poisoned)",
    8: "handle poisoned by an earlier panic",
    9: "codec has no encoding in this container format",
    10: "packet's stream id matches no registered track",
  };
  throw new MediawayError(status, names[status] ?? "unknown container error");
}

// ── Muxer ──────────────────────────────────────────────────────────────────────

/**
 * A muxer in the track-registration (Open) state. Stream indices are assigned
 * in registration order starting at 1: the first `add*Track` call returns 1,
 * the second 2 — not 0, since WebM/Matroska's TrackNumber element must not
 * be 0 (MP4 tolerates 0, but there is no reason to special-case it).
 * `begin()` makes the muxer live; after it, track registration fails.
 */
export class Muxer {
  private handle: unknown;
  private nextIndex = 1;

  constructor(format: ContainerFormat = "mp4") {
    this.handle =
      format === "mp4" ? container.muxerCreate() : container.muxerCreateForFormat(FORMAT_TO_ABI[format]);
    if (!this.handle) throw new MediawayError(7, "muxer creation panicked");
  }

  addVideoTrack(info: VideoTrackInfo): number {
    const index = this.nextIndex++;
    const raw: RawVideoTrackInfo = {
      id: index,
      codec: CODEC_TO_ABI[info.codec] ?? 0,
      time_base: { num: BigInt(info.timeBase.num), den: info.timeBase.den },
      width: info.width,
      height: info.height,
      extra_data: info.extraData ?? null,
      extra_data_len: info.extraData?.length ?? 0,
    };
    check(container.muxerAddVideoTrack(this.handle, raw));
    return index;
  }

  addAudioTrack(info: AudioTrackInfo): number {
    const index = this.nextIndex++;
    const raw: RawAudioTrackInfo = {
      id: index,
      codec: CODEC_TO_ABI[info.codec] ?? 4,
      time_base: { num: BigInt(info.timeBase.num), den: info.timeBase.den },
      sample_rate: info.sampleRate,
      channels: info.channels,
      extra_data: info.extraData ?? null,
      extra_data_len: info.extraData?.length ?? 0,
    };
    check(container.muxerAddAudioTrack(this.handle, raw));
    return index;
  }

  /**
   * Make the muxer live and return the bytes queued so far (the init segment,
   * ftyp + moov — may be empty until the first media bytes are flushed; the
   * ABI emits the init segment lazily with the first fragment).
   */
  begin(): Buffer {
    check(container.muxerBegin(this.handle));
    return this.pollBytes();
  }

  /** Push one packet (sync; the muxer never blocks on I/O). */
  push(packet: Packet): void {
    const raw: RawPacketView = {
      stream_id: packet.trackIndex,
      pts: BigInt(packet.pts),
      dts: BigInt(packet.pts), // dts defaults to pts (no B-frames in the DX contract)
      duration: BigInt(packet.duration),
      is_keyframe: packet.key ?? false,
      is_discard: false,
      payload: packet.data,
      payload_len: packet.data.length,
    };
    check(container.muxerPushPacket(this.handle, raw));
  }

  /** End of input — finalizes the last fragment. */
  flush(): void {
    check(container.muxerFlush(this.handle));
  }

  /** Drain bytes queued since the last call; empty Buffer when nothing is pending. */
  pollBytes(): Buffer {
    const outData: [unknown] = [null];
    const outLen: [number] = [0];
    check(container.muxerPollBytes(this.handle, outData, outLen));
    const data = copyBytes(outData[0], outLen[0]);
    if (outLen[0] > 0) container.bufferFree(outData[0], outLen[0]);
    return data;
  }

  close(): void {
    if (this.handle) {
      container.muxerClose(this.handle);
      this.handle = null;
    }
  }
}

// ── Demuxer ────────────────────────────────────────────────────────────────────

/**
 * A streaming demuxer: feed container bytes, poll streams and packets.
 * `packet.trackIndex` indexes the `streams()` list.
 */
export class Demuxer {
  private handle: unknown;
  private streamIndexById = new Map<number, number>();

  constructor(format: ContainerFormat = "mp4") {
    this.handle =
      format === "mp4" ? container.demuxerCreate() : container.demuxerCreateForFormat(FORMAT_TO_ABI[format]);
    if (!this.handle) throw new MediawayError(7, "demuxer creation panicked");
  }

  pushBytes(bytes: Buffer): void {
    check(container.demuxerPushBytes(this.handle, bytes, bytes.length));
  }

  /** Streams discovered so far; empty until the init segment has been parsed. */
  streams(): TrackInfo[] {
    const count = container.demuxerStreamCount(this.handle);
    const out: TrackInfo[] = [];
    this.streamIndexById.clear();
    for (let i = 0; i < count; i++) {
      const raw = {} as RawStreamInfo;
      check(container.demuxerStreamAt(this.handle, i, raw));
      const extra = copyBytes(raw.extra_data, raw.extra_data_len);
      container.streamInfoFree(raw);
      this.streamIndexById.set(raw.id, i);
      const common = {
        timeBase: { num: Number(raw.time_base.num), den: raw.time_base.den },
      };
      if (raw.has_geometry) {
        out.push({
          type: "video",
          codec: (ABI_TO_CODEC[raw.codec] ?? "h264") as VideoCodec,
          width: raw.width,
          height: raw.height,
          extraData: extra,
          ...common,
        } satisfies TrackInfo);
      } else {
        out.push({
          type: "audio",
          codec: (ABI_TO_CODEC[raw.codec] ?? "aac") as AudioCodec,
          sampleRate: raw.sample_rate,
          channels: raw.channels,
          extraData: extra,
          ...common,
        } satisfies TrackInfo);
      }
    }
    return out;
  }

  /** The next demuxed packet, or null when the stream is exhausted. */
  pollPacket(): Packet | null {
    const raw = {} as RawPacket;
    const has: [boolean] = [false];
    check(container.demuxerPollPacket(this.handle, raw, has));
    if (!has[0]) return null;
    const payload = copyBytes(raw.payload, raw.payload_len);
    container.packetFree(raw);
    const trackIndex = this.streamIndexById.get(raw.stream_id) ?? raw.stream_id;
    return {
      trackIndex,
      data: payload,
      pts: Number(raw.pts),
      duration: Number(raw.duration),
      key: raw.is_keyframe,
    };
  }

  close(): void {
    if (this.handle) {
      container.demuxerClose(this.handle);
      this.handle = null;
    }
  }
}

// ── The 6 dedicated-handle formats ──────────────────────────────────────────
// Ogg/ADTS/FLV/MPEG-TS/MP3/WAV each need a genuinely different C ABI shape
// from MP4/WebM's Muxer/Demuxer (no track registration, out-buffer-per-call
// mux, or a construction-time stream list) — see each module's own top comment.

export * from "./ogg.js";
export * from "./adts.js";
export * from "./flv.js";
export * from "./ts.js";
export * from "./mp3.js";
export * from "./wav.js";
