/*
 * adts.hpp — AdtsMuxer/AdtsDemuxer over the dedicated mediaway_adts_muxer_t/
 * mediaway_adts_demuxer_t handles (adr/container/0004-ogg-adts-c-abi.md): no
 * track registration, immediately ready for pushPacket.
 */

#ifndef MEDIAWAY_CONTAINER_ADTS_HPP
#define MEDIAWAY_CONTAINER_ADTS_HPP

#include <mediaway/container/detail.hpp>
#include <mediaway/core.hpp>

#include <memory>
#include <optional>
#include <vector>

namespace mediaway {
namespace container {

/// A live ADTS (raw AAC elementary stream) mux session for a fixed
/// sample_rate/channels. No track-registration step.
class AdtsMuxer {
public:
    /// Throws Error(Status::Panic) for a non-standard `sampleRate` — the ABI
    /// constructor has no status side channel, so a bad rate and a caught
    /// panic collapse to the same NULL (adr/container/0004-ogg-adts-c-abi.md
    /// § Decision 2).
    AdtsMuxer(std::uint32_t sampleRate, std::uint8_t channels)
        : handle_(mediaway_adts_muxer_create(sampleRate, channels), &mediaway_adts_muxer_close) {
        if (!handle_) {
            detail::throwError(Status::InvalidArgument, MEDIAWAY_STATUS_INVALID_ARGUMENT,
                               "unsupported ADTS sample rate, or muxer creation panicked");
        }
    }
    ~AdtsMuxer() = default;
    AdtsMuxer(AdtsMuxer&&) = default;
    AdtsMuxer& operator=(AdtsMuxer&&) = default;
    AdtsMuxer(const AdtsMuxer&) = delete;
    AdtsMuxer& operator=(const AdtsMuxer&) = delete;

    /// Append one already-encoded raw AAC frame (ADTS header added).
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
        detail::checkContainer(mediaway_adts_muxer_push_packet(handle_.get(), &raw));
    }

    /// No-op — ADTS frames are independently appendable. Kept for shape
    /// parity with the other muxer classes.
    void flush() { detail::checkContainer(mediaway_adts_muxer_flush(handle_.get())); }

    /// One chunk of muxed bytes; empty when nothing is ready.
    Bytes pollBytes() {
        std::uint8_t* data = nullptr;
        std::size_t len = 0;
        detail::checkContainer(mediaway_adts_muxer_poll_bytes(handle_.get(), &data, &len));
        if (len == 0) return {};
        Bytes out(data, data + len);
        mediaway_buffer_free(data, len);
        return out;
    }

private:
    std::unique_ptr<mediaway_adts_muxer_t, void (*)(mediaway_adts_muxer_t*)> handle_;
};

/// A streaming ADTS demuxer: feed elementary-stream bytes, poll the single
/// implicit AAC stream and its frames.
class AdtsDemuxer {
public:
    AdtsDemuxer() : handle_(mediaway_adts_demuxer_create(), &mediaway_adts_demuxer_close) {
        if (!handle_) {
            detail::throwError(Status::Panic, MEDIAWAY_STATUS_INTERNAL_PANIC,
                               "ADTS demuxer creation panicked");
        }
    }
    ~AdtsDemuxer() = default;
    AdtsDemuxer(AdtsDemuxer&&) = default;
    AdtsDemuxer& operator=(AdtsDemuxer&&) = default;
    AdtsDemuxer(const AdtsDemuxer&) = delete;
    AdtsDemuxer& operator=(const AdtsDemuxer&) = delete;

    void pushBytes(const Bytes& bytes) {
        detail::checkContainer(mediaway_adts_demuxer_push_bytes(handle_.get(), bytes.data(), bytes.size()));
    }

    /// The single implicit stream, once the first frame's header has been
    /// parsed (empty before that — ADTS carries no upfront track metadata).
    std::vector<StreamInfo> streams() const {
        std::vector<StreamInfo> out;
        const std::size_t count = mediaway_adts_demuxer_stream_count(handle_.get());
        out.reserve(count);
        for (std::size_t i = 0; i < count; ++i) {
            mediaway_stream_info_t raw{};
            detail::checkContainer(mediaway_adts_demuxer_stream_at(handle_.get(), i, &raw));
            Bytes extra(raw.extra_data, raw.extra_data + raw.extra_data_len);
            mediaway_stream_info_free(&raw);
            out.emplace_back(detail::toStreamInfo(raw, std::move(extra)));
        }
        return out;
    }

    /// The next demuxed packet (one AAC frame), if any is ready. pts/duration
    /// are synthesized from a running 1024-samples-per-frame count — ADTS
    /// carries no per-frame timing of its own.
    std::optional<Packet> pollPacket() {
        mediaway_packet_t raw{};
        bool has = false;
        detail::checkContainer(mediaway_adts_demuxer_poll_packet(handle_.get(), &raw, &has));
        if (!has) return std::nullopt;
        Bytes payload(raw.payload, raw.payload + raw.payload_len);
        mediaway_packet_free(&raw);
        return detail::toPacket(raw, std::move(payload));
    }

private:
    std::unique_ptr<mediaway_adts_demuxer_t, void (*)(mediaway_adts_demuxer_t*)> handle_;
};

}  // namespace container
}  // namespace mediaway

#endif  // MEDIAWAY_CONTAINER_ADTS_HPP
