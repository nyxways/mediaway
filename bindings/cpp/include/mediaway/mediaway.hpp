/*
 * mediaway.hpp — header-only C++ wrapper over Mediaway's C ABI.
 *
 * Implements the DX contract in bindings/cpp/README.md (see also
 * docs/spec/c-ffi.md · ADR-0004): RAII classes own the opaque C handles
 * (unique_ptr + custom deleter), the ABI's per-crate status enums are
 * translated into mediaway::Error exceptions at the boundary, and the ABI's
 * handle-consumption traps (mediaway_encode_session_open / _finish consume
 * their handle unconditionally) are made unrepresentable via rvalue-qualified
 * typestate methods (begin() && / finish() &&).
 *
 * Capability truth (bindings/README.md truth table): container mux/demux and
 * the auto video encode -> fMP4 pipeline are real; camera/mic capture are
 * real (CPU frames); Screen capture is not representable from C today —
 * ScreenCapture::open() throws Error(Status::Unsupported).
 */

#ifndef MEDIAWAY_MEDIAWAY_HPP
#define MEDIAWAY_MEDIAWAY_HPP

#include <mediaway/container.h>
#include <mediaway/device.h>
#include <mediaway/pipeline.h>

#include <cstdint>
#include <cstdlib>
#include <memory>
#include <optional>
#include <stdexcept>
#include <string>
#include <utility>
#include <variant>
#include <vector>

namespace mediaway {

// ── Core value types ──────────────────────────────────────────────────────────

using Bytes = std::vector<std::uint8_t>;
using TrackId = std::uint32_t;

/// Timebase: one tick = num/den seconds. {1,30} = 30 fps; {1,48000} = 48 kHz.
struct Rational {
    std::uint32_t num;
    std::uint32_t den;
};

enum class Codec { H264, Aac, Unknown };

enum class PixelFormat { Nv12, Bgra8 };

/// Wrapper-level merge of the three per-crate ABI status enums.
enum class Status {
    Ok,
    NoBackend,
    Unsupported,
    NoDevice,
    InvalidArgument,
    InvalidState,
    MuxError,
    DemuxError,
    EncodeError,
    CaptureError,
    Panic,
};

/// Thrown by every wrapper entry point. `rawCode()` carries the per-crate ABI
/// status value; `status()` the merged wrapper Status.
class Error : public std::runtime_error {
public:
    Error(Status status, std::int32_t rawCode, std::string message)
        : std::runtime_error(std::move(message)), status_(status), rawCode_(rawCode) {}

    Status status() const noexcept { return status_; }
    std::int32_t rawCode() const noexcept { return rawCode_; }

private:
    Status status_;
    std::int32_t rawCode_;
};

/// One video frame — shared by capture output and encode input. Planes are
/// back-to-back (NV12: Y then interleaved UV, w*h*3/2 bytes total; BGRA8:
/// tightly packed w*h*4). `pts` is in timebase ticks.
struct VideoFrame {
    PixelFormat format;
    std::uint32_t width;
    std::uint32_t height;
    std::int64_t pts;
    Bytes data;
};

/// A video stream description. For muxing, `id` is ignored — the muxer
/// assigns it and the addVideoTrack return value is authoritative. For
/// demuxing, `id` is filled from the demuxed stream.
struct VideoStreamInfo {
    TrackId id;
    Codec codec;
    Rational timescale;
    std::uint32_t width;
    std::uint32_t height;
    Bytes codecConfig;  // e.g. avcC; empty when unknown
};

/// An audio stream description. Same id semantics as VideoStreamInfo.
struct AudioStreamInfo {
    TrackId id;
    Codec codec;
    Rational timescale;
    std::uint32_t sampleRate;
    std::uint16_t channels;
    Bytes codecConfig;  // e.g. AudioSpecificConfig; empty when unknown
};

using StreamInfo = std::variant<VideoStreamInfo, AudioStreamInfo>;

/// One muxed/demuxed packet. pts/dts are in the track's timebase ticks.
struct Packet {
    TrackId trackId;
    std::int64_t pts;
    std::int64_t dts;
    bool keyframe;
    Bytes data;
};

// ── Status mapping helpers ─────────────────────────────────────────────────────

namespace detail {

[[noreturn]] inline void throwError(Status status, std::int32_t rawCode, const char* what) {
    throw Error(status, rawCode, what);
}

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
        default: throwError(Status::MuxError, st, "unknown container error");
    }
}

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

