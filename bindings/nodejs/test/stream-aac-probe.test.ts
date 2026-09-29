/**
 * stream-aac-probe.test.ts — pipeline ABI v7 (crates/mediaway-ffi/adr/pipeline/0007): streaming
 * fMP4 bytes (`EncodeSession.pollBytes`), AAC decode (`AudioDecodeSession.open({ codec: "aac" })`)
 * and the capability probes (`encoderSupport` / `decoderSupport`), against the real
 * mediaway_ffi.dll. Mirrors crates/mediaway-ffi/tests/{stream_bytes,aac_decode,capability_probe}
 * _smoke.rs.
 *
 * Assertion-based with node:assert (no test framework); any failed assertion exits nonzero.
 * Hardware/OS-dependent parts skip loudly when the backend is absent.
 *
 * Run: bun run test   (from bindings/nodejs)
 */

import assert from "node:assert/strict";
import {
  AudioEncoder,
  AutoVideoEncodeConfig,
  EncoderUnavailableError,
  encoderSupport,
  openAutoEncoder,
  type EncoderCapability,
} from "@mediaway/encoder";
import { AudioDecodeSession, DecoderUnavailableError, decoderSupport } from "@mediaway/decoder";
import { Demuxer, MediawayError } from "@mediaway/container";
import {
  MwAudioDecodeConfig,
  MwEncoderCapability,
  MwRational,
  pipeline,
  structLayout,
} from "@mediaway/ffi";

const isWindows = process.platform === "win32";
/** Platforms with an OS AAC decoder (Media Foundation / AudioToolbox). */
const hasAacDecoder = isWindows || process.platform === "darwin";

async function rejectsStatus(promise: Promise<unknown>, status: number, what: string): Promise<void> {
  try {
    await promise;
  } catch (error) {
    assert.ok(error instanceof MediawayError, `${what}: expected MediawayError, got ${String(error)}`);
    assert.equal(error.status, status, `${what}: status`);
    return;
  }
  assert.fail(`${what}: expected a rejection with status ${status}`);
}

// ── layout + ABI version ───────────────────────────────────────────────────────

function layoutPin(): void {
  // Offsets come from compiling a probe against the real pipeline.h with gcc (64-bit).
  assert.deepEqual(structLayout(MwRational, ["num", "den"]), { size: 16, offsets: { num: 0, den: 8 } });
  assert.deepEqual(
    structLayout(MwAudioDecodeConfig, [
      "codec",
      "sample_rate",
      "channels",
      "time_base",
      "extra_data",
      "extra_data_len",
    ]),
    {
      size: 48,
      offsets: { codec: 0, sample_rate: 4, channels: 8, time_base: 16, extra_data: 32, extra_data_len: 40 },
    }
  );
  assert.deepEqual(structLayout(MwEncoderCapability, ["backend", "state", "path_class"]), {
    size: 12,
    offsets: { backend: 0, state: 4, path_class: 8 },
  });
  assert.equal(Number(pipeline.abiVersion()), 7, "pipeline ABI version");
  console.log("layout: audio decode config 48 B, capability row 12 B, pipeline ABI 7");
}

// ── streaming bytes ────────────────────────────────────────────────────────────

const WIDTH = 64;
const HEIGHT = 64;
const FRAME_COUNT = 100;

function countPackets(fmp4: Buffer): number {
  const demuxer = new Demuxer("mp4");
  demuxer.pushBytes(fmp4);
  let n = 0;
  while (demuxer.pollPacket()) n++;
  demuxer.close();
  return n;
}

