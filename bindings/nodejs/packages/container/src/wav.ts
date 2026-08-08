/**
 * @mediaway/container — WAV mux + one-shot parse (adr/container/0008-wav-c-abi.md).
 *
 * `wav::Muxer::finish` consumes `self` by value on the Rust side (RIFF chunk
 * sizes must be known up front), so there is no `pollBytes` step — `finish`
 * returns the complete byte stream directly. Demux has NO handle at all:
 * `parseWav` is a one-shot whole-buffer function, unlike every other format
 * in this package.
 */

import { container, copyBytes, type RawPacket, type RawPacketView, type RawStreamInfo, type RawWaveFormat } from "@mediaway/ffi";
import { ABI_TO_CODEC, MediawayError, check, type AudioCodec, type Packet, type TrackInfo } from "./index.js";

export type WavSampleFormat = "pcm" | "float";
const WAV_SAMPLE_FORMAT_TO_ABI: Record<WavSampleFormat, number> = { pcm: 0, float: 1 };

/** Explicit RIFF/WAVE fmt chunk for `WavMuxer`. */
export interface WaveFormat {
  sampleFormat: WavSampleFormat;
  channels: number;
  sampleRate: number;
  bitsPerSample: number;
}

export interface WavParseResult {
  info: TrackInfo;
  packet: Packet;
}

export class WavMuxer {
  private handle: unknown;
  private finished = false;

  /** Start an integer-PCM mux session, or pass a `WaveFormat` for an
   * explicit format (e.g. IEEE float PCM). */
  constructor(sampleRateOrFormat: number | WaveFormat, channels?: number, bitsPerSample?: number) {
    if (typeof sampleRateOrFormat === "number") {
      this.handle = container.wavMuxerCreate(sampleRateOrFormat, channels!, bitsPerSample!);
    } else {
      const raw: RawWaveFormat = {
        sample_format: WAV_SAMPLE_FORMAT_TO_ABI[sampleRateOrFormat.sampleFormat],
        channels: sampleRateOrFormat.channels,
        sample_rate: sampleRateOrFormat.sampleRate,
        bits_per_sample: sampleRateOrFormat.bitsPerSample,
      };
      this.handle = container.wavMuxerCreateWithFormat(raw);
    }
    if (!this.handle) throw new MediawayError(7, "WAV muxer creation panicked");
  }

  /** Append raw interleaved PCM bytes, already encoded per the session's format. */
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
    check(container.wavMuxerPushPacket(this.handle, raw));
  }

  /** Finalize the mux session and return the complete RIFF/WAVE byte stream.
   * Only the native-side internal state is consumed — this `WavMuxer` stays
   * usable for `close()` afterward. A second call fails with INVALID_STATE
   * rather than re-finalizing. */
  finish(): Buffer {
    const outData: [unknown] = [null];
    const outLen: [number] = [0];
    check(container.wavMuxerFinish(this.handle, outData, outLen));
    this.finished = true;
    const data = copyBytes(outData[0], outLen[0]);
    if (outLen[0] > 0) container.bufferFree(outData[0], outLen[0]);
    return data;
  }

  get isFinished(): boolean {
    return this.finished;
  }

  close(): void {
    if (this.handle) {
      container.wavMuxerClose(this.handle);
      this.handle = null;
    }
  }
}

/** Parse a complete RIFF/WAVE buffer into its single track's stream info
 * and one packet holding the whole PCM payload. */
export function parseWav(data: Buffer): WavParseResult {
  const outInfo = {} as RawStreamInfo;
  const outPacket = {} as RawPacket;
  check(container.wavParse(data, data.length, outInfo, outPacket));

  const extraData = copyBytes(outInfo.extra_data, outInfo.extra_data_len);
  const timeBase = { num: Number(outInfo.time_base.num), den: outInfo.time_base.den };
  // WAV always parses to an audio track (RIFF/WAVE has no video concept).
  const info: TrackInfo = {
    type: "audio",
    codec: (ABI_TO_CODEC[outInfo.codec] ?? "raw_audio") as AudioCodec,
    sampleRate: outInfo.sample_rate,
    channels: outInfo.channels,
    extraData,
    timeBase,
  } satisfies TrackInfo;
  container.streamInfoFree(outInfo);

  const payload = copyBytes(outPacket.payload, outPacket.payload_len);
  const packet: Packet = {
    trackIndex: outPacket.stream_id,
    data: payload,
    pts: Number(outPacket.pts),
    duration: Number(outPacket.duration),
    key: outPacket.is_keyframe,
  };
  container.packetFree(outPacket);

  return { info, packet };
}
