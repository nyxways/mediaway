/*
 * mp4_webm.hpp — Muxer/LiveMuxer/Demuxer over the shared mediaway_muxer_t/
 * mediaway_demuxer_t handles (adr/container/0001, adr/container/0003 §
 * Decision 1: WebM shares MP4's typestated add_track/begin shape, reached
 * through the same handles via mediaway_*_create_for_format).
 */

#ifndef MEDIAWAY_CONTAINER_MP4_WEBM_HPP
#define MEDIAWAY_CONTAINER_MP4_WEBM_HPP

#include <mediaway/container/detail.hpp>
#include <mediaway/core.hpp>

#include <memory>
#include <optional>
#include <vector>

namespace mediaway {
namespace container {

/// Which container format Muxer/Demuxer open — see
/// adr/container/0003-multi-format-c-abi.md § Decision 1 for why WebM is a
/// constructor parameter rather than a separate class: it shares MP4's exact
/// typestated add_track/begin/push_packet/poll_bytes shape.
enum class Format { Mp4, Webm };

class LiveMuxer;

/// A muxer in the track-registration (Open) state. begin() (rvalue-only)
/// consumes this object and returns the streaming LiveMuxer — track
/// registration after begin() is a compile error, mirroring the ABI's
/// INVALID_STATE.
class Muxer {
public:
    explicit Muxer(Format format = Format::Mp4)
        : handle_(format == Format::Mp4
                      ? mediaway_muxer_create()
                      : mediaway_muxer_create_for_format(MEDIAWAY_CONTAINER_FORMAT_WEBM),
                  &mediaway_muxer_close) {
        if (!handle_) {
            detail::throwError(Status::Panic, MEDIAWAY_STATUS_INTERNAL_PANIC,
                               "muxer creation panicked");
        }
    }
    ~Muxer() = default;
    Muxer(Muxer&&) = default;
    Muxer& operator=(Muxer&&) = default;
    Muxer(const Muxer&) = delete;
    Muxer& operator=(const Muxer&) = delete;

    /// Register a video track. The id is assigned by the muxer in
    /// registration order; the return value is authoritative.
    TrackId addVideoTrack(const VideoStreamInfo& info) {
        const TrackId id = nextId_++;
        mediaway_video_track_info_t raw{};
        raw.id = id;
        raw.codec = static_cast<mediaway_codec_kind_t>(detail::toAbiCodec(info.codec));
        raw.time_base = {info.timescale.num, info.timescale.den};
        raw.width = info.width;
        raw.height = info.height;
        raw.extra_data = info.codecConfig.empty() ? nullptr : info.codecConfig.data();
        raw.extra_data_len = info.codecConfig.size();
        detail::checkContainer(mediaway_muxer_add_video_track(handle_.get(), &raw));
        return id;
    }

    /// Register an audio track. Same id semantics as addVideoTrack.
    TrackId addAudioTrack(const AudioStreamInfo& info) {
        const TrackId id = nextId_++;
        mediaway_audio_track_info_t raw{};
        raw.id = id;
        raw.codec = static_cast<mediaway_codec_kind_t>(detail::toAbiCodec(info.codec));
        raw.time_base = {info.timescale.num, info.timescale.den};
        raw.sample_rate = info.sampleRate;
        raw.channels = info.channels;
        raw.extra_data = info.codecConfig.empty() ? nullptr : info.codecConfig.data();
        raw.extra_data_len = info.codecConfig.size();
        detail::checkContainer(mediaway_muxer_add_audio_track(handle_.get(), &raw));
        return id;
    }

    /// Consume the Open state; returns the streaming muxer.
    LiveMuxer begin() &&;

private:
    friend class LiveMuxer;
    std::unique_ptr<mediaway_muxer_t, void (*)(mediaway_muxer_t*)> handle_;
    // Starts at 1, not 0: WebM/Matroska's TrackNumber element must not be 0 (found via a
    // real end-to-end failure while wiring the Format::Webm constructor — MP4 tolerates 0
    // but there is no reason to special-case it, since ISO/IEC 14496-12 track_ID 0 is
    // reserved too).
    std::uint32_t nextId_ = 1;
};

/// A muxer in the streaming (Live) state. The muxer never touches files: the
/// caller owns all byte I/O, draining output with pollBytes().
class LiveMuxer {
public:
    ~LiveMuxer() = default;
    LiveMuxer(LiveMuxer&&) = default;
    LiveMuxer& operator=(LiveMuxer&&) = default;
    LiveMuxer(const LiveMuxer&) = delete;
    LiveMuxer& operator=(const LiveMuxer&) = delete;

