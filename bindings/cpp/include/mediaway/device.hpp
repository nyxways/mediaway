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
#include <string>
#include <vector>

namespace mediaway {

// Forward declaration for the capture-to-encode bridge friend grant below
// (adr/pipeline/0005-capture-encode-bridge-c-abi.md) — defined in
// pipeline.hpp, which includes this header, not the other way around.
namespace encoder { class EncodeSession; }

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

/// One enumerated DXGI adapter — mirrors mediaway_gpu_adapter_info_t.
/// Returned by GpuDevice::listAdapters().
struct GpuAdapter {
    std::uint32_t index;
    std::string name;
    std::uint32_t vendorId;
    std::uint32_t deviceId;
    std::uint64_t dedicatedVideoMemory;  // bytes
    bool isHardware;
};

struct GpuDeviceOptions {
    std::optional<std::uint32_t> adapterIndex;  // nullopt = backend default adapter
    bool videoSupport = false;  // required for GPU-input encode
    bool debugLayer = false;
};

/// A real GPU device (e.g. a DirectX11 ID3D11Device) created by the native
/// backend — closes the "no C++ caller can construct a GPU device" gap for
/// ScreenCapture and GPU-input encode (encoder::VideoEncoderConfig::gpuDevice),
/// both of which require a live device handle with no CPU fallback
/// (adr/0007-gpu-device-factory.md).
class GpuDevice {
public:
    /// Enumerate every DXGI adapter on this machine (name, VRAM, hardware-vs-software).
    static std::vector<GpuAdapter> listAdapters() {
        mediaway_gpu_adapter_info_t* adapters = nullptr;
        std::size_t count = 0;
        detail::checkDevice(mediaway_gpu_adapter_list(&adapters, &count));
        std::vector<GpuAdapter> result;
        try {
            result.reserve(count);
            for (std::size_t i = 0; i < count; ++i) {
                const mediaway_gpu_adapter_info_t& raw = adapters[i];
                result.push_back(GpuAdapter{
                    raw.index,
                    raw.name ? std::string(raw.name) : std::string(),
                    raw.vendor_id,
                    raw.device_id,
                    raw.dedicated_video_memory,
                    raw.is_hardware,
                });
            }
        } catch (...) {
            mediaway_gpu_adapter_list_free(adapters, count);
            throw;
        }
        mediaway_gpu_adapter_list_free(adapters, count);
        return result;
    }

    /// Create a real device from `options` (default or an explicit adapter index).
    static GpuDevice create(const GpuDeviceOptions& options = {}) {
        mediaway_gpu_device_options_t raw{};
        if (options.adapterIndex) {
            raw.adapter.kind = MEDIAWAY_GPU_ADAPTER_SELECT_INDEX;
            raw.adapter.index = *options.adapterIndex;
        } else {
            raw.adapter.kind = MEDIAWAY_GPU_ADAPTER_SELECT_DEFAULT;
            raw.adapter.index = 0;
        }
        raw.video_support = options.videoSupport;
        raw.debug_layer = options.debugLayer;
        mediaway_gpu_device_t* device = nullptr;
        detail::checkDevice(mediaway_gpu_device_create(&raw, &device));
        if (!device) {
            detail::throwError(Status::Panic, MEDIAWAY_DEVICE_STATUS_INTERNAL_PANIC,
                               "GPU device create returned no handle");
        }
        mediaway_gpu_device_handle_t handle{};
        const mediaway_device_status_t st = mediaway_gpu_device_handle(device, &handle);
        if (st != MEDIAWAY_DEVICE_STATUS_OK) {
            mediaway_gpu_device_close(device);
            detail::checkDevice(st);
        }
        return GpuDevice(device, handle);
    }

    ~GpuDevice() { close(); }
    GpuDevice(GpuDevice&&) = default;
    GpuDevice& operator=(GpuDevice&&) = default;
    GpuDevice(const GpuDevice&) = delete;
    GpuDevice& operator=(const GpuDevice&) = delete;

    /// The caller-facing handle — pass this into ScreenCaptureConfig::gpuDevice
    /// or encoder::VideoEncoderConfig::gpuDevice. Stays valid only while this
    /// GpuDevice has not been closed/destroyed.
    const mediaway_gpu_device_handle_t& handle() const noexcept { return handle_; }

