// encode_to_mp4.cpp — Mediaway auto video encoder -> fragmented MP4.
//
// ASPIRATIONAL EXAMPLE: no C++ Mediaway binding package exists yet, and no
// `mediaway-encoder-ffi` / `mediaway-pipeline-ffi` crate or
// <mediaway/encoder.h> / <mediaway/pipeline.h> header ships today (see
// docs/spec/c-ffi.md and bindings/README.md). This file shows the target
// ergonomics a future thin C++ RAII wrapper over Mediaway's C ABI should aim
// for. It mirrors examples/encode_to_mp4.rs.
//
// The wrapper classes below (AutoEncoder, EncodeSession) are exactly that —
// a thin layer: opaque C handles owned via unique_ptr + custom deleters,
// C ABI status codes translated into a `mediaway::Error` exception. They do
// not add capabilities beyond the raw C ABI (declared just below, standing
// in for the header a real binding would `#include`).
//
// Requires C++20 (designated initializers, std::span).

#include <cstdint>
#include <cstring>
#include <fstream>
#include <iostream>
#include <memory>
#include <optional>
#include <span>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

// ---------------------------------------------------------------------------
// Raw C ABI, as if declared in <mediaway/encoder.h> + <mediaway/pipeline.h>.
// Opaque handles + status codes only; no exceptions/panics cross this layer.
// A real binding would `#include` the generated headers instead of this.
// ---------------------------------------------------------------------------
extern "C" {

typedef struct mediaway_auto_encoder mediaway_auto_encoder_t;
typedef struct mediaway_encode_session mediaway_encode_session_t;

typedef enum mediaway_status {
    MEDIAWAY_OK = 0,
    MEDIAWAY_ERR_UNSUPPORTED_PLATFORM = 1,
    MEDIAWAY_ERR_INVALID_ARGUMENT = 2,
    MEDIAWAY_ERR_INTERNAL = 3,
} mediaway_status_t;

typedef enum mediaway_codec {
    MEDIAWAY_CODEC_H264 = 0,
} mediaway_codec_t;

typedef enum mediaway_pixel_format {
    MEDIAWAY_PIXEL_FORMAT_NV12 = 0,
} mediaway_pixel_format_t;

typedef struct mediaway_rational {
    uint32_t num;
    uint32_t den;
} mediaway_rational_t;

typedef struct mediaway_auto_encode_config {
    mediaway_codec_t codec;
    uint32_t width;
    uint32_t height;
    mediaway_rational_t frame_rate;
    uint64_t bitrate_bps;
} mediaway_auto_encode_config_t;

typedef struct mediaway_video_frame {
    int64_t pts;
    int64_t duration;
    uint32_t width;
    uint32_t height;
    mediaway_pixel_format_t pixel_format;
    const uint8_t *data;
    size_t data_len;
} mediaway_video_frame_t;

// Opens the best available H.264 encoder backend for this platform/GPU
// (Zero-Copy GPU path preferred, CPU-upload path as fallback). Returns
// MEDIAWAY_ERR_UNSUPPORTED_PLATFORM when no suitable backend exists here —
// that is an expected, recoverable outcome, not a bug.
mediaway_status_t mediaway_auto_encoder_open(const mediaway_auto_encode_config_t *config,
                                              mediaway_auto_encoder_t **out_encoder);
void mediaway_auto_encoder_close(mediaway_auto_encoder_t *encoder);

// Wraps an opened encoder together with an internal fragmented-MP4 muxer.
// Takes ownership of `encoder` on success (it must not be closed separately).
mediaway_status_t mediaway_encode_session_open(mediaway_auto_encoder_t *encoder,
                                                mediaway_encode_session_t **out_session);
mediaway_status_t mediaway_encode_session_write_frame(mediaway_encode_session_t *session,
                                                       const mediaway_video_frame_t *frame);
// Flushes the encoder and finalizes/flushes the muxer, returning the
// complete MP4 file bytes. Ownership of *out_bytes passes to the caller;
// free it with mediaway_buffer_free.
mediaway_status_t mediaway_encode_session_finish(mediaway_encode_session_t *session,
                                                  uint8_t **out_bytes, size_t *out_len);
void mediaway_encode_session_close(mediaway_encode_session_t *session);

void mediaway_buffer_free(uint8_t *bytes);

} // extern "C"

