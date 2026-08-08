// decode_roundtrip.cpp - pipeline capability: auto video decode + Opus audio
// decode, round-tripped end to end through the C++ wrapper.
//
// Status: real - the C ABI's decode sessions exist today (adr/0004-auto-decode
// -c-abi.md, adr/pipeline/0006-audio-decode-c-abi.md) and this example calls
// the same decoder::DecodeSession/AudioDecodeSession classes any C++ caller
// would use, mirroring the Rust FFI smoke tests
// (crates/mediaway-ffi/tests/{decode,audio_decode}_smoke.rs).
//
// Demonstrates:
//   - Video: AutoVideoEncoder -> EncodeSession::finish() produces real fMP4
//     bytes; container::Demuxer recovers the H.264 packets + AVCC extra_data
//     a downloaded/received file would have (not straight from the encoder);
//     decoder::DecodeSession::open() throws Status::NoBackend gracefully when
//     no decode backend exists (WMF is Windows-only).
//   - Audio: a real Opus encode (via the raw C ABI - the C++ AudioEncoder
//     wrapper is AAC-only today) feeds decoder::AudioDecodeSession, which is
//     cross-platform (mediaway-sw, no OS dependency).
//
// Build:
//   g++ -std=c++17 -Ibindings/cpp/include -Icrates/mediaway-ffi/include
//   bindings/cpp/examples/pipeline/decode_roundtrip.cpp
//   -Ltarget/x86_64-pc-windows-gnu/debug -lmediaway_ffi -o decode_roundtrip.exe

#include <mediaway/mediaway.hpp>

#include <cmath>
#include <cstdint>
#include <iostream>
#include <vector>

