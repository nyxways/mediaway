/*
 * detail.hpp — shared container status-code mapping, used by every
 * mediaway::container::* class across all 8 format headers.
 */

#ifndef MEDIAWAY_CONTAINER_DETAIL_HPP
#define MEDIAWAY_CONTAINER_DETAIL_HPP

#include <mediaway/container.h>
#include <mediaway/core.hpp>

namespace mediaway {
namespace container {
namespace detail {

using mediaway::detail::fromAbiCodec;
using mediaway::detail::throwError;
using mediaway::detail::toAbiCodec;

/// Shared across all 8 formats' handles — `mediaway_status_t` is one status
/// enum for every format this crate wraps, not a per-format one (see
/// adr/container/0001 §2, adr/container/0003-multi-format-c-abi.md).
inline void checkContainer(mediaway_status_t st) {
    switch (st) {
        case MEDIAWAY_OK: return;
        case MEDIAWAY_STATUS_INVALID_ARGUMENT: throwError(Status::InvalidArgument, st, "invalid argument");
        case MEDIAWAY_STATUS_INVALID_STATE: throwError(Status::InvalidState, st, "invalid state (typestate violation)");
        case MEDIAWAY_STATUS_INVALID_TRACK: throwError(Status::MuxError, st, "invalid or duplicate track id");
        case MEDIAWAY_STATUS_INVALID_PACKET: throwError(Status::MuxError, st, "packet does not match a registered track");
        case MEDIAWAY_STATUS_INVALID_DATA: throwError(Status::DemuxError, st, "truncated or malformed container data");
        case MEDIAWAY_STATUS_INTERNAL_PANIC: throwError(Status::Panic, st, "caught Rust panic (handle poisoned)");
        case MEDIAWAY_STATUS_HANDLE_POISONED: throwError(Status::Panic, st, "handle poisoned by an earlier panic");
        case MEDIAWAY_STATUS_UNSUPPORTED_CODEC: throwError(Status::MuxError, st, "codec has no encoding in this container format");
        case MEDIAWAY_STATUS_UNKNOWN_STREAM: throwError(Status::MuxError, st, "stream_id/pid matches no registered track");
        default: throwError(Status::MuxError, st, "unknown container error");
    }
}

/// Shared conversion for every demuxer's `streams()` — `mediaway_stream_info_t`
/// has the identical shape (has_geometry discriminates Video/Audio) across
/// all 7 demux-capable formats.
inline StreamInfo toStreamInfo(const mediaway_stream_info_t& raw, Bytes extra) {
    const Rational timescale{static_cast<std::uint32_t>(raw.time_base.num), raw.time_base.den};
    if (raw.has_geometry) {
        return VideoStreamInfo{raw.id, mediaway::detail::fromAbiCodec(raw.codec), timescale,
                               raw.width, raw.height, std::move(extra)};
    }
    return AudioStreamInfo{raw.id, mediaway::detail::fromAbiCodec(raw.codec), timescale,
                           raw.sample_rate, raw.channels, std::move(extra)};
}

/// Shared conversion for every demuxer's `pollPacket()` — `mediaway_packet_t`
/// has the identical shape across all 7 demux-capable formats.
inline Packet toPacket(const mediaway_packet_t& raw, Bytes payload) {
    return Packet{raw.stream_id, raw.pts, raw.dts, raw.is_keyframe, std::move(payload)};
}

}  // namespace detail
}  // namespace container
}  // namespace mediaway

#endif  // MEDIAWAY_CONTAINER_DETAIL_HPP
