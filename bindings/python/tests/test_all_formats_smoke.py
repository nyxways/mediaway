"""RC-stage binding check: round-trip the 7 non-MP4 container formats wired
into this binding (WebM/Ogg/ADTS/FLV/MPEG-TS/MP3/WAV) against the real
mediaway_ffi.dll. Every payload/expected value mirrors a verified round trip
already checked in the Rust FFI smoke tests and the C++/C# bindings' own
smoke tests — not invented here.

Run from bindings/python (the DLL must be staged at
mediaway/_native/mediaway_ffi.dll):

    python tests/test_all_formats_smoke.py

A failed assertion raises AssertionError and exits nonzero.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from mediaway import (
    AdtsDemuxer,
    AdtsMuxer,
    AudioStreamInfo,
    ChannelMode,
    Codec,
    ContainerFormat,
    Demuxer,
    FLV_AUDIO_TRACK_ID,
    FLV_VIDEO_TRACK_ID,
    FlvDemuxer,
    FlvMuxer,
    Mp3Demuxer,
    Mp3FrameHeader,
    Mp3Muxer,
    MpegVersion,
    Muxer,
    OggDemuxer,
    OggMuxer,
    Packet,
    Rational,
    RawPacket,
    TsDemuxer,
    TsElementaryStream,
    TsMuxer,
    VideoStreamInfo,
    WavMuxer,
    wav_parse,
)


def smoke_webm() -> None:
    with Muxer(format=ContainerFormat.WEBM) as muxer:
        # Track id starts at 1, not 0 — WebM/Matroska's TrackNumber must not be 0.
        track_id = muxer.add_video_track(
            VideoStreamInfo(codec=Codec.VP8, width=64, height=64, frame_rate=Rational(1, 30))
        )
        assert track_id == 1

        webm_bytes = b""
        with muxer.begin() as live:
            for i in range(5):
                live.push_packet(
                    Packet(stream_index=track_id, pts=Rational(i, 30), dts=Rational(i, 30), key=(i == 0), payload=b"\xAA" * 16)
                )
                chunk = live.poll_bytes()
                if chunk:
                    webm_bytes += chunk
            live.flush()
            tail = live.poll_bytes()
            if tail:
                webm_bytes += tail

    assert len(webm_bytes) > 4 and webm_bytes[0] == 0x1A and webm_bytes[1] == 0x45, "EBML magic present"

    with Demuxer(format=ContainerFormat.WEBM) as demuxer:
        demuxer.push_bytes(webm_bytes)
        count = 0
        while demuxer.poll_packet() is not None:
            count += 1
    assert count == 5, f"expected 5 WebM packets, got {count}"


def smoke_ogg() -> None:
    head = bytearray(b"OpusHead")
    head += bytes([1, 2, 0, 0])  # version, channels, pre-skip
    head += (48000).to_bytes(4, "little")
    head += bytes([0, 0, 0])  # output gain, channel mapping family

    with OggMuxer(1) as muxer:
        muxer.push_packet(RawPacket(stream_id=0, pts=0, dts=0, key=True, payload=bytes(head)))
        ogg_bytes = muxer.poll_bytes() or b""
        muxer.push_packet(RawPacket(stream_id=0, pts=960, dts=960, key=True, payload=b"\x01\x02\x03\x04"))
        chunk = muxer.poll_bytes()
        if chunk:
            ogg_bytes += chunk
        muxer.flush()

    assert len(ogg_bytes) > 4 and ogg_bytes[0:2] == b"Og", "capture pattern present"

    with OggDemuxer() as demuxer:
        demuxer.push_bytes(ogg_bytes)
        packet = demuxer.poll_packet()
        assert packet is not None and len(packet.payload) == 4 and packet.pts == 960, "Opus packet recovered"


def smoke_adts() -> None:
    raw_aac = b"\xAB" * 100
    with AdtsMuxer(44100, 2) as muxer:
        for _ in range(2):
            muxer.push_packet(RawPacket(stream_id=0, pts=0, dts=0, key=True, payload=raw_aac))
        muxer.flush()
        adts_bytes = muxer.poll_bytes() or b""

    assert len(adts_bytes) > 2 and adts_bytes[0] == 0xFF and (adts_bytes[1] & 0xF0) == 0xF0, "sync word present"

    with AdtsDemuxer() as demuxer:
        demuxer.push_bytes(adts_bytes)
        expected_pts = 0
        for _ in range(2):
            packet = demuxer.poll_packet()
            assert packet is not None and packet.pts == expected_pts and len(packet.payload) == 100
            expected_pts += 1024


def smoke_flv() -> None:
    with FlvMuxer() as muxer:
        flv_bytes = muxer.write_header(has_audio=True, has_video=True)

        muxer.add_video_track(
            VideoStreamInfo(
                codec=Codec.H264, width=1280, height=720, frame_rate=Rational(1, 1000),
                extra_data=bytes([1, 0x42, 0, 0x1E, 0xFF, 0xE1, 0, 0]),
            )
        )
        muxer.add_audio_track(
            AudioStreamInfo(codec=Codec.AAC, sample_rate=44100, channels=2, extra_data=bytes([0x12, 0x10]))
        )

        flv_bytes += muxer.push_packet(
            RawPacket(stream_id=FLV_VIDEO_TRACK_ID, pts=45, dts=33, key=True, payload=bytes([0, 0, 0, 2, 0x65, 0x88]))
        )
        flv_bytes += muxer.push_packet(
            RawPacket(stream_id=FLV_AUDIO_TRACK_ID, pts=23, dts=23, key=True, payload=bytes([1, 2, 3, 4]))
        )

    assert flv_bytes[0:3] == b"FLV", "file signature present"

    with FlvDemuxer() as demuxer:
        demuxer.push_bytes(flv_bytes)
        got_video = got_audio = False
        while True:
            packet = demuxer.poll_packet()
            if packet is None:
                break
            if packet.stream_id == FLV_VIDEO_TRACK_ID:
                got_video = True
            if packet.stream_id == FLV_AUDIO_TRACK_ID:
                got_audio = True
    assert got_video and got_audio, "both tracks recovered"


def smoke_ts() -> None:
    video_pid, audio_pid = 0x100, 0x101
    streams = [
        TsElementaryStream(pid=video_pid, codec=Codec.H264),
        TsElementaryStream(pid=audio_pid, codec=Codec.AAC),
    ]

    with TsMuxer(1, 0x1000, streams) as muxer:
        ts_bytes = muxer.write_pat_pmt()
        ts_bytes += muxer.write_access_unit(video_pid, bytes([0, 0, 0, 1, 0x65, 0x88]), 90000, None, True)
        ts_bytes += muxer.write_access_unit(video_pid, bytes([0, 0, 0, 1, 0x41]), 90033, None, False)

    with TsDemuxer() as demuxer:
        demuxer.push_bytes(ts_bytes)
        packet = demuxer.poll_packet()
        assert packet is not None and packet.pts == 90000 and packet.key, "video access unit recovered"

    # finish() recovers a trailing access unit with no confirming marker.
    with TsMuxer(1, 0x1000, streams) as muxer2:
        ts_bytes2 = muxer2.write_pat_pmt()
        tail = bytes([9, 9, 9])
        ts_bytes2 += muxer2.write_access_unit(video_pid, tail, 90000, None, True)

    with TsDemuxer() as demuxer2:
        demuxer2.push_bytes(ts_bytes2)
        assert demuxer2.poll_packet() is None, "no packet ready before finish()"
        finished = demuxer2.finish()
        assert len(finished) == 1 and finished[0].payload == tail, "finish() recovers trailing AU"


def smoke_mp3() -> None:
    header = Mp3FrameHeader(version=MpegVersion.MPEG1, bitrate_kbps=128, sample_rate=44100, channel_mode=ChannelMode.STEREO)
    # frame_len(false) for 128kbps/44100Hz = floor(144000*128/44100) = 417; body = 417-4 = 413.
    body = b"\xAB" * 413
    with Mp3Muxer(header) as muxer:
        mp3_bytes = muxer.write_frame(body, padding=False)
    assert mp3_bytes[0] == 0xFF, "frame sync byte present"

    with Mp3Demuxer() as demuxer:
        demuxer.push_bytes(mp3_bytes)
        packet = demuxer.poll_packet()
        assert packet is not None and len(packet.payload) == 413, "frame recovered"


def smoke_wav() -> None:
    pcm = bytes([1, 2, 3, 4, 5, 6, 7, 8])
    with WavMuxer(44100, 2, 16) as muxer:
        muxer.push_packet(RawPacket(stream_id=0, pts=0, dts=0, key=True, payload=pcm))
        wav_bytes = muxer.finish()
        assert wav_bytes[0:1] == b"R" and wav_bytes[8:9] == b"W", "RIFF/WAVE header present"

        info, packet = wav_parse(wav_bytes)
        assert isinstance(info, AudioStreamInfo) and info.sample_rate == 44100 and info.channels == 2
        assert packet.payload == pcm, "parsed packet payload matches"

        # A second finish() fails honestly rather than corrupting anything.
        try:
            muxer.finish()
            raised = False
        except Exception:
            raised = True
        assert raised, "second finish() must raise"


def main() -> None:
    smoke_webm()
    smoke_ogg()
    smoke_adts()
    smoke_flv()
    smoke_ts()
    smoke_mp3()
    smoke_wav()
    print("PASS: all 7 newly-wired container formats verified")


if __name__ == "__main__":
    main()
