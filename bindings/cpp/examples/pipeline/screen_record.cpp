// screen_record.cpp - pipeline + device capability: screen + mic -> encode ->
// MP4.
//
// Status: real. GpuDevice::create() (adr/0007-gpu-device-factory.md) drives
// real Zero-Copy Screen capture, and EncodeSession::writeFrameFromDesktopCapture
// (adr/pipeline/0005-capture-encode-bridge-c-abi.md) polls + pushes each frame
// in one native call — no intermediate VideoFrame, Zero-Copy for Screen's
// GPU-backed frames. AutoVideoEncoder::open's VideoEncoderConfig::gpuDevice +
// inputFormat = PixelFormat::Bgra8 negotiate the GPU-input-capable path before
// the bridge is ever called (DXGI delivers BGRA8, not the NV12 default).
//
// On a machine whose encoder backend accepts a GPU-configured open but
// rejects the GPU-input path once frames actually start flowing (a real,
// pre-existing WMF/DX11 limitation — see the Rust gpu_write_frame_smoke.rs
// test and the C/Node.js/C#/Python screen_record siblings), this gracefully
// skips instead of crashing.
//
// Flow:
//   1. Create a GpuDevice; open display 0 at 30 fps with it.
//   2. Open the microphone at 48 kHz (drained, not muxed — same as
//      ScreenRecord.cs; camera_record.cpp's two-track remux is a separate
//      flow this file doesn't duplicate).
//   3. Open the auto H.264 encoder at the negotiated geometry, sharing the
//      same GpuDevice.
//   4. Record ~3 s: bridge screen frames straight into the encode session,
//      drain mic.
//   5. finish() -> MP4 bytes written to screen_out.mp4 by the caller.

#include <mediaway/mediaway.hpp>

#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <optional>
#include <string>
#include <thread>

namespace {

// NO_BACKEND / UNSUPPORTED / NO_DEVICE are expected outcomes in the ABI, not
// hard failures: the caller degrades gracefully (screen only, or skip).
bool isExpectedUnavailable(const mediaway::Error& error) {
  return error.status() == mediaway::Status::NoBackend ||
         error.status() == mediaway::Status::Unsupported ||
         error.status() == mediaway::Status::NoDevice;
}

bool writeFile(const std::string& path, const mediaway::Bytes& bytes) {
  std::ofstream out(path, std::ios::binary);
  if (!out) return false;
  out.write(reinterpret_cast<const char*>(bytes.data()),
            static_cast<std::streamsize>(bytes.size()));
  return static_cast<bool>(out);
}

}  // namespace

int main() {
  try {
    // ---- GPU device + Screen: display 0, 30 fps ------------------------------
    mediaway::device::GpuDevice gpu = mediaway::device::GpuDevice::create(
        {std::nullopt, /*videoSupport=*/true, /*debugLayer=*/false});

    mediaway::device::ScreenCapture screen;
    try {
      screen = mediaway::device::ScreenCapture::open({0, {1, 30}, gpu.handle()});
    } catch (const mediaway::Error& error) {
      if (!isExpectedUnavailable(error)) throw;
      std::cout << "screen capture unavailable: " << error.what()
                << " - exiting.\n";
      return EXIT_SUCCESS;
    }
    const mediaway::device::CaptureInfo& info = screen.info();
    std::cout << "screen geometry: " << info.width << 'x' << info.height
              << " @ " << info.frameRate.num << '/' << info.frameRate.den
              << '\n';

    // ---- Microphone: 48 kHz; drain-only (not wired into this MP4's tracks) ---
    std::optional<mediaway::device::AudioCapture> mic;
    try {
      mic.emplace(mediaway::device::AudioCapture::open({0, 48000}));
    } catch (const mediaway::Error& error) {
      if (!isExpectedUnavailable(error)) throw;
      std::cerr << "microphone unavailable: " << error.what()
                << " - screen only\n";
    }

    // ---- Encode at the negotiated geometry, sharing the same GPU device -----
    std::optional<mediaway::encoder::AutoVideoEncoder> encoder;
    try {
      encoder.emplace(mediaway::encoder::AutoVideoEncoder::open({
          mediaway::Codec::H264,
          info.width, info.height,
          info.frameRate,
          mediaway::PixelFormat::Bgra8,  // Screen delivers BGRA8, not NV12
          gpu.handle(),
      }));
    } catch (const mediaway::Error& error) {
      if (!isExpectedUnavailable(error)) throw;
      std::cout << "no GPU-input encoder available for " << info.width << 'x'
                << info.height << ": " << error.what() << " - exiting.\n";
      return EXIT_SUCCESS;
    }
    mediaway::encoder::EncodeSession session = std::move(*encoder).begin();

    // ---- Record ~3 s, bridging screen frames straight into the encoder ------
    const auto deadline =
        std::chrono::steady_clock::now() + std::chrono::seconds(3);
    std::size_t framesWritten = 0;
    std::size_t micFrames = 0;
    try {
      while (std::chrono::steady_clock::now() < deadline) {
        if (session.writeFrameFromDesktopCapture(screen)) {
          ++framesWritten;
        } else {
          std::this_thread::sleep_for(std::chrono::milliseconds(4));
        }
        if (mic) {
          while (std::optional<mediaway::device::AudioFrame> audio =
                     mic->pollFrame()) {
            ++micFrames;
          }
        }
      }
    } catch (const mediaway::Error& error) {
      if (!isExpectedUnavailable(error)) throw;
      // Same known dev-machine limitation as camera/encode_to_mp4's own
      // encoder — the WMF/DX11 backend accepts a GpuDevice-configured open
      // but rejects the GPU-input encode path once frames actually start
      // flowing (adr/pipeline/0002-gpu-frame-input-c-abi.md).
      std::cout << "GPU-input encode unsupported on this backend ("
                << error.what() << ") - exiting.\n";
      return EXIT_SUCCESS;
    }
    if (mic) mic->close();
    screen.close();

    if (framesWritten == 0) {
      std::cout << "screen capture opened but delivered no frames within the "
                   "deadline - exiting.\n";
      return EXIT_SUCCESS;
    }

    mediaway::Bytes mp4 = std::move(session).finish();
    std::cout << "bridged " << framesWritten << " real screen frame(s) (+ "
              << micFrames << " mic frames drained, not muxed) -> "
              << mp4.size() << " bytes\n";
    if (!writeFile("screen_out.mp4", mp4)) {
      std::cerr << "failed to write screen_out.mp4\n";
      return EXIT_FAILURE;
    }
    std::cout << "wrote screen_out.mp4\n";
    return EXIT_SUCCESS;
  } catch (const mediaway::Error& error) {
    std::cerr << "mediaway error: " << error.what() << " (status "
              << static_cast<int>(error.status()) << ")\n";
    return EXIT_FAILURE;
  }
}
