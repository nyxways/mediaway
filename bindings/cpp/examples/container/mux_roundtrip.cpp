// mux_roundtrip.cpp - container capability: mux + demux roundtrip.
//
// Status: real - the C ABI's container capability (sans-io fragmented-MP4 mux
// and demux) exists today and runs underneath this example. Everything here
// calls the ideal C++ wrapper surface from bindings/cpp/README.md, which is
// itself still at design stage (nothing compiles yet), but the ABI it wraps is
// real.
//
// Demonstrates:
//   - Muxer (Open state): addVideoTrack / addAudioTrack, then begin() moves
//     the Open state into a LiveMuxer - typestate via move, so adding a track
//     after begin() is a compile error instead of the ABI's INVALID_STATE.
//   - LiveMuxer: pushPacket, flush, pollBytes (the caller owns byte I/O; the
//     muxer never touches files).
//   - Demuxer: pushBytes, streams(), pollPacket - recovering the same packets.
//
// Flow: 90 synthetic H.264 video packets (timebase 1/30) + 90 synthetic AAC
// audio packets (timebase 1/48000) are muxed to fMP4 bytes, fed back to a
// Demuxer, and the recovered stream table and packet counts are printed.

#include <mediaway/mediaway.hpp>

#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <optional>
#include <variant>
#include <vector>

namespace {

// Deterministic placeholder payload for one synthetic H.264 access unit. The
// muxer treats packet bytes as opaque; these are not real bitstream data.
mediaway::Bytes makeVideoPacketPayload(std::int64_t index) {
  mediaway::Bytes payload(128);
  for (std::size_t i = 0; i < payload.size(); ++i) {
    payload[i] = static_cast<std::uint8_t>(
        (index * 31 + static_cast<std::int64_t>(i)) % 251);
  }
  return payload;
}

// Deterministic placeholder payload for one synthetic AAC-LC access unit
// (1024 samples, 96 bytes is a plausible ~128 kbps stereo frame size).
mediaway::Bytes makeAudioPacketPayload(std::int64_t index) {
  mediaway::Bytes payload(96);
  for (std::size_t i = 0; i < payload.size(); ++i) {
    payload[i] = static_cast<std::uint8_t>(
        (index * 17 + static_cast<std::int64_t>(i) * 3) % 239);
  }
  return payload;
}

const char* codecName(mediaway::Codec codec) {
  switch (codec) {
    case mediaway::Codec::H264:
      return "H.264";
    case mediaway::Codec::Aac:
      return "AAC";
    default:
      return "unknown";
  }
}

}  // namespace

int main() {
  try {
    // ---- Mux side: Open state, register tracks -------------------------------
    mediaway::container::Muxer muxer;

    const mediaway::TrackId videoTrack = muxer.addVideoTrack({
        0,                        // id - assigned by the muxer; the return value is authoritative
        mediaway::Codec::H264,
        {1, 30},                  // timebase: 30 ticks per second
        640, 480,
        {},                       // no codec config (placeholder Annex-B packets)
    });
    const mediaway::TrackId audioTrack = muxer.addAudioTrack({
        0,
        mediaway::Codec::Aac,
        {1, 48000},               // timebase: 48000 ticks per second
        48000, 2,
        {},                       // no AudioSpecificConfig (placeholder packets)
    });

    // ---- Typestate: Open -> Live ---------------------------------------------
    // begin() consumes the Open-state muxer; addVideoTrack / addAudioTrack are
    // no longer callable after this point.
    mediaway::container::LiveMuxer live = std::move(muxer).begin();

    // ---- Push 90 video + 90 audio packets ------------------------------------
    constexpr int kPacketCount = 90;
    for (std::int64_t i = 0; i < kPacketCount; ++i) {
      // Video: pts == dts, one frame per timebase tick at 30 fps.
      live.pushPacket({videoTrack, i, i, /*keyframe=*/true,
                       makeVideoPacketPayload(i)});
      // Audio: AAC-LC carries 1024 samples per access unit (48000 ticks/s).
      live.pushPacket({audioTrack, i * 1024, i * 1024, /*keyframe=*/true,
                       makeAudioPacketPayload(i)});
    }
    live.flush();

    // ---- Caller owns byte I/O: drain the muxer's output ----------------------
    mediaway::Bytes mp4;
    for (;;) {
      mediaway::Bytes chunk = live.pollBytes();
      if (chunk.empty()) break;
      mp4.insert(mp4.end(), chunk.begin(), chunk.end());
    }
    std::cout << "muxed " << kPacketCount << " video + " << kPacketCount
              << " audio packets into " << mp4.size() << " bytes\n";

    // ---- Demux side -----------------------------------------------------------
    mediaway::container::Demuxer demuxer;
    demuxer.pushBytes(mp4);

    const std::vector<mediaway::StreamInfo> streams = demuxer.streams();
    std::cout << "demuxed " << streams.size() << " stream(s):\n";
    for (const mediaway::StreamInfo& stream : streams) {
      if (const auto* video = std::get_if<mediaway::VideoStreamInfo>(&stream)) {
        std::cout << "  #" << video->id << " video " << codecName(video->codec)
                  << ' ' << video->width << 'x' << video->height << " @ "
                  << video->timescale.num << '/' << video->timescale.den << '\n';
      } else if (const auto* audio = std::get_if<mediaway::AudioStreamInfo>(&stream)) {
        std::cout << "  #" << audio->id << " audio " << codecName(audio->codec)
                  << ' ' << audio->sampleRate << " Hz, " << audio->channels
                  << " ch @ " << audio->timescale.num << '/'
                  << audio->timescale.den << '\n';
      }
    }

    std::size_t recoveredVideo = 0;
    std::size_t recoveredAudio = 0;
    while (std::optional<mediaway::Packet> packet = demuxer.pollPacket()) {
      if (packet->trackId == videoTrack) {
        ++recoveredVideo;
      } else if (packet->trackId == audioTrack) {
        ++recoveredAudio;
      }
    }

    std::cout << "recovered " << recoveredVideo << " video + " << recoveredAudio
              << " audio packets\n";
    const bool roundtripOk =
        recoveredVideo == static_cast<std::size_t>(kPacketCount) &&
        recoveredAudio == static_cast<std::size_t>(kPacketCount);
    std::cout << (roundtripOk ? "roundtrip OK" : "roundtrip MISMATCH") << '\n';
    return roundtripOk ? EXIT_SUCCESS : EXIT_FAILURE;
  } catch (const mediaway::Error& error) {
    std::cerr << "mediaway error: " << error.what() << " (status "
              << static_cast<int>(error.status()) << ")\n";
    return EXIT_FAILURE;
  }
}
