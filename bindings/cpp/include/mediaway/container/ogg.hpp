/*
 * ogg.hpp — OggMuxer/OggDemuxer over the dedicated mediaway_ogg_muxer_t/
 * mediaway_ogg_demuxer_t handles (adr/container/0004-ogg-adts-c-abi.md): no
 * track registration, immediately ready for pushPacket.
 */

#ifndef MEDIAWAY_CONTAINER_OGG_HPP
#define MEDIAWAY_CONTAINER_OGG_HPP

#include <mediaway/container/detail.hpp>
#include <mediaway/core.hpp>

#include <memory>
#include <optional>
#include <vector>

namespace mediaway {
namespace container {

/// A live Ogg mux session for one logical bitstream. No track-registration
/// step — immediately ready for pushPacket().
class OggMuxer {
public:
    /// `serial` identifies the logical bitstream (Ogg page header field).
    explicit OggMuxer(std::uint32_t serial)
        : handle_(mediaway_ogg_muxer_create(serial), &mediaway_ogg_muxer_close) {
        if (!handle_) {
            detail::throwError(Status::Panic, MEDIAWAY_STATUS_INTERNAL_PANIC,
                               "Ogg muxer creation panicked");
        }
    }
    ~OggMuxer() = default;
    OggMuxer(OggMuxer&&) = default;
    OggMuxer& operator=(OggMuxer&&) = default;
    OggMuxer(const OggMuxer&) = delete;
    OggMuxer& operator=(const OggMuxer&) = delete;

    /// Write one Ogg page containing `packet`'s payload. `packet.pts` becomes
    /// the page's granule_position; fails when the payload exceeds a single
    /// page's capacity (this mux always emits one page per packet).
    void pushPacket(const Packet& packet) {
        mediaway_packet_view_t raw{};
        raw.stream_id = packet.trackId;
        raw.pts = packet.pts;
        raw.dts = packet.dts;
        raw.duration = 0;
        raw.is_keyframe = packet.keyframe;
        raw.is_discard = false;
        raw.payload = packet.data.empty() ? nullptr : packet.data.data();
        raw.payload_len = packet.data.size();
        detail::checkContainer(mediaway_ogg_muxer_push_packet(handle_.get(), &raw));
    }

    /// No-op — every pushPacket() call already wrote a complete, independently
    /// valid Ogg page. Kept for shape parity with the other muxer classes.
    void flush() { detail::checkContainer(mediaway_ogg_muxer_flush(handle_.get())); }

    /// One chunk of muxed bytes; empty when nothing is ready.
    Bytes pollBytes() {
        std::uint8_t* data = nullptr;
        std::size_t len = 0;
        detail::checkContainer(mediaway_ogg_muxer_poll_bytes(handle_.get(), &data, &len));
        if (len == 0) return {};
        Bytes out(data, data + len);
        mediaway_buffer_free(data, len);
        return out;
    }

private:
    std::unique_ptr<mediaway_ogg_muxer_t, void (*)(mediaway_ogg_muxer_t*)> handle_;
};

/// A streaming Ogg demuxer: feed container bytes, poll the recognized stream
/// (Opus or Vorbis — see mediaway-container::ogg module docs) and packets.
class OggDemuxer {
public:
    OggDemuxer() : handle_(mediaway_ogg_demuxer_create(), &mediaway_ogg_demuxer_close) {
        if (!handle_) {
            detail::throwError(Status::Panic, MEDIAWAY_STATUS_INTERNAL_PANIC,
                               "Ogg demuxer creation panicked");
        }
    }
    ~OggDemuxer() = default;
    OggDemuxer(OggDemuxer&&) = default;
    OggDemuxer& operator=(OggDemuxer&&) = default;
    OggDemuxer(const OggDemuxer&) = delete;
    OggDemuxer& operator=(const OggDemuxer&) = delete;

    void pushBytes(const Bytes& bytes) {
        detail::checkContainer(mediaway_ogg_demuxer_push_bytes(handle_.get(), bytes.data(), bytes.size()));
    }

    /// The single logical bitstream, once the identification-header packet
    /// has been recognized (empty before that).
    std::vector<StreamInfo> streams() const {
        std::vector<StreamInfo> out;
        const std::size_t count = mediaway_ogg_demuxer_stream_count(handle_.get());
        out.reserve(count);
        for (std::size_t i = 0; i < count; ++i) {
            mediaway_stream_info_t raw{};
            detail::checkContainer(mediaway_ogg_demuxer_stream_at(handle_.get(), i, &raw));
            Bytes extra(raw.extra_data, raw.extra_data + raw.extra_data_len);
            mediaway_stream_info_free(&raw);
            out.emplace_back(detail::toStreamInfo(raw, std::move(extra)));
        }
        return out;
    }

    /// The next demuxed packet, if any is ready. A frame from an unrecognized
    /// codec (not OpusHead/Vorbis) is silently skipped.
    std::optional<Packet> pollPacket() {
        mediaway_packet_t raw{};
        bool has = false;
        detail::checkContainer(mediaway_ogg_demuxer_poll_packet(handle_.get(), &raw, &has));
        if (!has) return std::nullopt;
        Bytes payload(raw.payload, raw.payload + raw.payload_len);
        mediaway_packet_free(&raw);
        return detail::toPacket(raw, std::move(payload));
    }

private:
    std::unique_ptr<mediaway_ogg_demuxer_t, void (*)(mediaway_ogg_demuxer_t*)> handle_;
};

}  // namespace container
}  // namespace mediaway

#endif  // MEDIAWAY_CONTAINER_OGG_HPP
