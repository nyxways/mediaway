/**
 * @mediaway/container — FLV mux + demux (adr/container/0005-flv-c-abi.md).
 *
 * Unlike `Muxer`, every write method here returns its own freshly allocated
 * output buffer directly — there is no separate `pollBytes` step. FLV has
 * exactly one video and one audio slot (no track-id field in the format
 * itself); `addVideoTrack`/`addAudioTrack` ignore the info's own id and the
 * fixed `VIDEO_TRACK_ID`/`AUDIO_TRACK_ID` are used instead.
 */

import {
  container,
  copyBytes,
  type RawAudioTrackInfo,
  type RawPacket,
  type RawPacketView,
  type RawStreamInfo,
  type RawVideoTrackInfo,
} from "@mediaway/ffi";
import {
  ABI_TO_CODEC,
  CODEC_TO_ABI,
  MediawayError,
  check,
  type AudioCodec,
  type AudioTrackInfo,
  type Packet,
  type TrackInfo,
  type VideoCodec,
  type VideoTrackInfo,
} from "./index.js";

export const VIDEO_TRACK_ID = 0;
export const AUDIO_TRACK_ID = 1;

export class FlvMuxer {
  private handle: unknown;

  constructor() {
    this.handle = container.flvMuxerCreate();
    if (!this.handle) throw new MediawayError(7, "FLV muxer creation panicked");
  }

  /** Write the FLV file header. Call before any track registration or packet. */
  writeHeader(hasAudio: boolean, hasVideo: boolean): Buffer {
    const outData: [unknown] = [null];
    const outLen: [number] = [0];
    check(container.flvMuxerWriteHeader(this.handle, hasAudio, hasVideo, outData, outLen));
    const data = copyBytes(outData[0], outLen[0]);
    if (outLen[0] > 0) container.bufferFree(outData[0], outLen[0]);
    return data;
  }

  /** Register the video track. Only H264 is recognized (UNSUPPORTED_CODEC otherwise). */
  addVideoTrack(info: VideoTrackInfo): number {
    const raw: RawVideoTrackInfo = {
      id: 0,
      codec: CODEC_TO_ABI[info.codec] ?? 0,
      time_base: { num: BigInt(info.timeBase.num), den: info.timeBase.den },
      width: info.width,
      height: info.height,
      extra_data: info.extraData ?? null,
      extra_data_len: info.extraData?.length ?? 0,
    };
    check(container.flvMuxerAddVideoTrack(this.handle, raw));
    return VIDEO_TRACK_ID;
  }

  /** Register the audio track. AAC and MP3 are recognized (UNSUPPORTED_CODEC otherwise). */
  addAudioTrack(info: AudioTrackInfo): number {
    const raw: RawAudioTrackInfo = {
      id: 0,
      codec: CODEC_TO_ABI[info.codec] ?? 4,
      time_base: { num: BigInt(info.timeBase.num), den: info.timeBase.den },
      sample_rate: info.sampleRate,
      channels: info.channels,
      extra_data: info.extraData ?? null,
      extra_data_len: info.extraData?.length ?? 0,
    };
    check(container.flvMuxerAddAudioTrack(this.handle, raw));
    return AUDIO_TRACK_ID;
  }

  /** Mux one packet: writes the track's sequence-header tag first (once,
   * only for codecs that have one) then the data tag. `packet.trackIndex`
   * selects `VIDEO_TRACK_ID`/`AUDIO_TRACK_ID` and must have a matching
   * `addVideoTrack`/`addAudioTrack` call already made, else UNKNOWN_STREAM. */
  push(packet: Packet): Buffer {
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
    const outData: [unknown] = [null];
    const outLen: [number] = [0];
    check(container.flvMuxerPushPacket(this.handle, raw, outData, outLen));
    const data = copyBytes(outData[0], outLen[0]);
    if (outLen[0] > 0) container.bufferFree(outData[0], outLen[0]);
    return data;
  }

  close(): void {
    if (this.handle) {
      container.flvMuxerClose(this.handle);
      this.handle = null;
    }
  }
}

export class FlvDemuxer {
  private handle: unknown;

  constructor() {
    this.handle = container.flvDemuxerCreate();
    if (!this.handle) throw new MediawayError(7, "FLV demuxer creation panicked");
  }

  pushBytes(bytes: Buffer): void {
    check(container.flvDemuxerPushBytes(this.handle, bytes, bytes.length));
  }

  /** Streams recognized so far — 0, 1, or 2 (fixed video-then-audio slots). */
  streams(): TrackInfo[] {
    const count = container.flvDemuxerStreamCount(this.handle);
    const out: TrackInfo[] = [];
    for (let i = 0; i < count; i++) {
      const raw = {} as RawStreamInfo;
      check(container.flvDemuxerStreamAt(this.handle, i, raw));
      const extraData = copyBytes(raw.extra_data, raw.extra_data_len);
      container.streamInfoFree(raw);
      const timeBase = { num: Number(raw.time_base.num), den: raw.time_base.den };
      if (raw.has_geometry) {
        out.push({
          type: "video",
          codec: (ABI_TO_CODEC[raw.codec] ?? "h264") as VideoCodec,
          width: raw.width,
          height: raw.height,
          extraData,
          timeBase,
        } satisfies TrackInfo);
      } else {
        out.push({
          type: "audio",
          codec: (ABI_TO_CODEC[raw.codec] ?? "aac") as AudioCodec,
          sampleRate: raw.sample_rate,
          channels: raw.channels,
          extraData,
          timeBase,
        } satisfies TrackInfo);
      }
    }
    return out;
  }

  /** Sequence-header tags (AVC/AAC config) update the matching stream's
   * extra data internally and are not themselves returned as packets. */
  pollPacket(): Packet | null {
    const raw = {} as RawPacket;
    const has: [boolean] = [false];
    check(container.flvDemuxerPollPacket(this.handle, raw, has));
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
      container.flvDemuxerClose(this.handle);
      this.handle = null;
    }
  }
}
