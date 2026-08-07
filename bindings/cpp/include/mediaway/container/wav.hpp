/*
 * wav.hpp — WavMuxer over the dedicated mediaway_wav_muxer_t handle, plus the
 * one-shot wavParse() function (adr/container/0008-wav-c-abi.md): WAV is
 * mux-only as a class — wav::Muxer::finish consumes the underlying Rust
 * state (RIFF sizes must be known up front), so finish() can be called
 * exactly once; a second call throws Error(Status::InvalidState). Demux has
 * no handle at all — wavParse() is a one-shot whole-buffer function, the
 * only format like this in the whole binding.
 */

#ifndef MEDIAWAY_CONTAINER_WAV_HPP
#define MEDIAWAY_CONTAINER_WAV_HPP

#include <mediaway/container/detail.hpp>
#include <mediaway/core.hpp>

#include <memory>

namespace mediaway {
namespace container {

/// PCM sample encoding — ordinally identical to mediaway_wav_sample_format_t
/// (NOT the same axis as PixelFormat/Codec — WAVE's wFormatTag, unrelated to
/// device/pipeline audio's raw PCM bit-depth format).
enum class WavSampleFormat { Pcm = 0, Float = 1 };

/// Explicit WAVE format for WavMuxer's non-integer-PCM constructor.
struct WaveFormat {
    WavSampleFormat sampleFormat;
    std::uint16_t channels;
    std::uint32_t sampleRate;
    std::uint16_t bitsPerSample;
};

/// A WAV (RIFF/WAVE PCM) mux session. push_packet is infallible on the ABI
/// side (no per-call validation); finish() consumes the muxer's internal
/// state — the object itself stays alive afterward (its destructor still
/// runs), but a second finish() or a pushPacket() after fails with
/// Error(Status::InvalidState) rather than corrupting anything.
class WavMuxer {
public:
    /// Start an integer-PCM mux session.
    WavMuxer(std::uint32_t sampleRate, std::uint16_t channels, std::uint16_t bitsPerSample)
        : handle_(mediaway_wav_muxer_create(sampleRate, channels, bitsPerSample),
                  &mediaway_wav_muxer_close) {
        if (!handle_) {
            detail::throwError(Status::Panic, MEDIAWAY_STATUS_INTERNAL_PANIC,
                               "WAV muxer creation panicked");
        }
    }

    /// Start a mux session for an explicit format (e.g. IEEE float PCM).
    explicit WavMuxer(const WaveFormat& format)
        : handle_(createHandle(format), &mediaway_wav_muxer_close) {
        if (!handle_) {
            detail::throwError(Status::Panic, MEDIAWAY_STATUS_INTERNAL_PANIC,
                               "WAV muxer creation panicked");
        }
    }

    ~WavMuxer() = default;
    WavMuxer(WavMuxer&&) = default;
    WavMuxer& operator=(WavMuxer&&) = default;
    WavMuxer(const WavMuxer&) = delete;
    WavMuxer& operator=(const WavMuxer&) = delete;

    /// Append raw interleaved PCM bytes (already encoded per the session's
    /// format).
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
        detail::checkContainer(mediaway_wav_muxer_push_packet(handle_.get(), &raw));
    }

    /// Finalize the mux session and return the complete RIFF/WAVE byte
    /// stream. A second call throws Error(Status::InvalidState).
    Bytes finish() {
        std::uint8_t* data = nullptr;
        std::size_t len = 0;
        detail::checkContainer(mediaway_wav_muxer_finish(handle_.get(), &data, &len));
        if (len == 0) return {};
        Bytes out(data, data + len);
        mediaway_buffer_free(data, len);
        return out;
    }

private:
    static mediaway_wav_muxer_t* createHandle(const WaveFormat& format) {
        mediaway_wave_format_t raw{};
        raw.sample_format = static_cast<mediaway_wav_sample_format_t>(format.sampleFormat);
        raw.channels = format.channels;
        raw.sample_rate = format.sampleRate;
        raw.bits_per_sample = format.bitsPerSample;
        return mediaway_wav_muxer_create_with_format(&raw);
    }

    std::unique_ptr<mediaway_wav_muxer_t, void (*)(mediaway_wav_muxer_t*)> handle_;
};

/// Result of wavParse(): the single track's stream info and one packet
/// holding the whole PCM payload (RIFF/WAVE carries no internal frame
/// boundaries).
struct WavParseResult {
    StreamInfo info;
    Packet packet;
};

/// Parse a complete RIFF/WAVE buffer — a one-shot function, not a demuxer
/// class (WAV demux has no streaming state to hold: RIFF chunk sizes are
/// read from the header up front, not discovered incrementally).
inline WavParseResult wavParse(const Bytes& data) {
    mediaway_stream_info_t rawInfo{};
    mediaway_packet_t rawPacket{};
    detail::checkContainer(mediaway_wav_parse(data.data(), data.size(), &rawInfo, &rawPacket));
    Bytes extra(rawInfo.extra_data, rawInfo.extra_data + rawInfo.extra_data_len);
    Bytes payload(rawPacket.payload, rawPacket.payload + rawPacket.payload_len);
    WavParseResult result{detail::toStreamInfo(rawInfo, std::move(extra)),
                          detail::toPacket(rawPacket, std::move(payload))};
    mediaway_stream_info_free(&rawInfo);
    mediaway_packet_free(&rawPacket);
    return result;
}

}  // namespace container
}  // namespace mediaway

#endif  // MEDIAWAY_CONTAINER_WAV_HPP
