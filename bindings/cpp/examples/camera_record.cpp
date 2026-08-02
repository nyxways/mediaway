// camera_record.cpp — Mediaway camera + mic capture -> encode -> fragmented MP4.
//
// ASPIRATIONAL EXAMPLE: no `mediaway-device-ffi` crate exists yet and no
// <mediaway/device.h> header ships today (see docs/spec/c-ffi.md and
// bindings/README.md). This file shows the target ergonomics a future thin
// C++ RAII wrapper covering camera + mic capture and recording should aim
// for. It mirrors bindings/c/screen_record.c and examples/screen_record.rs,
// with a camera source in place of a screen source.
//
// The camera recorder is built from the SAME building blocks as the
// encode-only quick start (bindings/cpp/encode_to_mp4.cpp): a config -> open
// auto encoder -> open encode session -> writeFrame -> finish flow. The only
// new piece is a device-capture layer, glued to that flow by one small,
// backend-agnostic `record()` function below — the exact same function
// would work unchanged for screen capture (see bindings/c/screen_record.c),
// since it only ever sees the `VideoCapture` / `AudioCapture` abstract base
// classes, never a concrete camera/screen/OS type.
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

typedef struct mediaway_video_capture mediaway_video_capture_t;
typedef struct mediaway_audio_capture mediaway_audio_capture_t;
typedef struct mediaway_auto_encoder mediaway_auto_encoder_t;
typedef struct mediaway_encode_session mediaway_encode_session_t;

typedef enum mediaway_status {
    MEDIAWAY_OK = 0,
    MEDIAWAY_ERR_DEVICE_UNAVAILABLE = 1,
    MEDIAWAY_ERR_UNSUPPORTED_PLATFORM = 2,
    MEDIAWAY_ERR_INVALID_ARGUMENT = 3,
    MEDIAWAY_ERR_INTERNAL = 4,
} mediaway_status_t;

// Non-blocking poll outcome for both video and audio capture.
typedef enum mediaway_poll_status {
    MEDIAWAY_POLL_FRAME_READY = 0,
    MEDIAWAY_POLL_NO_FRAME = 1,
    MEDIAWAY_POLL_ERROR = 2,
} mediaway_poll_status_t;

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

typedef struct mediaway_video_capture_config {
    uint32_t device_index;
    mediaway_rational_t frame_rate;
} mediaway_video_capture_config_t;

typedef struct mediaway_audio_capture_config {
    mediaway_rational_t sample_rate;
} mediaway_audio_capture_config_t;

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

// Opens camera `device_index` (0 = default/first camera) at the requested
// frame rate. Fallible: the specific device may not be available (in use,
// unplugged, permission denied, ...) — MEDIAWAY_ERR_DEVICE_UNAVAILABLE is an
// expected, recoverable outcome, not a bug.
mediaway_status_t mediaway_video_capture_open(const mediaway_video_capture_config_t *config,
                                               mediaway_video_capture_t **out_capture);
// Reports the stream geometry the backend actually negotiated with the
// device (may differ from any resolution hint the caller passed).
mediaway_status_t mediaway_video_capture_geometry(mediaway_video_capture_t *capture,
                                                   uint32_t *out_width, uint32_t *out_height);
mediaway_poll_status_t mediaway_video_capture_poll_frame(mediaway_video_capture_t *capture);
// Releases the most recently polled frame back to the OS. Video frames may
// reference GPU-resident memory that the backend cannot reuse until this is
// called.
mediaway_status_t mediaway_video_capture_release_frame(mediaway_video_capture_t *capture);
mediaway_status_t mediaway_video_capture_close(mediaway_video_capture_t *capture);

// Opens the default microphone at the requested sample rate. Fallible;
// unlike the camera, a caller may reasonably choose to keep recording
// video-only when this fails.
mediaway_status_t mediaway_audio_capture_open(const mediaway_audio_capture_config_t *config,
                                               mediaway_audio_capture_t **out_capture);
mediaway_poll_status_t mediaway_audio_capture_poll_frame(mediaway_audio_capture_t *capture);
mediaway_status_t mediaway_audio_capture_close(mediaway_audio_capture_t *capture);

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

