// encode_to_mp4.cpp - pipeline capability: auto video encode -> fragmented MP4.
//
// Status: real - the C ABI's auto-encode pipeline exists today (video only, no
// audio encoder) and runs underneath this example. Everything here calls the
// ideal C++ wrapper surface from bindings/cpp/README.md, which is itself still
// at design stage (nothing compiles yet), but the ABI it wraps is real.
//
// Demonstrates:
//   - AutoVideoEncoder::open: one call picks the best available OS/GPU H.264
//     encoder for the config; it throws mediaway::Error with Status::NoBackend
//     when no encoder exists (an expected outcome, not a hard failure).
//   - EncodeSession typestate: begin() transfers the encoder's ownership into
//     the session, finish() transfers it out again - the ABI's unconditional
//     handle consumption (open/finish consume their handle even on failure)
//     becomes unrepresentable in C++.
//   - 90 synthetic grey NV12 frames (0x80) at 640x480, 30 fps, encoded and
//     muxed by the pipeline; the finished fMP4 bytes are written to out.mp4 by
//     the caller (the pipeline never touches files).

#include <mediaway/mediaway.hpp>

#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

namespace {

// Grey NV12 frame: Y plane (w*h bytes) followed by interleaved UV plane
// (w*h/2 bytes). 0x80 in every byte is a flat mid-grey.
mediaway::Bytes makeGreyNv12(std::uint32_t width, std::uint32_t height) {
  const std::size_t ySize = static_cast<std::size_t>(width) * height;
  return mediaway::Bytes(ySize + ySize / 2, 0x80);
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
    constexpr std::uint32_t kWidth = 640;
    constexpr std::uint32_t kHeight = 480;
    constexpr int kFrameCount = 90;  // 3 seconds at 30 fps

    // One call: pick the best OS/GPU H.264 encoder for 640x480@30 NV12 input.
    mediaway::encoder::AutoVideoEncoder encoder =
        mediaway::encoder::AutoVideoEncoder::open({
            mediaway::Codec::H264,
            kWidth, kHeight,
            {1, 30},                      // 30 frames per second
            mediaway::PixelFormat::Nv12,  // synthetic input frames are NV12
        });

    // Ownership of the encoder transfers into the session; the session is a
    // single-use object (finish() is rvalue-only, so it cannot be reused).
    mediaway::encoder::EncodeSession session = std::move(encoder).begin();

    for (std::int64_t i = 0; i < kFrameCount; ++i) {
      session.writeFrame({
          mediaway::PixelFormat::Nv12,
          kWidth, kHeight,
          i,  // pts in timebase units ({1,30} -> the frame index)
          makeGreyNv12(kWidth, kHeight),
      });
    }

    // finish() consumes the session and returns the complete fMP4 bytes.
    mediaway::Bytes mp4 = std::move(session).finish();
    std::cout << "encoded " << kFrameCount << " grey NV12 frames -> "
              << mp4.size() << " bytes\n";
    if (!writeFile("out.mp4", mp4)) {
      std::cerr << "failed to write out.mp4\n";
      return EXIT_FAILURE;
    }
    std::cout << "wrote out.mp4\n";
    return EXIT_SUCCESS;
  } catch (const mediaway::Error& error) {
    std::cerr << "mediaway error: " << error.what() << " (status "
              << static_cast<int>(error.status()) << ")\n";
    return EXIT_FAILURE;
  }
}
