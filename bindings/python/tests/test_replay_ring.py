"""Binding check for the replay ring and MP4 payload placements
(`crates/mediaway-ffi/adr/container/0009-replay-ring-c-abi.md`).

Everything is hermetic: synthetic packets whose payload names the frame they
belong to, no encoder and no files. Pure CPU, no hardware required. Run from
bindings/python with the native library built (`cargo build -p mediaway-ffi`):

    python tests/test_replay_ring.py

A failed assertion raises AssertionError and exits nonzero.
"""

import ctypes
import os
import sys
from datetime import timedelta

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from mediaway import (
    Codec,
    ContainerFormat,
    InvalidStateError,
    MediawayError,
    Muxer,
    OutOfOrderPacketError,
    Packet,
    PacketMeta,
    Rational,
    ReplayRing,
    StoredPayload,
    UnknownStreamError,
    VideoStreamInfo,
    _ffi,
    _ffi_replay,
)

VIDEO = 1
AUDIO = 2
FPS = Rational(1, 30)
AUDIO_RATE = Rational(1, 48000)


def frame_byte(n: int) -> int:
    return n % 256


def video_packet(n: int) -> Packet:
    """Frame n of a 30 fps stream, a keyframe every 30 frames; the payload is the frame number."""
    return Packet(
        stream_index=VIDEO,
        pts=Rational(n, 30),
        payload=bytes([frame_byte(n)]) * 4,
        key=n % 30 == 0,
        duration=Rational(1, 30),
    )


def audio_packet(k: int) -> Packet:
    return Packet(
        stream_index=AUDIO,
        pts=Rational(k * 1024, 48000),
        payload=b"\xa0" * 3,
        key=True,
        duration=Rational(1024, 48000),
    )


def seconds(r: Rational) -> float:
    return r.num / r.den


# ── layout: pinned against a gcc probe of the real container.h ───────────────


def test_layout_matches_the_header() -> None:
    def off(struct, field):
        return getattr(struct, field).offset

    if ctypes.sizeof(ctypes.c_void_p) != 8:
        return  # the pinned numbers are the 64-bit ones

    C = _ffi_replay.ReplayRingConfig
    assert ctypes.sizeof(C) == 48
    assert [off(C, f) for f in ("anchor_stream_id", "anchor_time_base", "window_ms", "max_bytes", "payload_kind")] == [
        0, 8, 24, 32, 40,
    ]

    M = _ffi_replay.PacketMeta
    assert ctypes.sizeof(M) == 40
    assert [off(M, f) for f in ("stream_id", "pts", "dts", "duration", "is_keyframe", "is_discard")] == [
        0, 8, 16, 24, 32, 33,
    ]

    S = _ffi_replay.StoredPayload
    assert ctypes.sizeof(S) == 24
    assert [off(S, f) for f in ("file", "offset", "len")] == [0, 8, 16]

    E = _ffi_replay.ReplayClipEntry
    assert ctypes.sizeof(E) == 80
    assert [
        off(E, f)
        for f in (
            "stream_id", "pts", "dts", "duration", "is_keyframe", "is_discard", "payload_kind",
            "payload", "payload_len", "stored_file", "stored_offset", "stored_len",
        )
    ] == [0, 8, 16, 24, 32, 33, 36, 40, 48, 56, 64, 72]

    P = _ffi_replay.Placement
    assert ctypes.sizeof(P) == 32
    assert [off(P, f) for f in ("track_id", "dts", "offset", "len")] == [0, 8, 16, 24]


def test_abi_version_is_eight() -> None:
    assert _ffi_replay.CONTAINER_ABI_VERSION == 8
    assert _ffi.container.dll.mediaway_container_ffi_abi_version() == 8, (
        "the loaded native library is not ABI 8 — rebuild it (a stale DLL shadows the fresh one?)"
    )
    assert _ffi_replay.REPLAY_PAYLOAD_BYTES == 0 and _ffi_replay.REPLAY_PAYLOAD_STORED == 1


# ── ring behaviour ───────────────────────────────────────────────────────────


