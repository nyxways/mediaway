/*
 * pipeline.hpp — auto video encode -> fMP4 and audio encode wrapper classes.
 *
 * Split out of the original single-file mediaway.hpp once wiring all 8
 * container formats pushed the combined header past the workspace's
 * 1000-line source-file cap.
 */

#ifndef MEDIAWAY_PIPELINE_HPP
#define MEDIAWAY_PIPELINE_HPP

#include <mediaway/core.hpp>
#include <mediaway/device.hpp>
#include <mediaway/pipeline.h>

#include <memory>
#include <optional>

namespace mediaway {

namespace detail {

inline void checkPipeline(mediaway_pipeline_status_t st) {
    switch (st) {
        case MEDIAWAY_PIPELINE_STATUS_OK: return;
        case MEDIAWAY_PIPELINE_STATUS_NO_BACKEND: throwError(Status::NoBackend, st, "no encode backend compiled in or openable");
        case MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED: throwError(Status::Unsupported, st, "codec/pixel-format/geometry not supported");
        case MEDIAWAY_PIPELINE_STATUS_INVALID_ARGUMENT:
        case MEDIAWAY_PIPELINE_STATUS_INVALID_INPUT: throwError(Status::InvalidArgument, st, "invalid argument or input");
        case MEDIAWAY_PIPELINE_STATUS_ENCODER_BACKEND_FAILURE:
        case MEDIAWAY_PIPELINE_STATUS_ENCODER_CLOSED: throwError(Status::EncodeError, st, "encoder backend failure or closed session");
        case MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_TRACK:
        case MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_PACKET:
        case MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_DATA: throwError(Status::MuxError, st, "muxer rejected encoder output");
        case MEDIAWAY_PIPELINE_STATUS_INTERNAL_PANIC:
        case MEDIAWAY_PIPELINE_STATUS_HANDLE_POISONED: throwError(Status::Panic, st, "caught Rust panic (handle poisoned)");
        default: throwError(Status::EncodeError, st, "unknown pipeline error");
    }
}

}  // namespace detail

namespace encoder {

class EncodeSession;

struct VideoEncoderConfig {
    Codec codec;
    std::uint32_t width;
    std::uint32_t height;
    Rational frameRate;
    PixelFormat inputFormat = PixelFormat::Nv12;
};

/// An opened auto encoder: the best available backend for the config. open()
/// throws Error(Status::NoBackend) when no encoder exists on this machine —
/// an expected outcome, not a hard failure. begin() (rvalue-only) transfers
/// ownership into an EncodeSession.
class AutoVideoEncoder {
public:
    static AutoVideoEncoder open(const VideoEncoderConfig& config) {
        mediaway_auto_video_encode_config_t raw = mediaway_auto_video_encode_config_new(
            static_cast<mediaway_pipeline_codec_kind_t>(detail::toAbiCodec(config.codec)),
            config.width, config.height,
            {config.frameRate.num, config.frameRate.den});
        raw.bitrate_bps = 0;  // backend default
        raw.pixel_format = detail::toAbiPixel(config.inputFormat);
        mediaway_auto_encoder_t* encoder = nullptr;
        detail::checkPipeline(mediaway_auto_encoder_open(&raw, &encoder));
        if (!encoder) {
            detail::throwError(Status::Panic, MEDIAWAY_PIPELINE_STATUS_INTERNAL_PANIC,
                               "encoder open returned no handle");
        }
        return AutoVideoEncoder(encoder);
    }

    ~AutoVideoEncoder() = default;
    AutoVideoEncoder(AutoVideoEncoder&&) = default;
    AutoVideoEncoder& operator=(AutoVideoEncoder&&) = default;
    AutoVideoEncoder(const AutoVideoEncoder&) = delete;
    AutoVideoEncoder& operator=(const AutoVideoEncoder&) = delete;

    /// Transfer the encoder into an encode session (consumes this object).
    EncodeSession begin() &&;

private:
    friend class EncodeSession;
    explicit AutoVideoEncoder(mediaway_auto_encoder_t* handle)
        : handle_(handle, &mediaway_auto_encoder_close) {}
    std::unique_ptr<mediaway_auto_encoder_t, void (*)(mediaway_auto_encoder_t*)> handle_;
};

/// A single-use encode session. finish() (rvalue-only) consumes the session
/// and returns the complete fMP4 bytes — the ABI's unconditional handle
/// consumption cannot be double-released.
class EncodeSession {
public:
    ~EncodeSession() = default;
    EncodeSession(EncodeSession&&) = default;
    EncodeSession& operator=(EncodeSession&&) = default;
    EncodeSession(const EncodeSession&) = delete;
    EncodeSession& operator=(const EncodeSession&) = delete;

    void writeFrame(const VideoFrame& frame) {
        mediaway_video_frame_t raw{};
        raw.pts = frame.pts;
        raw.duration = 0;  // unknown
        raw.width = frame.width;
        raw.height = frame.height;
        raw.pixel_format = detail::toAbiPixel(frame.format);
        raw.storage_kind = MEDIAWAY_VIDEO_FRAME_STORAGE_CPU;
        raw.raw_bytes = frame.data.empty() ? nullptr : frame.data.data();
        raw.raw_bytes_len = frame.data.size();
        detail::checkPipeline(mediaway_encode_session_write_frame(handle_.get(), &raw));
    }

