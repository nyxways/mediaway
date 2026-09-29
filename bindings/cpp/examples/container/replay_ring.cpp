// replay_ring.cpp - container capability: keep the last N seconds, save a clip.
//
// Status: real - links and runs against the shipped C ABI (adr/container/0009-replay-ring-c-abi.md).
//
// Demonstrates:
//   - ReplayRing (Bytes): push packets as they are produced; the ring keeps `window` of history and
//     evicts whole GOPs.
//   - clipLast(span): "the last 2 s", cut at a keyframe, as an owned ReplayClip snapshot with
//     pts/dts rebased so the cut keyframe decodes at zero.
//   - Writing the clip out with an ordinary container::Muxer: the ring never writes a file, and
//     there is deliberately no "mux this clip" function - the clip is a packet sequence.
//   - The Stored variant: a caller that already writes every packet to a file keeps only where
//     each one landed (Muxer::withPlacements -> pollPlacements -> ReplayRing::pushStored) and reads
//     the bytes back itself when it saves a clip.
//
// LIMIT: the C ABI has no video-packet source. encoder::EncodeSession muxes its encoder's packets
// internally, so this feeds the ring synthetic packets; a real program feeds it packets from a
// Demuxer, from an AudioEncoder, or from an encoder it drives itself.
//
// Output: replay_clip.mp4 (a standalone fMP4 of the clip), re-demuxed here to prove it.

#include <mediaway/mediaway.hpp>

#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <map>
#include <utility>
#include <vector>

namespace {

using namespace mediaway;
using namespace mediaway::container;
using std::chrono::milliseconds;

constexpr TrackId kVideo = 1;  // a Muxer numbers its tracks from 1, in registration order
constexpr TrackId kAudio = 2;
constexpr Rational kFps{1, 30};
constexpr Rational kAudioRate{1, 48000};
constexpr std::int64_t kSeconds = 6;

// Synthetic 30 fps video (a keyframe every second) with 1024-sample AAC frames alongside. The bytes
// are placeholders: the muxer treats packet payloads as opaque.
Packet videoPacket(std::int64_t n) {
    return Packet{kVideo, n, n, n % 30 == 0, Bytes(64, static_cast<std::uint8_t>(0xB0 + n % 64))};
}
Packet audioPacket(std::int64_t k) {
    return Packet{kAudio, k * 1024, k * 1024, true, Bytes(32, 0xA0)};
}

VideoStreamInfo videoInfo() { return VideoStreamInfo{0, Codec::H264, kFps, 640, 480, {}}; }
AudioStreamInfo audioInfo() { return AudioStreamInfo{0, Codec::Aac, kAudioRate, 48000, 2, {}}; }

// The synthetic source: one second is 30 video frames and 46.875 audio frames.
template <typename Sink>
void produce(Sink&& sink) {
    std::int64_t audio = 0;
    for (std::int64_t n = 0; n < kSeconds * 30; ++n) {
        sink(videoPacket(n));
        while (audio * 1024 * 30 <= n * 48000) sink(audioPacket(audio++));
    }
}

// Mux every entry of a clip into a standalone fMP4.
Bytes writeClip(const ReplayClip& clip) {
    Muxer muxer;
    muxer.addVideoTrack(videoInfo());
    muxer.addAudioTrack(audioInfo());
    LiveMuxer live = std::move(muxer).begin();
    for (const ReplayEntry e : clip) live.pushPacket(e.toPacket());
    live.flush();
    Bytes file;
    for (Bytes chunk = live.pollBytes(); !chunk.empty(); chunk = live.pollBytes()) {
        file.insert(file.end(), chunk.begin(), chunk.end());
    }
    return file;
}

void countPackets(const Bytes& file, std::size_t* video, std::size_t* audio) {
    Demuxer demux;
    demux.pushBytes(file);
    *video = *audio = 0;
    while (auto p = demux.pollPacket()) (p->trackId == kVideo ? *video : *audio)++;
}

}  // namespace

