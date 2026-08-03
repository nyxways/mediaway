// capture-microphone.ts — browser microphone capture quick start.
//
// REAL — capture is a native browser capability in this host (Tier C: WASM +
// Web APIs; the C ABI is never involved). Mediaway's WASM module does not wrap
// capture; the browser owns it via getUserMedia(). This is the browser analog
// of examples/device/capture_microphone.rs: open the default mic, observe ~2 s
// of audio levels, stop. (No Mediaway import needed — the whole capability is
// the platform's.)
//
// Run: open in a Chromium-based browser; requires microphone permission.

const CAPTURE_MS = 2_000;

async function main(): Promise<void> {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: {
      echoCancellation: false,
      noiseSuppression: false,
      autoGainControl: false,
    },
  });

  const audioContext = new AudioContext();
  const source = audioContext.createMediaStreamSource(stream);
  const analyser = audioContext.createAnalyser();
  analyser.fftSize = 2048;
  source.connect(analyser);

  const levels = new Uint8Array(analyser.frequencyBinCount);
  let samples = 0;
  let peak = 0;

  const startedAt = performance.now();
  while (performance.now() - startedAt < CAPTURE_MS) {
    analyser.getByteTimeDomainData(levels);
    let framePeak = 0;
    for (let i = 0; i < levels.length; i++) {
      const deviation = Math.abs(levels[i] - 128);
      if (deviation > framePeak) framePeak = deviation;
    }
    peak = Math.max(peak, framePeak);
    samples++;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }

  console.log(
    `captured ${samples} level samples in ~${CAPTURE_MS} ms; peak deviation ${peak}/128` +
      (peak > 4 ? " (audio present)" : " (silence)")
  );

  stream.getTracks().forEach((track) => track.stop());
  await audioContext.close();
}

main().catch((err) => {
  console.error(err);
});