    /// Flush the encoder + muxer; returns the complete fMP4 bytes. Consumes
    /// the session — the ABI frees the handle inside finish(), so it is
    /// released here (even on failure), never closed (double-free otherwise).
    Bytes finish() && {
        std::uint8_t* data = nullptr;
        std::size_t len = 0;
        const mediaway_pipeline_status_t st =
            mediaway_encode_session_finish(handle_.get(), &data, &len);
        handle_.release();  // consumed by finish unconditionally, success or failure
        detail::checkPipeline(st);
        Bytes out;
        if (len > 0) out.assign(data, data + len);
        mediaway_pipeline_ffi_buffer_free(data, len);
        return out;
    }

private:
    friend class AutoVideoEncoder;
    explicit EncodeSession(mediaway_encode_session_t* handle)
        : handle_(handle, &mediaway_encode_session_close) {}
    std::unique_ptr<mediaway_encode_session_t, void (*)(mediaway_encode_session_t*)> handle_;
};

inline EncodeSession AutoVideoEncoder::begin() && {
    mediaway_encode_session_t* session = nullptr;
    // mediaway_encode_session_open consumes `encoder` UNCONDITIONALLY — the
    // unique_ptr releases it even when the call fails, matching the ABI.
    const mediaway_pipeline_status_t st =
        mediaway_encode_session_open(handle_.get(), &session);
    handle_.release();
    detail::checkPipeline(st);
    if (!session) {
        detail::throwError(Status::Panic, MEDIAWAY_PIPELINE_STATUS_INTERNAL_PANIC,
                           "session open returned no handle");
    }
    return EncodeSession(session);
}

/// An opened auto audio encoder — the session IS the encoder (ABI v2,
/// adr/0003): single-step open, no intermediate handle, no consumption trap.
/// open() throws Error(Status::NoBackend) when no audio backend exists — an
/// expected outcome, not a hard failure.
class AudioEncoder {
public:
    /// `channels`/`sampleRate` must match the PCM frames pushed afterward
    /// (e.g. the mic's negotiated values — the AAC sugar defaults to stereo,
    /// which a mono mic is not); `timeBase` is the sample clock.
    static AudioEncoder open(std::uint32_t sampleRate, std::uint16_t channels,
                             Rational timeBase, std::uint32_t bitrateBps = 0) {
        mediaway_audio_encode_config_t raw =
            mediaway_audio_encode_config_aac(sampleRate, {timeBase.num, timeBase.den});
        raw.channels = channels;
        raw.bitrate_bps = bitrateBps;
        mediaway_audio_encode_session_t* session = nullptr;
        detail::checkPipeline(mediaway_audio_encoder_open(&raw, &session));
        if (!session) {
            detail::throwError(Status::Panic, MEDIAWAY_PIPELINE_STATUS_INTERNAL_PANIC,
                               "audio encoder open returned no handle");
        }
        return AudioEncoder(session);
    }

    ~AudioEncoder() = default;
    AudioEncoder(AudioEncoder&&) = default;
    AudioEncoder& operator=(AudioEncoder&&) = default;
    AudioEncoder(const AudioEncoder&) = delete;
    AudioEncoder& operator=(const AudioEncoder&) = delete;

    /// Push one interleaved F32 PCM chunk (device::AudioFrame is F32 by
    /// contract). `pts` is in the stream timebase; `data` is copied
    /// synchronously inside the call.
    void pushPcm(const device::AudioFrame& frame) {
        mediaway_audio_frame_view_t raw{};
        raw.pts = frame.pts;
        raw.duration = 0;  // unknown
        raw.sample_rate = frame.sampleRate;
        raw.channels = frame.channels;
        raw.sample_format = MEDIAWAY_SAMPLE_FORMAT_F32;
        raw.data = frame.data.empty() ? nullptr : frame.data.data();
        raw.data_len = frame.data.size();
        detail::checkPipeline(mediaway_audio_encode_session_push_pcm(handle_.get(), &raw));
    }

    /// Pull the next encoded packet, if one is ready. nullopt is a valid
    /// "nothing ready" result, not an error. `Packet::trackId` is 0 — set it
    /// to the muxer-assigned audio track id before pushing.
    std::optional<Packet> pollPacket() {
        mediaway_audio_packet_t raw{};
        bool has = false;
        detail::checkPipeline(
            mediaway_audio_encode_session_poll_packet(handle_.get(), &raw, &has));
        if (!has) return std::nullopt;
        Bytes data(raw.payload, raw.payload + raw.payload_len);
        mediaway_pipeline_ffi_packet_free(&raw);
        return Packet{0, raw.pts, raw.dts, raw.is_keyframe, std::move(data)};
    }

    /// Signal end of input; drain the remaining packets with pollPacket().
    void flush() {
        detail::checkPipeline(mediaway_audio_encode_session_flush(handle_.get()));
    }

    /// Stream metadata: codec, timescale, negotiated sample rate/channels and
    /// the codec config (AudioSpecificConfig an MP4 track needs). Available
    /// after the first pushed frame — the WMF backend materializes it then
    /// (adr/0003).
    AudioStreamInfo streamInfo() {
        mediaway_audio_stream_info_t raw{};
        detail::checkPipeline(mediaway_audio_encode_session_stream_info(handle_.get(), &raw));
        AudioStreamInfo info{0, detail::fromAbiCodec(static_cast<int>(raw.codec)),
                             {static_cast<std::uint32_t>(raw.time_base.num),
                              raw.time_base.den},
                             raw.sample_rate, raw.channels, {}};
        if (raw.extra_data_len > 0) {
            info.codecConfig.assign(raw.extra_data, raw.extra_data + raw.extra_data_len);
        }
        mediaway_pipeline_ffi_stream_info_free(&raw);
        return info;
    }

private:
    explicit AudioEncoder(mediaway_audio_encode_session_t* handle)
        : handle_(handle, &mediaway_audio_encode_session_close) {}
    std::unique_ptr<mediaway_audio_encode_session_t,
                    void (*)(mediaway_audio_encode_session_t*)>
        handle_;
};

}  // namespace encoder
}  // namespace mediaway

#endif  // MEDIAWAY_PIPELINE_HPP
