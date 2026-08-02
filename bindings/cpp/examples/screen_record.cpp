// screen_record.cpp — Mediaway screen + mic capture -> encode -> fragmented MP4.
//
// ASPIRATIONAL EXAMPLE: no C++ Mediaway binding package exists yet, and no
// `mediaway-device-ffi` / `mediaway-pipeline-ffi` crate or <mediaway/device.h>
// / <mediaway/pipeline.h> header ships today (see docs/spec/c-ffi.md and
// bindings/README.md). This file shows the target ergonomics a future thin
// C++ RAII wrapper over Mediaway's C ABI should aim for. It mirrors
// examples/screen_record.rs.
//
// This is the SAME building blocks as bindings/cpp/encode_to_mp4.cpp (a
// config -> open auto encoder -> open encode session -> writeFrame -> finish
// flow), plus a device-capture layer, glued together by one small
// platform-agnostic `record(...)` function. That function is typed only
// against abstract interfaces (`VideoCapture&`, `AudioCapture&`) — swapping
// the concrete OS backend underneath requires no change to it.
//
// Requires C++20 (designated initializers, std::span).

#include <chrono>
#include <cstdint>
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
// Raw C ABI, as if declared in <mediaway/device.h> + <mediaway/pipeline.h>.
// Opaque handles + status codes only; no exceptions/panics cross this layer.
// A real binding would `#include` the generated headers instead of this.
// ---------------------------------------------------------------------------
extern "C" {

typedef struct mediaway_auto_encoder mediaway_auto_encoder_t;
typedef struct mediaway_encode_session mediaway_encode_session_t;
typedef struct mediaway_video_capture mediaway_video_capture_t;
typedef struct mediaway_audio_capture mediaway_audio_capture_t;

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
    int32_t num;
    int32_t den;
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

mediaway_status_t mediaway_auto_encoder_open(const mediaway_auto_encode_config_t *config,
                                              mediaway_auto_encoder_t **out_encoder);
void mediaway_auto_encoder_close(mediaway_auto_encoder_t *encoder);

mediaway_status_t mediaway_encode_session_open(mediaway_auto_encoder_t *encoder,
                                                mediaway_encode_session_t **out_session);
mediaway_status_t mediaway_encode_session_write_frame(mediaway_encode_session_t *session,
                                                       const mediaway_video_frame_t *frame);
mediaway_status_t mediaway_encode_session_finish(mediaway_encode_session_t *session,
                                                  uint8_t **out_bytes, size_t *out_len);
void mediaway_encode_session_close(mediaway_encode_session_t *session);

void mediaway_buffer_free(uint8_t *bytes);

// ── Device capture ──────────────────────────────────────────────────────────

typedef struct mediaway_video_capture_config {
    uint32_t display_index; // 0 = primary display
    mediaway_rational_t frame_rate;
} mediaway_video_capture_config_t;

typedef struct mediaway_audio_capture_config {
    mediaway_rational_t sample_rate;
} mediaway_audio_capture_config_t;

typedef struct mediaway_video_geometry {
    uint32_t width;
    uint32_t height;
} mediaway_video_geometry_t;

// Opaque handle for one polled video frame. Under a real GPU-capture backend
// this stands for GPU-resident memory (e.g. a DXGI/D3D11 texture) that must
// be released back to the OS via mediaway_video_capture_release_frame before
// the next poll; it is never read directly through this token.
typedef struct mediaway_video_frame_token {
    uint64_t opaque;
} mediaway_video_frame_token_t;

typedef struct mediaway_audio_frame_token {
    uint64_t opaque;
} mediaway_audio_frame_token_t;

// Opens screen capture for `config.display_index` at `config.frame_rate`.
// Returns MEDIAWAY_ERR_UNSUPPORTED_PLATFORM when no screen-capture backend
// exists here yet — an expected, recoverable outcome, not a bug.
mediaway_status_t mediaway_video_capture_open(const mediaway_video_capture_config_t *config,
                                               mediaway_video_capture_t **out_capture);
// The actual stream geometry the backend settled on (e.g. the display's
// native resolution) — valid only after a successful open.
mediaway_status_t mediaway_video_capture_geometry(mediaway_video_capture_t *capture,
                                                   mediaway_video_geometry_t *out_geometry);
// Non-blocking poll. `*out_has_frame` reports whether `*out_token` was
// filled; `MEDIAWAY_OK` with `*out_has_frame == false` means "nothing new
// yet", not an error.
mediaway_status_t mediaway_video_capture_poll_frame(mediaway_video_capture_t *capture,
                                                     mediaway_video_frame_token_t *out_token,
                                                     bool *out_has_frame);
mediaway_status_t mediaway_video_capture_release_frame(mediaway_video_capture_t *capture,
                                                        const mediaway_video_frame_token_t *token);
// Explicit close, callable once ahead of destruction; idempotent no-op if
// the capture was already closed.
mediaway_status_t mediaway_video_capture_close(mediaway_video_capture_t *capture);

// Opens the microphone at `config.sample_rate`. Returns
// MEDIAWAY_ERR_UNSUPPORTED_PLATFORM when no microphone backend exists here
// yet — callers should keep running without audio, not abort.
mediaway_status_t mediaway_audio_capture_open(const mediaway_audio_capture_config_t *config,
                                               mediaway_audio_capture_t **out_capture);
mediaway_status_t mediaway_audio_capture_poll_frame(mediaway_audio_capture_t *capture,
                                                     mediaway_audio_frame_token_t *out_token,
                                                     bool *out_has_frame);
mediaway_status_t mediaway_audio_capture_close(mediaway_audio_capture_t *capture);

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
    int32_t num;
    int32_t den;
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
    //   auto config = AutoEncodeConfig::h264(1920, 1080, {1, 30});
    //   config.bitrateBps = 8'000'000;
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
        const mediaway_status_t status =
            mediaway_encode_session_write_frame(handle_.get(), &rawFrame);
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

// ---------------------------------------------------------------------------
// Device capture: abstract interfaces + concrete screen/mic backends.
// ---------------------------------------------------------------------------
namespace device {

struct VideoGeometry {
    uint32_t width;
    uint32_t height;
};

// Opaque token for one polled video frame. May stand for GPU-resident memory
// under the hood (e.g. a DXGI/D3D11 texture) — pass it to
// VideoCapture::releaseFrame() once you're done with it and before the next
// poll. This example never reads pixels through it directly.
struct VideoFrameToken {
    uint64_t opaque;
};

struct AudioFrameToken {
    uint64_t opaque;
};

// Abstract video capture interface. `record()` below is written purely
// against this — it never knows which concrete OS backend sits behind the
// reference (screen capture today; window capture, etc. later).
class VideoCapture {
public:
    virtual ~VideoCapture() = default;

    // Non-blocking: a new frame, or nullopt if nothing is ready yet. Throws
    // mediaway::Error on a hard capture failure.
    virtual std::optional<VideoFrameToken> pollFrame() = 0;

    // Gives a polled frame's underlying (possibly GPU-resident) memory back
    // to the OS. Call once per frame returned by pollFrame(), before polling
    // again.
    virtual void releaseFrame(const VideoFrameToken &frame) = 0;

    // Explicit close, in addition to the RAII destructor. Safe to call at
    // most once; the destructor is a no-op afterward.
    virtual void close() = 0;
};

// Abstract audio capture interface, mirroring VideoCapture minus frame
// release (audio frames here are plain buffers, not GPU-resident memory).
class AudioCapture {
public:
    virtual ~AudioCapture() = default;
    virtual std::optional<AudioFrameToken> pollFrame() = 0;
    virtual void close() = 0;
};

struct VideoCaptureConfig {
    uint32_t displayIndex; // 0 = primary display
    Rational frameRate;

    static VideoCaptureConfig screen(uint32_t displayIndex, Rational frameRate) {
        return VideoCaptureConfig{.displayIndex = displayIndex, .frameRate = frameRate};
    }
};

struct AudioCaptureConfig {
    Rational sampleRate;

    static AudioCaptureConfig microphone(Rational sampleRate) {
        return AudioCaptureConfig{.sampleRate = sampleRate};
    }
};

// RAII wrapper around an opened screen-capture backend.
class ScreenCapture final : public VideoCapture {
public:
    // Opens display `config.displayIndex` at `config.frameRate`. Throws
    // mediaway::Error (status MEDIAWAY_ERR_UNSUPPORTED_PLATFORM) when no
    // screen-capture backend is available here yet — callers should catch
    // that, report it, and exit cleanly rather than crash.
    static ScreenCapture open(const VideoCaptureConfig &config) {
        const mediaway_video_capture_config_t rawConfig{
            .display_index = config.displayIndex,
            .frame_rate = {.num = config.frameRate.num, .den = config.frameRate.den},
        };
        mediaway_video_capture_t *raw = nullptr;
        const mediaway_status_t status = mediaway_video_capture_open(&rawConfig, &raw);
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_video_capture_open");
        }
        return ScreenCapture(raw);
    }

    // The actual stream geometry the backend settled on — read after open(),
    // it is not an input.
    VideoGeometry geometry() const {
        mediaway_video_geometry_t raw{};
        const mediaway_status_t status = mediaway_video_capture_geometry(handle_.get(), &raw);
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_video_capture_geometry");
        }
        return VideoGeometry{.width = raw.width, .height = raw.height};
    }

    std::optional<VideoFrameToken> pollFrame() override {
        mediaway_video_frame_token_t raw{};
        bool hasFrame = false;
        const mediaway_status_t status =
            mediaway_video_capture_poll_frame(handle_.get(), &raw, &hasFrame);
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_video_capture_poll_frame");
        }
        if (!hasFrame) {
            return std::nullopt;
        }
        return VideoFrameToken{.opaque = raw.opaque};
    }

    void releaseFrame(const VideoFrameToken &frame) override {
        const mediaway_video_frame_token_t raw{.opaque = frame.opaque};
        const mediaway_status_t status = mediaway_video_capture_release_frame(handle_.get(), &raw);
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_video_capture_release_frame");
        }
    }

    void close() override {
        if (handle_) {
            // Take ownership away from unique_ptr first so its deleter does
            // not also try to close an already-closed handle.
            mediaway_video_capture_t *raw = handle_.release();
            const mediaway_status_t status = mediaway_video_capture_close(raw);
            if (status != MEDIAWAY_OK) {
                throw Error(status, "mediaway_video_capture_close");
            }
        }
    }

    ScreenCapture(ScreenCapture &&) noexcept = default;
    ScreenCapture &operator=(ScreenCapture &&) noexcept = default;
    ScreenCapture(const ScreenCapture &) = delete;
    ScreenCapture &operator=(const ScreenCapture &) = delete;

