/**
 * WebCodecs AAC encode -> audio-only fMP4.
 *
 * Status: ✅ REAL — @mediaway/browser (ADR-0020). The browser's native
 * WebCodecs `AudioEncoder` produces AAC; `EncodeSession` pulls the
 * AudioSpecificConfig from the first output's metadata (the browser analog of
 * the C ABI's push → stream_info → mux order, adr/0003) and muxes an `aac`
 * track into a fragmented MP4.
 *
 * Encodes 2 s of synthetic 48 kHz mono PCM (a sine sweep) to AAC, muxes, then
 * demuxes and counts packets. BROWSER ONLY — `AudioEncoder`/`AudioData` are
 * WebCodecs APIs, not available in Node.
 */
import { init, Muxer, Demuxer, EncodeSession, type Sample } from "@mediaway/browser";

const SAMPLE_RATE = 48_000;
const CHANNELS = 1;
const DURATION_S = 2;

async function main(): Promise<void> {
  await init(); // browsers: resolves the packaged wasm URL automatically

  const muxer = new Muxer(1);
  const session = new EncodeSession(muxer);
  const encoder = await session.audio({
    codec: "mp4a.40.2", // AAC-LC
    sampleRate: SAMPLE_RATE,
    numberOfChannels: CHANNELS,
    bitrate: 128_000,
  });

  // Synthesize 2 s of PCM: 1024-sample AudioData chunks, f32 planar.
  const chunkFrames = 1024;
  const chunks = Math.ceil((SAMPLE_RATE * DURATION_S) / chunkFrames);
  for (let c = 0; c < chunks; c += 1) {
    const pcm = new Float32Array(chunkFrames);
    for (let s = 0; s < chunkFrames; s += 1) {
      const t = (c * chunkFrames + s) / SAMPLE_RATE;
      // 440 Hz sine, slightly rising amplitude so the encoder sees variation.
      pcm[s] = 0.5 * Math.sin(2 * Math.PI * 440 * t) * (0.5 + 0.5 * t / DURATION_S);
    }
    const data = new AudioData({
      format: "f32-planar",
      sampleRate: SAMPLE_RATE,
      numberOfFrames: chunkFrames,
      numberOfChannels: CHANNELS,
      timestamp: c * chunkFrames * 1_000_000 / SAMPLE_RATE,
      data: pcm,
    });
    encoder.encode(data);
    data.close();
  }

  const mp4 = await session.finish();
  console.log(`encode-audio: muxed ${mp4.length} bytes (${chunks} AAC chunks)`);
  muxer.free();

  // --- Demux roundtrip ---
  const demuxer = new Demuxer();
  demuxer.pushBytes(mp4);
  const streams = demuxer.streams();
  let recovered = 0;
  for (let packet: Sample | null = demuxer.pollPacket(); packet !== null; packet = demuxer.pollPacket()) {
    recovered += 1;
  }
  demuxer.free();
  console.log(
    `encode-audio: ${streams.length} stream(s) ` +
      `(${streams.map((s) => `${s.codec} ${s.timeBase.num}/${s.timeBase.den} extraData=${s.extraData.length}B`).join(", ")}), ` +
      `${recovered} packets recovered`,
  );
  if (streams.length !== 1 || streams[0].codec !== "aac" || recovered !== chunks) {
    throw new Error("encode-audio: unexpected roundtrip result");
  }
}

main().catch((err) => {
  console.error("encode-audio failed:", err);
});