// Marker for "a frame is ready" as returned by pollFrame() below. A real
// binding would carry the actual payload here (a GPU buffer handle for
// video, PCM samples for audio); this example never touches it, since
// record() writes a synthetic placeholder frame instead of real captured
// pixels (see record() below).
struct Frame {
    int64_t pts;
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

struct VideoCaptureDeleter {
    void operator()(mediaway_video_capture_t *handle) const noexcept {
        if (handle != nullptr) {
            mediaway_video_capture_close(handle);
        }
    }
};

struct AudioCaptureDeleter {
    void operator()(mediaway_audio_capture_t *handle) const noexcept {
        if (handle != nullptr) {
            mediaway_audio_capture_close(handle);
        }
    }
};

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

// ---------------------------------------------------------------------------
// Capture contracts. `record()` further below only ever sees these two
// abstract base classes — it does not know or care which concrete OS
// backend (camera, screen, window, ...) is underneath.
// ---------------------------------------------------------------------------

// Abstract video capture source (camera, screen, window, ...).
class VideoCapture {
public:
    virtual ~VideoCapture() = default;

    // Actual stream geometry the backend settled on, once opened.
    virtual uint32_t width() const noexcept = 0;
    virtual uint32_t height() const noexcept = 0;

    // Non-blocking poll: a new frame, or nullopt if nothing is ready yet.
    // Throws mediaway::Error on a hard capture error.
    virtual std::optional<Frame> pollFrame() = 0;

    // Releases the most recently polled frame back to the OS.
    virtual void releaseFrame() = 0;

    virtual void close() = 0;
};

// Abstract audio capture source (microphone, loopback, ...).
class AudioCapture {
public:
    virtual ~AudioCapture() = default;

    // Non-blocking poll: a new frame, or nullopt if nothing is ready yet.
    // Throws mediaway::Error on a hard capture error.
    virtual std::optional<Frame> pollFrame() = 0;

    virtual void close() = 0;
};

// RAII wrapper around an opened camera device. Move-only.
class CameraCapture : public VideoCapture {
public:
    // Opens camera `deviceIndex` (0 = default/first camera) at `frameRate`.
    // Throws mediaway::Error (status MEDIAWAY_ERR_DEVICE_UNAVAILABLE) when
    // this specific device is not available — callers should catch that and
    // report it, not treat it as a crash-worthy bug.
    static CameraCapture open(uint32_t deviceIndex, Rational frameRate) {
        const mediaway_video_capture_config_t rawConfig{
            .device_index = deviceIndex,
            .frame_rate = {.num = frameRate.num, .den = frameRate.den},
        };
        mediaway_video_capture_t *raw = nullptr;
        const mediaway_status_t status = mediaway_video_capture_open(&rawConfig, &raw);
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_video_capture_open");
        }

        // Query the geometry the backend actually negotiated with the
        // device — do not assume a resolution.
        uint32_t width = 0;
        uint32_t height = 0;
        const mediaway_status_t geometryStatus = mediaway_video_capture_geometry(raw, &width, &height);
        if (geometryStatus != MEDIAWAY_OK) {
            mediaway_video_capture_close(raw);
            throw Error(geometryStatus, "mediaway_video_capture_geometry");
        }

        return CameraCapture(raw, width, height);
    }

    CameraCapture(CameraCapture &&) noexcept = default;
    CameraCapture &operator=(CameraCapture &&) noexcept = default;
    CameraCapture(const CameraCapture &) = delete;
    CameraCapture &operator=(const CameraCapture &) = delete;

    uint32_t width() const noexcept override { return width_; }
    uint32_t height() const noexcept override { return height_; }

    std::optional<Frame> pollFrame() override {
        switch (mediaway_video_capture_poll_frame(handle_.get())) {
            case MEDIAWAY_POLL_FRAME_READY:
                return Frame{.pts = nextPts_++};
            case MEDIAWAY_POLL_NO_FRAME:
                return std::nullopt;
            case MEDIAWAY_POLL_ERROR:
            default:
                throw Error(MEDIAWAY_ERR_INTERNAL, "mediaway_video_capture_poll_frame");
        }
    }

    void releaseFrame() override {
        const mediaway_status_t status = mediaway_video_capture_release_frame(handle_.get());
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_video_capture_release_frame");
        }
    }

    void close() override {
        const mediaway_status_t status = mediaway_video_capture_close(handle_.release());
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_video_capture_close");
        }
    }

