/**
 * @mediaway/container — MP3 (MPEG Layer III) mux + demux (adr/container/0007-mp3-c-abi.md).
 *
 * A fixed header for the mux session's lifetime (no track registration at
 * all); `writeFrame` takes an explicit `padding` bit no `Packet` has a slot for.
 */

import { container, copyBytes, type RawMp3FrameHeader, type RawPacket, type RawStreamInfo } from "@mediaway/ffi";
import { MediawayError, check, type Packet } from "./index.js";

export type MpegVersion = "mpeg1" | "mpeg2" | "mpeg2_5";
export type ChannelMode = "stereo" | "joint_stereo" | "dual_channel" | "mono";

const MPEG_VERSION_TO_ABI: Record<MpegVersion, number> = { mpeg1: 0, mpeg2: 1, mpeg2_5: 2 };
const CHANNEL_MODE_TO_ABI: Record<ChannelMode, number> = {
  stereo: 0,
  joint_stereo: 1,
  dual_channel: 2,
  mono: 3,
};

/** Fixed Layer III frame header for `Mp3Muxer` — bitrate/sample rate/
 * channel mode stay constant for the whole mux session's lifetime. */
export interface Mp3FrameHeader {
  version: MpegVersion;
  /** Must be one of the 14 standard Layer III rates for `version`. */
  bitrateKbps: number;
  /** Must be one of the 3 standard rates for `version`. */
  sampleRate: number;
  channelMode: ChannelMode;
}

export interface Mp3StreamInfo {
  sampleRate: number;
  channels: number;
  extraData: Buffer;
}

export class Mp3Muxer {
  private handle: unknown;

  /** `header` must be a standard Layer III bitrate/sample-rate combination for its version. */
  constructor(header: Mp3FrameHeader) {
    const raw: RawMp3FrameHeader = {
      version: MPEG_VERSION_TO_ABI[header.version],
      bitrate_kbps: header.bitrateKbps,
      sample_rate: header.sampleRate,
      channel_mode: CHANNEL_MODE_TO_ABI[header.channelMode],
    };
    this.handle = container.mp3MuxerCreate(raw);
    if (!this.handle) {
      throw new MediawayError(1, "non-standard MP3 bitrate/sample-rate combination, or the native call panicked");
    }
  }

  /** Append one already-encoded Layer III frame body. Fails with
   * INVALID_PACKET when `frameBody`'s length doesn't match what the
   * header's bitrate/sample-rate/padding combination requires. */
  writeFrame(frameBody: Buffer, padding: boolean): Buffer {
    const outData: [unknown] = [null];
    const outLen: [number] = [0];
    check(container.mp3MuxerWriteFrame(this.handle, frameBody, frameBody.length, padding, outData, outLen));
    const data = copyBytes(outData[0], outLen[0]);
    if (outLen[0] > 0) container.bufferFree(outData[0], outLen[0]);
    return data;
  }

  close(): void {
    if (this.handle) {
      container.mp3MuxerClose(this.handle);
      this.handle = null;
    }
  }
}

export class Mp3Demuxer {
  private handle: unknown;

  constructor() {
    this.handle = container.mp3DemuxerCreate();
    if (!this.handle) throw new MediawayError(7, "MP3 demuxer creation panicked");
  }

  pushBytes(bytes: Buffer): void {
    check(container.mp3DemuxerPushBytes(this.handle, bytes, bytes.length));
  }

  /** Streams discovered so far — 0 or 1 (MP3 carries a single implicit stream). */
  streams(): Mp3StreamInfo[] {
    const count = container.mp3DemuxerStreamCount(this.handle);
    const out: Mp3StreamInfo[] = [];
    for (let i = 0; i < count; i++) {
      const raw = {} as RawStreamInfo;
      check(container.mp3DemuxerStreamAt(this.handle, i, raw));
      const extraData = copyBytes(raw.extra_data, raw.extra_data_len);
      container.streamInfoFree(raw);
      out.push({ sampleRate: raw.sample_rate, channels: raw.channels, extraData });
    }
    return out;
  }

  /** pts/duration are synthesized from a running samples-per-frame count —
   * MPEG audio carries no per-frame timing of its own. */
  pollPacket(): Packet | null {
    const raw = {} as RawPacket;
    const has: [boolean] = [false];
    check(container.mp3DemuxerPollPacket(this.handle, raw, has));
    if (!has[0]) return null;
    const payload = copyBytes(raw.payload, raw.payload_len);
    container.packetFree(raw);
    return {
      trackIndex: raw.stream_id,
      data: payload,
      pts: Number(raw.pts),
      duration: Number(raw.duration),
      key: raw.is_keyframe,
    };
  }

  close(): void {
    if (this.handle) {
      container.mp3DemuxerClose(this.handle);
      this.handle = null;
    }
  }
}
