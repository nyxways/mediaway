"""Microphone capture quick start — raw PCM.

✅ REAL — the microphone capture capability is implemented in the native C ABI
(`mediaway_ffi`, `mediaway_audio_capture_*`); this example runs against
it. Mirrors `examples/device/capture_microphone.rs`: open the default mic,
poll ~2 seconds of raw interleaved f32le PCM frames, print the negotiated
format, close. No encoding — there is no audio encoder in the ABI.
"""

import time

from mediaway import AudioCapture


def main() -> None:
    try:
        mic = AudioCapture.open(sample_rate=48000)
    except Exception as err:  # DeviceUnavailableError etc. — expected outcome
        print(f"no microphone on this machine: {err}")
        return

    with mic:
        rate = mic.sample_rate()
        channels = mic.channels()
        print(f"mic negotiated: {rate} Hz, {channels} ch")

        frames = 0
        total_bytes = 0
        deadline = time.monotonic() + 2.0
        while time.monotonic() < deadline:
            data = mic.poll_pcm()
            if data is not None:
                total_bytes += len(data)
                frames += 1
            else:
                time.sleep(0.002)

    print(f"captured {frames} PCM frame(s), {total_bytes} bytes in 2 s")


if __name__ == "__main__":
    main()