private:
    explicit ScreenCapture(mediaway_video_capture_t *handle) : handle_(handle) {}

    struct Deleter {
        void operator()(mediaway_video_capture_t *handle) const noexcept {
            if (handle != nullptr) {
                mediaway_video_capture_close(handle);
            }
        }
    };

    std::unique_ptr<mediaway_video_capture_t, Deleter> handle_;
};

// RAII wrapper around an opened microphone backend.
class Microphone final : public AudioCapture {
public:
    // Opens the microphone at `config.sampleRate`. Throws mediaway::Error
    // (status MEDIAWAY_ERR_UNSUPPORTED_PLATFORM) when no microphone backend
    // is available here yet — callers should keep running without audio.
    static Microphone open(const AudioCaptureConfig &config) {
        const mediaway_audio_capture_config_t rawConfig{
            .sample_rate = {.num = config.sampleRate.num, .den = config.sampleRate.den},
        };
        mediaway_audio_capture_t *raw = nullptr;
        const mediaway_status_t status = mediaway_audio_capture_open(&rawConfig, &raw);
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_audio_capture_open");
        }
        return Microphone(raw);
    }

    std::optional<AudioFrameToken> pollFrame() override {
        mediaway_audio_frame_token_t raw{};
        bool hasFrame = false;
        const mediaway_status_t status =
            mediaway_audio_capture_poll_frame(handle_.get(), &raw, &hasFrame);
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_audio_capture_poll_frame");
        }
        if (!hasFrame) {
            return std::nullopt;
        }
        return AudioFrameToken{.opaque = raw.opaque};
    }

    void close() override {
        if (handle_) {
            mediaway_audio_capture_t *raw = handle_.release();
            const mediaway_status_t status = mediaway_audio_capture_close(raw);
            if (status != MEDIAWAY_OK) {
                throw Error(status, "mediaway_audio_capture_close");
            }
        }
    }

    Microphone(Microphone &&) noexcept = default;
    Microphone &operator=(Microphone &&) noexcept = default;
    Microphone(const Microphone &) = delete;
    Microphone &operator=(const Microphone &) = delete;

