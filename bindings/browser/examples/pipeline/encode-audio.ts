/**
 * Encode microphone PCM -> AAC -> audio-only fragmented MP4 (WASM path).
 *
 * Status: 🚧 ASPIRATIONAL — no AAC encoder exists in the WASM module yet
 * (the C ABI gained audio encode in v2, adr/0003-auto-audio-encode-c-abi.md,
 * but `@mediaway/browser` has not shipped a wasm-bindgen surface for it; this
 * sketches the ideal API). Capture itself is native Web APIs (getUserMedia +
 * AudioWorklet). The audio-only encode scenario is fully real on the C-ABI
 * hosts (see bindings/{c,cpp,python,nodejs}/examples/pipeline/encode_audio.*).
 */
import {
  init,
  AudioEncoder,
  EncoderUnavailableError,
  type AudioEncodeConfig,
} from "@mediaway/browser";

// AudioWorklet adds PCM to a ring buffer; the encoder pulls from it. Ideal
// DX mirrors the native hosts: AudioEncoder.open() is single-step (the session
// IS the encoder), pushPcm() takes f32le chunks, pollPacket() yields AAC.
declare global {
  class AudioWorkletProcessor {}
  function registerProcessor(
    name: string,
    processorCtor: new () => AudioWorkletProcessor
  ): void;
}

const SAMPLE_RATE = 48_000;
const CHANNELS = 2;
const RECORD_MS = 2_000;

async function main(): Promise<void> {
  await init();

  let encoder: AudioEncoder;
  try {
    encoder = await AudioEncoder.open({ sampleRate: SAMPLE_RATE, channels: CHANNELS });
  } catch (error) {
    if (error instanceof EncoderUnavailableError) {
      console.log("no audio encode backend in this WASM build — exiting gracefully");
      return;
    }
    throw error;
  }

  const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  const audioCtx = new AudioContext({ sampleRate: SAMPLE_RATE });
  const source = audioCtx.createMediaStreamSource(stream);
  const worklet = await audioCtx.audioWorklet.addModule(
    new URL("./pcm-ring.worklet.js", import.meta.url)
  );
  const node = new AudioWorkletNode(audioCtx, "pcm-ring", {
    numberOfInputs: 1,
    numberOfOutputs: 0,
  });
  source.connect(node);

  // Pull PCM from the worklet ring and encode it.
  const chunks: Buffer[] = [];
  const startedAt = performance.now();
  while (performance.now() - startedAt < RECORD_MS) {
    const pcm = ringPop(worklet); // async: waits for the next worklet chunk
    if (pcm !== null) {
      await encoder.pushPcm({ pts: pcm.pts, data: pcm.data });
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  await encoder.flush();

  // Poll AAC packets and mux with the encoder's AudioSpecificConfig — same
  // shape as the native hosts' encode_audio examples (container::Muxer here
  // maps to the WASM muxer when it ships).
  const info = await encoder.streamInfo();
  console.log(
    `encoded ${chunks.length} PCM chunk(s) -> AAC, ASC ${info.extraData.length} bytes`
  );
  console.log(
    "muxing: addAudioTrack({ codec: 'aac', sampleRate, channels, extraData: info.extraData })"
  );

  await audioCtx.close();
}

main().catch((error) => {
  console.error("browser audio encode error:", error);
});
