// all_formats_smoke.cpp - container capability: round-trip every one of the
// 8 mediaway-container formats reachable from the C++ wrapper.
//
// Demonstrates the format-specific class each ADR introduced:
//   - container::Muxer/Demuxer(Format::Webm) - WebM shares MP4's shape.
//   - container::OggMuxer/OggDemuxer, AdtsMuxer/AdtsDemuxer - no track
//     registration, immediately live.
//   - container::FlvMuxer/FlvDemuxer - out-buffer-per-call mux, fixed
//     one-video/one-audio slot.
//   - container::TsMuxer/TsDemuxer - construction-time elementary stream
//     list, raw 90 kHz pts/dts, TsDemuxer::finish() returns an array.
//   - container::Mp3Muxer/Mp3Demuxer - fixed frame header, explicit padding.
//   - container::WavMuxer + wavParse() - consuming finish(), one-shot parse.
//
// Every payload/expected value below mirrors a verified round trip already
// checked in the Rust FFI smoke tests (crates/mediaway-ffi/tests/
// {webm,ogg_adts,flv,ts,mp3,wav}_container_smoke.rs) — not invented here.

#include <mediaway/mediaway.hpp>

#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <vector>

namespace {

void check(bool condition, const char* what) {
    if (!condition) {
        std::cerr << "FAILED: " << what << std::endl;
        std::exit(1);
    }
    std::cout << "ok: " << what << std::endl;
}

void smokeWebm() {
    mediaway::container::Muxer muxer(mediaway::container::Format::Webm);
    // addVideoTrack assigns the id (registration order, starting at 1 — see
    // mp4_webm.hpp's nextId_ comment on why not 0) — the info.id field
    // passed in is ignored.
    const mediaway::TrackId trackId = muxer.addVideoTrack(mediaway::VideoStreamInfo{
        0, mediaway::Codec::Vp8, {1, 30}, 64, 64, {}});
    auto live = std::move(muxer).begin();
    mediaway::Bytes webmBytes;
    for (std::int64_t i = 0; i < 5; ++i) {
        mediaway::Bytes payload(16, 0xAA);
        live.pushPacket(mediaway::Packet{trackId, i, i, i == 0, payload});
        auto chunk = live.pollBytes();
        webmBytes.insert(webmBytes.end(), chunk.begin(), chunk.end());
    }
    live.flush();
    auto tail = live.pollBytes();
    webmBytes.insert(webmBytes.end(), tail.begin(), tail.end());
    check(webmBytes.size() > 4 && webmBytes[0] == 0x1A && webmBytes[1] == 0x45,
          "WebM: EBML magic present");

    mediaway::container::Demuxer demuxer(mediaway::container::Format::Webm);
    demuxer.pushBytes(webmBytes);
    int count = 0;
    while (demuxer.pollPacket().has_value()) ++count;
    check(count == 5, "WebM: 5 packets recovered");
}

void smokeOgg() {
    mediaway::Bytes head;
    const char magic[] = "OpusHead";
    head.insert(head.end(), magic, magic + 8);
    head.push_back(1);   // version
    head.push_back(2);   // channels
    head.push_back(0); head.push_back(0);  // pre-skip
    std::uint32_t rate = 48000;
    for (int i = 0; i < 4; ++i) head.push_back(static_cast<std::uint8_t>(rate >> (8 * i)));
    head.push_back(0); head.push_back(0);  // output gain
    head.push_back(0);                     // channel mapping family

    mediaway::container::OggMuxer muxer(1);
    muxer.pushPacket(mediaway::Packet{0, 0, 0, true, head});
    mediaway::Bytes oggBytes = muxer.pollBytes();
    mediaway::Bytes audio{1, 2, 3, 4};
    muxer.pushPacket(mediaway::Packet{0, 960, 960, true, audio});
    auto chunk = muxer.pollBytes();
    oggBytes.insert(oggBytes.end(), chunk.begin(), chunk.end());
    muxer.flush();
    check(oggBytes.size() > 4 && oggBytes[0] == 'O' && oggBytes[1] == 'g', "Ogg: capture pattern present");

    mediaway::container::OggDemuxer demuxer;
    demuxer.pushBytes(oggBytes);
    auto packet = demuxer.pollPacket();
    check(packet.has_value() && packet->data.size() == 4 && packet->pts == 960,
          "Ogg: Opus packet recovered");
}

void smokeAdts() {
    mediaway::container::AdtsMuxer muxer(44100, 2);
    mediaway::Bytes rawAac(100, 0xAB);
    for (int i = 0; i < 2; ++i) muxer.pushPacket(mediaway::Packet{0, 0, 0, true, rawAac});
    muxer.flush();
    mediaway::Bytes adtsBytes = muxer.pollBytes();
    check(adtsBytes.size() > 2 && adtsBytes[0] == 0xFF && (adtsBytes[1] & 0xF0) == 0xF0,
          "ADTS: sync word present");

    mediaway::container::AdtsDemuxer demuxer;
    demuxer.pushBytes(adtsBytes);
    std::int64_t expectedPts = 0;
    for (int i = 0; i < 2; ++i) {
        auto packet = demuxer.pollPacket();
        check(packet.has_value() && packet->pts == expectedPts && packet->data.size() == 100,
              "ADTS: frame recovered with synthesized pts");
        expectedPts += 1024;
    }
}

void smokeFlv() {
    mediaway::container::FlvMuxer muxer;
    mediaway::Bytes flvBytes = muxer.writeHeader(true, true);

    mediaway::Bytes avcc{1, 0x42, 0, 0x1e, 0xff, 0xe1, 0, 0};
    muxer.addVideoTrack(mediaway::VideoStreamInfo{0, mediaway::Codec::H264, {1, 1000}, 1280, 720, avcc});
    mediaway::Bytes asc{0x12, 0x10};
    muxer.addAudioTrack(mediaway::AudioStreamInfo{0, mediaway::Codec::Aac, {1, 1000}, 44100, 2, asc});

    mediaway::Bytes videoPayload{0, 0, 0, 2, 0x65, 0x88};
    auto chunk = muxer.pushPacket(
        mediaway::Packet{mediaway::container::kFlvVideoTrackId, 45, 33, true, videoPayload});
    flvBytes.insert(flvBytes.end(), chunk.begin(), chunk.end());

    mediaway::Bytes audioPayload{1, 2, 3, 4};
    chunk = muxer.pushPacket(
        mediaway::Packet{mediaway::container::kFlvAudioTrackId, 23, 23, true, audioPayload});
    flvBytes.insert(flvBytes.end(), chunk.begin(), chunk.end());

    check(flvBytes.size() > 3 && flvBytes[0] == 'F' && flvBytes[1] == 'L' && flvBytes[2] == 'V',
          "FLV: file signature present");

    mediaway::container::FlvDemuxer demuxer;
    demuxer.pushBytes(flvBytes);
    bool gotVideo = false, gotAudio = false;
    while (auto packet = demuxer.pollPacket()) {
        if (packet->trackId == mediaway::container::kFlvVideoTrackId) gotVideo = true;
        if (packet->trackId == mediaway::container::kFlvAudioTrackId) gotAudio = true;
    }
    check(gotVideo && gotAudio, "FLV: both tracks recovered");
}

void smokeTs() {
    constexpr std::uint16_t kVideoPid = 0x100;
    constexpr std::uint16_t kAudioPid = 0x101;
    std::vector<mediaway::container::ElementaryStream> streams{
        {kVideoPid, mediaway::Codec::H264}, {kAudioPid, mediaway::Codec::Aac}};
    mediaway::container::TsMuxer muxer(1, 0x1000, streams);

    mediaway::Bytes tsBytes = muxer.writePatPmt();
    mediaway::Bytes videoAu{0, 0, 0, 1, 0x65, 0x88};
    auto chunk = muxer.writeAccessUnit(kVideoPid, videoAu, 90000, std::nullopt, true);
    tsBytes.insert(tsBytes.end(), chunk.begin(), chunk.end());
    mediaway::Bytes videoAu2{0, 0, 0, 1, 0x41};
    chunk = muxer.writeAccessUnit(kVideoPid, videoAu2, 90033, std::nullopt, false);
    tsBytes.insert(tsBytes.end(), chunk.begin(), chunk.end());

    mediaway::container::TsDemuxer demuxer;
    demuxer.pushBytes(tsBytes);
    auto packet = demuxer.pollPacket();
    check(packet.has_value() && packet->pts == 90000 && packet->keyframe,
          "MPEG-TS: video access unit recovered");

    // finish() recovers a trailing access unit with no confirming marker.
    mediaway::container::TsMuxer muxer2(1, 0x1000, streams);
    mediaway::Bytes tsBytes2 = muxer2.writePatPmt();
    mediaway::Bytes tail{9, 9, 9};
    chunk = muxer2.writeAccessUnit(kVideoPid, tail, 90000, std::nullopt, true);
    tsBytes2.insert(tsBytes2.end(), chunk.begin(), chunk.end());
    mediaway::container::TsDemuxer demuxer2;
    demuxer2.pushBytes(tsBytes2);
    check(!demuxer2.pollPacket().has_value(), "MPEG-TS: no packet ready before finish()");
    auto finished = demuxer2.finish();
    check(finished.size() == 1 && finished[0].data == tail, "MPEG-TS: finish() recovers trailing AU");
}

void smokeMp3() {
    mediaway::container::Mp3FrameHeader header{
        mediaway::container::MpegVersion::Mpeg1, 128, 44100,
        mediaway::container::ChannelMode::Stereo};
    mediaway::container::Mp3Muxer muxer(header);
    // frame_len(false) for 128kbps/44100Hz = floor(144000*128/44100) = 417; body = 417-4 = 413.
    mediaway::Bytes body(413, 0xAB);
    mediaway::Bytes mp3Bytes = muxer.writeFrame(body, false);
    check(mp3Bytes[0] == 0xFF, "MP3: frame sync byte present");

    mediaway::container::Mp3Demuxer demuxer;
    demuxer.pushBytes(mp3Bytes);
    auto packet = demuxer.pollPacket();
    check(packet.has_value() && packet->data.size() == 413,
          "MP3: frame recovered (duration/pts synthesized internally, not exposed on Packet)");
}

void smokeWav() {
    mediaway::container::WavMuxer muxer(44100, 2, 16);
    mediaway::Bytes pcm{1, 2, 3, 4, 5, 6, 7, 8};
    muxer.pushPacket(mediaway::Packet{0, 0, 0, true, pcm});
    mediaway::Bytes wavBytes = muxer.finish();
    check(wavBytes.size() > 12 && wavBytes[0] == 'R' && wavBytes[8] == 'W',
          "WAV: RIFF/WAVE header present");

    auto result = mediaway::container::wavParse(wavBytes);
    auto* audio = std::get_if<mediaway::AudioStreamInfo>(&result.info);
    check(audio != nullptr && audio->sampleRate == 44100 && audio->channels == 2,
          "WAV: parsed stream info");
    check(result.packet.data == pcm, "WAV: parsed packet payload matches");

    // A second finish() fails honestly rather than corrupting anything.
    bool threw = false;
    try {
        muxer.finish();
    } catch (const mediaway::Error& e) {
        threw = (e.status() == mediaway::Status::InvalidState);
    }
    check(threw, "WAV: second finish() throws InvalidState");
}

}  // namespace

int main() {
    try {
        std::cout << "-- WebM --" << std::endl;
        smokeWebm();
        std::cout << "-- Ogg --" << std::endl;
        smokeOgg();
        std::cout << "-- ADTS --" << std::endl;
        smokeAdts();
        std::cout << "-- FLV --" << std::endl;
        smokeFlv();
        std::cout << "-- MPEG-TS --" << std::endl;
        smokeTs();
        std::cout << "-- MP3 --" << std::endl;
        smokeMp3();
        std::cout << "-- WAV --" << std::endl;
        smokeWav();
    } catch (const mediaway::Error& e) {
        std::cerr << "mediaway::Error: " << e.what() << " (status=" << static_cast<int>(e.status())
                   << ", rawCode=" << e.rawCode() << ")" << std::endl;
        return 1;
    }
    std::cout << "\nall 7 newly-wired container formats verified.\n";
    return 0;
}
