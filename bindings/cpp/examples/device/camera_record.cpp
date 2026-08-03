// camera_record.cpp - device capability: camera + mic capture -> H.264 + AAC
// encode -> single two-track MP4.
//
// Status: real. Camera capture (CPU NV12 frames), auto H.264 encode, and the
// container remux all run against the shipped C ABI underneath this example;
// the audio encoder is ABI v2 (adr/0003-auto-audio-encode-c-abi.md), so the
// microphone PCM is now ENCODED to AAC and muxed, not drained (the drain-only
// gap the old example carried is gone).
//
// Flow:
//   1. Open camera index 0 at 30 fps - graceful (skip) if absent.
//   2. Open the microphone at 48 kHz - graceful (video-only) if absent.
//   3. Read the negotiated capture geometry + mic format and open the auto
//      encoders at the REAL negotiated values.
//   4. Record ~3 s: poll video frames into the video encode session, push mic
//      PCM into the audio encode session.
//   5. finish() the video session -> fMP4; flush the audio session -> AAC
//      packets. REMUX: demux the video fMP4, mux video + AAC audio into one
//      two-track fMP4 (the audio track registered with the encoder's
//      AudioSpecificConfig). Without audio, write the video-only fMP4.
//   6. close() the captures (joins their worker threads); the result is
//      written to camera_out.mp4 by the caller.

#include <mediaway/mediaway.hpp>

#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <optional>
#include <string>
#include <thread>
#include <vector>

namespace {

// NO_BACKEND / UNSUPPORTED / NO_DEVICE are expected outcomes in the ABI, not
// hard failures: the caller degrades gracefully (video-only, or skip capture).
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
    // ---- Camera: index 0, 30 fps; 0x0 geometry = pick the camera default ----
    std::optional<mediaway::device::VideoCapture> camera;
    try {
      camera.emplace(mediaway::device::VideoCapture::open({0, {1, 30}}));
    } catch (const mediaway::Error& error) {
      if (!isExpectedUnavailable(error)) throw;
      std::cerr << "camera unavailable: " << error.what()
                << " - nothing to record\n";
      return EXIT_SUCCESS;  // graceful: no camera is a normal situation
    }

    // ---- Microphone: 48 kHz requested; negotiated format is authoritative ---
    std::optional<mediaway::device::AudioCapture> mic;
    try {
      mic.emplace(mediaway::device::AudioCapture::open({0, 48000}));
    } catch (const mediaway::Error& error) {
      if (!isExpectedUnavailable(error)) throw;
      std::cerr << "microphone unavailable: " << error.what()
                << " - recording video only\n";
    }

    // ---- Negotiated geometry -> encoder at the REAL resolution --------------
    const mediaway::device::CaptureInfo& info = camera->info();
    std::cout << "camera negotiated " << info.width << 'x' << info.height
              << " @ " << info.frameRate.num << '/' << info.frameRate.den
              << '\n';

    mediaway::encoder::AutoVideoEncoder encoder =
        mediaway::encoder::AutoVideoEncoder::open({
            mediaway::Codec::H264,
            info.width, info.height,
            info.frameRate,
            info.format,  // the camera delivers NV12 CPU frames
        });
    mediaway::encoder::EncodeSession session = std::move(encoder).begin();

    // ---- Audio encoder at the mic's negotiated format -----------------------
    std::optional<mediaway::encoder::AudioEncoder> audioEncoder;
    if (mic) {
      std::uint32_t sampleRate = 0;
      std::uint16_t channels = 0;
      mic->format(sampleRate, channels);
      std::cout << "mic negotiated " << sampleRate << " Hz, " << channels
                << " channel(s)\n";
      try {
        audioEncoder.emplace(mediaway::encoder::AudioEncoder::open(
            sampleRate, channels, {1, sampleRate}));
      } catch (const mediaway::Error& error) {
        if (!isExpectedUnavailable(error)) throw;
        std::cerr << "audio encoder unavailable: " << error.what()
                  << " - recording video only\n";
      }
    }

