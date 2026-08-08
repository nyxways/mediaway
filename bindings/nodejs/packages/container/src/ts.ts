/**
 * @mediaway/container — MPEG-TS mux + demux (adr/container/0006-mpeg-ts-c-abi.md).
 *
 * The full elementary-stream list is fixed at construction (no `addTrack`
 * after); `writePatPmt`/`writeAccessUnit` write directly into a freshly
 * allocated output buffer with explicit 90 kHz `pts`/`dts` clock values —
 * not a track-relative time base.
 */

import {
  MwPacket,
  container,
  copyBytes,
  decodeArray,
  type RawPacket,
  type RawStreamInfo,
  type RawTsElementaryStream,
} from "@mediaway/ffi";
import { ABI_TO_CODEC, CODEC_TO_ABI, MediawayError, check, type AudioCodec, type Packet, type TrackInfo, type VideoCodec } from "./index.js";

export interface TsElementaryStream {
  /** TS packet identifier, must be in `2..=0x1FFF` (0/1 are reserved for PAT/CAT). */
  pid: number;
  /** Must be H264, HEVC, AAC, or MP3. */
  codec: VideoCodec | AudioCodec;
}

export class TsMuxer {
  private handle: unknown;

  /** `pmtPid` and every stream's `pid` must be in `2..=0x1FFF`; every
   * stream's codec must be H264/HEVC/AAC/MP3. */
  constructor(programNumber: number, pmtPid: number, streams: TsElementaryStream[]) {
    const raw: RawTsElementaryStream[] = streams.map((s) => ({ pid: s.pid, codec: CODEC_TO_ABI[s.codec] ?? 0 }));
    this.handle = container.tsMuxerCreate(programNumber, pmtPid, raw, raw.length);
    if (!this.handle) {
      throw new MediawayError(
        1,
        "invalid PMT/elementary-stream PID, an unsupported elementary-stream codec, or the native call panicked"
      );
    }
  }

  /** Write PAT + PMT packets. Call once at the start and periodically
   * thereafter — real players expect PAT/PMT to repeat. */
  writePatPmt(): Buffer {
    const outData: [unknown] = [null];
    const outLen: [number] = [0];
    check(container.tsMuxerWritePatPmt(this.handle, outData, outLen));
    const data = copyBytes(outData[0], outLen[0]);
    if (outLen[0] > 0) container.bufferFree(outData[0], outLen[0]);
    return data;
  }

  /** Packetize one access unit for `pid` into PES + TS packets. `pts90k`/
   * `dts90k` are the real MPEG-TS 90 kHz clock values; `dts90k === null`
   * means "no DTS". */
  writeAccessUnit(
    pid: number,
    data: Buffer,
    pts90k: number,
    dts90k: number | null,
    randomAccess: boolean
  ): Buffer {
    const outData: [unknown] = [null];
    const outLen: [number] = [0];
    check(
      container.tsMuxerWriteAccessUnit(
        this.handle,
        pid,
        data,
        data.length,
        BigInt(pts90k),
        dts90k !== null,
        BigInt(dts90k ?? 0),
        randomAccess,
        outData,
        outLen
      )
    );
    const result = copyBytes(outData[0], outLen[0]);
    if (outLen[0] > 0) container.bufferFree(outData[0], outLen[0]);
    return result;
  }

  close(): void {
    if (this.handle) {
      container.tsMuxerClose(this.handle);
      this.handle = null;
    }
  }
}

function streamInfoToManaged(raw: RawStreamInfo, extraData: Buffer): TrackInfo {
  const timeBase = { num: Number(raw.time_base.num), den: raw.time_base.den };
  if (raw.has_geometry) {
    return {
      type: "video",
      codec: (ABI_TO_CODEC[raw.codec] ?? "h264") as VideoCodec,
      width: raw.width,
      height: raw.height,
      extraData,
      timeBase,
    } satisfies TrackInfo;
  }
  return {
    type: "audio",
    codec: (ABI_TO_CODEC[raw.codec] ?? "aac") as AudioCodec,
    sampleRate: raw.sample_rate,
    channels: raw.channels,
    extraData,
    timeBase,
  } satisfies TrackInfo;
}

function packetToManaged(raw: RawPacket): Packet {
  return {
    trackIndex: raw.stream_id,
    data: copyBytes(raw.payload, raw.payload_len),
    pts: Number(raw.pts),
    duration: Number(raw.duration),
    key: raw.is_keyframe,
  };
}

export class TsDemuxer {
  private handle: unknown;

  constructor() {
    this.handle = container.tsDemuxerCreate();
    if (!this.handle) throw new MediawayError(7, "MPEG-TS demuxer creation panicked");
  }

  /** Feed bytes — need not be 188-byte aligned across calls. */
  pushBytes(bytes: Buffer): void {
    check(container.tsDemuxerPushBytes(this.handle, bytes, bytes.length));
  }

  /** Streams whose stream_type maps to a recognized codec (H264/HEVC/AAC/MP3).
   * Empty until `pollPacket` has actually consumed the PMT (lazy PSI parsing). */
  streams(): TrackInfo[] {
    const count = container.tsDemuxerStreamCount(this.handle);
    const out: TrackInfo[] = [];
    for (let i = 0; i < count; i++) {
      const raw = {} as RawStreamInfo;
      check(container.tsDemuxerStreamAt(this.handle, i, raw));
      const extraData = copyBytes(raw.extra_data, raw.extra_data_len);
      container.streamInfoFree(raw);
      out.push(streamInfoToManaged(raw, extraData));
    }
    return out;
  }

  /** A PID with no recognized codec mapping is silently skipped. */
  pollPacket(): Packet | null {
    const raw = {} as RawPacket;
    const has: [boolean] = [false];
    check(container.tsDemuxerPollPacket(this.handle, raw, has));
    if (!has[0]) return null;
    const packet = packetToManaged(raw);
    container.packetFree(raw);
    return packet;
  }

  /** Force-emit whatever is still accumulating per PID — call once at the
   * end of a stream so the very last access unit per PID isn't lost
   * (MPEG-TS only confirms a PES boundary once the next packet on the same
   * PID starts). */
  finish(): Packet[] {
    const outPackets: [unknown] = [null];
    const outCount: [number] = [0];
    check(container.tsDemuxerFinish(this.handle, outPackets, outCount));
    const count = outCount[0];
    const decoded = decodeArray<RawPacket>(outPackets[0], MwPacket, count);
    const result = decoded.map(packetToManaged);
    if (count > 0) container.tsDemuxerFinishFree(outPackets[0], count);
    return result;
  }

  close(): void {
    if (this.handle) {
      container.tsDemuxerClose(this.handle);
      this.handle = null;
    }
  }
}
