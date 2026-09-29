/**
 * Replay ring — keep the last N milliseconds of encoded packets in memory (or just their
 * locations on disk) and cut "the last M milliseconds" at a keyframe, as a packet sequence
 * rebased to zero that a fresh {@link Muxer} writes as a standalone file.
 * (adr/container/0009-replay-ring-c-abi.md)
 *
 * **Where the packets come from:** the C ABI has no video-packet source. `EncodeSession` muxes
 * its encoder's packets internally, and packet-level output exists only for the audio encoder.
 * Feed the ring packets from a {@link Demuxer}, from the audio encoder, or from an encoder you
 * drive yourself.
 *
 * Ring timestamps are ticks of each stream's own `timeBase`; the anchor stream's keyframes
 * decide where a clip may start, and other streams (audio) are cut by time to match.
 */

import {
  copyBytes,
  decodeArray,
  replay,
  type RawPacketView,
  type RawPlacement,
  type RawReplayClipEntry,
} from "@mediaway/ffi";
import {
  MediawayError,
  ReplayOutOfOrderError,
  ReplayPayloadKindError,
  ReplayUnknownStreamError,
  check,
  type Rational,
} from "./index.js";

const PAYLOAD_BYTES = 0;
const PAYLOAD_STORED = 1;

/** What a ring holds for each packet. */
export type ReplayPayloadKind = "bytes" | "stored";

export interface ReplayRingOptions {
  /** The stream whose keyframes decide where a clip may start (normally video). */
  anchorStreamId: number;
  /** The anchor's timebase. */
  anchorTimeBase: Rational;
  /** How much history to keep, in milliseconds. */
  windowMs: number;
  /**
   * Evict oldest-GOP-first while more than this many payload bytes are held; 0 or omitted = no
   * ceiling. The newest GOP is always kept, so one larger GOP can exceed it.
   */
  maxBytes?: number;
  /**
   * `"bytes"`: the ring copies each payload in ({@link ReplayRing.push}). `"stored"`: the ring
   * keeps only where the bytes are ({@link ReplayRing.pushStored}), for a caller that already
   * writes packets to a file.
   */
  payload: ReplayPayloadKind;
}

/** A packet to push into a `"bytes"` ring. Timestamps are ticks of the stream's timebase. */
export interface ReplayPacket {
  streamId: number;
  pts: number;
  /** Decode timestamp. Must not go backwards within a stream. Defaults to `pts` (no B-frames). */
  dts?: number;
  duration: number;
  key?: boolean;
  data: Buffer;
}

/** A packet's metadata without its payload — input to {@link ReplayRing.pushStored}. */
export interface ReplayPacketMeta {
  streamId: number;
  pts: number;
  dts?: number;
  duration: number;
  key?: boolean;
}

/** Where a packet's payload was stored on the caller's disk. */
export interface StoredLocation {
  /** The caller's own id for the file. */
  file: number;
  /** Byte offset of the payload in that file (exact up to 2^53). */
  offset: number;
  /** Payload length in bytes. */
  length: number;
}

interface ReplayEntryBase {
  streamId: number;
  /** Rebased so the cut keyframe decodes at zero. */
  pts: number;
  dts: number;
  duration: number;
  isKeyframe: boolean;
  isDiscard: boolean;
}

/** One packet of a clip cut from a `"bytes"` ring. */
export interface ReplayBytesEntry extends ReplayEntryBase {
  kind: "bytes";
  /** A COPY of the payload: the clip's own pointer is borrowed and never leaves this package. */
  payload: Buffer;
}

/** One packet of a clip cut from a `"stored"` ring. */
export interface ReplayStoredEntry extends ReplayEntryBase {
  kind: "stored";
  stored: StoredLocation;
}

export type ReplayEntry = ReplayBytesEntry | ReplayStoredEntry;

/** Map the ring's statuses to distinct classes; everything else is the shared container error. */
function checkReplay(status: number): void {
  switch (status) {
    case 0:
      return;
    case 2:
      throw new ReplayPayloadKindError();
    case 4:
      throw new ReplayOutOfOrderError();
    case 10:
      throw new ReplayUnknownStreamError();
    default:
      check(status);
  }
}