private:
    explicit Microphone(mediaway_audio_capture_t *handle) : handle_(handle) {}

    struct Deleter {
        void operator()(mediaway_audio_capture_t *handle) const noexcept {
            if (handle != nullptr) {
                mediaway_audio_capture_close(handle);
            }
        }
    };

    std::unique_ptr<mediaway_audio_capture_t, Deleter> handle_;
};

// No-op stand-in used when the microphone is unavailable, so callers (and
// `record()` below) can keep working with a plain `AudioCapture&` instead of
// a nullable type.
class NullAudioCapture final : public AudioCapture {
public:
    std::optional<AudioFrameToken> pollFrame() override { return std::nullopt; }
    void close() override {}
};

} // namespace device

} // namespace mediaway

// ---------------------------------------------------------------------------
// The one small, reusable record loop. Typed purely against the abstract
// capture interfaces (+ the encode session) — it compiles and behaves
// identically no matter which concrete OS backend was opened by the caller.
// ---------------------------------------------------------------------------
void record(mediaway::device::VideoCapture &video, mediaway::device::AudioCapture &audio,
            mediaway::EncodeSession &session, uint32_t width, uint32_t height,
            std::chrono::steady_clock::duration duration) {
    const auto deadline = std::chrono::steady_clock::now() + duration;

    // Synthetic grey NV12 placeholder: width*height Y bytes (128) followed by
    // width*height/2 interleaved UV bytes (128). A real backend would convert
    // each captured frame's actual pixels here instead.
    const size_t nv12Len =
        static_cast<size_t>(width) * height + static_cast<size_t>(width) * height / 2;
    const std::vector<uint8_t> greyNv12(nv12Len, 128);

    int64_t pts = 0;
    while (std::chrono::steady_clock::now() < deadline) {
        // ── Video: poll, encode a placeholder frame, release back to the OS ──
        if (const std::optional<mediaway::device::VideoFrameToken> frame = video.pollFrame()) {
            session.writeFrame(mediaway::VideoFrame{
                .pts = pts++,
                .duration = 1,
                .width = width,
                .height = height,
                .pixelFormat = mediaway::PixelFormat::Nv12,
                .bytes = std::span<const uint8_t>(greyNv12),
            });
            // Give the (possibly GPU-resident) frame memory back to the OS
            // before the next poll.
            video.releaseFrame(*frame);
        }

        // ── Audio: drain whatever showed up; not wired into the encoder yet ──
        while (audio.pollFrame()) {
            // no-op: pushing this into a second (audio) track is a follow-up,
            // not part of this example.
        }
    }
}