    /// Releases the native device. Every handle obtained from it becomes invalid immediately.
    void close() noexcept {
        if (device_) {
            mediaway_gpu_device_close(device_.get());
            device_.release();  // already closed; release so the deleter cannot double-close
        }
    }

private:
    explicit GpuDevice(mediaway_gpu_device_t* device, mediaway_gpu_device_handle_t handle)
        : device_(device, &mediaway_gpu_device_close), handle_(handle) {}
    std::unique_ptr<mediaway_gpu_device_t, void (*)(mediaway_gpu_device_t*)> device_;
    mediaway_gpu_device_handle_t handle_;
};

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
    /// Mandatory — from GpuDevice::create().handle(). Screen has no CPU
    /// fallback; a MEDIAWAY_GPU_DEVICE_NONE handle is rejected as INVALID_INPUT.
    mediaway_gpu_device_handle_t gpuDevice;
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
    friend class encoder::EncodeSession;

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

    /// Raw handle access for EncodeSession::writeFrameFromCameraCapture's
    /// capture-to-encode bridge (adr/pipeline/0005) — the only sanctioned use
    /// of this class's opaque pointer outside its own methods.
    mediaway_camera_capture_t* rawHandle() const noexcept { return handle_.get(); }

    std::unique_ptr<mediaway_camera_capture_t, void (*)(mediaway_camera_capture_t*)> handle_;
    CaptureInfo info_;
};

/// A real, Zero-Copy Screen (DXGI Desktop Duplication) capture session.
/// Requires a live GpuDevice — see ScreenCaptureConfig::gpuDevice.
class ScreenCapture {
public:
    /// Open display `config.displayIndex`. Throws Error(Status::NoDevice) when
    /// no supported capture backend is compiled in here — catch it and
    /// degrade gracefully, same as VideoCapture::open.
    static ScreenCapture open(const ScreenCaptureConfig& config) {
        mediaway_desktop_capture_config_t raw = mediaway_desktop_capture_config_screen(
            config.displayIndex, {config.frameRate.num, config.frameRate.den}, config.gpuDevice);
        mediaway_desktop_capture_t* capture = nullptr;
        detail::checkDevice(mediaway_desktop_capture_open(&raw, &capture));
        if (!capture) {
            detail::throwError(Status::Panic, MEDIAWAY_DEVICE_STATUS_INTERNAL_PANIC,
                               "capture open returned no handle");
        }
        ScreenCapture session(capture, config.frameRate);
        session.queryGeometry();
        return session;
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
    /// For a GPU-storage frame (the real case — Screen has no CPU fallback),
    /// `data` stays empty: there is no CPU pixel readback path for GPU-backed
    /// frames in the wrapped Rust backend (mediaway_desktop_frame_free is a
    /// documented no-op for that case). This still proves a real frame arrived
    /// via width/height/pts; real pixels only ever move through
    /// EncodeSession::writeFrameFromDesktopCapture. Release with releaseFrame()
    /// before the next acquiring poll.
    std::optional<VideoFrame> pollFrame() {
        mediaway_desktop_frame_t raw{};
        bool has = false;
        detail::checkDevice(mediaway_desktop_capture_poll_frame(handle_.get(), &raw, &has));
        if (!has) return std::nullopt;
        Bytes data;
        if (raw.storage_kind == MEDIAWAY_VIDEO_FRAME_STORAGE_CPU && raw.data_len > 0) {
            data.assign(raw.data, raw.data + raw.data_len);
        }
        mediaway_desktop_frame_free(&raw);
        return VideoFrame{detail::fromAbiPixel(raw.pixel_format), raw.width, raw.height,
                          raw.pts, std::move(data)};
    }

    /// Release backend resources held by the last polled frame — the real
    /// release point for a GPU-backed frame's texture; call before the next
    /// frame-acquiring poll.
    void releaseFrame() {
        detail::checkDevice(mediaway_desktop_capture_release_frame(handle_.get()));
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
    friend class encoder::EncodeSession;

    explicit ScreenCapture(mediaway_desktop_capture_t* handle, Rational frameRate)
        : handle_(handle, &detail::desktopCaptureClose),
          info_{0, 0, frameRate, PixelFormat::Bgra8} {}

    void queryGeometry() {
        std::uint32_t width = 0;
        std::uint32_t height = 0;
        if (mediaway_desktop_capture_geometry(handle_.get(), &width, &height) ==
            MEDIAWAY_DEVICE_STATUS_OK) {
            info_.width = width;
            info_.height = height;
        }
    }

    /// Raw handle access for EncodeSession::writeFrameFromDesktopCapture's
    /// capture-to-encode bridge (adr/pipeline/0005) — the only sanctioned use
    /// of this class's opaque pointer outside its own methods.
    mediaway_desktop_capture_t* rawHandle() const noexcept { return handle_.get(); }

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