private:
    CameraCapture(mediaway_video_capture_t *handle, uint32_t width, uint32_t height)
        : handle_(handle), width_(width), height_(height) {}

    std::unique_ptr<mediaway_video_capture_t, detail::VideoCaptureDeleter> handle_;
    uint32_t width_ = 0;
    uint32_t height_ = 0;
    int64_t nextPts_ = 0;
};

// RAII wrapper around an opened microphone. Move-only.
class Microphone : public AudioCapture {
public:
    // Throws mediaway::Error (status MEDIAWAY_ERR_DEVICE_UNAVAILABLE) when no
    // microphone is available — callers should treat that as a recoverable
    // "record video-only" outcome, not a crash-worthy bug.
    static Microphone open(Rational sampleRate) {
        const mediaway_audio_capture_config_t rawConfig{
            .sample_rate = {.num = sampleRate.num, .den = sampleRate.den},
        };
        mediaway_audio_capture_t *raw = nullptr;
        const mediaway_status_t status = mediaway_audio_capture_open(&rawConfig, &raw);
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_audio_capture_open");
        }
        return Microphone(raw);
    }

    Microphone(Microphone &&) noexcept = default;
    Microphone &operator=(Microphone &&) noexcept = default;
    Microphone(const Microphone &) = delete;
    Microphone &operator=(const Microphone &) = delete;

    std::optional<Frame> pollFrame() override {
        switch (mediaway_audio_capture_poll_frame(handle_.get())) {
            case MEDIAWAY_POLL_FRAME_READY:
                return Frame{.pts = nextPts_++};
            case MEDIAWAY_POLL_NO_FRAME:
                return std::nullopt;
            case MEDIAWAY_POLL_ERROR:
            default:
                throw Error(MEDIAWAY_ERR_INTERNAL, "mediaway_audio_capture_poll_frame");
        }
    }

    void close() override {
        const mediaway_status_t status = mediaway_audio_capture_close(handle_.release());
        if (status != MEDIAWAY_OK) {
            throw Error(status, "mediaway_audio_capture_close");
        }
    }

private:
    explicit Microphone(mediaway_audio_capture_t *handle) : handle_(handle) {}

    std::unique_ptr<mediaway_audio_capture_t, detail::AudioCaptureDeleter> handle_;
    int64_t nextPts_ = 0;
};

// Null Object standing in for a microphone that failed to open. Lets callers
// keep passing a plain `AudioCapture&` into record() either way — the "no
// mic available" branch is handled once, in main(), instead of forking
// record()'s signature into an optional/pointer.
class NullAudioCapture : public AudioCapture {
public:
    std::optional<Frame> pollFrame() override { return std::nullopt; }
    void close() override {}
};

// RAII wrapper around an opened encoder backend. Move-only: opening an
// EncodeSession consumes it.
class AutoEncoder {
public:
    // Tries the best available encoder backend for this platform/GPU.
    // Throws mediaway::Error (status MEDIAWAY_ERR_UNSUPPORTED_PLATFORM) when
    // none is available here.
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

// ---------------------------------------------------------------------------
// record() — the reusable pipeline glue.
//
// Typed purely against the VideoCapture / AudioCapture abstract base
// classes: it has no idea whether `video` is a camera, a screen, or a
// window, and no idea whether `audio` is a real microphone or the
// NullAudioCapture stand-in. The exact same function drives every source
// combination — swap CameraCapture for a ScreenCapture backend and this
// function does not change.
// ---------------------------------------------------------------------------
void record(VideoCapture &video, AudioCapture &audio, EncodeSession &session,
            std::chrono::seconds duration) {
    const uint32_t width = video.width();
    const uint32_t height = video.height();

    // Synthetic solid-grey NV12 placeholder: width*height Y bytes (128)
    // followed by width*height/2 interleaved UV bytes (128). Stands in for a
    // real captured-frame -> NV12 conversion, which this quick-start example
    // deliberately leaves out. Reused for every frame.
    const size_t nv12Len =
        static_cast<size_t>(width) * height + static_cast<size_t>(width) * height / 2;
    const std::vector<uint8_t> greyNv12(nv12Len, 128);

    const auto deadline = std::chrono::steady_clock::now() + duration;
    int64_t pts = 0;

    while (std::chrono::steady_clock::now() < deadline) {
        // ── Video: poll, write a frame on arrival, then release it back to
        // the OS (video frames may reference GPU-resident memory that the
        // capture backend needs returned before it can reuse the underlying
        // surface). ──────────────────────────────────────────────────────
        if (video.pollFrame()) {
            // A real backend would convert the polled frame to NV12 here;
            // this example writes the placeholder buffer instead.
            session.writeFrame(VideoFrame{
                .pts = pts++,
                .duration = 1,
                .width = width,
                .height = height,
                .pixelFormat = PixelFormat::Nv12,
                .bytes = std::span<const uint8_t>(greyNv12),
            });
            video.releaseFrame();
        }

        // ── Audio: drain whatever is pending. Not wired into the encode
        // session yet (no audio track/encoder in this example) — just keep
        // the capture queue from backing up. ────────────────────────────
        while (audio.pollFrame()) {
            // TODO(#issue): push to an audio encoder / second track.
        }
    }
}

} // namespace mediaway