/**
 * A clip: an OWNED SNAPSHOT of the ring at the moment {@link ReplayRing.clipLast} was called.
 * Unlike the Rust `Clip` it does not borrow the ring, so it stays valid while the ring keeps
 * taking packets and after the ring is closed. Release it with {@link close}.
 */
export class ReplayClip {
  private handle: unknown;

  /** @internal Use {@link ReplayRing.clipLast}. */
  constructor(handle: unknown) {
    this.handle = handle;
  }

  private live(): unknown {
    if (!this.handle) throw new MediawayError(2, "replay clip is closed");
    return this.handle;
  }

  /** Number of packets. */
  get length(): number {
    return Number(replay.clipPacketCount(this.live()));
  }

  /** From the cut keyframe to the newest packet pushed when the clip was taken, in ms. */
  get durationMs(): number {
    const out: [bigint] = [0n];
    checkReplay(replay.clipDurationMs(this.live(), out) as number);
    return Number(out[0]);
  }

  /**
   * Packet `index` in decode order across streams, timestamps rebased so the cut keyframe
   * decodes at zero. A `"bytes"` entry's `payload` is a copy.
   */
  entry(index: number): ReplayEntry {
    const raw = {} as RawReplayClipEntry;
    checkReplay(replay.clipPacketAt(this.live(), index, raw) as number);
    const base: ReplayEntryBase = {
      streamId: raw.stream_id,
      pts: Number(raw.pts),
      dts: Number(raw.dts),
      duration: Number(raw.duration),
      isKeyframe: raw.is_keyframe,
      isDiscard: raw.is_discard,
    };
    if (raw.payload_kind === PAYLOAD_STORED) {
      return {
        ...base,
        kind: "stored",
        stored: { file: raw.stored_file, offset: Number(raw.stored_offset), length: raw.stored_len },
      };
    }
    return { ...base, kind: "bytes", payload: copyBytes(raw.payload, raw.payload_len) };
  }

  /** Every entry, in order. Copies each `"bytes"` payload once. */
  entries(): ReplayEntry[] {
    const n = this.length;
    const out: ReplayEntry[] = [];
    for (let i = 0; i < n; i++) out.push(this.entry(i));
    return out;
  }

  *[Symbol.iterator](): IterableIterator<ReplayEntry> {
    const n = this.length;
    for (let i = 0; i < n; i++) yield this.entry(i);
  }

  /** Free the snapshot. Idempotent. */
  close(): void {
    if (this.handle) {
      replay.clipFree(this.handle);
      this.handle = null;
    }
  }
}

/**
 * A rolling buffer of the last `windowMs` of encoded packets.
 *
 * Push packets in decode order per stream; {@link clipLast} cuts at a keyframe of the anchor
 * stream. Anchor packets before the anchor's first keyframe are dropped without error.
 */
export class ReplayRing {
  private handle: unknown;
  private readonly kind: ReplayPayloadKind;

  private constructor(handle: unknown, kind: ReplayPayloadKind) {
    this.handle = handle;
    this.kind = kind;
  }

  /** Create a ring. */
  static create(options: ReplayRingOptions): ReplayRing {
    const config = {
      anchor_stream_id: options.anchorStreamId,
      anchor_time_base: { num: BigInt(options.anchorTimeBase.num), den: options.anchorTimeBase.den },
      window_ms: BigInt(options.windowMs),
      max_bytes: BigInt(options.maxBytes ?? 0),
      payload_kind: options.payload === "stored" ? PAYLOAD_STORED : PAYLOAD_BYTES,
    };
    const out: [unknown] = [null];
    checkReplay(replay.ringCreate(config, out) as number);
    return new ReplayRing(out[0], options.payload);
  }

  private live(): unknown {
    if (!this.handle) throw new MediawayError(2, "replay ring is closed");
    return this.handle;
  }

  /** Carry another stream (e.g. audio), cut by time to match the anchor. */
  addStream(streamId: number, timeBase: Rational): void {
    checkReplay(
      replay.ringAddStream(this.live(), streamId, { num: BigInt(timeBase.num), den: timeBase.den }) as number
    );
  }

