/*
 * mp3.hpp — Mp3Muxer/Mp3Demuxer over the dedicated mediaway_mp3_muxer_t/
 * mediaway_mp3_demuxer_t handles (adr/container/0007-mp3-c-abi.md): a fixed
 * frame header for the mux session's lifetime (no track registration at
 * all), and an explicit padding bit on writeFrame no Packet-shaped type has
 * a slot for.
 */

#ifndef MEDIAWAY_CONTAINER_MP3_HPP
#define MEDIAWAY_CONTAINER_MP3_HPP

#include <mediaway/container/detail.hpp>
#include <mediaway/core.hpp>

#include <memory>
#include <optional>
#include <vector>

namespace mediaway {
namespace container {

/// MPEG audio version — ordinally identical to mediaway_mpeg_version_t, so
/// `static_cast` between them is safe (both defined in this codebase).
enum class MpegVersion { Mpeg1 = 0, Mpeg2 = 1, Mpeg25 = 2 };

/// MPEG audio channel mode — ordinally identical to mediaway_channel_mode_t.
enum class ChannelMode { Stereo = 0, JointStereo = 1, DualChannel = 2, Mono = 3 };

/// Fixed Layer III frame header for the mux session's lifetime — real
/// streams don't vary bitrate/sample-rate/channel mode mid-stream (VBR would
/// need a new header per frame, out of scope).
struct Mp3FrameHeader {
    MpegVersion version;
    std::uint16_t bitrateKbps;  // must be one of the 14 standard values for `version`
    std::uint32_t sampleRate;   // must be one of the 3 standard rates for `version`
    ChannelMode channelMode;
};

/// A live MP3 (MPEG Layer III) mux session for a fixed frame header. No
/// track-registration step.
class Mp3Muxer {
public:
    /// Throws Error(Status::InvalidArgument) for a non-standard
    /// bitrate/sample-rate combination — the ABI constructor has no status
    /// side channel, so a bad header and a caught panic collapse to NULL.
    explicit Mp3Muxer(const Mp3FrameHeader& header)
        : handle_(createHandle(header), &mediaway_mp3_muxer_close) {
        if (!handle_) {
            detail::throwError(Status::InvalidArgument, MEDIAWAY_STATUS_INVALID_ARGUMENT,
                               "unsupported bitrate/sample-rate combination, or muxer "
                               "creation panicked");
        }
    }
    ~Mp3Muxer() = default;
    Mp3Muxer(Mp3Muxer&&) = default;
    Mp3Muxer& operator=(Mp3Muxer&&) = default;
    Mp3Muxer(const Mp3Muxer&) = delete;
    Mp3Muxer& operator=(const Mp3Muxer&) = delete;

    /// Append one already-encoded Layer III frame body. `padding` is the
    /// bit-reservoir padding bit real encoders flip per frame to average out
    /// fractional frame lengths — fails when `frameBody`'s length doesn't
    /// match what the header's bitrate/sample-rate/padding combination
    /// requires.
    Bytes writeFrame(const Bytes& frameBody, bool padding) {
        std::uint8_t* data = nullptr;
        std::size_t len = 0;
        detail::checkContainer(mediaway_mp3_muxer_write_frame(
            handle_.get(), frameBody.empty() ? nullptr : frameBody.data(), frameBody.size(),
            padding, &data, &len));
        if (len == 0) return {};
        Bytes out(data, data + len);
        mediaway_buffer_free(data, len);
        return out;
    }

private:
    static mediaway_mp3_muxer_t* createHandle(const Mp3FrameHeader& header) {
        mediaway_mp3_frame_header_t raw{};
        raw.version = static_cast<mediaway_mpeg_version_t>(header.version);
        raw.bitrate_kbps = header.bitrateKbps;
        raw.sample_rate = header.sampleRate;
        raw.channel_mode = static_cast<mediaway_channel_mode_t>(header.channelMode);
        return mediaway_mp3_muxer_create(&raw);
    }

    std::unique_ptr<mediaway_mp3_muxer_t, void (*)(mediaway_mp3_muxer_t*)> handle_;
};

/// A streaming MP3 demuxer: feed elementary-stream bytes, poll the single
/// implicit Layer III stream and its frames.
class Mp3Demuxer {
public:
    Mp3Demuxer() : handle_(mediaway_mp3_demuxer_create(), &mediaway_mp3_demuxer_close) {
        if (!handle_) {
            detail::throwError(Status::Panic, MEDIAWAY_STATUS_INTERNAL_PANIC,
                               "MP3 demuxer creation panicked");
        }
    }
    ~Mp3Demuxer() = default;
    Mp3Demuxer(Mp3Demuxer&&) = default;
    Mp3Demuxer& operator=(Mp3Demuxer&&) = default;
    Mp3Demuxer(const Mp3Demuxer&) = delete;
    Mp3Demuxer& operator=(const Mp3Demuxer&) = delete;

    void pushBytes(const Bytes& bytes) {
        detail::checkContainer(mediaway_mp3_demuxer_push_bytes(handle_.get(), bytes.data(), bytes.size()));
    }

    /// The single implicit stream, once the first frame's header has been
    /// parsed (empty before that).
    std::vector<StreamInfo> streams() const {
        std::vector<StreamInfo> out;
        const std::size_t count = mediaway_mp3_demuxer_stream_count(handle_.get());
        out.reserve(count);
        for (std::size_t i = 0; i < count; ++i) {
            mediaway_stream_info_t raw{};
            detail::checkContainer(mediaway_mp3_demuxer_stream_at(handle_.get(), i, &raw));
            Bytes extra(raw.extra_data, raw.extra_data + raw.extra_data_len);
            mediaway_stream_info_free(&raw);
            out.emplace_back(detail::toStreamInfo(raw, std::move(extra)));
        }
        return out;
    }

    /// The next demuxed packet (one Layer III frame), if any is ready.
    /// pts/duration are synthesized from a running samples-per-frame count —
    /// MPEG audio carries no per-frame timing of its own.
    std::optional<Packet> pollPacket() {
        mediaway_packet_t raw{};
        bool has = false;
        detail::checkContainer(mediaway_mp3_demuxer_poll_packet(handle_.get(), &raw, &has));
        if (!has) return std::nullopt;
        Bytes payload(raw.payload, raw.payload + raw.payload_len);
        mediaway_packet_free(&raw);
        return detail::toPacket(raw, std::move(payload));
    }

private:
    std::unique_ptr<mediaway_mp3_demuxer_t, void (*)(mediaway_mp3_demuxer_t*)> handle_;
};

}  // namespace container
}  // namespace mediaway

#endif  // MEDIAWAY_CONTAINER_MP3_HPP
