// pipeline_abi.cpp - ABI pins and real-backend checks for adr/pipeline/0007:
// streaming fMP4 bytes, AAC decode, and the encoder/decoder capability probes.
//
// The static_asserts pin the C layout this wrapper depends on (values taken from
// compiling a probe against the real pipeline.h with gcc, 64-bit). The runtime part
// drives the shipped native library: it fails with a non-zero exit code on any
// mismatch, and skips gracefully where a backend is absent.
//
// Build (from the repo root, against a FRESH native library - not a stale staged copy):
//   g++ -std=c++17 -Wall -Wextra -Ibindings/cpp/include -Icrates/mediaway-ffi/include
//       bindings/cpp/tests/pipeline_abi.cpp -L<dir with mediaway_ffi.dll> -lmediaway_ffi
//       -o pipeline_abi.exe

#include <mediaway/mediaway.hpp>

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <stdexcept>
#include <string>

// ---- Compile-time pins ------------------------------------------------------------

static_assert(MEDIAWAY_PIPELINE_FFI_ABI_VERSION == 7, "wrapper targets pipeline ABI 7");

#if UINTPTR_MAX == 0xFFFFFFFFFFFFFFFFu
static_assert(sizeof(mediaway_audio_decode_config_t) == 48, "audio decode config size");
static_assert(offsetof(mediaway_audio_decode_config_t, codec) == 0, "codec");
static_assert(offsetof(mediaway_audio_decode_config_t, sample_rate) == 4, "sample_rate");
static_assert(offsetof(mediaway_audio_decode_config_t, channels) == 8, "channels");
static_assert(offsetof(mediaway_audio_decode_config_t, time_base) == 16, "time_base");
static_assert(offsetof(mediaway_audio_decode_config_t, extra_data) == 32, "extra_data");
static_assert(offsetof(mediaway_audio_decode_config_t, extra_data_len) == 40, "extra_data_len");
#endif
static_assert(sizeof(mediaway_encoder_capability_t) == 12, "encoder capability size");
static_assert(offsetof(mediaway_encoder_capability_t, backend) == 0, "backend");
static_assert(offsetof(mediaway_encoder_capability_t, state) == 4, "state");
static_assert(offsetof(mediaway_encoder_capability_t, path_class) == 8, "path_class");