    void pushPacket(const Packet& packet) {
        mediaway_packet_view_t raw{};
        raw.stream_id = packet.trackId;
        raw.pts = packet.pts;
        raw.dts = packet.dts;
        raw.duration = 0;  // unknown
        raw.is_keyframe = packet.keyframe;
        raw.is_discard = false;
        raw.payload = packet.data.empty() ? nullptr : packet.data.data();
        raw.payload_len = packet.data.size();
        detail::checkContainer(mediaway_muxer_push_packet(handle_.get(), &raw));
    }

    void flush() { detail::checkContainer(mediaway_muxer_flush(handle_.get())); }

    /// One chunk of muxed bytes; empty when nothing is ready.
    Bytes pollBytes() {
        std::uint8_t* data = nullptr;
        std::size_t len = 0;
        detail::checkContainer(mediaway_muxer_poll_bytes(handle_.get(), &data, &len));
        if (len == 0) return {};
        Bytes out(data, data + len);
        mediaway_buffer_free(data, len);
        return out;
    }

private:
    friend class Muxer;
    explicit LiveMuxer(std::unique_ptr<mediaway_muxer_t, void (*)(mediaway_muxer_t*)> handle)
        : handle_(std::move(handle)) {}
    std::unique_ptr<mediaway_muxer_t, void (*)(mediaway_muxer_t*)> handle_;
};

inline LiveMuxer Muxer::begin() && {
    detail::checkContainer(mediaway_muxer_begin(handle_.get()));
    return LiveMuxer(std::move(handle_));
}

/// A streaming demuxer: feed container bytes, poll streams and packets.
class Demuxer {
public:
    explicit Demuxer(Format format = Format::Mp4)
        : handle_(format == Format::Mp4
                      ? mediaway_demuxer_create()
                      : mediaway_demuxer_create_for_format(MEDIAWAY_CONTAINER_FORMAT_WEBM),
                  &mediaway_demuxer_close) {
        if (!handle_) {
            detail::throwError(Status::Panic, MEDIAWAY_STATUS_INTERNAL_PANIC,
                               "demuxer creation panicked");
        }
    }
    ~Demuxer() = default;
    Demuxer(Demuxer&&) = default;
    Demuxer& operator=(Demuxer&&) = default;
    Demuxer(const Demuxer&) = delete;
    Demuxer& operator=(const Demuxer&) = delete;

    void pushBytes(const Bytes& bytes) {
        detail::checkContainer(mediaway_demuxer_push_bytes(handle_.get(), bytes.data(), bytes.size()));
    }

    /// Streams discovered so far; empty until the init segment is parsed.
    std::vector<StreamInfo> streams() const {
        std::vector<StreamInfo> out;
        const std::size_t count = mediaway_demuxer_stream_count(handle_.get());
        out.reserve(count);
        for (std::size_t i = 0; i < count; ++i) {
            mediaway_stream_info_t raw{};
            detail::checkContainer(mediaway_demuxer_stream_at(handle_.get(), i, &raw));
            Bytes extra(raw.extra_data, raw.extra_data + raw.extra_data_len);
            mediaway_stream_info_free(&raw);
            out.emplace_back(detail::toStreamInfo(raw, std::move(extra)));
        }
        return out;
    }

    /// The next demuxed packet, if any is ready.
    std::optional<Packet> pollPacket() {
        mediaway_packet_t raw{};
        bool has = false;
        detail::checkContainer(mediaway_demuxer_poll_packet(handle_.get(), &raw, &has));
        if (!has) return std::nullopt;
        Bytes payload(raw.payload, raw.payload + raw.payload_len);
        mediaway_packet_free(&raw);
        return detail::toPacket(raw, std::move(payload));
    }

    /// Set the ClearKey decryption key (exactly 16 bytes) for all encrypted
    /// tracks. MP4 only — throws Error(Status::InvalidState) on a WebM handle
    /// (adr/container/0003-multi-format-c-abi.md). Only affects samples
    /// drained from SUBSEQUENT pushBytes calls.
    void setDecryptionKey(const Bytes& key) {
        if (key.size() != 16) {
            detail::throwError(Status::InvalidArgument, MEDIAWAY_STATUS_INVALID_ARGUMENT,
                               "decryption key must be exactly 16 bytes");
        }
        detail::checkContainer(mediaway_demuxer_set_decryption_key(handle_.get(), key.data(), key.size()));
    }

private:
    std::unique_ptr<mediaway_demuxer_t, void (*)(mediaway_demuxer_t*)> handle_;
};

}  // namespace container
}  // namespace mediaway

#endif  // MEDIAWAY_CONTAINER_MP4_WEBM_HPP