namespace {

constexpr std::uint32_t kWidth = 64;
constexpr std::uint32_t kHeight = 64;
constexpr std::uint32_t kFrameCount = 10;

void videoRoundTrip() {
    std::cout << "-- video decode round trip --\n";
    mediaway::encoder::VideoEncoderConfig config{mediaway::Codec::H264, kWidth, kHeight,
                                                  mediaway::Rational{1, 30}};
    mediaway::encoder::AutoVideoEncoder encoder = [&] {
        try {
            return mediaway::encoder::AutoVideoEncoder::open(config);
        } catch (const mediaway::Error& e) {
            if (e.status() == mediaway::Status::NoBackend) {
                std::cout << "skip: no encode backend compiled in\n";
                std::exit(0);
            }
            throw;
        }
    }();
    mediaway::encoder::EncodeSession session = std::move(encoder).begin();

    const std::size_t nv12Len = kWidth * kHeight + kWidth * kHeight / 2;
    const mediaway::Bytes plane(nv12Len, 0x80);
    for (std::uint32_t i = 0; i < kFrameCount; ++i) {
        mediaway::VideoFrame frame{mediaway::PixelFormat::Nv12, kWidth, kHeight,
                                    static_cast<std::int64_t>(i), plane};
        session.writeFrame(frame);
    }
    const mediaway::Bytes fmp4 = std::move(session).finish();
    std::cout << "encoded " << fmp4.size() << " fMP4 bytes\n";

    // Demux back to real H.264 packets + AVCC extra_data (the shape a C
    // caller decoding a downloaded file would have, not straight off the
    // encoder).
    mediaway::container::Demuxer demuxer;
    demuxer.pushBytes(fmp4);
    const auto streams = demuxer.streams();
    if (streams.empty() || !std::holds_alternative<mediaway::VideoStreamInfo>(streams[0])) {
        throw std::runtime_error("expected one video stream");
    }
    const auto& videoInfo = std::get<mediaway::VideoStreamInfo>(streams[0]);

    std::vector<mediaway::Packet> packets;
    while (auto packet = demuxer.pollPacket()) {
        packets.push_back(std::move(*packet));
    }
    if (packets.size() != kFrameCount) {
        throw std::runtime_error("expected every frame to demux back");
    }

    mediaway::decoder::DecodeSession decodeSession = [&] {
        try {
            return mediaway::decoder::DecodeSession::open(
                mediaway::Codec::H264, kWidth, kHeight, mediaway::Rational{1, 30},
                videoInfo.codecConfig);
        } catch (const mediaway::Error& e) {
            if (e.status() == mediaway::Status::NoBackend) {
                std::cout << "skip: no decode backend compiled in\n";
                std::exit(0);
            }
            throw;
        }
    }();
    for (const auto& packet : packets) {
        decodeSession.pushPacket(packet.pts, packet.dts, 0, packet.keyframe,
                                  packet.data.empty() ? nullptr : packet.data.data(),
                                  packet.data.size());
    }
    decodeSession.flush();

    std::uint32_t decoded = 0;
    while (auto frame = decodeSession.pollFrame()) {
        if (frame->width != kWidth || frame->height != kHeight) {
            throw std::runtime_error("decoded frame geometry mismatch");
        }
        if (frame->data.size() < nv12Len) {
            throw std::runtime_error("decoded frame implausibly small");
        }
        ++decoded;
    }
    if (decoded == 0) {
        throw std::runtime_error("expected at least one decoded frame");
    }
    std::cout << "decoded " << decoded << " frames\n";
}

constexpr std::uint32_t kSampleRate = 48000;
constexpr std::uint16_t kChannels = 1;
constexpr std::size_t kFrameSamples = 960;  // 20ms @ 48kHz mono
constexpr std::uint32_t kAudioFrameCount = 50;

void audioRoundTrip() {
    std::cout << "-- audio decode round trip --\n";
    const mediaway_rational_t timeBase{1, 50};
    const mediaway_audio_encode_config_t encConfig =
        mediaway_audio_encode_config_opus(kSampleRate, kChannels, timeBase);
    mediaway_audio_encode_session_t* encSession = nullptr;
    const mediaway_pipeline_status_t openSt = mediaway_audio_encoder_open(&encConfig, &encSession);
    if (openSt == MEDIAWAY_PIPELINE_STATUS_NO_BACKEND) {
        std::cout << "skip: no Opus encode backend compiled in\n";
        return;
    }
    if (openSt != MEDIAWAY_PIPELINE_STATUS_OK) {
        throw std::runtime_error("audio encoder open failed");
    }

    struct EncodedPacket {
        std::int64_t pts;
        mediaway::Bytes data;
    };
    std::vector<EncodedPacket> encoded;
    for (std::uint32_t i = 0; i < kAudioFrameCount; ++i) {
        std::vector<float> pcm(kFrameSamples);
        for (std::size_t s = 0; s < kFrameSamples; ++s) {
            const float t = static_cast<float>(i * kFrameSamples + s) / static_cast<float>(kSampleRate);
            pcm[s] = std::sin(t * 440.0f * 6.283185307f);
        }
        mediaway_audio_frame_view_t view{};
        view.pts = i;
        view.duration = 0;
        view.sample_rate = kSampleRate;
        view.channels = kChannels;
        view.sample_format = MEDIAWAY_SAMPLE_FORMAT_F32;
        view.data = reinterpret_cast<const std::uint8_t*>(pcm.data());
        view.data_len = pcm.size() * sizeof(float);
        if (mediaway_audio_encode_session_push_pcm(encSession, &view) != MEDIAWAY_PIPELINE_STATUS_OK) {
            throw std::runtime_error("push_pcm failed");
        }
        mediaway_audio_packet_t packet{};
        bool has = false;
        while (true) {
            if (mediaway_audio_encode_session_poll_packet(encSession, &packet, &has) !=
                MEDIAWAY_PIPELINE_STATUS_OK) {
                throw std::runtime_error("poll_packet failed");
            }
            if (!has) break;
            mediaway::Bytes data(packet.payload, packet.payload + packet.payload_len);
            encoded.push_back({packet.pts, std::move(data)});
            mediaway_pipeline_ffi_packet_free(&packet);
        }
    }
    mediaway_audio_encode_session_flush(encSession);
    mediaway_audio_encode_session_close(encSession);
    std::cout << "encoded " << encoded.size() << " Opus packets\n";

    mediaway::decoder::AudioDecodeSession decodeSession =
        mediaway::decoder::AudioDecodeSession::open(kSampleRate, kChannels,
                                                     mediaway::Rational{1, 50});
    for (const auto& packet : encoded) {
        decodeSession.pushPacket(packet.pts, packet.data.empty() ? nullptr : packet.data.data(),
                                  packet.data.size());
    }
    decodeSession.flush();

    std::uint32_t decoded = 0;
    while (auto frame = decodeSession.pollFrame()) {
        if (frame->sampleRate != kSampleRate || frame->channels != kChannels) {
            throw std::runtime_error("decoded audio frame format mismatch");
        }
        ++decoded;
    }
    if (decoded == 0) {
        throw std::runtime_error("expected at least one decoded audio frame");
    }
    std::cout << "decoded " << decoded << " Opus frames\n";
}

}  // namespace

int main() {
    try {
        videoRoundTrip();
        audioRoundTrip();
    } catch (const mediaway::Error& e) {
        std::cerr << "mediaway::Error: " << e.what() << " (raw=" << e.rawCode() << ")\n";
        return 1;
    } catch (const std::exception& e) {
        std::cerr << "error: " << e.what() << "\n";
        return 1;
    }
    std::cout << "decode_roundtrip: OK\n";
    return 0;
}