inline void checkDevice(mediaway_device_status_t st) {
    switch (st) {
        case MEDIAWAY_DEVICE_STATUS_OK: return;
        case MEDIAWAY_DEVICE_STATUS_NO_BACKEND: throwError(Status::NoDevice, st, "no capture backend compiled in");
        case MEDIAWAY_DEVICE_STATUS_UNSUPPORTED: throwError(Status::Unsupported, st, "this capture configuration is unsupported by the ABI");
        case MEDIAWAY_DEVICE_STATUS_BACKEND_FAILURE:
        case MEDIAWAY_DEVICE_STATUS_ACCESS_DENIED:
        case MEDIAWAY_DEVICE_STATUS_CLOSED: throwError(Status::CaptureError, st, "capture backend failure");
        case MEDIAWAY_DEVICE_STATUS_INVALID_ARGUMENT:
        case MEDIAWAY_DEVICE_STATUS_INVALID_INPUT: throwError(Status::CaptureError, st, "invalid capture config");
        case MEDIAWAY_DEVICE_STATUS_TIMEOUT: throwError(Status::CaptureError, st, "timed out waiting for a frame");
        case MEDIAWAY_DEVICE_STATUS_CALLBACK_ALREADY_REGISTERED:
        case MEDIAWAY_DEVICE_STATUS_CALLBACK_MODE_ACTIVE: throwError(Status::CaptureError, st, "hotplug callback mode conflict");
        case MEDIAWAY_DEVICE_STATUS_INTERNAL_PANIC:
        case MEDIAWAY_DEVICE_STATUS_HANDLE_POISONED: throwError(Status::Panic, st, "caught Rust panic (handle poisoned)");
        default: throwError(Status::CaptureError, st, "unknown device error");
    }
}

inline int toAbiCodec(Codec codec) {
    // Both ABI codec enums (mediaway_codec_kind_t / mediaway_pipeline_codec_kind_t)
    // mirror the same Rust CodecKind 1:1 — values are deliberately identical.
    switch (codec) {
        case Codec::H264: return MEDIAWAY_CODEC_H264;
        case Codec::Aac: return MEDIAWAY_CODEC_AAC;
        default: return MEDIAWAY_CODEC_H264;  // Unknown is not muxable; H264 is the safest default
    }
}

inline Codec fromAbiCodec(int codec) {
    switch (codec) {
        case MEDIAWAY_CODEC_H264: return Codec::H264;
        case MEDIAWAY_CODEC_AAC: return Codec::Aac;
        default: return Codec::Unknown;
    }
}

inline mediaway_pixel_format_t toAbiPixel(PixelFormat format) {
    switch (format) {
        case PixelFormat::Nv12: return MEDIAWAY_PIXEL_FORMAT_NV12;
        case PixelFormat::Bgra8: return MEDIAWAY_PIXEL_FORMAT_BGRA8;
    }
    return MEDIAWAY_PIXEL_FORMAT_NV12;
}

inline PixelFormat fromAbiPixel(int format) {
    switch (format) {
        case MEDIAWAY_PIXEL_FORMAT_NV12: return PixelFormat::Nv12;
        case MEDIAWAY_PIXEL_FORMAT_BGRA8: return PixelFormat::Bgra8;
        default: return PixelFormat::Nv12;
    }
}

inline void cameraCaptureClose(mediaway_camera_capture_t* capture) noexcept {
    // close() returns a real status (it joins the backend worker thread); the
    // unique_ptr deleter must be void, so the status is intentionally dropped.
    (void)mediaway_camera_capture_close(capture);
}

inline void desktopCaptureClose(mediaway_desktop_capture_t* capture) noexcept {
    (void)mediaway_desktop_capture_close(capture);
}

inline void audioCaptureClose(mediaway_audio_capture_t* capture) noexcept {
    (void)mediaway_audio_capture_close(capture);
}

}  // namespace detail

// ── Container: mux + demux ────────────────────────────────────────────────────