def test_a_clip_starts_at_a_keyframe_rebased_to_zero_and_carries_every_stream() -> None:
    with ReplayRing(VIDEO, FPS, timedelta(seconds=2)) as ring:
        ring.add_stream(AUDIO, AUDIO_RATE)
        for n in range(180):  # 6 s of video with matching audio
            ring.push(video_packet(n))
            if n % 2 == 0:
                ring.push(audio_packet(n // 2 * 3))
        assert ring.span() >= timedelta(seconds=2), ring.span()

        with ring.clip_last(2.0) as clip:
            assert clip is not None and len(clip) > 0
            entries = list(clip)
            video = [e for e in entries if e.stream_index == VIDEO]
            first = video[0]
            assert first.key and seconds(first.pts) == 0 and seconds(first.dts) == 0
            assert any(e.stream_index == AUDIO for e in entries), "audio was cut with the video"

            # Decode order across streams, and every payload is the frame it claims to be.
            dts = [seconds(e.dts) for e in entries]
            assert dts == sorted(dts), "clip entries are in decode order"
            cut_frame = 180 - len(video)
            assert cut_frame % 30 == 0, f"the clip starts on a GOP boundary, frame {cut_frame}"
            for e in video:
                frame = round(seconds(e.dts) * 30)
                assert e.payload == bytes([frame_byte(cut_frame + frame)]) * 4
                assert e.stored is None
            assert clip.duration >= timedelta(seconds=2), clip.duration
            assert clip[0] == entries[0] and clip[-1] == entries[-1]


def test_a_clip_is_an_owned_snapshot_that_outlives_pushes_and_the_ring() -> None:
    ring = ReplayRing(VIDEO, FPS, 1.0)
    for n in range(90):
        ring.push(video_packet(n))
    clip = ring.clip_last(1.0)
    assert clip is not None
    before = list(clip)
    for n in range(90, 400):  # evict everything the clip was cut from
        ring.push(video_packet(n))
    ring.close()
    ring.close()  # closing twice is safe
    assert list(clip) == before, "a clip must not change when the ring moves on or closes"
    clip.close()
    clip.close()
    try:
        len(clip)
    except InvalidStateError:
        pass
    else:
        raise AssertionError("a closed clip must raise")


def test_no_clip_until_the_anchor_has_a_keyframe() -> None:
    with ReplayRing(VIDEO, FPS, 1.0) as ring:
        ring.push(video_packet(5))  # not a keyframe: dropped without error
        assert ring.clip_last(1.0) is None
        assert ring.span() == timedelta(0)


def test_a_stored_ring_hands_back_locations_not_bytes() -> None:
    with ReplayRing(VIDEO, FPS, 2.0, payload="stored") as ring:
        for n in range(100):
            ring.push_stored(
                PacketMeta(stream_index=VIDEO, pts=Rational(n, 30), key=n % 30 == 0, duration=Rational(1, 30)),
                StoredPayload(file=7, offset=n * 100, length=40),
            )
        with ring.clip_last(1.0) as clip:
            entries = list(clip)
            cut = 99 - round(seconds(entries[-1].dts) * 30)
            for e in entries:
                assert e.payload is None
                assert e.stored == StoredPayload(7, (cut + round(seconds(e.dts) * 30)) * 100, 40)
            try:
                entries[0].to_packet()
            except ValueError:
                pass
            else:
                raise AssertionError("a Stored entry has no payload to make a Packet from")


def test_a_byte_ceiling_evicts_oldest_gop_first() -> None:
    with ReplayRing(VIDEO, FPS, 60.0, max_bytes=30 * 4 * 2) as ring:  # about two GOPs of 4-byte frames
        for n in range(300):
            ring.push(video_packet(n))
        with ring.clip_last(60.0) as clip:
            assert len(clip) <= 3 * 30, f"a two-GOP ceiling must bound the ring, holds {len(clip)}"


def test_a_ring_refuses_what_it_cannot_hold_and_says_why() -> None:
    with ReplayRing(VIDEO, FPS, 1.0) as bytes_ring, ReplayRing(VIDEO, FPS, 1.0, payload="stored") as stored_ring:
        # Wrong payload kind, both ways.
        try:
            stored_ring.push(video_packet(0))
        except InvalidStateError:
            pass
        else:
            raise AssertionError("push on a Stored ring must be InvalidStateError")
        try:
            bytes_ring.push_stored(PacketMeta(VIDEO, Rational(0, 1), key=True), StoredPayload(0, 0, 1))
        except InvalidStateError:
            pass
        else:
            raise AssertionError("push_stored on a Bytes ring must be InvalidStateError")

        # dts went backwards: a distinct, droppable error, and the ring carries on.
        bytes_ring.push(video_packet(0))
        bytes_ring.push(video_packet(10))
        try:
            bytes_ring.push(video_packet(4))
        except OutOfOrderPacketError as e:
            assert e.status == _ffi.MEDIAWAY_STATUS_INVALID_PACKET
        else:
            raise AssertionError("an out-of-order dts must be OutOfOrderPacketError")
        bytes_ring.push(video_packet(11))

        # A stream nobody added — refused before any native call, and by the native ring.
        try:
            bytes_ring.push(audio_packet(0))
        except UnknownStreamError:
            pass
        else:
            raise AssertionError("an unknown stream must be UnknownStreamError")

        # The anchor is already there; a degenerate timebase is refused.
        try:
            bytes_ring.add_stream(VIDEO, FPS)
        except MediawayError as e:
            assert e.status == _ffi.MEDIAWAY_STATUS_INVALID_TRACK
        else:
            raise AssertionError("a duplicate stream must be refused")
        try:
            bytes_ring.add_stream(AUDIO, Rational(0, 1))
        except MediawayError as e:
            assert e.status == _ffi.MEDIAWAY_STATUS_INVALID_ARGUMENT
        else:
            raise AssertionError("a zero timebase must be refused")
        try:
            ReplayRing(VIDEO, Rational(0, 1), 1.0)
        except MediawayError as e:
            assert e.status == _ffi.MEDIAWAY_STATUS_INVALID_ARGUMENT
        else:
            raise AssertionError("a zero anchor timebase must be refused")


def test_argument_and_lifetime_rules() -> None:
    for bad in (
        lambda: ReplayRing(VIDEO, FPS, 1.0, payload="disk"),
        lambda: ReplayRing(VIDEO, FPS, -1.0),
        lambda: ReplayRing(VIDEO, FPS, 1.0, max_bytes=-1),
    ):
        try:
            bad()
        except ValueError:
            pass
        else:
            raise AssertionError("a bad argument must be ValueError")
    ring = ReplayRing(VIDEO, FPS, 1.0)
    ring.push(video_packet(0))
    with ring.clip_last(1.0) as clip:
        try:
            clip[5]
        except IndexError:
            pass
        else:
            raise AssertionError("an out-of-range index must be IndexError")
    ring.close()
    try:
        ring.push(video_packet(1))
    except InvalidStateError:
        pass
    else:
        raise AssertionError("a closed ring must raise")


# ── MP4 payload placements ───────────────────────────────────────────────────


def mux_packets(create, count: int, payload_for) -> tuple[bytes, list]:
    """Mux `count` opaque video packets with the muxer made by `create()`."""
    muxer = create()
    track = muxer.add_video_track(VideoStreamInfo(codec=Codec.H264, width=64, height=64, frame_rate=FPS))
    assert track == VIDEO
    chunks, placements = [], []
    with muxer.begin() as live:
        for i in range(count):
            live.push_packet(Packet(stream_index=track, pts=Rational(i, 30), payload=payload_for(i), key=i == 0))
        live.flush()
        while (chunk := live.poll_bytes()) is not None:
            chunks.append(chunk)
        placements = live.poll_placements()
    return b"".join(chunks), placements


def test_placements_point_at_the_real_bytes_and_change_nothing() -> None:
    # No zero bytes, so nothing here can look like an Annex-B start code and be rewritten.
    def payload(i: int) -> bytes:
        return bytes([(i % 200) + 20]) * (32 + i)

    count = 60
    plain, none = mux_packets(Muxer, count, payload)
    file, placements = mux_packets(Muxer.create_with_placements, count, payload)

    assert none == [], "a plain MP4 muxer records no placements"
    assert file == plain, "recording placements must not change the output bytes"
    assert len(placements) == count, "one placement per sample"
    for i, pl in enumerate(placements):
        assert pl.stream_index == VIDEO
        assert pl.dts.num * 30 == i * pl.dts.den, f"placement {i} has dts {pl.dts}"
        assert file[pl.offset : pl.offset + pl.length] == payload(i), f"placement {i} points at the wrong bytes"


def test_a_webm_muxer_cannot_record_placements() -> None:
    muxer = Muxer(format=ContainerFormat.WEBM)
    muxer.add_video_track(VideoStreamInfo(codec=Codec.VP9, width=64, height=64, frame_rate=FPS))
    with muxer.begin() as live:
        try:
            live.poll_placements()
        except InvalidStateError:
            pass
        else:
            raise AssertionError("WebM has no placements: InvalidStateError")
    for bad in (
        lambda: Muxer(placements=True, format=ContainerFormat.WEBM),
        lambda: Muxer(placements=True, fragment_batch=2),
    ):
        try:
            bad()
        except ValueError:
            pass
        else:
            raise AssertionError("placements are MP4-default-only")


def test_a_stored_ring_fed_from_placements_reads_back_the_bytes() -> None:
    def payload(i: int) -> bytes:
        return bytes([(i % 200) + 20]) * (32 + i)

    count = 120
    file, placements = mux_packets(Muxer.create_with_placements, count, payload)
    with ReplayRing(VIDEO, FPS, 1.5, payload="stored") as ring:
        for i, pl in enumerate(placements):
            ring.push_stored(
                PacketMeta(stream_index=VIDEO, pts=Rational(i, 30), key=i % 30 == 0),
                StoredPayload(file=1, offset=pl.offset, length=pl.length),
            )
        with ring.clip_last(1.0) as clip:
            assert len(clip) >= 30
            cut = count - len(clip)
            for e in clip:
                i = cut + round(seconds(e.dts) * 30)
                assert file[e.stored.offset : e.stored.offset + e.stored.length] == payload(i)


TESTS = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]

if __name__ == "__main__":
    for test in TESTS:
        test()
        print(f"ok  {test.__name__}")
    print(f"PASS: {len(TESTS)} replay ring + placements checks")
