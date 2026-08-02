// ASPIRATIONAL EXAMPLE — no `mediaway-container-ffi` C ABI and no C++ binding
// package exist yet. This file shows the target ergonomics a future C++
// wrapper over Mediaway's container mux/demux C ABI should aim for: RAII
// classes that own the underlying opaque C handles (`unique_ptr` + a custom
// deleter each), with C ABI error codes translated into C++ exceptions at
// the wrapper boundary. See ../README.md and docs/spec/c-ffi.md.
//
// Mirrors examples/mux_roundtrip.rs: build a muxer, register one H.264 video
// track and one AAC audio track, push placeholder packets for a simulated
// 3-second clip, flush, pull the fMP4 bytes, then demux those same bytes
// back and count the recovered packets.

#include <mediaway/container.hpp>

#include <cstdint>
#include <cstdlib>
#include <iostream>

int main() {
    using namespace mediaway::container;

    constexpr Rational kVideoTimeBase{1, 30};
    constexpr Rational kAudioTimeBase{1, 48'000};
    constexpr std::uint32_t kFrameCount = 90; // 3 s at 30 fps

    try {
        // ── 1. Register tracks (Open state) ─────────────────────────────
        Muxer muxer;

        const TrackId videoTrack = muxer.addTrack(VideoStreamInfo{
            .id = 0,
            .codec = CodecKind::H264,
            .timeBase = kVideoTimeBase,
            .width = 1920,
            .height = 1080,
            .extraData = {},
        });

        const TrackId audioTrack = muxer.addTrack(AudioStreamInfo{
            .id = 1,
            .codec = CodecKind::Aac,
            .timeBase = kAudioTimeBase,
            .extraData = {},
            .sampleRate = 48'000,
            .channels = 2,
        });

        // ── 2. Transition to streaming (Live state) ─────────────────────
        // begin() consumes the Open-state Muxer; track registration closes
        // and packet submission opens on the returned LiveMuxer.
        LiveMuxer liveMuxer = std::move(muxer).begin();

        for (std::uint32_t i = 0; i < kFrameCount; ++i) {
            liveMuxer.pushPacket(Packet{
                .streamId = videoTrack,
                .pts = static_cast<std::int64_t>(i),
                .dts = static_cast<std::int64_t>(i),
                .duration = 1,
                .isKeyframe = (i % 30 == 0),
                .isDiscard = false,
                .payload = {0x00, 0x00, 0x00, 0x01}, // placeholder NAL start code
            });

            liveMuxer.pushPacket(Packet{
                .streamId = audioTrack,
                .pts = static_cast<std::int64_t>(i) * 1'600,
                .dts = static_cast<std::int64_t>(i) * 1'600,
                .duration = 1'600,
                .isKeyframe = true,
                .isDiscard = false,
                .payload = {0xff, 0xf1}, // placeholder ADTS-ish header
            });
        }
        liveMuxer.flush();

        // ── 3. Pull bytes — the muxer never touches files/sockets itself ─
        const Bytes mp4Bytes = liveMuxer.pollBytes();
        std::cout << "mux_roundtrip: " << kFrameCount << " frames -> "
                  << mp4Bytes.size() << " bytes of fMP4\n";

        // ── 4. Demux the same bytes back ────────────────────────────────
        Demuxer demuxer;
        demuxer.pushBytes(mp4Bytes);

        const auto streams = demuxer.streams();
        std::cout << "mux_roundtrip: demuxer sees " << streams.size()
                  << " stream(s)\n";
        for (const auto& s : streams) {
            std::cout << "  stream " << s.id << " - codec "
                       << static_cast<int>(s.codec);
            if (s.geometry) {
                std::cout << " " << s.geometry->width << "x" << s.geometry->height;
            }
            std::cout << '\n';
        }

        std::uint32_t nVideo = 0;
        std::uint32_t nAudio = 0;
        while (const std::optional<Packet> pkt = demuxer.pollPacket()) {
            if (pkt->streamId == videoTrack) {
                ++nVideo;
            } else {
                ++nAudio;
            }
        }
        std::cout << "mux_roundtrip: recovered " << nVideo << " video + "
                  << nAudio << " audio packets\n";
    } catch (const mediaway::Error& e) {
        std::cerr << "mux_roundtrip: " << e.what() << '\n';
        return EXIT_FAILURE;
    }

    return EXIT_SUCCESS;
}
