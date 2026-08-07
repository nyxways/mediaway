/*
 * flv.hpp — FlvMuxer/FlvDemuxer over the dedicated mediaway_flv_muxer_t/
 * mediaway_flv_demuxer_t handles (adr/container/0005-flv-c-abi.md): the mux
 * side writes tag bytes directly into a fresh buffer on every call (no
 * pollBytes step), with a fixed one-video/one-audio track slot.
 */

#ifndef MEDIAWAY_CONTAINER_FLV_HPP
#define MEDIAWAY_CONTAINER_FLV_HPP

#include <mediaway/container/detail.hpp>
#include <mediaway/core.hpp>

#include <memory>
#include <optional>
#include <vector>

namespace mediaway {
namespace container {

/// FLV's fixed stream ids — matches the format's one-video/one-audio-slot
/// shape (no track-id field in the format itself).
inline constexpr TrackId kFlvVideoTrackId = 0;
inline constexpr TrackId kFlvAudioTrackId = 1;

/// A live FLV mux session with a fixed video/audio slot. Unlike every other
/// muxer class, writeHeader()/pushPacket() each return the bytes written by
/// that call directly — there is no internal accumulation, so no pollBytes.
class FlvMuxer {
public:
    FlvMuxer() : handle_(mediaway_flv_muxer_create(), &mediaway_flv_muxer_close) {
        if (!handle_) {
            detail::throwError(Status::Panic, MEDIAWAY_STATUS_INTERNAL_PANIC,
                               "FLV muxer creation panicked");
        }
    }
    ~FlvMuxer() = default;
    FlvMuxer(FlvMuxer&&) = default;
    FlvMuxer& operator=(FlvMuxer&&) = default;
    FlvMuxer(const FlvMuxer&) = delete;
    FlvMuxer& operator=(const FlvMuxer&) = delete;

    /// Write the FLV file header, declaring whether audio/video tags follow.
    /// Call once, before any track registration or pushPacket.
    Bytes writeHeader(bool hasAudio, bool hasVideo) {
        std::uint8_t* data = nullptr;
        std::size_t len = 0;
        detail::checkContainer(mediaway_flv_muxer_write_header(handle_.get(), hasAudio, hasVideo, &data, &len));
        if (len == 0) return {};
        Bytes out(data, data + len);
        mediaway_buffer_free(data, len);
        return out;
    }

    /// Register the video (H264-only) track. `id` is always kFlvVideoTrackId
    /// — FLV has no track-id field, video/audio are distinguished by which
    /// add_*_track function was called.
    TrackId addVideoTrack(const VideoStreamInfo& info) {
        mediaway_video_track_info_t raw{};
        raw.id = kFlvVideoTrackId;
        raw.codec = static_cast<mediaway_codec_kind_t>(detail::toAbiCodec(info.codec));
        raw.time_base = {info.timescale.num, info.timescale.den};
        raw.width = info.width;
        raw.height = info.height;
        raw.extra_data = info.codecConfig.empty() ? nullptr : info.codecConfig.data();
        raw.extra_data_len = info.codecConfig.size();
        detail::checkContainer(mediaway_flv_muxer_add_video_track(handle_.get(), &raw));
        return kFlvVideoTrackId;
    }

    /// Register the audio (AAC/MP3) track. Same fixed-slot reasoning as
    /// addVideoTrack; `id` is always kFlvAudioTrackId.
    TrackId addAudioTrack(const AudioStreamInfo& info) {
        mediaway_audio_track_info_t raw{};
        raw.id = kFlvAudioTrackId;
        raw.codec = static_cast<mediaway_codec_kind_t>(detail::toAbiCodec(info.codec));
        raw.time_base = {info.timescale.num, info.timescale.den};
        raw.sample_rate = info.sampleRate;
        raw.channels = info.channels;
        raw.extra_data = info.codecConfig.empty() ? nullptr : info.codecConfig.data();
        raw.extra_data_len = info.codecConfig.size();
        detail::checkContainer(mediaway_flv_muxer_add_audio_track(handle_.get(), &raw));
        return kFlvAudioTrackId;
    }

    /// Mux one packet. Writes the track's sequence-header tag first (once,
    /// only for codecs that have one) then the data tag, and returns the
    /// bytes written directly — no separate poll step.
    Bytes pushPacket(const Packet& packet) {
        mediaway_packet_view_t raw{};
        raw.stream_id = packet.trackId;
        raw.pts = packet.pts;
        raw.dts = packet.dts;
        raw.duration = 0;
        raw.is_keyframe = packet.keyframe;
        raw.is_discard = false;
        raw.payload = packet.data.empty() ? nullptr : packet.data.data();
        raw.payload_len = packet.data.size();
        std::uint8_t* data = nullptr;
        std::size_t len = 0;
        detail::checkContainer(mediaway_flv_muxer_push_packet(handle_.get(), &raw, &data, &len));
        if (len == 0) return {};
        Bytes out(data, data + len);
        mediaway_buffer_free(data, len);
        return out;
    }

private:
    std::unique_ptr<mediaway_flv_muxer_t, void (*)(mediaway_flv_muxer_t*)> handle_;
};

/// A streaming FLV demuxer: feed container bytes, poll streams (AVC video,
/// AAC/MP3 audio — see mediaway-container::flv module docs on codec
/// coverage) and packets.
class FlvDemuxer {
public:
    FlvDemuxer() : handle_(mediaway_flv_demuxer_create(), &mediaway_flv_demuxer_close) {
        if (!handle_) {
            detail::throwError(Status::Panic, MEDIAWAY_STATUS_INTERNAL_PANIC,
                               "FLV demuxer creation panicked");
        }
    }
    ~FlvDemuxer() = default;
    FlvDemuxer(FlvDemuxer&&) = default;
    FlvDemuxer& operator=(FlvDemuxer&&) = default;
    FlvDemuxer(const FlvDemuxer&) = delete;
    FlvDemuxer& operator=(const FlvDemuxer&) = delete;

    void pushBytes(const Bytes& bytes) {
        detail::checkContainer(mediaway_flv_demuxer_push_bytes(handle_.get(), bytes.data(), bytes.size()));
    }

    std::vector<StreamInfo> streams() const {
        std::vector<StreamInfo> out;
        const std::size_t count = mediaway_flv_demuxer_stream_count(handle_.get());
        out.reserve(count);
        for (std::size_t i = 0; i < count; ++i) {
            mediaway_stream_info_t raw{};
            detail::checkContainer(mediaway_flv_demuxer_stream_at(handle_.get(), i, &raw));
            Bytes extra(raw.extra_data, raw.extra_data + raw.extra_data_len);
            mediaway_stream_info_free(&raw);
            out.emplace_back(detail::toStreamInfo(raw, std::move(extra)));
        }
        return out;
    }

    /// The next demuxed packet, if any is ready. Sequence-header tags
    /// (AVC/AAC config) update the matching stream's extra data internally
    /// and are not themselves returned as packets.
    std::optional<Packet> pollPacket() {
        mediaway_packet_t raw{};
        bool has = false;
        detail::checkContainer(mediaway_flv_demuxer_poll_packet(handle_.get(), &raw, &has));
        if (!has) return std::nullopt;
        Bytes payload(raw.payload, raw.payload + raw.payload_len);
        mediaway_packet_free(&raw);
        return detail::toPacket(raw, std::move(payload));
    }

private:
    std::unique_ptr<mediaway_flv_demuxer_t, void (*)(mediaway_flv_demuxer_t*)> handle_;
};

}  // namespace container
}  // namespace mediaway

#endif  // MEDIAWAY_CONTAINER_FLV_HPP
