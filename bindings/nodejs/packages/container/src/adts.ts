/**
 * @mediaway/container — ADTS mux + demux (adr/container/0004-ogg-adts-c-abi.md).
 *
 * Same dedicated-handle reasoning as `ogg.ts`: ADTS has no track-registration
 * step and no Open/Live typestate.
 */

import { container, copyBytes, type RawPacket, type RawPacketView, type RawStreamInfo } from "@mediaway/ffi";
import { MediawayError, check, type Packet } from "./index.js";

export interface AdtsStreamInfo {
  sampleRate: number;
  channels: number;
  extraData: Buffer;
}

export class AdtsMuxer {
  private handle: unknown;

  /** `sampleRate` must be a standard ADTS sample rate. */
  constructor(sampleRate: number, channels: number) {
    this.handle = container.adtsMuxerCreate(sampleRate, channels);
    if (!this.handle) {
      throw new MediawayError(1, `non-standard ADTS sample rate (${sampleRate} Hz), or the native call panicked`);
    }
  }

  /** Append one AAC frame (raw, ADTS header added). Fails with INVALID_PACKET
   * if the payload is too large for ADTS's 13-bit frame-length field. */
  push(packet: Packet): void {
    const raw: RawPacketView = {
      stream_id: packet.trackIndex,
      pts: BigInt(packet.pts),
      dts: BigInt(packet.pts),
      duration: BigInt(packet.duration),
      is_keyframe: packet.key ?? false,
      is_discard: false,
      payload: packet.data,
      payload_len: packet.data.length,
    };
    check(container.adtsMuxerPushPacket(this.handle, raw));
  }

  /** No-op — ADTS frames are independently appendable. */
  flush(): void {
    check(container.adtsMuxerFlush(this.handle));
  }

  pollBytes(): Buffer {
    const outData: [unknown] = [null];
    const outLen: [number] = [0];
    check(container.adtsMuxerPollBytes(this.handle, outData, outLen));
    const data = copyBytes(outData[0], outLen[0]);
    if (outLen[0] > 0) container.bufferFree(outData[0], outLen[0]);
    return data;
  }

  close(): void {
    if (this.handle) {
      container.adtsMuxerClose(this.handle);
      this.handle = null;
    }
  }
}

export class AdtsDemuxer {
  private handle: unknown;

  constructor() {
    this.handle = container.adtsDemuxerCreate();
    if (!this.handle) throw new MediawayError(7, "ADTS demuxer creation panicked");
  }

  pushBytes(bytes: Buffer): void {
    check(container.adtsDemuxerPushBytes(this.handle, bytes, bytes.length));
  }

  /** Streams discovered so far — 0 or 1 (ADTS carries a single implicit stream). */
  streams(): AdtsStreamInfo[] {
    const count = container.adtsDemuxerStreamCount(this.handle);
    const out: AdtsStreamInfo[] = [];
    for (let i = 0; i < count; i++) {
      const raw = {} as RawStreamInfo;
      check(container.adtsDemuxerStreamAt(this.handle, i, raw));
      const extraData = copyBytes(raw.extra_data, raw.extra_data_len);
      container.streamInfoFree(raw);
      out.push({ sampleRate: raw.sample_rate, channels: raw.channels, extraData });
    }
    return out;
  }

  /** pts/duration are synthesized from a running 1024-samples-per-frame
   * count — ADTS carries no per-frame timing of its own. */
  pollPacket(): Packet | null {
    const raw = {} as RawPacket;
    const has: [boolean] = [false];
    check(container.adtsDemuxerPollPacket(this.handle, raw, has));
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
      container.adtsDemuxerClose(this.handle);
      this.handle = null;
    }
  }
}