    // ---- Record ~3 s ----------------------------------------------------------
    const auto deadline =
        std::chrono::steady_clock::now() + std::chrono::seconds(3);
    std::size_t videoFrames = 0;
    std::size_t audioFrames = 0;
    while (std::chrono::steady_clock::now() < deadline) {
      if (std::optional<mediaway::VideoFrame> frame = camera->pollFrame()) {
        session.writeFrame(*frame);
        ++videoFrames;
      }
      if (mic && audioEncoder) {
        while (std::optional<mediaway::device::AudioFrame> audio =
                   mic->pollFrame()) {
          audioEncoder->pushPcm(*audio);
          ++audioFrames;
        }
      }
      std::this_thread::sleep_for(std::chrono::milliseconds(2));
    }

    // Explicit close joins the capture worker threads (can block up to one
    // frame interval); the destructors would do the same, but finishing the
    // encode with captures stopped is tidier.
    camera->close();
    if (mic) mic->close();

    // ---- Finish encodes -------------------------------------------------------
    mediaway::Bytes mp4 = std::move(session).finish();

    std::vector<mediaway::Packet> audioPackets;
    std::optional<mediaway::AudioStreamInfo> audioInfo;
    if (audioEncoder) {
      audioEncoder->flush();
      while (std::optional<mediaway::Packet> packet = audioEncoder->pollPacket()) {
        audioPackets.push_back(std::move(*packet));
      }
      audioInfo = audioEncoder->streamInfo();  // ASC materialized after the first push
    }
    const bool haveAudio = audioPackets.size() > 0 && audioInfo.has_value() &&
                           !audioInfo->codecConfig.empty();

    // ---- Remux video + AAC into one two-track MP4 -----------------------------
    mediaway::Bytes out;
    if (haveAudio) {
      mediaway::container::Demuxer demuxer;
      demuxer.pushBytes(mp4);
      std::vector<mediaway::StreamInfo> streams = demuxer.streams();
      if (streams.size() != 1 ||
          !std::holds_alternative<mediaway::VideoStreamInfo>(streams[0])) {
        std::cerr << "expected exactly one video stream from the encode "
                     "session's fMP4\n";
        return EXIT_FAILURE;
      }
      const mediaway::VideoStreamInfo& vinfo =
          std::get<mediaway::VideoStreamInfo>(streams[0]);

      mediaway::container::Muxer muxer;
      const mediaway::TrackId videoTrack = muxer.addVideoTrack(vinfo);
      const mediaway::TrackId audioTrack = muxer.addAudioTrack(*audioInfo);
      mediaway::container::LiveMuxer live = std::move(muxer).begin();

      while (std::optional<mediaway::Packet> packet = demuxer.pollPacket()) {
        packet->trackId = videoTrack;  // remap to the new muxer's registration
        live.pushPacket(*packet);
      }
      for (mediaway::Packet& packet : audioPackets) {
        packet.trackId = audioTrack;
        live.pushPacket(packet);
      }
      live.flush();
      out = live.pollBytes();
    } else {
      out = std::move(mp4);
    }

    std::cout << "recorded " << videoFrames << " video frames";
    if (haveAudio) {
      std::cout << " from " << audioFrames << " PCM frame(s) -> "
                << audioPackets.size() << " AAC packets ("
                << audioInfo->sampleRate << " Hz, " << audioInfo->channels
                << " ch)";
    } else {
      std::cout << " (audio unavailable)";
    }
    std::cout << " -> " << out.size() << " bytes\n";
    if (!writeFile("camera_out.mp4", out)) {
      std::cerr << "failed to write camera_out.mp4\n";
      return EXIT_FAILURE;
    }
    std::cout << "wrote camera_out.mp4\n";
    return EXIT_SUCCESS;
  } catch (const mediaway::Error& error) {
    std::cerr << "mediaway error: " << error.what() << " (status "
              << static_cast<int>(error.status()) << ")\n";
    return EXIT_FAILURE;
  }
}