async function streamingBytes(): Promise<void> {
  const config = AutoVideoEncodeConfig.defaults("h264", WIDTH, HEIGHT, { num: 1, den: 30 });
  const plane = Buffer.alloc(WIDTH * HEIGHT + (WIDTH * HEIGHT) / 2, 0x80);
  const frame = (i: number) => ({
    pts: i,
    duration: 1,
    width: WIDTH,
    height: HEIGHT,
    pixelFormat: "nv12" as const,
    data: plane,
  });

  let session;
  try {
    session = await openAutoEncoder(config);
  } catch (error) {
    if (error instanceof EncoderUnavailableError) {
      console.log("skip streaming: no encode backend");
      return;
    }
    throw error;
  }

  // ── poll after every frame ───────────────────────────────────────────────
  const polled: Buffer[] = [];
  let chunks = 0;
  for (let i = 0; i < FRAME_COUNT; i++) {
    await session.writeFrame(frame(i));
    const bytes = await session.pollBytes();
    if (bytes.length > 0) chunks++;
    polled.push(bytes);
  }
  assert.ok(chunks >= 1, `no fMP4 bytes were ready before finish over ${FRAME_COUNT} frames`);
  const first = polled.find((b) => b.length > 0) as Buffer;
  assert.equal(first.subarray(4, 8).toString("latin1"), "ftyp", "first polled bytes start the file");

  // A second poll straight after a drain has nothing to give.
  await session.pollBytes();
  assert.equal((await session.pollBytes()).length, 0, "an idle poll returns an empty buffer");

  // ── finish returns the tail only ─────────────────────────────────────────
  const tail = await session.finish();
  const polledTotal = polled.reduce((n, b) => n + b.length, 0);
  assert.ok(
    tail.length < polledTotal,
    `finish after polling must return the unpolled tail (${tail.length} B), not the whole stream (${polledTotal} B polled)`
  );
  await rejectsStatus(session.pollBytes(), 7, "pollBytes after finish");
  const streamed = Buffer.concat([...polled, tail]);

  // ── the same input without polling is the reference ──────────────────────
  const reference = await openAutoEncoder(config);
  for (let i = 0; i < FRAME_COUNT; i++) await reference.writeFrame(frame(i));
  const whole = await reference.finish();

  assert.equal(countPackets(streamed), FRAME_COUNT, "streamed output must demux to every frame");
  assert.equal(countPackets(whole), FRAME_COUNT, "reference output must demux to every frame");
  console.log(
    `streaming: ${chunks} non-empty polls, streamed ${streamed.length} B (tail ${tail.length} B) vs unpolled ${whole.length} B`
  );
}

// ── AAC decode ─────────────────────────────────────────────────────────────────

const SAMPLE_RATE = 48_000;
const CHANNELS = 2;
const FRAME_SAMPLES = 1024;
const AAC_FRAMES = 48;

async function aacRoundTrip(): Promise<void> {
  if (!isWindows) {
    console.log("skip AAC round trip: the AAC encoder is Windows-only");
    return;
  }
  let encoder: AudioEncoder;
  try {
    encoder = await AudioEncoder.open({ sampleRate: SAMPLE_RATE, channels: CHANNELS });
  } catch (error) {
    if (error instanceof EncoderUnavailableError) {
      console.log("skip AAC round trip: no AAC encode backend");
      return;
    }
    throw error;
  }
  for (let i = 0; i < AAC_FRAMES; i++) {
    const pcm = Buffer.alloc(FRAME_SAMPLES * CHANNELS * 4);
    for (let s = 0; s < FRAME_SAMPLES; s++) {
      const v = Math.sin(((i * FRAME_SAMPLES + s) / SAMPLE_RATE) * 440 * 2 * Math.PI);
      for (let c = 0; c < CHANNELS; c++) pcm.writeFloatLE(v, (s * CHANNELS + c) * 4);
    }
    await encoder.pushPcm({ pts: i * FRAME_SAMPLES, data: pcm });
  }
  await encoder.flush();
  const info = await encoder.streamInfo(); // the ASC only exists after the first push
  assert.ok(info.extraData.length > 0, "AAC AudioSpecificConfig expected");
  const packets = [];
  for (;;) {
    const packet = await encoder.pollPacket();
    if (!packet) break;
    packets.push(packet);
  }
  encoder.close();
  assert.ok(packets.length > 0, "expected AAC packets");

  const decoder = await AudioDecodeSession.open({
    codec: "aac",
    sampleRate: SAMPLE_RATE,
    channels: CHANNELS,
    extraData: info.extraData,
  });
  assert.equal(decoder.codec, "aac");
  for (const packet of packets) {
    await decoder.pushPacket({ pts: packet.pts, duration: packet.duration, keyframe: true, payload: packet.data });
  }
  await decoder.flush();

  let samples = 0;
  let energy = 0;
  for (;;) {
    const frame = await decoder.pollFrame();
    if (!frame) break;
    assert.equal(frame.channels, CHANNELS);
    for (let off = 0; off + 4 <= frame.data.length; off += 4) {
      const v = frame.data.readFloatLE(off);
      energy += v * v;
    }
    samples += frame.data.length / 4 / CHANNELS;
  }
  decoder.close();

  assert.equal(samples, packets.length * FRAME_SAMPLES, "decoded samples per channel == packets * 1024");
  const meanSquare = energy / (samples * CHANNELS);
  assert.ok(meanSquare > 0.1, `decoded audio is near-silent (mean square ${meanSquare})`);
  console.log(
    `aac: ${packets.length} packets -> ${samples} samples/ch, mean square ${meanSquare.toFixed(3)}`
  );
}

