/**
 * decode-roundtrip.ts — auto video decode (encode -> mux -> demux -> decode)
 * and Opus audio decode (encode -> decode), round-tripped through the real
 * native C ABI.
 *
 * Status: ✅ real ABI under it.
 * The decode session C ABI (adr/0004-auto-decode-c-abi.md,
 * adr/pipeline/0006-audio-decode-c-abi.md) is implemented; this example runs
 * against it. Mirrors test/decode-roundtrip.test.ts's scenario, narrated for
 * a human reader instead of asserting.
 *
 * DecodeSession/AudioDecodeSession are single-step handles (the handle IS
 * the decoder, same shape as openAutoEncoder()/AudioEncoder) — NO_BACKEND
 * throws DecoderUnavailableError, an expected outcome to catch and exit
 * gracefully.
 *
 * Run: npx tsx examples/pipeline/decode-roundtrip.ts
 */

import {
  AutoVideoEncodeConfig,
  EncoderUnavailableError,
  openAutoEncoder,
  type VideoFrame,
} from "@mediaway/encoder";
import { AudioDecodeSession, DecodeSession, DecoderUnavailableError } from "@mediaway/decoder";
import { Demuxer, type VideoTrackInfo } from "@mediaway/container";
import { pipeline, copyBytes, type RawAudioEncodeConfig, type RawAudioFrameView, type RawAudioPacket } from "@mediaway/ffi";

const WIDTH = 64;
const HEIGHT = 64;
const FRAME_COUNT = 10;

const SAMPLE_RATE = 48_000;
const CHANNELS = 1;
const FRAME_SAMPLES = 960; // 20ms @ 48kHz mono
const AUDIO_FRAME_COUNT = 50;

async function videoRoundTrip(): Promise<void> {
  let session;
  try {
    session = await openAutoEncoder(AutoVideoEncodeConfig.defaults("h264", WIDTH, HEIGHT, { num: 1, den: 30 }));
  } catch (err) {
    if (err instanceof EncoderUnavailableError) {
      console.log("skip: no H.264 encoder available");
      return;
    }
    throw err;
  }

  const nv12Len = WIDTH * HEIGHT + (WIDTH * HEIGHT) / 2;
  const plane = Buffer.alloc(nv12Len, 0x80);
  for (let i = 0; i < FRAME_COUNT; i++) {
    const frame: VideoFrame = { pts: i, duration: 1, width: WIDTH, height: HEIGHT, pixelFormat: "nv12", data: plane };
    await session.writeFrame(frame);
  }
  const fmp4 = await session.finish();
  console.log(`encoded ${FRAME_COUNT} frames -> ${fmp4.length} fMP4 bytes`);

  const demuxer = new Demuxer("mp4");
  demuxer.pushBytes(fmp4);
  const videoStream = demuxer.streams()[0] as { type: "video" } & VideoTrackInfo;
  const packets: { pts: number; data: Buffer; key?: boolean }[] = [];
  for (;;) {
    const packet = demuxer.pollPacket();
    if (!packet) break;
    packets.push(packet);
  }
  demuxer.close();
  console.log(`demuxed ${packets.length} H.264 packets, ${videoStream.extraData?.length ?? 0} bytes of AVCC extra_data`);

  let decodeSession: DecodeSession;
  try {
    decodeSession = await DecodeSession.open({
      codec: "h264",
      width: WIDTH,
      height: HEIGHT,
      timeBase: { num: 1, den: 30 },
      extraData: videoStream.extraData,
    });
  } catch (err) {
    if (err instanceof DecoderUnavailableError) {
      console.log("skip: no H.264 decoder available");
      return;
    }
    throw err;
  }

  for (const packet of packets) {
    await decodeSession.pushPacket({ pts: packet.pts, keyframe: packet.key, payload: packet.data });
  }
  await decodeSession.flush();
  let decoded = 0;
  for (;;) {
    const frame = await decodeSession.pollFrame();
    if (!frame) break;
    decoded++;
  }
  decodeSession.close();
  console.log(`decoded ${decoded} frames back`);
}

async function audioRoundTrip(): Promise<void> {
  // Opus encode via the raw @mediaway/ffi layer — @mediaway/encoder's public
  // AudioEncoder wrapper is AAC-only today.
  const encConfig: RawAudioEncodeConfig = {
    codec: 5, // Opus
    sample_rate: SAMPLE_RATE,
    channels: CHANNELS,
    sample_format: 2, // F32
    time_base: { num: 1n, den: 50 },
    bitrate_bps: 0,
  };
  const out: [unknown] = [null];
  if (pipeline.audioEncoderOpen(encConfig, out) === 3) {
    console.log("skip: no Opus encode backend available");
    return;
  }
  const encSession = out[0];

  const encoded: { pts: number; data: Buffer }[] = [];
  for (let i = 0; i < AUDIO_FRAME_COUNT; i++) {
    const pcm = Buffer.alloc(FRAME_SAMPLES * 4);
    for (let s = 0; s < FRAME_SAMPLES; s++) {
      const t = (i * FRAME_SAMPLES + s) / SAMPLE_RATE;
      pcm.writeFloatLE(Math.sin(t * 440.0 * 2 * Math.PI), s * 4);
    }
    const view: RawAudioFrameView = {
      pts: BigInt(i),
      duration: 0n,
      sample_rate: SAMPLE_RATE,
      channels: CHANNELS,
      sample_format: 2,
      data: pcm,
      data_len: pcm.length,
    };
    pipeline.audioPushPcm(encSession, view);
    for (;;) {
      const raw = {} as RawAudioPacket;
      const has: [boolean] = [false];
      pipeline.audioPollPacket(encSession, raw, has);
      if (!has[0]) break;
      encoded.push({ pts: Number(raw.pts), data: copyBytes(raw.payload, raw.payload_len) });
      pipeline.pipelinePacketFree(raw);
    }
  }
  pipeline.audioFlush(encSession);
  pipeline.audioSessionClose(encSession);
  console.log(`encoded ${encoded.length} Opus packets`);

  const decodeSession = await AudioDecodeSession.open(SAMPLE_RATE, CHANNELS, { num: 1, den: 50 });
  for (const packet of encoded) {
    await decodeSession.pushPacket({ pts: packet.pts, payload: packet.data });
  }
  await decodeSession.flush();
  let decoded = 0;
  for (;;) {
    const frame = await decodeSession.pollFrame();
    if (!frame) break;
    decoded++;
  }
  decodeSession.close();
  console.log(`decoded ${decoded} Opus frames back`);
}

async function main(): Promise<void> {
  await videoRoundTrip();
  await audioRoundTrip();
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
