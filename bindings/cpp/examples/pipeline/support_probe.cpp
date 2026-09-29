// support_probe.cpp - pipeline capability: ask what this machine can encode / decode
// BEFORE opening a session (adr/pipeline/0007-stream-bytes-aac-decode-support-probe.md §3).
//
// Status: real.
//
// Both probes are COSTLY - each opens throwaway sessions (a real MFT / VA-API /
// VideoToolbox session per row). Call them when a settings screen opens, never per frame.
// Encoder support is resolution-dependent, so encoderSupport() takes the size you will
// encode; there is no resolution-free form.
//
// Usage: support_probe [width height]   (default: 1920 1080)
//
// Build:
//   g++ -std=c++17 -Ibindings/cpp/include -Icrates/mediaway-ffi/include
//       bindings/cpp/examples/pipeline/support_probe.cpp
//       -L<dir with mediaway_ffi.dll> -lmediaway_ffi -o support_probe.exe

#include <mediaway/mediaway.hpp>

#include <cstdlib>
#include <iostream>

namespace {

const char* name(mediaway::encoder::EncodeBackend b) {
  using B = mediaway::encoder::EncodeBackend;
  switch (b) {
    case B::Os: return "os";
    case B::Nvenc: return "nvenc";
    case B::QuickSync: return "quicksync";
    case B::Amf: return "amf";
    case B::Vulkan: return "vulkan";
    case B::Software: return "software";
    default: return "unknown";
  }
}

const char* name(mediaway::SupportState s) {
  switch (s) {
    case mediaway::SupportState::Supported: return "supported";
    case mediaway::SupportState::NotImplemented: return "not-implemented";
    case mediaway::SupportState::NoDevice: return "no-device";
    default: return "unknown";
  }
}

const char* name(mediaway::encoder::EncodePathClass p) {
  using P = mediaway::encoder::EncodePathClass;
  switch (p) {
    case P::None: return "-";
    case P::ZeroCopy: return "zero-copy";
    case P::GpuCopy: return "gpu-copy";
    case P::CpuUpload: return "cpu-upload";
    case P::Readback: return "readback";
    case P::Software: return "software";
    default: return "unknown";
  }
}

}  // namespace

int main(int argc, char** argv) {
  const std::uint32_t width = argc > 2 ? static_cast<std::uint32_t>(std::atoi(argv[1])) : 1920;
  const std::uint32_t height = argc > 2 ? static_cast<std::uint32_t>(std::atoi(argv[2])) : 1080;
  try {
    std::cout << "H.264 encode at " << width << "x" << height << ":\n";
    for (const auto& row : mediaway::encoder::encoderSupport(mediaway::Codec::H264, width, height)) {
      std::cout << "  " << name(row.backend) << ": " << name(row.state) << " (" << name(row.pathClass)
                << ")\n";
    }
    std::cout << "decode:\n";
    for (mediaway::Codec c : {mediaway::Codec::H264, mediaway::Codec::Hevc, mediaway::Codec::Opus,
                              mediaway::Codec::Aac}) {
      std::cout << "  codec " << static_cast<int>(c) << ": "
                << name(mediaway::decoder::decoderSupport(c)) << "\n";
    }
    return EXIT_SUCCESS;
  } catch (const mediaway::Error& e) {
    std::cerr << "mediaway error: " << e.what() << "\n";
    return EXIT_FAILURE;
  }
}
