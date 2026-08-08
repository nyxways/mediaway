"""Container capability: FLV mux + demux (adr/container/0005-flv-c-abi.md).

Unlike `LiveMuxer`, every write method here returns its own freshly allocated
output buffer directly — there is no separate `poll_bytes` step. FLV has
exactly one video and one audio slot (no track-id field in the format
itself); `add_video_track`/`add_audio_track` ignore the info's own `id` and
the fixed stream ids `VIDEO_TRACK_ID`/`AUDIO_TRACK_ID` are used instead.
"""

from __future__ import annotations

from ctypes import byref, c_bool, c_size_t, cast, create_string_buffer

from . import _ffi
from ._container import _check_container, _copy_bytes
from ._errors import MediawayError
from ._types import AudioStreamInfo, Codec, RawPacket, Rational, VideoStreamInfo

__all__ = ["FlvMuxer", "FlvDemuxer", "VIDEO_TRACK_ID", "AUDIO_TRACK_ID"]

VIDEO_TRACK_ID = 0
AUDIO_TRACK_ID = 1


class FlvMuxer:
    """Muxes packets into FLV tags."""

    def __init__(self):
        self._handle = _ffi.container.dll.mediaway_flv_muxer_create()
        if not self._handle:
            raise MediawayError(_ffi.MEDIAWAY_STATUS_INTERNAL_PANIC, "FLV muxer creation panicked")

    def write_header(self, has_audio: bool, has_video: bool) -> bytes:
        """Write the FLV file header. Call before any track registration or packet."""
        out_data = _ffi.U8P()
        out_len = c_size_t(0)
        _check_container(
            _ffi.container.dll.mediaway_flv_muxer_write_header(
                self._handle, has_audio, has_video, byref(out_data), byref(out_len)
            )
        )
        data = _copy_bytes(out_data, out_len.value)
        _ffi.container.dll.mediaway_buffer_free(out_data, out_len)
        return data

    def add_video_track(self, info: VideoStreamInfo) -> int:
        """Register the video track. Only H264 is recognized (UNSUPPORTED_CODEC otherwise)."""
        raw = _ffi.VideoTrackInfo(
            id=0,
            codec=int(info.codec),
            time_base=_ffi.Rational(info.frame_rate.num, info.frame_rate.den),
            width=info.width,
            height=info.height,
            extra_data=None,
            extra_data_len=0,
        )
        if info.extra_data:
            buf = create_string_buffer(info.extra_data, len(info.extra_data))
            raw.extra_data = cast(buf, _ffi.U8P)
            raw.extra_data_len = len(info.extra_data)
        _check_container(_ffi.container.dll.mediaway_flv_muxer_add_video_track(self._handle, byref(raw)))
        return VIDEO_TRACK_ID

    def add_audio_track(self, info: AudioStreamInfo) -> int:
        """Register the audio track. AAC and MP3 are recognized (UNSUPPORTED_CODEC otherwise)."""
        raw = _ffi.AudioTrackInfo(
            id=0,
            codec=int(info.codec),
            time_base=_ffi.Rational(1, info.sample_rate),
            sample_rate=info.sample_rate,
            channels=info.channels,
            extra_data=None,
            extra_data_len=0,
        )
        if info.extra_data:
            buf = create_string_buffer(info.extra_data, len(info.extra_data))
            raw.extra_data = cast(buf, _ffi.U8P)
            raw.extra_data_len = len(info.extra_data)
        _check_container(_ffi.container.dll.mediaway_flv_muxer_add_audio_track(self._handle, byref(raw)))
        return AUDIO_TRACK_ID

    def push_packet(self, packet: RawPacket) -> bytes:
        """Mux one packet: writes the track's sequence-header tag first (once,
        only for codecs that have one) then the data tag. `packet.stream_id`
        selects `VIDEO_TRACK_ID`/`AUDIO_TRACK_ID` and must have a matching
        `add_*_track` call already made, else UNKNOWN_STREAM."""
        raw = _ffi.PacketView(
            stream_id=packet.stream_id,
            pts=packet.pts,
            dts=packet.dts,
            duration=packet.duration,
            is_keyframe=packet.key,
            is_discard=packet.discard,
            payload=None,
            payload_len=0,
        )
        if packet.payload:
            buf = create_string_buffer(packet.payload, len(packet.payload))
            raw.payload = cast(buf, _ffi.U8P)
            raw.payload_len = len(packet.payload)
        out_data = _ffi.U8P()
        out_len = c_size_t(0)
        _check_container(
            _ffi.container.dll.mediaway_flv_muxer_push_packet(
                self._handle, byref(raw), byref(out_data), byref(out_len)
            )
        )
        data = _copy_bytes(out_data, out_len.value)
        _ffi.container.dll.mediaway_buffer_free(out_data, out_len)
        return data

    def __enter__(self) -> "FlvMuxer":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        if self._handle:
            _ffi.container.dll.mediaway_flv_muxer_close(self._handle)
            self._handle = None