int main() {
    using namespace mediaway;

    constexpr uint32_t kFps = 30;
    constexpr Rational kVideoTimeBase{1, kFps};
    constexpr Rational kAudioTimeBase{1, 48'000};

    // ── 1. Open the camera. Opening is fallible — this specific device may
    // not be available; handle that gracefully instead of crashing. Device
    // index 0 = default/first camera. ───────────────────────────────────────
    std::optional<CameraCapture> camera;
    try {
        camera.emplace(CameraCapture::open(0, kVideoTimeBase));
    } catch (const Error &e) {
        std::cerr << "camera_record: camera unavailable (" << e.what() << ") — nothing to record\n";
        return 0;
    }

    // ── 2. Open the microphone. Also fallible; unlike the camera, a missing
    // mic should not stop recording — continue video-only. ─────────────────
    std::unique_ptr<AudioCapture> mic;
    bool micReady = true;
    try {
        mic = std::make_unique<Microphone>(Microphone::open(kAudioTimeBase));
    } catch (const Error &e) {
        std::cerr << "camera_record: microphone unavailable (" << e.what()
                   << ") — continuing without audio\n";
        mic = std::make_unique<NullAudioCapture>();
        micReady = false;
    }

    // ── 3. The camera exposes the stream geometry it actually negotiated —
    // do not assume a resolution. ───────────────────────────────────────────
    const uint32_t width = camera->width();
    const uint32_t height = camera->height();
    std::cout << "camera_record: " << width << "x" << height << " camera, mic "
              << (micReady ? "ready" : "unavailable") << "\n";

    // ── 4. Config: H.264 at the capture's real resolution and frame rate, 4
    // Mbps (the default) — same shape as the encode-only quick start
    // (bindings/cpp/encode_to_mp4.cpp). ─────────────────────────────────────
    const AutoEncodeConfig encodeConfig = AutoEncodeConfig::h264(width, height, kVideoTimeBase);

    std::optional<EncodeSession> session;
    try {
        AutoEncoder encoder = AutoEncoder::open(encodeConfig);
        session.emplace(EncodeSession::open(std::move(encoder)));
    } catch (const Error &e) {
        std::cerr << "camera_record: no auto encoder available on this platform (" << e.what()
                   << ")\n";
        camera->close();
        mic->close();
        return 0;
    }

    // ── 5. Record: one small, reusable function that only ever sees the
    // VideoCapture / AudioCapture abstract base classes — no OS-specific or
    // camera-specific code below this line. ─────────────────────────────────
    record(*camera, *mic, *session, std::chrono::seconds(3));

    // ── 6. Close capture handles once recording is done. ───────────────────
    camera->close();
    mic->close();

    // ── 7. Flush the encoder, finalize the muxer, get the complete MP4
    // file. ──────────────────────────────────────────────────────────────────
    const std::vector<uint8_t> mp4Bytes = session->finish();

    std::ofstream out("out_camera.mp4", std::ios::binary);
    out.write(reinterpret_cast<const char *>(mp4Bytes.data()),
              static_cast<std::streamsize>(mp4Bytes.size()));
    out.close();

    std::cout << "camera_record: -> out_camera.mp4 (" << mp4Bytes.size() << " bytes)\n";
    return 0;
}
