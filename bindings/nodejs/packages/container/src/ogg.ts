/**
 * @mediaway/container — Ogg mux + demux (adr/container/0004-ogg-adts-c-abi.md).
 *
 * Dedicated handles, not `Muxer`/`Demuxer`: Ogg has no track-registration
 * step and no Open/Live typestate — `OggMuxer` is immediately ready for
 * `push`. Reuses the shared `Packet`/`TrackInfo` shapes and `MediawayError`.
 */

import { container, copyBytes, type RawPacket, type RawPacketView, type RawStreamInfo } from "@mediaway/ffi";
import { ABI_TO_CODEC, MediawayError, check, type AudioCodec, type Packet, type VideoCodec } from "./index.js";

export interface OggStreamInfo {
  codec: AudioCodec | VideoCodec;
  sampleRate: number;
  channels: number;
  extraData: Buffer;
}

export class OggMuxer {
  private handle: unknown;

  constructor(serial: number) {
    this.handle = container.oggMuxerCreate(serial);
    if (!this.handle) throw new MediawayError(7, "Ogg muxer creation panicked");
  }

  /** Write one Ogg page. `packet.pts` becomes the page's granule position;
   * fails with INVALID_DATA when the payload exceeds a single page. */
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
    check(container.oggMuxerPushPacket(this.handle, raw));
  }

  /** No-op — every `push` call already wrote a complete, independently valid Ogg page. */
  flush(): void {
    check(container.oggMuxerFlush(this.handle));
  }

  pollBytes(): Buffer {
    const outData: [unknown] = [null];
    const outLen: [number] = [0];
    check(container.oggMuxerPollBytes(this.handle, outData, outLen));
    const data = copyBytes(outData[0], outLen[0]);
    if (outLen[0] > 0) container.bufferFree(outData[0], outLen[0]);
    return data;
  }

  close(): void {
    if (this.handle) {
      container.oggMuxerClose(this.handle);
      this.handle = null;
    }
  }
}

export class OggDemuxer {
  private handle: unknown;

  constructor() {
    this.handle = container.oggDemuxerCreate();
    if (!this.handle) throw new MediawayError(7, "Ogg demuxer creation panicked");
  }

  pushBytes(bytes: Buffer): void {
    check(container.oggDemuxerPushBytes(this.handle, bytes, bytes.length));
  }

  /** Streams discovered so far — 0 or 1 (Ogg carries a single logical bitstream). */
  streams(): OggStreamInfo[] {
    const count = container.oggDemuxerStreamCount(this.handle);
    const out: OggStreamInfo[] = [];
    for (let i = 0; i < count; i++) {
      const raw = {} as RawStreamInfo;
      check(container.oggDemuxerStreamAt(this.handle, i, raw));
      const extraData = copyBytes(raw.extra_data, raw.extra_data_len);
      container.streamInfoFree(raw);
      out.push({
        codec: (ABI_TO_CODEC[raw.codec] ?? "opus") as AudioCodec,
        sampleRate: raw.sample_rate,
        channels: raw.channels,
        extraData,
      });
    }
    return out;
  }

  pollPacket(): Packet | null {
    const raw = {} as RawPacket;
    const has: [boolean] = [false];
    check(container.oggDemuxerPollPacket(this.handle, raw, has));
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
      container.oggDemuxerClose(this.handle);
      this.handle = null;
    }
  }
}
