// stream_encode.cpp - pipeline capability: auto H.264 encode streamed to a file
// incrementally with EncodeSession::pollBytes().
//
// Status: real (adr/pipeline/0007-stream-bytes-aac-decode-support-probe.md §1).
//
// encode_to_mp4.cpp keeps the whole recording in RAM until finish(). Here the
// session is drained after every frame and each chunk is appended to the output
// file, so memory is bounded by the poll cadence instead of the recording's
// length. finish() then returns ONLY the bytes not yet polled (the tail), which
// is appended last - write every polled chunk, then the tail.
//
// Usage: stream_encode [out.mp4]   (default: stream_out.mp4)
// NoBackend (no encoder on this machine) is graceful, not a failure.
//
// Build:
//   g++ -std=c++17 -Ibindings/cpp/include -Icrates/mediaway-ffi/include
//       bindings/cpp/examples/pipeline/stream_encode.cpp
//       -L<dir with mediaway_ffi.dll> -lmediaway_ffi -o stream_encode.exe

#include <mediaway/mediaway.hpp>

#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <string>

namespace {

constexpr std::uint32_t kWidth = 640;
constexpr std::uint32_t kHeight = 480;
constexpr int kFrameCount = 150;  // 5 s at 30 fps

void append(std::ofstream& out, const mediaway::Bytes& bytes) {
  if (bytes.empty()) return;
  out.write(reinterpret_cast<const char*>(bytes.data()),
            static_cast<std::streamsize>(bytes.size()));
  if (!out) throw std::runtime_error("short write to the output file");
}

}  // namespace

int main(int argc, char** argv) {
  const std::string path = argc > 1 ? argv[1] : "stream_out.mp4";
  try {
    mediaway::encoder::AutoVideoEncoder encoder = mediaway::encoder::AutoVideoEncoder::open(
        {mediaway::Codec::H264, kWidth, kHeight, {1, 30}, mediaway::PixelFormat::Nv12});
    mediaway::encoder::EncodeSession session = std::move(encoder).begin();

    std::ofstream out(path, std::ios::binary);
    if (!out) throw std::runtime_error("cannot open " + path + " for writing");

    const mediaway::Bytes grey(static_cast<std::size_t>(kWidth) * kHeight * 3 / 2, 0x80);
    std::size_t streamed = 0;
    int chunks = 0;
    for (std::int64_t i = 0; i < kFrameCount; ++i) {
      // duration = 1 tick at {1,30}: frames of unknown duration get colliding
      // timestamps from the encoder and a player drops some of them.
      session.writeFrame({mediaway::PixelFormat::Nv12, kWidth, kHeight, i, grey}, 1);

      // Whatever fMP4 bytes are ready now - often none between fragments.
      const mediaway::Bytes chunk = session.pollBytes();
      if (!chunk.empty()) ++chunks;
      streamed += chunk.size();
      append(out, chunk);
    }

    // finish() consumes the session and returns only the unpolled tail.
    const mediaway::Bytes tail = std::move(session).finish();
    append(out, tail);
    out.close();

    std::cout << "streamed " << kFrameCount << " frame(s) into " << path << ": " << streamed
              << " bytes in " << chunks << " poll(s) + " << tail.size() << "-byte tail\n";
    return EXIT_SUCCESS;
  } catch (const mediaway::Error& e) {
    if (e.status() == mediaway::Status::NoBackend) {
      std::cout << "no encode backend on this machine (NoBackend) - exiting gracefully\n";
      return EXIT_SUCCESS;
    }
    std::cerr << "mediaway error: " << e.what() << "\n";
    return EXIT_FAILURE;
  } catch (const std::exception& e) {
    std::cerr << e.what() << "\n";
    return EXIT_FAILURE;
  }
}
