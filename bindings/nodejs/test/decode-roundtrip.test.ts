/**
 * decode-roundtrip.test.ts — round-trips video decode (encode->mux->demux
 * ->decode) and Opus audio decode (encode->decode) against the real
 * mediaway_ffi.dll. Mirrors crates/mediaway-ffi/tests/{decode,audio_decode}
 * _smoke.rs and the C++/C#/Python bindings' own decode round-trip tests.
 *
 * Assertion-based with node:assert (no test framework); any failed
 * assertion exits nonzero.
 *
 * Run: npm test   (from bindings/nodejs)
 */

import assert from "node:assert/strict";
import {
  AudioDecodeSession,
  AutoVideoEncodeConfig,
  DecodeSession,
  EncodeSession,
  openAutoEncoder,
  type DecodedAudioFrame,
  type DecodedVideoFrame,
  type VideoFrame,
} from "@mediaway/encoder";
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
  // ── encode + mux (mid-gray NV12, no real capture needed) ──────────────
  const config = AutoVideoEncodeConfig.defaults("h264", WIDTH, HEIGHT, { num: 1, den: 30 });
  const session: EncodeSession = await openAutoEncoder(config);
  const nv12Len = WIDTH * HEIGHT + (WIDTH * HEIGHT) / 2;
  const plane = Buffer.alloc(nv12Len, 0x80);
  for (let i = 0; i < FRAME_COUNT; i++) {
    const frame: VideoFrame = { pts: i, duration: 1, width: WIDTH, height: HEIGHT, pixelFormat: "nv12", data: plane };
    await session.writeFrame(frame);
  }
  const fmp4 = await session.finish();
  assert.ok(fmp4.length > 0, "encoder produced no bytes");

  // ── demux: recover real H.264 packets + AVCC extra_data ────────────────
  const demuxer = new Demuxer("mp4");
  demuxer.pushBytes(fmp4);
  const streams = demuxer.streams();
  assert.equal(streams.length, 1, "expected 1 stream");
  const videoStream = streams[0] as { type: "video" } & VideoTrackInfo;
  assert.equal(videoStream.type, "video");
  assert.ok(videoStream.extraData && videoStream.extraData.length > 0, "expected AVCC extra_data");

  const packets: { pts: number; data: Buffer; key?: boolean }[] = [];
  for (;;) {
    const packet = demuxer.pollPacket();
    if (!packet) break;
    packets.push(packet);
  }
  demuxer.close();
  assert.equal(packets.length, FRAME_COUNT, "every frame must demux back");

  // ── decode: extra_data supplied at open time (adr/0004 §1) ────────────
  const decodeSession = await DecodeSession.open({
    codec: "h264",
    width: WIDTH,
    height: HEIGHT,
    timeBase: { num: 1, den: 30 },
    extraData: videoStream.extraData,
  });
  for (const packet of packets) {
    await decodeSession.pushPacket({ pts: packet.pts, keyframe: packet.key, payload: packet.data });
  }
  await decodeSession.flush();

  let decoded = 0;
  for (;;) {
    const frame: DecodedVideoFrame | null = await decodeSession.pollFrame();
    if (!frame) break;
    assert.equal(frame.width, WIDTH);
    assert.equal(frame.height, HEIGHT);
    assert.ok(frame.data.length >= nv12Len, "decoded frame implausibly small");
    decoded++;
  }
  decodeSession.close();
  assert.ok(decoded > 0, "expected at least one decoded frame");
  console.log(`video: encoded ${fmp4.length} bytes, decoded ${decoded} frames`);
}

async function audioRoundTrip(): Promise<void> {
  // ── encode via raw @mediaway/ffi (the public AudioEncoder wrapper is AAC-only) ──
  const encConfig: RawAudioEncodeConfig = {
    codec: 5, // Opus
    sample_rate: SAMPLE_RATE,
    channels: CHANNELS,
    sample_format: 2, // F32
    time_base: { num: 1n, den: 50 },
    bitrate_bps: 0,
  };
  const out: [unknown] = [null];
  const openStatus = pipeline.audioEncoderOpen(encConfig, out);
  if (openStatus === 3) {
    console.log("skip: no Opus encode backend compiled in");
    return;
  }
  assert.equal(openStatus, 0, "audio encoder open must succeed");
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
    assert.equal(pipeline.audioPushPcm(encSession, view), 0, "push_pcm must succeed");

    for (;;) {
      const raw = {} as RawAudioPacket;
      const has: [boolean] = [false];
      assert.equal(pipeline.audioPollPacket(encSession, raw, has), 0, "poll_packet must succeed");
      if (!has[0]) break;
      const data = copyBytes(raw.payload, raw.payload_len);
      encoded.push({ pts: Number(raw.pts), data });
      pipeline.pipelinePacketFree(raw);
    }
  }
  pipeline.audioFlush(encSession);
  pipeline.audioSessionClose(encSession);
  assert.ok(encoded.length > 0, "expected at least one encoded Opus packet");

  // ── decode via the public wrapper ───────────────────────────────────────
  const decodeSession = await AudioDecodeSession.open(SAMPLE_RATE, CHANNELS, { num: 1, den: 50 });
  for (const packet of encoded) {
    await decodeSession.pushPacket({ pts: packet.pts, payload: packet.data });
  }
  await decodeSession.flush();

  let decoded = 0;
  for (;;) {
    const frame: DecodedAudioFrame | null = await decodeSession.pollFrame();
    if (!frame) break;
    assert.equal(frame.sampleRate, SAMPLE_RATE);
    assert.equal(frame.channels, CHANNELS);
    decoded++;
  }
  decodeSession.close();
  assert.ok(decoded > 0, "expected at least one decoded Opus frame");
  console.log(`audio: encoded ${encoded.length} Opus packets, decoded ${decoded} frames`);
}

async function main(): Promise<void> {
  await videoRoundTrip();
  await audioRoundTrip();
  console.log("PASS: video decode + Opus audio decode round-tripped through the native library");
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