  /**
   * Add a packet to a `"bytes"` ring, then evict whatever fell out of the window.
   *
   * The payload is COPIED into the ring (the native call only borrows it), and every clip then
   * shares that copy.
   *
   * @throws {ReplayOutOfOrderError} this stream's `dts` went backwards; the packet was not added
   *   and the ring is still usable, so drop it and carry on.
   * @throws {ReplayUnknownStreamError} the stream was never added.
   * @throws {ReplayPayloadKindError} this is a `"stored"` ring.
   */
  push(packet: ReplayPacket): void {
    const raw: RawPacketView = {
      stream_id: packet.streamId,
      pts: BigInt(packet.pts),
      dts: BigInt(packet.dts ?? packet.pts),
      duration: BigInt(packet.duration),
      is_keyframe: packet.key ?? false,
      is_discard: false,
      payload: packet.data,
      payload_len: packet.data.length,
    };
    checkReplay(replay.ringPush(this.live(), raw) as number);
  }

  /**
   * Add a packet to a `"stored"` ring: its metadata and where its bytes are. The ring never
   * reads the file and holds a few dozen bytes per packet. Take the location from
   * {@link Muxer.pollPlacements} when every polled muxer byte is written to a file.
   * Errors as {@link push}, with {@link ReplayPayloadKindError} on a `"bytes"` ring.
   */
  pushStored(meta: ReplayPacketMeta, stored: StoredLocation): void {
    checkReplay(
      replay.ringPushStored(
        this.live(),
        {
          stream_id: meta.streamId,
          pts: BigInt(meta.pts),
          dts: BigInt(meta.dts ?? meta.pts),
          duration: BigInt(meta.duration),
          is_keyframe: meta.key ?? false,
          is_discard: false,
        },
        { file: stored.file, offset: BigInt(stored.offset), len: stored.length }
      ) as number
    );
  }

  /** The longest clip {@link clipLast} can return now, in ms; 0 before the first keyframe. */
  spanMs(): number {
    const out: [bigint] = [0n];
    checkReplay(replay.ringSpanMs(this.live(), out) as number);
    return Number(out[0]);
  }

  /**
   * Cut the last `spanMs` of every stream at a keyframe. The clip starts at the latest anchor
   * keyframe at or before `newest - spanMs`: up to one keyframe interval EARLIER than asked,
   * never later; with less than `spanMs` held it starts at the oldest keyframe.
   *
   * @returns the clip, or `null` until the anchor's first keyframe has been pushed.
   */
  clipLast(spanMs: number): ReplayClip | null {
    const clip: [unknown] = [null];
    const has: [boolean] = [false];
    checkReplay(replay.ringClipLast(this.live(), BigInt(spanMs), clip, has) as number);
    return has[0] ? new ReplayClip(clip[0]) : null;
  }

  /** Whether this ring holds payload bytes or locations. */
  get payload(): ReplayPayloadKind {
    return this.kind;
  }

  /** Close the ring. Clips already taken stay valid. Idempotent. */
  close(): void {
    if (this.handle) {
      replay.ringClose(this.handle);
      this.handle = null;
    }
  }
}

/** Where one muxed sample's payload landed in the output stream. */
export interface Placement {
  /** The packet's track index (as passed to `Muxer.push`). */
  trackIndex: number;
  /** The sample's decode timestamp, as pushed. */
  dts: number;
  /**
   * Absolute byte offset of the payload in the muxer's output: counted from the first byte
   * `pollBytes` ever returned, so it is the file offset when every polled byte is written
   * sequentially from 0. Exact up to 2^53.
   */
  offset: number;
  /** Payload length as written: Annex-B becomes length-prefixed and ADTS is stripped. */
  length: number;
}

/** @internal Take the muxer's placements; used by `Muxer.pollPlacements`. */
export function takePlacements(muxer: unknown): Placement[] {
  const rows: [unknown] = [null];
  const count: [number] = [0];
  checkReplay(replay.muxerPollPlacements(muxer, rows, count) as number);
  const n = Number(count[0]);
  const decoded = decodeArray<RawPlacement>(rows[0], replay.structs.MwPlacement, n);
  const out = decoded.map((p) => ({
    trackIndex: p.track_id,
    dts: Number(p.dts),
    offset: Number(p.offset),
    length: p.len,
  }));
  if (n > 0) replay.placementsFree(rows[0], n);
  return out;
}
