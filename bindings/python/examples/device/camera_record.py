"""Record ~3 seconds from the default camera and microphone, encode to H.264 +
AAC, and remux both into one two-track MP4 (out.mp4).

✅ REAL — camera capture, auto H.264 encode, audio encode (ABI v2, adr/0003)
and the container remux are all implemented in the native C ABI
(`mediaway_device_ffi` + `mediaway_pipeline_ffi` + `mediaway_container_ffi`);
this example runs against them. The old "drained, not muxed" gap is gone.

Flow: camera frames go through the auto video encoder's internal MP4 session
(video-only by design); mic PCM goes through AudioEncoder (single-step open —
the session IS the encoder). Then we REMUX: demux the video session's fMP4 and
mux video + AAC audio into one two-track fMP4, registering the audio track
with the encoder's AudioSpecificConfig (`stream_info()`, available after the
first pushed frame). Without a mic/audio backend, the result is video-only.
Missing hardware degrades gracefully: no status-code checks, just exceptions.
"""

import dataclasses
from contextlib import nullcontext
from pathlib import Path
import time

from mediaway import (
    AudioCapture,
    AudioEncoder,
    AutoVideoEncoder,
    Demuxer,
    DeviceUnavailableError,
    EncodeSession,
    EncoderUnavailableError,
    Muxer,
    Rational,
    VideoCapture,
    VideoStreamInfo,
)

RECORD_SECONDS = 3.0
MIC_DRAIN_SECONDS = 0.25  # encode a short window of buffered PCM after recording


def fmt_rational(r: Rational) -> str:
    """Render a Rational as a compact "num/den" string."""
    return f"{r.num}/{r.den}"


def main() -> None:
    try:
        camera = VideoCapture.open("camera", index=0, frame_rate=Rational(1, 30))
    except DeviceUnavailableError as err:
        print(f"camera unavailable: {err}; nothing to record")
        return

    with camera:
        # The capture backend negotiates the real geometry, which may differ
        # from what we asked for — encode at what we actually got.
        width, height = camera.size()
        fps = camera.frame_rate()
        print(f"negotiated geometry: {width}x{height} @ {fmt_rational(fps)}")

        try:
            encoder = AutoVideoEncoder.pick(
                width=width, height=height, frame_rate=fps
            )
        except EncoderUnavailableError as err:
            print(f"no encoder available for {width}x{height}: {err}")
            return
        print(f"picked encoder: {encoder.name} ({encoder.codec.name})")

        # Mic is optional: without it we record video only.
        mic = None
        audio_encoder = None
        try:
            mic = AudioCapture.open("mic", 48000)
            print(f"mic open: {mic.sample_rate()} Hz, {mic.channels()} ch")
        except DeviceUnavailableError as err:
            print(f"mic unavailable ({err}); recording video only")

        if mic is not None:
            try:
                # Encode at the mic's NEGOTIATED format (a mono mic is not
                # the AAC sugar's default stereo).
                audio_encoder = AudioEncoder.open(
                    sample_rate=mic.sample_rate(), channels=mic.channels()
                )
            except EncoderUnavailableError as err:
                print(f"no audio encoder ({err}); recording video only")
                audio_encoder = None

        with (
            mic or nullcontext(),
            EncodeSession(encoder) as session,
            audio_encoder or nullcontext(),
        ):
            deadline = time.monotonic() + RECORD_SECONDS
            frames_recorded = 0
            pcm_bytes = 0
            while time.monotonic() < deadline:
                frame = camera.poll_frame(timeout=0.05)
                if frame is not None:
                    session.push_frame(frame)
                    frames_recorded += 1

                if audio_encoder is not None:
                    pcm = mic.poll_pcm(timeout=0.0)
                    if pcm is not None:
                        audio_encoder.push_pcm(pcm)  # pts auto-advances
                        pcm_bytes += len(pcm)

            # Encode whatever the mic still had buffered during the loop.
            if audio_encoder is not None:
                drain_until = time.monotonic() + MIC_DRAIN_SECONDS
                while time.monotonic() < drain_until:
                    pcm = mic.poll_pcm(timeout=0.05)
                    if pcm is not None:
                        audio_encoder.push_pcm(pcm)
                        pcm_bytes += len(pcm)

            # Audio encode finish must happen inside the `with` block — the
            # context manager closes the encoder on exit (close is always safe,
            # but a closed handle rejects further calls).
            audio_packets = []
            audio_info = None
            if audio_encoder is not None:
                audio_encoder.flush()
                while True:
                    packet = audio_encoder.poll_packet()
                    if packet is None:
                        break
                    audio_packets.append(packet)
                audio_info = audio_encoder.stream_info()  # ASC materialized after the first push

            mp4 = session.finish()  # terminal: consumes the session

        # ---- Remux video + AAC into one two-track MP4 ------------------------
        have_audio = bool(audio_packets) and bool(audio_info and audio_info.extra_data)

        if have_audio:
            demuxer = Demuxer()
            demuxer.push_bytes(mp4)
            streams = demuxer.streams()
            if len(streams) != 1 or not isinstance(streams[0], VideoStreamInfo):
                raise RuntimeError(
                    "expected exactly one video stream from the encode session's fMP4"
                )

            muxer = Muxer()
            video_track = muxer.add_video_track(streams[0])
            audio_track = muxer.add_audio_track(audio_info)
            live = muxer.begin()
            while True:
                packet = demuxer.poll_packet()
                if packet is None:
                    break
                live.push_packet(dataclasses.replace(packet, stream_index=video_track))
            for packet in audio_packets:
                live.push_packet(dataclasses.replace(packet, stream_index=audio_track))
            live.flush()
            mp4 = live.poll_bytes()

    out_path = Path("out.mp4")
    out_path.write_bytes(mp4)
    if have_audio:
        print(
            f"recorded {frames_recorded} frames over {RECORD_SECONDS:.0f} s + "
            f"{len(audio_packets)} AAC packets from {pcm_bytes} PCM bytes -> "
            f"{out_path} ({len(mp4)} bytes, two tracks)"
        )
    else:
        print(
            f"recorded {frames_recorded} frames over {RECORD_SECONDS:.0f} s "
            f"(audio unavailable) -> {out_path} ({len(mp4)} bytes, video only)"
        )


if __name__ == "__main__":
    main()
