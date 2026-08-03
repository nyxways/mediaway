/**
 * encode-audio.ts — AAC audio encode → audio-only fragmented MP4
 *
 * Status: ✅ real ABI under it.
 * The C ABI's audio encode capability (ABI v2, adr/0003-auto-audio-encode-c-abi.md)
 * is implemented in mediaway_pipeline_ffi: `AudioEncoder.open()` returns the
 * encode session directly (single step — no intermediate handle, no consumption
 * trap). PCM is pushed as borrowed views, encoded packets are polled back, and
 * an audio track registered with the encoder's AudioSpecificConfig
 * (`streamInfo()` — materialized after the first pushed frame) is muxed into
 * an audio-only fMP4 via @mediaway/container.
 *
 * Deterministic: 96 frames of a 440 Hz sine (1024 samples @ 48 kHz, stereo
 * f32le) — no microphone needed. No audio backend → EncoderUnavailableError →
 * exit cleanly.
 *
 * Run: npx tsx examples/pipeline/encode-audio.ts
 */

import { AudioEncoder, EncoderUnavailableError } from "@mediaway/encoder";
import { Muxer, Rational } from "@mediaway/container";

const SAMPLE_RATE = 48000;
const CHANNELS = 2;
const FRAME_SAMPLES = 1024; // ~21 ms per pushed frame
const FRAME_COUNT = 96; // ~2.0 s of audio

/** One interleaved f32le stereo frame of a deterministic 440 Hz sine. */
function sineFrame(frameIndex: number): Buffer {
  const out = Buffer.alloc(FRAME_SAMPLES * CHANNELS * 4);
  for (let s = 0; s < FRAME_SAMPLES; s++) {
    const t = (frameIndex * FRAME_SAMPLES + s) / SAMPLE_RATE;
    const v = Math.sin(2 * Math.PI * 440 * t);
    for (let c = 0; c < CHANNELS; c++) {
      out.writeFloatLE(v, (s * CHANNELS + c) * 4);
    }
  }
  return out;
}

async function main(): Promise<void> {
  let encoder: AudioEncoder;
  try {
    encoder = await AudioEncoder.open({ sampleRate: SAMPLE_RATE, channels: CHANNELS });
  } catch (error) {
    if (error instanceof EncoderUnavailableError) {
      console.log("no audio encode backend — exiting gracefully");
      return;
    }
    throw error;
  }

  for (let i = 0; i < FRAME_COUNT; i++) {
    await encoder.pushPcm({ pts: i * FRAME_SAMPLES, data: sineFrame(i) });
  }
  await encoder.flush();

  const packets = [];
  for (;;) {
    const packet = await encoder.pollPacket();
    if (packet === null) break;
    packets.push(packet);
  }
  if (packets.length === 0) {
    console.log(`encoder produced no packets for ${FRAME_COUNT} PCM frames`);
    return;
  }

  const info = await encoder.streamInfo(); // ASC materialized after the first push
  if (info.extraData.length === 0) {
    console.log("stream info carries no AudioSpecificConfig");
    return;
  }
  console.log(`encoded ${packets.length} AAC packet(s), ASC ${info.extraData.length} bytes`);

  const muxer = new Muxer();
  const audioTrack = muxer.addAudioTrack({
    codec: "aac",
    sampleRate: info.sampleRate,
    channels: info.channels,
    timeBase: { num: 1, den: info.sampleRate },
    extraData: info.extraData,
  });
  muxer.begin(); // makes the muxer live; the same object pushes packets
  for (const packet of packets) {
    muxer.push({
      trackIndex: audioTrack,
      pts: packet.pts,
      duration: packet.duration,
      data: packet.data,
      key: packet.keyframe,
    });
  }
  muxer.flush();
  const out = muxer.pollBytes();
  console.log(`muxed ${packets.length} AAC packet(s) into ${out.length} bytes of audio-only fragmented MP4`);
}

main().catch((error) => {
  console.error("mediaway error:", error.message ?? error);
  process.exitCode = 1;
});
