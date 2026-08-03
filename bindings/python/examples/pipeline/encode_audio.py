"""AAC-encode 96 synthetic F32 stereo frames (440 Hz sine) and mux the result
into an audio-only fragmented MP4.

✅ REAL — the audio encode ABI (v2, adr/0003) is implemented in the native C
ABI (`mediaway_ffi`); this example runs against it. Only the PCM
itself is synthetic (deterministic sine, no microphone needed).

Flow: AudioEncoder.open() returns the encode session directly (single step —
no intermediate handle, no consumption trap), we push PCM, flush, poll the AAC
packets, then register an audio track with the encoder's AudioSpecificConfig
(`stream_info()`) and mux. The ASC materializes only after the first pushed
frame, so the call order is push -> stream_info -> mux (adr/0003).
"""

import dataclasses
import math
import struct

from mediaway import (
    AudioEncoder,
    EncoderUnavailableError,
    Muxer,
    Rational,
)

SAMPLE_RATE = 48000
CHANNELS = 2
FRAME_SAMPLES = 1024  # ~21 ms per pushed frame
FRAME_COUNT = 96  # ~2.0 s of audio


def sine_frame(frame_index: int) -> bytes:
    """One interleaved f32le stereo frame of a 440 Hz sine."""
    out = bytearray()
    for s in range(FRAME_SAMPLES):
        t = (frame_index * FRAME_SAMPLES + s) / SAMPLE_RATE
        v = math.sin(2.0 * math.pi * 440.0 * t)
        out += struct.pack("<f", v) * CHANNELS
    return bytes(out)


def main() -> None:
    try:
        encoder = AudioEncoder.open(sample_rate=SAMPLE_RATE, channels=CHANNELS)
    except EncoderUnavailableError:
        print("no audio encode backend (EncoderUnavailableError) - exiting gracefully")
        return

    for i in range(FRAME_COUNT):
        encoder.push_pcm(sine_frame(i))
    encoder.flush()

    packets = []
    while True:
        packet = encoder.poll_packet()
        if packet is None:
            break
        packets.append(packet)
    if not packets:
        print(f"encoder produced no packets for {FRAME_COUNT} PCM frames")
        return

    info = encoder.stream_info()  # AudioSpecificConfig materialized after the first push
    if not info.extra_data:
        print("stream info carries no AudioSpecificConfig")
        return
    print(f"encoded {len(packets)} AAC packet(s), ASC {len(info.extra_data)} bytes")

    muxer = Muxer()
    audio_track = muxer.add_audio_track(info)
    live = muxer.begin()
    for packet in packets:
        live.push_packet(dataclasses.replace(packet, stream_index=audio_track))
    live.flush()

    out = live.poll_bytes()
    print(f"muxed {len(packets)} AAC packet(s) into {len(out)} bytes of "
          f"audio-only fragmented MP4")


if __name__ == "__main__":
    main()
