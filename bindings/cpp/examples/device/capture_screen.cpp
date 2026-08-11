// capture_screen.cpp - device capability: screen capture quick start.
//
// Status: real. GpuDevice::create() (adr/0007-gpu-device-factory.md) builds a
// real ID3D11Device, closing the "no C++ caller can construct a GPU device"
// gap; ScreenCapture::open() uses it to drive real Zero-Copy Screen capture
// (DXGI Desktop Duplication). There is no CPU pixel readback path for
// GPU-backed frames in the wrapped Rust backend — pollFrame() proves frames
// are genuinely arriving (real pts/geometry) but VideoFrame::data is always
// empty; real pixels only ever move through the capture-to-encode bridge (see
// pipeline/screen_record.cpp).

#include <mediaway/mediaway.hpp>

#include <chrono>
#include <cstdlib>
#include <iostream>
#include <thread>

int main() {
  try {
    mediaway::device::GpuDevice gpu = mediaway::device::GpuDevice::create(
        {std::nullopt, /*videoSupport=*/true, /*debugLayer=*/false});

    mediaway::device::ScreenCapture screen =
        mediaway::device::ScreenCapture::open({0, {1, 30}, gpu.handle()});
    const mediaway::device::CaptureInfo& info = screen.info();
    std::cout << "Screen geometry: " << info.width << 'x' << info.height << '\n';

    std::size_t frameCount = 0;
    const auto deadline =
        std::chrono::steady_clock::now() + std::chrono::seconds(3);
    while (std::chrono::steady_clock::now() < deadline && frameCount < 5) {
      if (std::optional<mediaway::VideoFrame> frame = screen.pollFrame()) {
        std::cout << "  frame " << (frameCount + 1) << ": " << frame->width
                  << 'x' << frame->height << '\n';
        screen.releaseFrame();
        ++frameCount;
      } else {
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
      }
    }
    std::cout << "captured " << frameCount << " real frame(s)\n";
    return EXIT_SUCCESS;
  } catch (const mediaway::Error& error) {
    if (error.status() == mediaway::Status::NoDevice) {
      std::cout << "Screen capture unavailable on this machine: " << error.what()
                << '\n';
      return EXIT_SUCCESS;
    }
    std::cerr << "mediaway error: " << error.what() << " (status "
              << static_cast<int>(error.status()) << ")\n";
    return EXIT_FAILURE;
  }
}