// ---------------------------------------------------------------------------
// Idiomatic C++ wrapper: RAII handles + exceptions instead of status codes.
// ---------------------------------------------------------------------------
namespace mediaway {

// Thrown when a C ABI call returns anything other than MEDIAWAY_OK.
class Error : public std::runtime_error {
public:
    Error(mediaway_status_t status, std::string_view what)
        : std::runtime_error(std::string(what) + " failed: status " +
                              std::to_string(static_cast<int>(status))),
          status_(status) {}

    mediaway_status_t status() const noexcept { return status_; }

private:
    mediaway_status_t status_;
};

struct Rational {
    uint32_t num;
    uint32_t den;
};

enum class CodecKind { H264 };
enum class PixelFormat { Nv12 };

// What to encode: codec, resolution, frame rate, and bitrate.
struct AutoEncodeConfig {
    CodecKind codec;
    uint32_t width;
    uint32_t height;
    Rational frameRate;
    uint64_t bitrateBps;

    // Sensible defaults for `codec` at `width`x`height`/`frameRate`.
    // Override individual fields afterward, e.g.:
    //   auto config = AutoEncodeConfig::h264(640, 480, {1, 30});
    //   config.bitrateBps = 2'000'000;
    static AutoEncodeConfig h264(uint32_t width, uint32_t height, Rational frameRate) {
        constexpr uint64_t kDefaultBitrateBps = 4'000'000;
        return AutoEncodeConfig{
            .codec = CodecKind::H264,
            .width = width,
            .height = height,
            .frameRate = frameRate,
            .bitrateBps = kDefaultBitrateBps,
        };
    }
};

// One raw video frame handed to an EncodeSession. `bytes` is a non-owning
// view — the caller's buffer must outlive the writeFrame() call.
struct VideoFrame {
    int64_t pts;
    int64_t duration;
    uint32_t width;
    uint32_t height;
    PixelFormat pixelFormat;
    std::span<const uint8_t> bytes;
};

namespace detail {

inline mediaway_codec_t toCCodec(CodecKind codec) {
    switch (codec) {
        case CodecKind::H264:
            return MEDIAWAY_CODEC_H264;
    }
    throw Error(MEDIAWAY_ERR_INVALID_ARGUMENT, "toCCodec: unknown CodecKind");
}

inline mediaway_pixel_format_t toCPixelFormat(PixelFormat format) {
    switch (format) {
        case PixelFormat::Nv12:
            return MEDIAWAY_PIXEL_FORMAT_NV12;
    }
    throw Error(MEDIAWAY_ERR_INVALID_ARGUMENT, "toCPixelFormat: unknown PixelFormat");
}

struct AutoEncoderDeleter {
    void operator()(mediaway_auto_encoder_t *handle) const noexcept {
        if (handle != nullptr) {
            mediaway_auto_encoder_close(handle);
        }
    }
};

struct EncodeSessionDeleter {
    void operator()(mediaway_encode_session_t *handle) const noexcept {
        if (handle != nullptr) {
            mediaway_encode_session_close(handle);
        }
    }
};

} // namespace detail

// RAII wrapper around an opened encoder backend. Move-only: opening an
// EncodeSession consumes it.
class AutoEncoder {
public:
    // Tries the best available encoder backend for this platform/GPU.
    // Throws mediaway::Error (status MEDIAWAY_ERR_UNSUPPORTED_PLATFORM) when
    // none is available here — callers should catch that and exit cleanly,
    // not treat it as a crash-worthy bug.
    static AutoEncoder open(const AutoEncodeConfig &config) {
        const mediaway_auto_encode_config_t rawConfig{
            .codec = detail::toCCodec(config.codec),
            .width = config.width,
            .height = config.height,
            .frame_rate = {.num = config.frameRate.num, .den = config.frameRate.den},
            .bitrate_bps = config.bitrateBps,
        };
        mediaway_auto_encoder_t *raw = nullptr;
        const mediaway_status_t status = mediaway_auto_encoder_open(&rawConfig, &raw);
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_auto_encoder_open");
        }
        return AutoEncoder(raw);
    }

    AutoEncoder(AutoEncoder &&) noexcept = default;
    AutoEncoder &operator=(AutoEncoder &&) noexcept = default;
    AutoEncoder(const AutoEncoder &) = delete;
    AutoEncoder &operator=(const AutoEncoder &) = delete;

private:
    friend class EncodeSession;

    explicit AutoEncoder(mediaway_auto_encoder_t *handle) : handle_(handle) {}