int main() {
    try {
        // ── Bytes ring: keep 3 s, save the last 2 s ───────────────────────────────────────────
        ReplayRing ring(ReplayRingConfig{kVideo, kFps, milliseconds(3000)});
        ring.addStream(kAudio, kAudioRate);
        produce([&](const Packet& p) { ring.push(p); });

        auto clip = ring.clipLast(milliseconds(2000));
        if (!clip) {
            std::cerr << "no clip: the ring has not seen a keyframe\n";
            return EXIT_FAILURE;
        }
        std::cout << "ring holds " << ring.span().count() << " ms; clip: " << clip->size()
                  << " packets, " << clip->duration().count() << " ms, first dts "
                  << clip->at(0).dts << " (keyframe: " << std::boolalpha << clip->at(0).keyframe
                  << ")\n";

        const Bytes file = writeClip(*clip);
        std::ofstream("replay_clip.mp4", std::ios::binary)
            .write(reinterpret_cast<const char*>(file.data()), static_cast<std::streamsize>(file.size()));
        std::size_t video = 0, audio = 0;
        countPackets(file, &video, &audio);
        std::cout << "replay_clip.mp4: " << file.size() << " bytes, re-demuxed " << video
                  << " video + " << audio << " audio packets\n";
        if (video + audio != clip->size()) {
            std::cerr << "the standalone file does not hold every clip packet\n";
            return EXIT_FAILURE;
        }

        // ── Stored ring: the packets already live in a file; keep only where ──────────────────
        // Mux the whole stream with a placements muxer, as a recorder writing to disk would.
        Muxer recording = Muxer::withPlacements();
        recording.addVideoTrack(videoInfo());
        recording.addAudioTrack(audioInfo());
        LiveMuxer live = std::move(recording).begin();
        std::vector<Packet> all;
        produce([&](const Packet& p) {
            all.push_back(p);
            live.pushPacket(p);
        });
        live.flush();
        Bytes disk;  // stands in for the recording file
        for (Bytes chunk = live.pollBytes(); !chunk.empty(); chunk = live.pollBytes()) {
            disk.insert(disk.end(), chunk.begin(), chunk.end());
        }
        // Placements are in WRITE order, and a fragment writes its samples grouped by track, not in
        // push order - so with more than one track, key them by (track, dts), never by position.
        std::map<std::pair<TrackId, std::int64_t>, Placement> placements;
        for (const Placement& p : live.pollPlacements()) placements[{p.trackId, p.dts}] = p;

        ReplayRingConfig storedConfig{kVideo, kFps, milliseconds(3000)};
        storedConfig.payloadKind = ReplayPayloadKind::Stored;
        ReplayRing stored(storedConfig);
        stored.addStream(kAudio, kAudioRate);
        for (const Packet& p : all) {
            const Placement& at = placements.at({p.trackId, p.dts});
            stored.pushStored(ReplayPacketMeta{p.trackId, p.pts, p.dts, 0, p.keyframe},
                              StoredPayload{0, at.offset, at.len});
        }
        auto storedClip = stored.clipLast(milliseconds(2000));
        if (!storedClip || storedClip->size() != clip->size()) {
            std::cerr << "the Stored ring did not cut the same clip\n";
            return EXIT_FAILURE;
        }
        for (std::size_t i = 0; i < clip->size(); ++i) {
            const ReplayEntry s = storedClip->at(i);
            const ReplayEntry b = clip->at(i);
            const auto begin = disk.begin() + static_cast<std::ptrdiff_t>(s.stored.offset);
            if (Bytes(begin, begin + s.stored.len) != b.payload.toBytes()) {
                std::cerr << "packet " << i << ": stored location does not read back the same bytes\n";
                return EXIT_FAILURE;
            }
        }
        std::cout << "Stored ring: same " << storedClip->size()
                  << "-packet clip; every location reads back the Bytes ring's payload\n";
    } catch (const Error& e) {
        std::cerr << "mediaway error: " << e.what() << " (raw status " << e.rawCode() << ")\n";
        return EXIT_FAILURE;
    }
    return EXIT_SUCCESS;
}
