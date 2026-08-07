/*
 * device.hpp — camera / screen / microphone capture wrapper classes.
 *
 * Split out of the original single-file mediaway.hpp once wiring all 8
 * container formats pushed the combined header past the workspace's
 * 1000-line source-file cap.
 */

#ifndef MEDIAWAY_DEVICE_HPP
#define MEDIAWAY_DEVICE_HPP

#include <mediaway/core.hpp>
#include <mediaway/device.h>

#include <memory>
#include <optional>

namespace mediaway {

namespace detail {

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

namespace device {

/// One polled PCM chunk; data is raw interleaved F32 samples, and pts is the
/// first sample index in the stream timebase.
struct AudioFrame {
    std::int64_t pts;
    std::uint32_t sampleRate;
    std::uint16_t channels;
    Bytes data;
};

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

#endif  // MEDIAWAY_DEVICE_HPP