namespace container {

class LiveMuxer;

/// A muxer in the track-registration (Open) state. begin() (rvalue-only)
/// consumes this object and returns the streaming LiveMuxer — track
/// registration after begin() is a compile error, mirroring the ABI's
/// INVALID_STATE.
class Muxer {
public:
    Muxer() : handle_(mediaway_muxer_create(), &mediaway_muxer_close) {
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
    std::uint32_t nextId_ = 0;
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
    Demuxer() : handle_(mediaway_demuxer_create(), &mediaway_demuxer_close) {
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
            if (raw.has_geometry) {
                out.emplace_back(VideoStreamInfo{
                    raw.id, detail::fromAbiCodec(raw.codec),
                    {static_cast<std::uint32_t>(raw.time_base.num), raw.time_base.den},
                    raw.width, raw.height, std::move(extra)});
            } else {
                out.emplace_back(AudioStreamInfo{
                    raw.id, detail::fromAbiCodec(raw.codec),
                    {static_cast<std::uint32_t>(raw.time_base.num), raw.time_base.den},
                    raw.sample_rate, raw.channels, std::move(extra)});
            }
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
        return Packet{raw.stream_id, raw.pts, raw.dts, raw.is_keyframe, std::move(payload)};
    }

    /// Set the ClearKey decryption key (exactly 16 bytes) for all encrypted
    /// tracks. Only affects samples drained from SUBSEQUENT pushBytes calls.
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

// ── Pipeline: auto video encode -> fMP4 ───────────────────────────────────────

namespace encoder {

class EncodeSession;

}  // namespace encoder

// device::AudioFrame is defined here, before the encoder namespace uses it in
// AudioEncoder::pushPcm (the device capture section below reopens this
// namespace).
namespace device {
/// One polled PCM chunk; data is raw interleaved F32 samples, and pts is the
/// first sample index in the stream timebase.
struct AudioFrame {
    std::int64_t pts;
    std::uint32_t sampleRate;
    std::uint16_t channels;
    Bytes data;
};
}  // namespace device

namespace encoder {

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

// ── Device: camera / screen / microphone capture ──────────────────────────────

namespace device {

struct VideoCaptureConfig {
    std::uint32_t deviceIndex;
    Rational frameRate;
    std::uint32_t width = 0;   // 0 = camera default (negotiated)
    std::uint32_t height = 0;
};

struct AudioCaptureConfig {
    std::uint32_t deviceIndex;
    std::uint32_t sampleRate;
    std::uint16_t channels = 1;
};

struct ScreenCaptureConfig {
    std::uint32_t displayIndex;
    Rational frameRate;
    std::uint32_t width = 0;   // 0 = native
    std::uint32_t height = 0;
};

/// Capture properties negotiated after open — authoritative over the config.
struct CaptureInfo {
    std::uint32_t width;
    std::uint32_t height;
    Rational frameRate;
    PixelFormat format;  // camera = NV12
};

/// A Camera video capture session (CPU frames). Screen is not representable
/// from C today — see ScreenCapture.
class VideoCapture {
public:
    /// Open camera `deviceIndex` at `frameRate`. Throws Error(Status::NoDevice)
    /// when no camera/backend exists — catch it and degrade gracefully.
    static VideoCapture open(const VideoCaptureConfig& config) {
        mediaway_camera_capture_config_t raw =
            mediaway_camera_capture_config_default(config.deviceIndex,
                                                   {config.frameRate.num, config.frameRate.den});
        mediaway_camera_capture_t* capture = nullptr;
        detail::checkDevice(mediaway_camera_capture_open(&raw, &capture));
        if (!capture) {
            detail::throwError(Status::Panic, MEDIAWAY_DEVICE_STATUS_INTERNAL_PANIC,
                               "capture open returned no handle");
        }
        VideoCapture session(capture, config.frameRate);
        session.queryGeometry();
        return session;
    }

    ~VideoCapture() { close(); }
    VideoCapture(VideoCapture&&) = default;
    VideoCapture& operator=(VideoCapture&&) = default;
    VideoCapture(const VideoCapture&) = delete;
    VideoCapture& operator=(const VideoCapture&) = delete;

    /// Negotiated capture properties (geometry queried at open; may be 0x0
    /// until the backend has negotiated).
    const CaptureInfo& info() const { return info_; }

    /// Poll the next frame without blocking; nullopt when nothing is ready.
    std::optional<VideoFrame> pollFrame() {
        mediaway_camera_frame_t raw{};
        bool has = false;
        detail::checkDevice(mediaway_camera_capture_poll_frame(handle_.get(), &raw, &has));
        if (!has) return std::nullopt;
        Bytes data(raw.data, raw.data + raw.data_len);
        mediaway_camera_frame_free(&raw);
        return VideoFrame{detail::fromAbiPixel(raw.pixel_format), raw.width, raw.height,
                          raw.pts, std::move(data)};
    }

    /// Block up to `timeoutMs` for the next frame.
    std::optional<VideoFrame> pollFrameBlocking(std::uint32_t timeoutMs) {
        mediaway_camera_frame_t raw{};
        const mediaway_device_status_t st =
            mediaway_camera_capture_poll_frame_blocking(handle_.get(), timeoutMs, &raw);
        if (st == MEDIAWAY_DEVICE_STATUS_TIMEOUT) return std::nullopt;
        detail::checkDevice(st);
        Bytes data(raw.data, raw.data + raw.data_len);
        mediaway_camera_frame_free(&raw);
        return VideoFrame{detail::fromAbiPixel(raw.pixel_format), raw.width, raw.height,
                          raw.pts, std::move(data)};
    }

    /// Release backend resources held by the last polled frame. Documented
    /// no-op for Camera today, but required before the next frame-acquiring poll.
    void releaseFrame() {
        detail::checkDevice(mediaway_camera_capture_release_frame(handle_.get()));
    }

    /// Close the session. BLOCKS up to one frame interval (joins the backend
    /// worker thread) — a real cost, not a pointer free.
    void close() noexcept {
        if (handle_) {
            mediaway_camera_capture_close(handle_.get());
            handle_.release();  // already closed; release so the deleter cannot double-close
        }
    }

private:
    explicit VideoCapture(mediaway_camera_capture_t* handle, Rational frameRate)
        : handle_(handle, &detail::cameraCaptureClose), info_{0, 0, frameRate, PixelFormat::Nv12} {}

    void queryGeometry() {
        std::uint32_t width = 0;
        std::uint32_t height = 0;
        if (mediaway_camera_capture_geometry(handle_.get(), &width, &height) ==
            MEDIAWAY_DEVICE_STATUS_OK) {
            info_.width = width;
            info_.height = height;
        }
    }

    std::unique_ptr<mediaway_camera_capture_t, void (*)(mediaway_camera_capture_t*)> handle_;
    CaptureInfo info_;
};

/// A Screen capture session — NOT representable from C today. open() throws
/// Error(Status::Unsupported): Screen needs a live GPU device handle
/// (ID3D11Device*) with no CPU fallback, and its C representation is deferred
/// (crates/mediaway-device-ffi/adr/0001 § Deferred). The rest of the class is
/// wired to the desktop ABI (mediaway_desktop_capture_*) for when that lands.
class ScreenCapture {
public:
    /// Throws Error(Status::Unsupported) today — see the class comment. The
    /// ideal surface (BGRA8 CPU frames at the display's native geometry) is
    /// what the aspirational screen_record example targets.
    static ScreenCapture open(const ScreenCaptureConfig& config) {
        (void)config;
        detail::throwError(Status::Unsupported, MEDIAWAY_DEVICE_STATUS_UNSUPPORTED,
                           "Screen capture needs a live GPU device handle with no CPU "
                           "fallback, and its C representation is deferred — not "
                           "available from this binding today");
    }

    ~ScreenCapture() { close(); }
    // Default-constructs an empty (not-yet-open) session; the unique_ptr's
    // function-pointer deleter must be supplied explicitly (SFINAE-disabled
    // otherwise).
    ScreenCapture()
        : handle_(nullptr, &detail::desktopCaptureClose), info_{0, 0, {0, 0}, PixelFormat::Bgra8} {}
    ScreenCapture(ScreenCapture&&) = default;
    ScreenCapture& operator=(ScreenCapture&&) = default;
    ScreenCapture(const ScreenCapture&) = delete;
    ScreenCapture& operator=(const ScreenCapture&) = delete;

    /// Negotiated capture properties (the ideal path delivers BGRA8 CPU
    /// frames at the native display geometry).
    const CaptureInfo& info() const { return info_; }

    /// Poll the next frame without blocking; nullopt when nothing is ready.
    /// CPU-storage frames only — a GPU frame surfaces as Status::CaptureError.
    std::optional<VideoFrame> pollFrame() {
        mediaway_desktop_frame_t raw{};
        bool has = false;
        detail::checkDevice(mediaway_desktop_capture_poll_frame(handle_.get(), &raw, &has));
        if (!has) return std::nullopt;
        if (raw.storage_kind != MEDIAWAY_VIDEO_FRAME_STORAGE_CPU) {
            mediaway_desktop_frame_free(&raw);
            detail::throwError(Status::CaptureError, MEDIAWAY_DEVICE_STATUS_UNSUPPORTED,
                               "GPU-storage screen frames are not exposed by this wrapper");
        }
        Bytes data(raw.data, raw.data + raw.data_len);
        mediaway_desktop_frame_free(&raw);
        return VideoFrame{detail::fromAbiPixel(raw.pixel_format), raw.width, raw.height,
                          raw.pts, std::move(data)};
    }

    /// Close the session. BLOCKS up to one frame interval (joins the backend
    /// worker thread).
    void close() noexcept {
        if (handle_) {
            detail::desktopCaptureClose(handle_.get());
            handle_.release();  // already closed; release so the deleter cannot double-close
        }
    }

private:
    explicit ScreenCapture(mediaway_desktop_capture_t* handle, Rational frameRate)
        : handle_(handle, &detail::desktopCaptureClose),
          info_{0, 0, frameRate, PixelFormat::Bgra8} {}

    std::unique_ptr<mediaway_desktop_capture_t, void (*)(mediaway_desktop_capture_t*)> handle_;
    CaptureInfo info_;
};

/// A Microphone audio capture session (raw interleaved PCM).
class AudioCapture {
public:
    /// Open microphone `deviceIndex` at `sampleRate` Hz. Throws
    /// Error(Status::NoDevice) when no mic/backend exists.
    static AudioCapture open(const AudioCaptureConfig& config) {
        mediaway_audio_capture_config_t raw =
            mediaway_audio_capture_config_microphone({1, config.sampleRate});
        raw.device_index = config.deviceIndex;
        mediaway_audio_capture_t* capture = nullptr;
        detail::checkDevice(mediaway_audio_capture_open(&raw, &capture));
        if (!capture) {
            detail::throwError(Status::Panic, MEDIAWAY_DEVICE_STATUS_INTERNAL_PANIC,
                               "capture open returned no handle");
        }
        return AudioCapture(capture);
    }

    ~AudioCapture() { close(); }
    AudioCapture(AudioCapture&&) = default;
    AudioCapture& operator=(AudioCapture&&) = default;
    AudioCapture(const AudioCapture&) = delete;
    AudioCapture& operator=(const AudioCapture&) = delete;

    /// Poll the next PCM chunk without blocking; nullopt when nothing is ready.
    std::optional<AudioFrame> pollFrame() {
        mediaway_device_audio_frame_t raw{};
        bool has = false;
        detail::checkDevice(mediaway_audio_capture_poll_frame(handle_.get(), &raw, &has));
        if (!has) return std::nullopt;
        Bytes data(raw.data, raw.data + raw.data_len);
        mediaway_audio_frame_free(&raw);
        return AudioFrame{raw.pts, raw.sample_rate, raw.channels, std::move(data)};
    }

    /// Negotiated capture format (WASAPI GetMixFormat values) — authoritative
    /// over the requested config; feed it to the audio encoder unchanged.
    void format(std::uint32_t& sampleRate, std::uint16_t& channels) {
        detail::checkDevice(mediaway_audio_capture_format(handle_.get(), &sampleRate, &channels));
    }

    /// Close the session. BLOCKS up to one period interval (joins the backend
    /// worker thread).
    void close() noexcept {
        if (handle_) {
            mediaway_audio_capture_close(handle_.get());
            handle_.release();  // already closed; release so the deleter cannot double-close
        }
    }

private:
    explicit AudioCapture(mediaway_audio_capture_t* handle)
        : handle_(handle, &detail::audioCaptureClose) {}
    std::unique_ptr<mediaway_audio_capture_t, void (*)(mediaway_audio_capture_t*)> handle_;
};

}  // namespace device

}  // namespace mediaway

#endif  // MEDIAWAY_MEDIAWAY_HPP