int main() {
    using namespace mediaway;
    using namespace mediaway::device;

    constexpr uint32_t kFps = 30;
    constexpr uint32_t kSeconds = 3;

    // ── 1. Open platform capture backends ───────────────────────────────────
    std::optional<ScreenCapture> screen;
    try {
        screen.emplace(ScreenCapture::open(VideoCaptureConfig::screen(0, Rational{1, kFps})));
    } catch (const Error &e) {
        std::cerr << "screen_record: capture unavailable (" << e.what()
                   << ") -- platform not supported yet\n";
        return 0;
    }

    bool micAvailable = true;
    std::unique_ptr<AudioCapture> mic;
    try {
        mic = std::make_unique<Microphone>(
            Microphone::open(AudioCaptureConfig::microphone(Rational{1, 48'000})));
    } catch (const Error &e) {
        std::cerr << "screen_record: mic unavailable (" << e.what()
                   << ") -- continuing without audio\n";
        micAvailable = false;
        mic = std::make_unique<NullAudioCapture>();
    }

    const VideoGeometry geometry = screen->geometry();
    std::cout << "screen_record: " << geometry.width << "x" << geometry.height << " display"
               << (micAvailable ? ", mic ready" : "") << "\n";

    try {
        // ── 2. Config + open the auto encoder + encode session at the
        // capture's real resolution ─────────────────────────────────────────
        AutoEncodeConfig encCfg =
            AutoEncodeConfig::h264(geometry.width, geometry.height, Rational{1, kFps});
        encCfg.bitrateBps = 8'000'000;

        AutoEncoder encoder = AutoEncoder::open(encCfg);
        EncodeSession session = EncodeSession::open(std::move(encoder));

        std::cout << "screen_record: encoding at " << geometry.width << "x" << geometry.height
                   << " @" << kFps << "fps\n";

        // ── 3. Core pipeline: one small, backend-agnostic record loop ───────
        record(*screen, *mic, session, geometry.width, geometry.height,
               std::chrono::seconds(kSeconds));

        screen->close();
        mic->close();

        // ── 4. Finish encoding and write the resulting bytes to disk ────────
        const std::vector<uint8_t> mp4Bytes = session.finish();
        std::ofstream out("out_screen.mp4", std::ios::binary);
        out.write(reinterpret_cast<const char *>(mp4Bytes.data()),
                  static_cast<std::streamsize>(mp4Bytes.size()));
        out.close();

        std::cout << "screen_record: -> out_screen.mp4 (" << mp4Bytes.size() << " bytes)\n";
    } catch (const Error &e) {
        std::cerr << "screen_record: " << e.what() << "\n";
        return 1;
    }

    return 0;
}