namespace {

using mediaway::encoder::EncodeBackend;
using mediaway::encoder::EncodePathClass;
using mediaway::SupportState;

// The C++ enums must carry exactly the header's values.
static_assert(static_cast<std::uint32_t>(SupportState::Supported) == 0);
static_assert(static_cast<std::uint32_t>(SupportState::NotImplemented) == 1);
static_assert(static_cast<std::uint32_t>(SupportState::NoDevice) == 2);
static_assert(static_cast<std::uint32_t>(SupportState::Unknown) == 255);
static_assert(static_cast<std::uint32_t>(EncodeBackend::Os) == 0);
static_assert(static_cast<std::uint32_t>(EncodeBackend::Nvenc) == 1);
static_assert(static_cast<std::uint32_t>(EncodeBackend::QuickSync) == 2);
static_assert(static_cast<std::uint32_t>(EncodeBackend::Amf) == 3);
static_assert(static_cast<std::uint32_t>(EncodeBackend::Vulkan) == 4);
static_assert(static_cast<std::uint32_t>(EncodeBackend::Software) == 5);
static_assert(static_cast<std::uint32_t>(EncodeBackend::Unknown) == 255);
static_assert(static_cast<std::uint32_t>(EncodePathClass::None) == 0);
static_assert(static_cast<std::uint32_t>(EncodePathClass::ZeroCopy) == 1);
static_assert(static_cast<std::uint32_t>(EncodePathClass::GpuCopy) == 2);
static_assert(static_cast<std::uint32_t>(EncodePathClass::CpuUpload) == 3);
static_assert(static_cast<std::uint32_t>(EncodePathClass::Readback) == 4);
static_assert(static_cast<std::uint32_t>(EncodePathClass::Software) == 5);
static_assert(static_cast<std::uint32_t>(EncodePathClass::Unknown) == 255);

void require(bool ok, const std::string& what) {
    if (!ok) throw std::runtime_error("FAILED: " + what);
}

template <typename Fn>
void requireThrows(mediaway::Status expected, const std::string& what, Fn&& fn) {
    try {
        fn();
    } catch (const mediaway::Error& e) {
        require(e.status() == expected, what + " threw the wrong status");
        return;
    }
    throw std::runtime_error("FAILED: " + what + " did not throw");
}

// ---- Probes -------------------------------------------------------------------------

void probes() {
    std::cout << "-- probes --\n";
    require(mediaway_pipeline_ffi_abi_version() == 7, "runtime pipeline ABI version is 7");

    requireThrows(mediaway::Status::InvalidArgument, "encoderSupport with zero width",
                  [] { mediaway::encoder::encoderSupport(mediaway::Codec::H264, 0, 720); });

    const auto rows = mediaway::encoder::encoderSupport(mediaway::Codec::H264, 1280, 720);
    for (const auto& row : rows) {
        const bool supported = row.state == SupportState::Supported;
        require(supported == (row.pathClass != EncodePathClass::None),
                "pathClass is None exactly when the row is not Supported");
    }
    std::cout << "H.264 1280x720: " << rows.size() << " backend row(s)\n";
#ifdef _WIN32
    require(!rows.empty(), "Windows reports at least one H.264 backend row");
#endif

    require(mediaway::decoder::decoderSupport(mediaway::Codec::H264) != SupportState::Unknown,
            "decoderSupport(H264) answers");
#ifdef _WIN32
    require(mediaway::decoder::decoderSupport(mediaway::Codec::Aac) == SupportState::Supported,
            "AAC decode is Supported on Windows");
#endif
}

// ---- Streaming bytes -----------------------------------------------------------------

constexpr std::uint32_t kWidth = 64;
constexpr std::uint32_t kHeight = 64;
constexpr int kFrameCount = 100;

mediaway::encoder::EncodeSession openVideoSession() {
    return mediaway::encoder::AutoVideoEncoder::open({mediaway::Codec::H264, kWidth, kHeight,
                                                      {1, 30}, mediaway::PixelFormat::Nv12})
        .begin();
}

void writeFrames(mediaway::encoder::EncodeSession& session, mediaway::Bytes* polled,
                 int* nonEmptyPolls) {
    const mediaway::Bytes grey(kWidth * kHeight * 3 / 2, 0x80);
    for (int i = 0; i < kFrameCount; ++i) {
        session.writeFrame({mediaway::PixelFormat::Nv12, kWidth, kHeight, i, grey});
        if (polled) {
            const mediaway::Bytes chunk = session.pollBytes();
            if (!chunk.empty()) ++*nonEmptyPolls;
            polled->insert(polled->end(), chunk.begin(), chunk.end());
        }
    }
}

std::size_t countPackets(const mediaway::Bytes& fmp4) {
    mediaway::container::Demuxer demux;
    demux.pushBytes(fmp4);
    std::size_t n = 0;
    while (demux.pollPacket()) ++n;
    return n;
}

void streaming() {
    std::cout << "-- streaming bytes --\n";
    mediaway::encoder::EncodeSession polledSession = [] {
        try {
            return openVideoSession();
        } catch (const mediaway::Error& e) {
            if (e.status() == mediaway::Status::NoBackend) {
                std::cout << "skip: no encode backend\n";
                std::exit(0);
            }
            throw;
        }
    }();

    mediaway::Bytes streamed;
    int nonEmptyPolls = 0;
    writeFrames(polledSession, &streamed, &nonEmptyPolls);
    require(nonEmptyPolls >= 1, "some fMP4 bytes were ready before finish");
    require(streamed.size() > 8 && std::string(streamed.begin() + 4, streamed.begin() + 8) == "ftyp",
            "first polled bytes start the file with ftyp");
    const std::size_t polledSize = streamed.size();

    const mediaway::Bytes tail = std::move(polledSession).finish();
    require(tail.size() < polledSize,
            "finish after polling returns the unpolled tail, not the whole stream");
    streamed.insert(streamed.end(), tail.begin(), tail.end());

    mediaway::encoder::EncodeSession referenceSession = openVideoSession();
    writeFrames(referenceSession, nullptr, nullptr);
    const mediaway::Bytes reference = std::move(referenceSession).finish();

    require(countPackets(streamed) == static_cast<std::size_t>(kFrameCount),
            "streamed output demuxes to every frame");
    require(countPackets(reference) == static_cast<std::size_t>(kFrameCount),
            "unpolled output demuxes to every frame");
    std::cout << nonEmptyPolls << " non-empty polls; streamed " << streamed.size()
              << " B (tail " << tail.size() << " B) vs unpolled " << reference.size() << " B\n";
    require(streamed.size() == reference.size(), "streamed and unpolled outputs are the same size");
}

// ---- AAC decode ---------------------------------------------------------------------------

constexpr std::uint32_t kSampleRate = 48000;
constexpr std::uint16_t kChannels = 2;
constexpr std::uint32_t kFrameSamples = 1024;
constexpr std::uint32_t kAudioFrames = 48;

mediaway::Bytes sineFrame(std::uint32_t frame) {
    mediaway::Bytes out(kFrameSamples * kChannels * sizeof(float));
    float* f = reinterpret_cast<float*>(out.data());
    for (std::uint32_t s = 0; s < kFrameSamples; ++s) {
        const float t = static_cast<float>(frame * kFrameSamples + s) / static_cast<float>(kSampleRate);
        const float v = std::sin(2.0F * 3.14159265F * 440.0F * t);
        for (std::uint16_t c = 0; c < kChannels; ++c) *f++ = v;
    }
    return out;
}

void aacDecode() {
    std::cout << "-- AAC decode --\n";
    // An empty AudioSpecificConfig is a config mistake, not a missing capability.
    requireThrows(mediaway::Status::InvalidArgument, "openAac with an empty AudioSpecificConfig",
                  [] { mediaway::decoder::AudioDecodeSession::openAac(kSampleRate, kChannels, {}); });

    mediaway::encoder::AudioEncoder encoder = [] {
        try {
            return mediaway::encoder::AudioEncoder::open(kSampleRate, kChannels, {1, kSampleRate});
        } catch (const mediaway::Error& e) {
            if (e.status() == mediaway::Status::NoBackend || e.status() == mediaway::Status::Unsupported) {
                std::cout << "skip: no AAC encode backend on this platform\n";
                std::exit(0);
            }
            throw;
        }
    }();
    for (std::uint32_t i = 0; i < kAudioFrames; ++i) {
        encoder.pushPcm({i * kFrameSamples, kSampleRate, kChannels, sineFrame(i)});
    }
    encoder.flush();
    std::vector<mediaway::Packet> packets;
    while (auto p = encoder.pollPacket()) packets.push_back(std::move(*p));
    require(!packets.empty(), "the AAC encoder produced packets");
    const mediaway::Bytes asc = encoder.streamInfo().codecConfig;
    require(!asc.empty(), "the AAC stream info carries an AudioSpecificConfig");

    mediaway::decoder::AudioDecodeSession decoder =
        mediaway::decoder::AudioDecodeSession::openAac(kSampleRate, kChannels, asc);
    // AAC has no loss-concealment convention: an empty packet is refused, not concealed.
    requireThrows(mediaway::Status::InvalidArgument, "an empty AAC packet",
                  [&] { decoder.pushPacket(0, nullptr, 0, kFrameSamples); });

    for (const auto& p : packets) {
        decoder.pushPacket(p.pts, p.data.data(), p.data.size(), kFrameSamples);
    }
    decoder.flush();

    std::size_t samples = 0;
    double energy = 0.0;
    while (auto frame = decoder.pollFrame()) {
        require(frame->channels == kChannels, "decoded channel count");
        const float* pcm = reinterpret_cast<const float*>(frame->data.data());
        const std::size_t n = frame->data.size() / sizeof(float);
        for (std::size_t i = 0; i < n; ++i) energy += static_cast<double>(pcm[i]) * pcm[i];
        samples += n / kChannels;
    }
    const double meanSquare = energy / static_cast<double>(samples * kChannels);
    std::cout << packets.size() << " packets -> " << samples << " samples/ch, mean square "
              << meanSquare << "\n";
    require(samples == packets.size() * kFrameSamples, "sample-exact: samples == packets * 1024");
    require(meanSquare > 0.1, "decoded audio is a real signal, not silence");
}

}  // namespace

int main() {
    try {
        probes();
        streaming();
        aacDecode();
    } catch (const std::exception& e) {
        std::cerr << e.what() << "\n";
        return EXIT_FAILURE;
    }
    std::cout << "pipeline_abi: ok\n";
    return EXIT_SUCCESS;
}
