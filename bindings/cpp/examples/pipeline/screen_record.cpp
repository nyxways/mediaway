// screen_record.cpp - pipeline + device capability: screen + mic -> encode ->
// MP4.
//
// Status: ASPIRATIONAL - the C ABI returns UNSUPPORTED for screen capture
// today: screen needs a live GPU device handle (e.g. ID3D11Device*) with no CPU
// fallback and no C representation yet (crates/mediaway-device-ffi/adr/0001,
// "Deferred"). Nothing in this file runs against the current ABI; it specifies
// the DX the wrapper should provide once the ABI lands. The encode -> fMP4 half
// of the stack is already real.
//
// Flow (ideal):
//   1. Open display 0 at 30 fps (BGRA8 CPU frames).
//   2. Open the microphone at 48 kHz (drained - no audio encoder in the ABI).
//   3. Encode at the negotiated geometry with the auto H.264 encoder.
//   4. Record ~3 s: poll screen frames into the encode session, drain mic.
//   5. finish() -> MP4 bytes written to screen_out.mp4 by the caller.
//
// Today, ScreenCapture::open would throw mediaway::Error with
// Status::Unsupported; this example is the acquisition loop the wrapper should
// support once screen capture is represented in the C ABI.

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
    // ---- Screen: display 0, 30 fps; 0x0 geometry = native resolution --------
    // Screen capture is not representable from the C ABI today (it needs a
    // live GPU device handle with no CPU fallback — see bindings/c/README.md's
    // truth table), so open() throws Status::Unsupported. The acquisition loop
    // below is the DX the wrapper should provide once the ABI lands.
    mediaway::device::ScreenCapture screen;
    try {
      screen = mediaway::device::ScreenCapture::open({0, {1, 30}});
    } catch (const mediaway::Error& error) {
      if (error.status() != mediaway::Status::Unsupported) throw;
      std::cout << "Screen capture is NOT available from this binding today:\n"
                << "  it needs a live GPU device handle (ID3D11Device*) with no\n"
                << "  CPU fallback, and its C representation is deferred.\n"
                << "  Exiting gracefully — nothing to encode yet.\n";
      return EXIT_SUCCESS;
    }
    const mediaway::device::CaptureInfo& info = screen.info();
    std::cout << "screen negotiated " << info.width << 'x' << info.height
              << " @ " << info.frameRate.num << '/' << info.frameRate.den
              << '\n';

    // ---- Microphone: 48 kHz; drain-only (no audio encoder in the ABI) --------
    std::optional<mediaway::device::AudioCapture> mic;
    try {
      mic.emplace(mediaway::device::AudioCapture::open({0, 48000}));
    } catch (const mediaway::Error& error) {
      if (!isExpectedUnavailable(error)) throw;
      std::cerr << "microphone unavailable: " << error.what()
                << " - screen only\n";
    }

    // ---- Encode at the negotiated geometry -----------------------------------
    mediaway::encoder::AutoVideoEncoder encoder =
        mediaway::encoder::AutoVideoEncoder::open({
            mediaway::Codec::H264,
            info.width, info.height,
            info.frameRate,
            info.format,  // the screen capture delivers BGRA8 CPU frames
        });
    mediaway::encoder::EncodeSession session = std::move(encoder).begin();

    // ---- Record ~3 s ----------------------------------------------------------
    const auto deadline =
        std::chrono::steady_clock::now() + std::chrono::seconds(3);
    std::size_t screenFrames = 0;
    std::size_t micFrames = 0;
    while (std::chrono::steady_clock::now() < deadline) {
      if (std::optional<mediaway::VideoFrame> frame = screen.pollFrame()) {
        session.writeFrame(*frame);
        ++screenFrames;
      }
      if (mic) {
        // Audio note: the screen path is blocked before audio matters — Screen
        // capture needs a live GPU device handle from C
        // (mediaway-device-ffi/adr/0001, § Deferred), so mic PCM is drained.
        while (std::optional<mediaway::device::AudioFrame> audio =
                   mic->pollFrame()) {
          ++micFrames;
        }
      }
      std::this_thread::sleep_for(std::chrono::milliseconds(2));
    }
    screen.close();
    if (mic) mic->close();

    mediaway::Bytes mp4 = std::move(session).finish();
    std::cout << "recorded " << screenFrames << " screen frames (+ " << micFrames
              << " mic frames drained, not muxed) -> " << mp4.size()
              << " bytes\n";
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
