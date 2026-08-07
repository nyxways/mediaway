/*
 * core.hpp — shared value types, error/status mapping, and codec/pixel enum
 * conversions used by every mediaway::* namespace (container/pipeline/device).
 *
 * Split out of the original single-file mediaway.hpp (ADR pending: see
 * bindings/cpp/README.md) once wiring all 8 container formats pushed the
 * combined header past the workspace's 1000-line source-file cap
 * (see docs/conventions/, forbid-long-source pre-commit hook).
 */

#ifndef MEDIAWAY_CORE_HPP
#define MEDIAWAY_CORE_HPP

#include <mediaway/container.h>

#include <cstdint>
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

/// Codec kind, shared across every container format this binding wraps.
/// `Unknown` is never valid to mux — only ever produced by a demuxer reading
/// a stream this binding has no mapping for.
enum class Codec { H264, Hevc, Vp8, Aac, Opus, Mp3, Vorbis, RawAudio, Unknown };

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

namespace detail {

[[noreturn]] inline void throwError(Status status, std::int32_t rawCode, const char* what) {
    throw Error(status, rawCode, what);
}

/// Both ABI codec enums (mediaway_codec_kind_t / mediaway_pipeline_codec_kind_t)
/// mirror the same Rust CodecKind 1:1 — values are deliberately identical, so
/// this int-returning helper is reused at every call site regardless of which
/// C enum type the caller needs to `static_cast` it into.
inline int toAbiCodec(Codec codec) {
    switch (codec) {
        case Codec::H264: return MEDIAWAY_CODEC_H264;
        case Codec::Hevc: return MEDIAWAY_CODEC_HEVC;
        case Codec::Vp8: return MEDIAWAY_CODEC_VP8;
        case Codec::Aac: return MEDIAWAY_CODEC_AAC;
        case Codec::Opus: return MEDIAWAY_CODEC_OPUS;
        case Codec::Mp3: return MEDIAWAY_CODEC_MP3;
        case Codec::Vorbis: return MEDIAWAY_CODEC_VORBIS;
        case Codec::RawAudio: return MEDIAWAY_CODEC_RAW_AUDIO;
        default: return MEDIAWAY_CODEC_H264;  // Unknown is not muxable; H264 is the safest default
    }
}

inline Codec fromAbiCodec(int codec) {
    switch (codec) {
        case MEDIAWAY_CODEC_H264: return Codec::H264;
        case MEDIAWAY_CODEC_HEVC: return Codec::Hevc;
        case MEDIAWAY_CODEC_VP8: return Codec::Vp8;
        case MEDIAWAY_CODEC_AAC: return Codec::Aac;
        case MEDIAWAY_CODEC_OPUS: return Codec::Opus;
        case MEDIAWAY_CODEC_MP3: return Codec::Mp3;
        case MEDIAWAY_CODEC_VORBIS: return Codec::Vorbis;
        case MEDIAWAY_CODEC_RAW_AUDIO: return Codec::RawAudio;
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

}  // namespace detail

}  // namespace mediaway

#endif  // MEDIAWAY_CORE_HPP