    // Hands raw ownership to EncodeSession::open; this wrapper no longer
    // closes the handle itself.
    mediaway_auto_encoder_t *release() noexcept { return handle_.release(); }

    std::unique_ptr<mediaway_auto_encoder_t, detail::AutoEncoderDeleter> handle_;
};

// Wires an opened AutoEncoder into an internal fragmented-MP4 muxer. Push
// frames with writeFrame(), then call finish() once for the complete file.
class EncodeSession {
public:
    static EncodeSession open(AutoEncoder encoder) {
        mediaway_encode_session_t *raw = nullptr;
        const mediaway_status_t status = mediaway_encode_session_open(encoder.release(), &raw);
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_encode_session_open");
        }
        return EncodeSession(raw);
    }

    void writeFrame(const VideoFrame &frame) {
        const mediaway_video_frame_t rawFrame{
            .pts = frame.pts,
            .duration = frame.duration,
            .width = frame.width,
            .height = frame.height,
            .pixel_format = detail::toCPixelFormat(frame.pixelFormat),
            .data = frame.bytes.data(),
            .data_len = frame.bytes.size(),
        };
        const mediaway_status_t status = mediaway_encode_session_write_frame(handle_.get(), &rawFrame);
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_encode_session_write_frame");
        }
    }

    // Flushes the encoder and the muxer, returning the complete MP4 bytes.
    std::vector<uint8_t> finish() {
        uint8_t *bytes = nullptr;
        size_t len = 0;
        const mediaway_status_t status = mediaway_encode_session_finish(handle_.get(), &bytes, &len);
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_encode_session_finish");
        }
        std::vector<uint8_t> result(bytes, bytes + len);
        mediaway_buffer_free(bytes);
        return result;
    }

private:
    explicit EncodeSession(mediaway_encode_session_t *handle) : handle_(handle) {}

    std::unique_ptr<mediaway_encode_session_t, detail::EncodeSessionDeleter> handle_;
};

} // namespace mediaway

int main() {
    using mediaway::AutoEncodeConfig;
    using mediaway::AutoEncoder;
    using mediaway::EncodeSession;
    using mediaway::PixelFormat;
    using mediaway::VideoFrame;

    constexpr uint32_t kWidth = 640;
    constexpr uint32_t kHeight = 480;
    constexpr uint32_t kFps = 30;
    constexpr uint32_t kSeconds = 3;
    constexpr uint32_t kFrameCount = kFps * kSeconds; // 90 frames

    AutoEncodeConfig config = AutoEncodeConfig::h264(kWidth, kHeight, {1, kFps});
    config.bitrateBps = 2'000'000;

    std::optional<EncodeSession> session;
    try {
        AutoEncoder encoder = AutoEncoder::open(config);
        session.emplace(EncodeSession::open(std::move(encoder)));
    } catch (const mediaway::Error &e) {
        // Expected, recoverable outcome: no suitable encoder backend on this
        // platform/GPU yet. Report it and exit cleanly — do not crash.
        std::cerr << "encode_to_mp4: no auto encoder available on this platform (" << e.what()
                   << ")\n";
        return 0;
    }

    std::cout << "encode_to_mp4: running on this platform\n";

    // Synthetic solid-grey NV12 source: width*height Y bytes (128) followed
    // by width*height/2 interleaved UV bytes (128). Replace with real frames
    // in your app.
    const size_t nv12Len = static_cast<size_t>(kWidth) * kHeight +
                           static_cast<size_t>(kWidth) * kHeight / 2;
    const std::vector<uint8_t> grey(nv12Len, 128);

    for (uint32_t pts = 0; pts < kFrameCount; ++pts) {
        const VideoFrame frame{
            .pts = static_cast<int64_t>(pts),
            .duration = 1,
            .width = kWidth,
            .height = kHeight,
            .pixelFormat = PixelFormat::Nv12,
            .bytes = std::span<const uint8_t>(grey),
        };
        session->writeFrame(frame);
    }

    const std::vector<uint8_t> mp4Bytes = session->finish();

    std::ofstream out("out.mp4", std::ios::binary);
    out.write(reinterpret_cast<const char *>(mp4Bytes.data()),
              static_cast<std::streamsize>(mp4Bytes.size()));
    out.close();

    std::cout << "encode_to_mp4: " << kFrameCount << " frames -> out.mp4 (" << mp4Bytes.size()
              << " bytes)\n";
    return 0;
}
