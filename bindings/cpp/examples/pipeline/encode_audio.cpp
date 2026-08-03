// encode_audio.cpp - pipeline capability: auto AAC encode of synthetic F32
// stereo PCM -> audio-only fragmented MP4.
//
// Status: real. Everything here runs against the shipped C ABI (ABI v2,
// adr/0003-auto-audio-encode-c-abi.md): the audio encoder is single-step
// (the session IS the encoder — no intermediate handle, no consumption trap),
// PCM is pushed as borrowed views, and encoded packets are polled back and
// muxed into an audio-only fMP4 whose track carries the encoder's
// AudioSpecificConfig (what the esds box needs to be playable).
//
// Deterministic: 96 frames of a 440 Hz sine (1024 samples @ 48 kHz, stereo
// F32) — no microphone needed. NO_BACKEND is graceful.
//
// Build:
//   g++ -std=c++17 -Ibindings/cpp/include -Icrates/mediaway-ffi/include
//   -Icrates/mediaway-container-ffi/include bindings/cpp/examples/pipeline/encode_audio.cpp
//   -Ltarget/x86_64-pc-windows-gnu/debug -lmediaway_pipeline_ffi -lmediaway_container_ffi -o encode_audio.exe

#include <mediaway/mediaway.hpp>

#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <vector>

namespace {

constexpr std::uint32_t kSampleRate = 48000;
constexpr std::uint16_t kChannels = 2;
constexpr std::uint32_t kFrameSamples = 1024;  // ~21 ms
constexpr std::uint32_t kFrameCount = 96;      // ~2.0 s of audio

// One interleaved F32 stereo frame of a deterministic 440 Hz sine.
mediaway::Bytes sineFrame(std::size_t frameIndex) {
  mediaway::Bytes out(kFrameSamples * kChannels * sizeof(float));
  float* f = reinterpret_cast<float*>(out.data());
  for (std::uint32_t s = 0; s < kFrameSamples; ++s) {
    const float t = static_cast<float>(frameIndex * kFrameSamples + s) /
                    static_cast<float>(kSampleRate);
    const float v = std::sin(2.0F * 3.14159265F * 440.0F * t);
    for (std::uint16_t c = 0; c < kChannels; ++c) *f++ = v;
  }
  return out;
}

}  // namespace

int main() {
  try {
    // ---- Open the audio encoder (single step) ---------------------------------
    mediaway::encoder::AudioEncoder encoder = mediaway::encoder::AudioEncoder::open(
        kSampleRate, kChannels, {1, kSampleRate});

    // ---- Push synthetic PCM -----------------------------------------------------
    for (std::uint32_t i = 0; i < kFrameCount; ++i) {
      mediaway::device::AudioFrame frame{i * kFrameSamples, kSampleRate, kChannels,
                                         sineFrame(i)};
      encoder.pushPcm(frame);
    }
    encoder.flush();

    // ---- Poll encoded packets ----------------------------------------------------
    std::vector<mediaway::Packet> packets;
    while (std::optional<mediaway::Packet> packet = encoder.pollPacket()) {
      packets.push_back(std::move(*packet));
    }
    if (packets.empty()) {
      std::cerr << "encoder produced no packets for " << kFrameCount
                << " PCM frames\n";
      return EXIT_FAILURE;
    }

    // ---- Stream info: the AudioSpecificConfig the track needs -------------------
    mediaway::AudioStreamInfo info = encoder.streamInfo();
    if (info.codecConfig.empty()) {
      std::cerr << "stream info carries no AudioSpecificConfig\n";
      return EXIT_FAILURE;
    }
    std::cout << "encoded " << packets.size() << " AAC packet(s), ASC "
              << info.codecConfig.size() << " bytes\n";

    // ---- Mux an audio-only fragmented MP4 ----------------------------------------
    mediaway::container::Muxer muxer;
    muxer.addAudioTrack(info);
    mediaway::container::LiveMuxer live = std::move(muxer).begin();
    for (mediaway::Packet& packet : packets) {
      packet.trackId = 0;
      live.pushPacket(packet);
    }
    live.flush();
    const mediaway::Bytes out = live.pollBytes();
    std::cout << "muxed " << packets.size()
              << " AAC packet(s) into " << out.size()
              << " bytes of audio-only fragmented MP4\n";
    return EXIT_SUCCESS;
  } catch (const mediaway::Error& error) {
    if (error.status() == mediaway::Status::NoBackend) {
      std::cerr << "no audio encode backend (NoBackend) - exiting gracefully\n";
      return EXIT_SUCCESS;
    }
    std::cerr << "mediaway error: " << error.what() << " (status "
              << static_cast<int>(error.status()) << ")\n";
    return EXIT_FAILURE;
  }
}