async function aacRefusals(): Promise<void> {
  // An AAC session needs an OS decoder: elsewhere the codec is simply unsupported (status 4).
  const noAsc = AudioDecodeSession.open({ codec: "aac", sampleRate: SAMPLE_RATE, channels: CHANNELS });
  if (!hasAacDecoder) {
    await rejectsStatus(noAsc, 4, "AAC on a platform with no AAC decoder");
    console.log("aac refusals: no OS AAC decoder here; only the unsupported status was checked");
    return;
  }
  // Empty AudioSpecificConfig: a config mistake, not a missing capability.
  await rejectsStatus(noAsc, 5, "AAC open with no AudioSpecificConfig");
  await rejectsStatus(
    AudioDecodeSession.open({ codec: "aac", sampleRate: SAMPLE_RATE, channels: CHANNELS, extraData: Buffer.alloc(0) }),
    5,
    "AAC open with an empty AudioSpecificConfig"
  );

  // A real ASC opens; an empty packet is refused (Opus's loss-concealment hint means nothing here).
  const session = await AudioDecodeSession.open({
    codec: "aac",
    sampleRate: SAMPLE_RATE,
    channels: CHANNELS,
    extraData: Buffer.from([0x11, 0x90]), // AAC-LC, 48 kHz, stereo
  });
  await rejectsStatus(session.pushPacket({ pts: 0, duration: 1024, payload: Buffer.alloc(0) }), 5, "empty AAC packet");
  session.close();
  console.log("aac refusals: empty ASC and empty packet are invalid-input");
}

async function opusStillOpens(): Promise<void> {
  // Both spellings of the Opus open must keep working.
  const positional = await AudioDecodeSession.open(SAMPLE_RATE, 1, { num: 1, den: 50 });
  assert.equal(positional.codec, "opus");
  positional.close();
  const config = await AudioDecodeSession.open({
    codec: "opus",
    sampleRate: SAMPLE_RATE,
    channels: 1,
    timeBase: { num: 1, den: 50 },
  });
  config.close();
  // Opus needs the frame duration; without it there is nothing sensible to default to.
  await rejectsStatus(
    AudioDecodeSession.open({ codec: "opus", sampleRate: SAMPLE_RATE, channels: 1 }),
    5,
    "Opus open without a timeBase"
  );
  console.log("opus: positional and config forms open");
}

// ── capability probes ──────────────────────────────────────────────────────────

function checkInvariants(rows: EncoderCapability[]): void {
  for (const row of rows) {
    if (row.state === "supported") {
      assert.notEqual(row.pathClass, null, `a supported row must report its path class: ${JSON.stringify(row)}`);
    } else {
      assert.equal(row.pathClass, null, `pathClass is meaningful only when supported: ${JSON.stringify(row)}`);
    }
  }
}

async function probes(): Promise<void> {
  for (const codec of ["h264", "hevc"] as const) checkInvariants(await encoderSupport(codec, 1280, 720));

  // Zero dimensions are refused rather than probed.
  await rejectsStatus(encoderSupport("h264", 0, 720), 5, "encoderSupport with a zero width");

  const rows = await encoderSupport("h264", 1280, 720);
  if (isWindows) {
    assert.ok(rows.length > 0, "Windows reports at least one backend row for H.264");
    console.log(`probe: H.264 1280x720 rows ${JSON.stringify(rows)}`);

    // The answer must agree with actually opening an encoder.
    const anySupported = rows.some((r) => r.state === "supported");
    let opened = false;
    try {
      const session = await openAutoEncoder(AutoVideoEncodeConfig.defaults("h264", 1280, 720, { num: 1, den: 30 }));
      session.close();
      opened = true;
    } catch (error) {
      if (!(error instanceof EncoderUnavailableError)) throw error;
    }
    assert.equal(anySupported, opened, `probe said supported=${anySupported} but opening an H.264 encoder ${opened ? "worked" : "failed"}`);

    assert.equal(await decoderSupport("aac"), "supported", "Windows decodes AAC");
  }
  const h264 = await decoderSupport("h264");
  assert.notEqual(h264, "unknown", "the decoder probe answers with a known state");
  console.log(`probe: decoder h264 ${h264}, aac ${await decoderSupport("aac")}`);
}

layoutPin();
await streamingBytes();
await aacRoundTrip();
await aacRefusals();
await opusStillOpens();
await probes();
// `DecoderUnavailableError` is exported for callers; referenced so the import stays honest.
assert.ok(new DecoderUnavailableError(3, "x") instanceof MediawayError);
console.log("PASS: pipeline ABI v7 (streaming bytes, AAC decode, capability probes)");
