"""Container capability: MPEG-TS mux + demux (adr/container/0006-mpeg-ts-c-abi.md).

The full elementary-stream list is fixed at construction (no `add_track`
after); `write_pat_pmt`/`write_access_unit` write directly into a freshly
allocated output buffer with explicit 90 kHz `pts`/`dts` clock values — not a
track-relative time base, so packets here use raw ints, never `Rational`.
"""

from __future__ import annotations

from ctypes import Array, byref, c_bool, c_size_t, cast, create_string_buffer

from . import _ffi
from ._container import _check_container, _copy_bytes
from ._errors import MediawayError
from ._types import AudioStreamInfo, Codec, Rational, RawPacket, TsElementaryStream, VideoStreamInfo

__all__ = ["TsMuxer", "TsDemuxer"]


class TsMuxer:
    """Muxes access units into MPEG-TS packets for one program."""

    def __init__(self, program_number: int, pmt_pid: int, streams: list[TsElementaryStream]):
        """`pmt_pid` and every stream's `pid` must be in `2..=0x1FFF`; every
        stream's codec must be H264/HEVC/AAC/MP3."""
        raw_streams: Array = (_ffi.TsElementaryStream * len(streams))()
        for i, s in enumerate(streams):
            raw_streams[i] = _ffi.TsElementaryStream(pid=s.pid, codec=int(s.codec))
        self._handle = _ffi.container.dll.mediaway_ts_muxer_create(
            program_number, pmt_pid, raw_streams, len(streams)
        )
        if not self._handle:
            raise MediawayError(
                _ffi.MEDIAWAY_STATUS_INVALID_ARGUMENT,
                "invalid PMT/elementary-stream PID, an unsupported elementary-stream codec, "
                "or the native call panicked",
            )

    def write_pat_pmt(self) -> bytes:
        """Write PAT + PMT packets. Call once at the start and periodically
        thereafter — real players expect PAT/PMT to repeat."""
        out_data = _ffi.U8P()
        out_len = c_size_t(0)
        _check_container(
            _ffi.container.dll.mediaway_ts_muxer_write_pat_pmt(self._handle, byref(out_data), byref(out_len))
        )
        data = _copy_bytes(out_data, out_len.value)
        _ffi.container.dll.mediaway_buffer_free(out_data, out_len)
        return data

    def write_access_unit(
        self, pid: int, data: bytes, pts_90k: int, dts_90k: int | None, random_access: bool
    ) -> bytes:
        """Packetize one access unit for `pid` into PES + TS packets.
        `pts_90k`/`dts_90k` are the real MPEG-TS 90 kHz clock values;
        `dts_90k is None` means "no DTS"."""
        buf = create_string_buffer(data, len(data)) if data else None
        payload = cast(buf, _ffi.U8P) if buf is not None else _ffi.U8P()
        out_data = _ffi.U8P()
        out_len = c_size_t(0)
        _check_container(
            _ffi.container.dll.mediaway_ts_muxer_write_access_unit(
                self._handle,
                pid,
                payload,
                len(data),
                pts_90k,
                dts_90k is not None,
                dts_90k or 0,
                random_access,
                byref(out_data),
                byref(out_len),
            )
        )
        result = _copy_bytes(out_data, out_len.value)
        _ffi.container.dll.mediaway_buffer_free(out_data, out_len)
        return result

    def __enter__(self) -> "TsMuxer":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        if self._handle:
            _ffi.container.dll.mediaway_ts_muxer_close(self._handle)
            self._handle = None


def _stream_info_to_managed(raw) -> VideoStreamInfo | AudioStreamInfo:
    extra = _copy_bytes(raw.extra_data, raw.extra_data_len)
    codec = Codec(raw.codec)
    if raw.has_geometry:
        return VideoStreamInfo(
            codec=codec,
            width=raw.width,
            height=raw.height,
            frame_rate=Rational(raw.time_base.num, raw.time_base.den),
            extra_data=extra,
        )
    return AudioStreamInfo(codec=codec, sample_rate=raw.sample_rate, channels=raw.channels, extra_data=extra)


def _packet_to_managed(raw) -> RawPacket:
    return RawPacket(
        stream_id=raw.stream_id,
        pts=raw.pts,
        dts=raw.dts,
        duration=raw.duration,
        key=raw.is_keyframe,
        discard=raw.is_discard,
        payload=_copy_bytes(raw.payload, raw.payload_len),
    )


class TsDemuxer:
    """Feeds MPEG-TS bytes in and pulls demuxed access units back out."""

    def __init__(self):
        self._handle = _ffi.container.dll.mediaway_ts_demuxer_create()
        if not self._handle:
            raise MediawayError(_ffi.MEDIAWAY_STATUS_INTERNAL_PANIC, "MPEG-TS demuxer creation panicked")

    def push_bytes(self, data: bytes) -> None:
        """Feed bytes — need not be 188-byte aligned across calls."""
        buf = create_string_buffer(data, len(data))
        _check_container(
            _ffi.container.dll.mediaway_ts_demuxer_push_bytes(self._handle, cast(buf, _ffi.U8P), len(data))
        )

    def streams(self) -> list[VideoStreamInfo | AudioStreamInfo]:
        """Streams whose stream_type maps to a recognized codec (H264/HEVC/AAC/MP3).
        Empty until `poll_packet` has actually consumed the PMT (lazy PSI parsing)."""
        count = _ffi.container.dll.mediaway_ts_demuxer_stream_count(self._handle)
        out = []
        for index in range(count):
            raw = _ffi.StreamInfo()
            _check_container(_ffi.container.dll.mediaway_ts_demuxer_stream_at(self._handle, index, byref(raw)))
            info = _stream_info_to_managed(raw)
            _ffi.container.dll.mediaway_stream_info_free(byref(raw))
            out.append(info)
        return out

    def poll_packet(self) -> RawPacket | None:
        """Pop the next demuxed packet. A PID with no recognized codec mapping is silently skipped."""
        raw = _ffi.Packet()
        has = c_bool(False)
        _check_container(_ffi.container.dll.mediaway_ts_demuxer_poll_packet(self._handle, byref(raw), byref(has)))
        if not has.value:
            return None
        packet = _packet_to_managed(raw)
        _ffi.container.dll.mediaway_packet_free(byref(raw))
        return packet

    def finish(self) -> list[RawPacket]:
        """Force-emit whatever is still accumulating per PID — call once at
        the end of a stream so the very last access unit per PID isn't lost
        (MPEG-TS only confirms a PES boundary once the next packet on the
        same PID starts)."""
        out_packets = _ffi.POINTER(_ffi.Packet)()
        out_count = c_size_t(0)
        _check_container(
            _ffi.container.dll.mediaway_ts_demuxer_finish(self._handle, byref(out_packets), byref(out_count))
        )
        result = [_packet_to_managed(out_packets[i]) for i in range(out_count.value)]
        _ffi.container.dll.mediaway_ts_demuxer_finish_free(out_packets, out_count)
        return result

    def __enter__(self) -> "TsDemuxer":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        if self._handle:
            _ffi.container.dll.mediaway_ts_demuxer_close(self._handle)
            self._handle = None
