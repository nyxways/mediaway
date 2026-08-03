// capture_microphone.cpp - device capability: microphone capture quick start.
//
// Status: real - the C ABI's microphone capture (raw interleaved f32le PCM,
// mediaway_audio_capture_*) exists today and runs underneath this example via
// the bindings/cpp/README.md wrapper surface (mediaway::device::AudioCapture).
// Mirrors examples/device/capture_microphone.rs: open the default mic, poll
// ~2 s of PCM frames, print the negotiated format, close. No encoding - there
// is no audio encoder in the ABI. No mic -> graceful exit.

#include <mediaway/mediaway.hpp>

#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <optional>
#include <thread>

int main() {
  try {
    mediaway::device::AudioCapture mic =
        mediaway::device::AudioCapture::open({0, 48000});

    std::size_t frames = 0;
    std::size_t totalBytes = 0;
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(2);
    while (std::chrono::steady_clock::now() < deadline) {
      if (std::optional<mediaway::device::AudioFrame> frame = mic.pollFrame()) {
        totalBytes += frame->data.size();
        frames++;
        if (frames == 1) {
          std::cout << "mic negotiated " << frame->sampleRate << " Hz, "
                    << frame->channels << " ch\n";
        }
      }
      std::this_thread::sleep_for(std::chrono::milliseconds(2));
    }

    std::cout << "captured " << frames << " PCM frame(s), " << totalBytes
              << " bytes in 2 s\n";
    mic.close();  // joins the backend worker thread (up to one period interval)
    return EXIT_SUCCESS;
  } catch (const mediaway::Error& error) {
    // NoBackend / NoDevice / Unsupported are expected outcomes in the ABI.
    if (error.status() == mediaway::Status::NoDevice ||
        error.status() == mediaway::Status::NoBackend) {
      std::cout << "no microphone on this machine; nothing to capture\n";
      return EXIT_SUCCESS;
    }
    std::cerr << "mediaway error: " << error.what() << " (status "
              << static_cast<int>(error.status()) << ")\n";
    return EXIT_FAILURE;
  }
}
