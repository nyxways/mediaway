// capture_window.cpp - device capability: single-window capture quick start.
//
// Status: real (link+run verified on Windows). WindowCapture::open() drives a
// WGC session over an HWND (adr/0005-window-capture-c-abi.md). Like Screen it
// needs a real GpuDevice and delivers GPU-backed frames: pollFrame() proves
// frames arrive (real pts/geometry) but VideoFrame::data is empty; real pixels
// move through EncodeSession::writeFrameFromDesktopCapture.
//
// Usage: capture_window [hwnd-in-hex]   (default: the foreground window)
//
// The config asks for a hidden OS capture border and reads the outcome back
// with borderHidden() - a refusal is not an error at open. Set
// `config.region` to record a rectangle; one away from the window's origin
// costs a GPU copy per frame (not Zero-Copy).

#include <mediaway/mediaway.hpp>

#include <windows.h>  // GetForegroundWindow - this example is Windows-only

#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <thread>

int main(int argc, char** argv) {
  std::uint64_t hwnd = argc > 1 ? std::strtoull(argv[1], nullptr, 16)
                                : reinterpret_cast<std::uintptr_t>(GetForegroundWindow());
  if (hwnd == 0) {
    std::cout << "no window to capture (pass an HWND in hex)\n";
    return EXIT_SUCCESS;
  }
  try {
    mediaway::device::GpuDevice gpu = mediaway::device::GpuDevice::create(
        {std::nullopt, /*videoSupport=*/true, /*debugLayer=*/false});

    mediaway::device::WindowCaptureConfig config{hwnd, {1, 30}, gpu.handle()};
    config.border = mediaway::device::CaptureBorder::Hidden;
    config.dimensions = mediaway::device::FrameDimensions::EvenCropped;
    mediaway::device::WindowCapture window = mediaway::device::WindowCapture::open(config);

    const mediaway::device::CaptureInfo& info = window.info();
    std::cout << "Window geometry: " << info.width << 'x' << info.height
              << ", border hidden by the OS: " << (window.borderHidden() ? "yes" : "no")
              << '\n';

    std::size_t frameCount = 0;
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(3);
    while (std::chrono::steady_clock::now() < deadline && frameCount < 5) {
      if (std::optional<mediaway::VideoFrame> frame = window.pollFrame()) {
        std::cout << "  frame " << (frameCount + 1) << ": " << frame->width << 'x'
                  << frame->height << '\n';
        window.releaseFrame();
        ++frameCount;
      } else {
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
      }
    }
    std::cout << "captured " << frameCount << " real frame(s)\n";
    return EXIT_SUCCESS;
  } catch (const mediaway::Error& error) {
    if (error.status() == mediaway::Status::NoDevice ||
        error.status() == mediaway::Status::Unsupported) {
      std::cout << "Window capture unavailable on this machine: " << error.what() << '\n';
      return EXIT_SUCCESS;
    }
    std::cerr << "mediaway error: " << error.what() << " (status "
              << static_cast<int>(error.status()) << ")\n";
    return EXIT_FAILURE;
  }
}