class FlvDemuxer:
    """Feeds FLV-container bytes in and pulls demuxed packets/stream info back out."""

    def __init__(self):
        self._handle = _ffi.container.dll.mediaway_flv_demuxer_create()
        if not self._handle:
            raise MediawayError(_ffi.MEDIAWAY_STATUS_INTERNAL_PANIC, "FLV demuxer creation panicked")

    def push_bytes(self, data: bytes) -> None:
        buf = create_string_buffer(data, len(data))
        _check_container(
            _ffi.container.dll.mediaway_flv_demuxer_push_bytes(self._handle, cast(buf, _ffi.U8P), len(data))
        )

    def streams(self) -> list[VideoStreamInfo | AudioStreamInfo]:
        """Streams recognized so far — 0, 1, or 2 (fixed video-then-audio slots)."""
        count = _ffi.container.dll.mediaway_flv_demuxer_stream_count(self._handle)
        out: list[VideoStreamInfo | AudioStreamInfo] = []
        for index in range(count):
            raw = _ffi.StreamInfo()
            _check_container(_ffi.container.dll.mediaway_flv_demuxer_stream_at(self._handle, index, byref(raw)))
            extra = _copy_bytes(raw.extra_data, raw.extra_data_len)
            _ffi.container.dll.mediaway_stream_info_free(byref(raw))
            codec = Codec(raw.codec)
            if raw.has_geometry:
                out.append(
                    VideoStreamInfo(
                        codec=codec,
                        width=raw.width,
                        height=raw.height,
                        frame_rate=Rational(raw.time_base.num, raw.time_base.den),
                        extra_data=extra,
                    )
                )
            else:
                out.append(
                    AudioStreamInfo(
                        codec=codec, sample_rate=raw.sample_rate, channels=raw.channels, extra_data=extra
                    )
                )
        return out

    def poll_packet(self) -> RawPacket | None:
        """Pop the next demuxed packet. Sequence-header tags (AVC/AAC config)
        update the matching stream's extra data internally and are not
        themselves returned."""
        raw = _ffi.Packet()
        has = c_bool(False)
        _check_container(_ffi.container.dll.mediaway_flv_demuxer_poll_packet(self._handle, byref(raw), byref(has)))
        if not has.value:
            return None
        payload = _copy_bytes(raw.payload, raw.payload_len)
        _ffi.container.dll.mediaway_packet_free(byref(raw))
        return RawPacket(
            stream_id=raw.stream_id,
            pts=raw.pts,
            dts=raw.dts,
            duration=raw.duration,
            key=raw.is_keyframe,
            discard=raw.is_discard,
            payload=payload,
        )

    def __enter__(self) -> "FlvDemuxer":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        if self._handle:
            _ffi.container.dll.mediaway_flv_demuxer_close(self._handle)
            self._handle = None
