// replay_ring.cpp - ReplayRing / ReplayClip / Muxer::withPlacements against the real native
// library (adr/container/0009-replay-ring-c-abi.md).
//
// Hermetic: synthetic packets whose payload names the frame they belong to. No encoder, no
// device, no files. Build (see bindings/cpp/README.md):
//   g++ -std=c++17 -Wall -Wextra -Ibindings/cpp/include -Icrates/mediaway-ffi/include \
//       bindings/cpp/tests/replay_ring.cpp -L<dir with mediaway_ffi.dll> -lmediaway_ffi
//
// It also pins the C struct layouts the wrapper depends on (64-bit only) and the ABI version, so
// a reordered or resized field in container.h fails here instead of corrupting a config.

#include <mediaway/mediaway.hpp>

#include <chrono>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <iostream>
#include <string>
#include <vector>

namespace {

int failures = 0;

#define EXPECT(cond)                                                                        \
    do {                                                                                    \
        if (!(cond)) {                                                                      \
            std::cerr << "FAILED: " #cond " (" << __FILE__ << ":" << __LINE__ << ")\n";     \
            ++failures;                                                                     \
        }                                                                                   \
    } while (0)

// ── Layout pins, derived by compiling a probe against the real container.h with gcc ─────────
#if INTPTR_MAX == INT64_MAX
static_assert(MEDIAWAY_CONTAINER_FFI_ABI_VERSION == 8, "container ABI 8 (replay ring)");
static_assert(sizeof(mediaway_replay_ring_config_t) == 48, "");
static_assert(offsetof(mediaway_replay_ring_config_t, anchor_stream_id) == 0, "");
static_assert(offsetof(mediaway_replay_ring_config_t, anchor_time_base) == 8, "");
static_assert(offsetof(mediaway_replay_ring_config_t, window_ms) == 24, "");
static_assert(offsetof(mediaway_replay_ring_config_t, max_bytes) == 32, "");
static_assert(offsetof(mediaway_replay_ring_config_t, payload_kind) == 40, "");
static_assert(sizeof(mediaway_packet_meta_t) == 40, "");
static_assert(offsetof(mediaway_packet_meta_t, pts) == 8, "");
static_assert(offsetof(mediaway_packet_meta_t, dts) == 16, "");
static_assert(offsetof(mediaway_packet_meta_t, duration) == 24, "");
static_assert(offsetof(mediaway_packet_meta_t, is_keyframe) == 32, "");
static_assert(offsetof(mediaway_packet_meta_t, is_discard) == 33, "");
static_assert(sizeof(mediaway_stored_payload_t) == 24, "");
static_assert(offsetof(mediaway_stored_payload_t, file) == 0, "");
static_assert(offsetof(mediaway_stored_payload_t, offset) == 8, "");
static_assert(offsetof(mediaway_stored_payload_t, len) == 16, "");
static_assert(sizeof(mediaway_replay_clip_entry_t) == 80, "");
static_assert(offsetof(mediaway_replay_clip_entry_t, pts) == 8, "");
static_assert(offsetof(mediaway_replay_clip_entry_t, dts) == 16, "");
static_assert(offsetof(mediaway_replay_clip_entry_t, duration) == 24, "");
static_assert(offsetof(mediaway_replay_clip_entry_t, is_keyframe) == 32, "");
static_assert(offsetof(mediaway_replay_clip_entry_t, is_discard) == 33, "");
static_assert(offsetof(mediaway_replay_clip_entry_t, payload_kind) == 36, "");
static_assert(offsetof(mediaway_replay_clip_entry_t, payload) == 40, "");
static_assert(offsetof(mediaway_replay_clip_entry_t, payload_len) == 48, "");
static_assert(offsetof(mediaway_replay_clip_entry_t, stored_file) == 56, "");
static_assert(offsetof(mediaway_replay_clip_entry_t, stored_offset) == 64, "");
static_assert(offsetof(mediaway_replay_clip_entry_t, stored_len) == 72, "");
static_assert(sizeof(mediaway_placement_t) == 32, "");
static_assert(offsetof(mediaway_placement_t, dts) == 8, "");
static_assert(offsetof(mediaway_placement_t, offset) == 16, "");
static_assert(offsetof(mediaway_placement_t, len) == 24, "");
#endif
static_assert(sizeof(mediaway_replay_payload_kind_t) == 4, "");

// The C++ enum mirrors the header's values 1:1.
using mediaway::container::ReplayPayloadKind;
static_assert(static_cast<int>(ReplayPayloadKind::Bytes) == MEDIAWAY_REPLAY_PAYLOAD_BYTES, "");
static_assert(static_cast<int>(ReplayPayloadKind::Stored) == MEDIAWAY_REPLAY_PAYLOAD_STORED, "");

using mediaway::Bytes;
using mediaway::Packet;
using mediaway::Rational;
using mediaway::Status;
using namespace mediaway::container;
using std::chrono::milliseconds;

constexpr mediaway::TrackId kVideo = 1;  // a Muxer numbers its tracks from 1
constexpr mediaway::TrackId kAudio = 2;
constexpr Rational kFps{1, 30};
constexpr Rational kAudioRate{1, 48000};

// The byte a frame's payload is made of. 0xB0.. keeps it clear of an Annex-B start code, so the
// muxer writes the payload unchanged and a placement's len equals the pushed length.
std::uint8_t frameByte(std::int64_t n) { return static_cast<std::uint8_t>(0xB0 + n % 64); }

Packet videoPacket(std::int64_t n) {
    return Packet{kVideo, n, n, n % 30 == 0, Bytes(6, frameByte(n))};
}

Packet audioPacket(std::int64_t k) {
    return Packet{kAudio, k * 1024, k * 1024, true, Bytes(3, 0xA0)};
}

ReplayRingConfig config(ReplayPayloadKind kind, int windowMs) {
    ReplayRingConfig c{kVideo, kFps, milliseconds(windowMs)};
    c.payloadKind = kind;
    return c;
}

// Runs `body`, which must throw a mediaway::Error carrying `rawCode`.
template <typename F>
bool throwsWithRaw(F&& body, mediaway::Status status, int rawCode) {
    try {
        body();
    } catch (const mediaway::Error& e) {
        return e.status() == status && e.rawCode() == rawCode;
    }
    return false;
}

void testClipStartsAtKeyframeRebasedToZero() {
    ReplayRing ring(config(ReplayPayloadKind::Bytes, 2000));
    ring.addStream(kAudio, kAudioRate);
    for (std::int64_t n = 0; n < 180; ++n) {  // 6 s of video with matching audio
        ring.push(videoPacket(n), 1);
        if (n % 2 == 0) ring.push(audioPacket(n / 2 * 3), 1024);
    }
    EXPECT(ring.span() >= milliseconds(2000));

    auto clip = ring.clipLast(milliseconds(2000));
    EXPECT(clip.has_value());
    if (!clip) return;
    EXPECT(clip->size() > 0);
    EXPECT(clip->duration() >= milliseconds(2000));

    std::int64_t videoCount = 0;
    bool sawAudio = false;
    std::int64_t previousVideoDts = -1;
    for (const ReplayEntry e : *clip) {
        if (e.trackId == kAudio) {
            sawAudio = true;
            continue;
        }
        if (videoCount == 0) {
            EXPECT(e.keyframe);
            EXPECT(e.pts == 0 && e.dts == 0);
        } else {
            EXPECT(e.dts == previousVideoDts + 1);
        }
        previousVideoDts = e.dts;
        ++videoCount;
    }
    EXPECT(sawAudio);

    // The clip is the tail of the stream, and each payload is the frame it claims to be.
    const std::int64_t cut = 180 - videoCount;
    EXPECT(cut % 30 == 0);
    for (const ReplayEntry e : *clip) {
        if (e.trackId != kVideo) continue;
        EXPECT(e.kind == ReplayPayloadKind::Bytes);
        EXPECT(e.payload.size() == 6);
        EXPECT(e.payload.size() == 6 && e.payload[0] == frameByte(cut + e.dts));
        const Packet p = e.toPacket();
        EXPECT(p.data == videoPacket(cut + e.dts).data);
    }
}

void testNoClipBeforeAKeyframe() {
    ReplayRing ring(config(ReplayPayloadKind::Bytes, 1000));
    ring.push(videoPacket(5));  // not a keyframe: nothing can be decoded from it, dropped silently
    EXPECT(!ring.clipLast(milliseconds(1000)).has_value());
    EXPECT(ring.span() == milliseconds(0));
}

// The Rust Clip borrows the ring. This one must not: pushes that evict, and destroying the ring,
// leave a clip that was already taken intact.
void testClipIsAnOwnedSnapshot() {
    std::vector<std::pair<std::int64_t, Bytes>> before;
    std::optional<ReplayClip> clip;
    {
        ReplayRing ring(config(ReplayPayloadKind::Bytes, 1000));
        for (std::int64_t n = 0; n < 90; ++n) ring.push(videoPacket(n));
        clip = ring.clipLast(milliseconds(1000));
        EXPECT(clip.has_value());
        if (!clip) return;
        for (const ReplayEntry e : *clip) before.emplace_back(e.dts, e.payload.toBytes());
        for (std::int64_t n = 90; n < 390; ++n) ring.push(videoPacket(n));  // evicts what was cut
    }  // ring destroyed here
    std::vector<std::pair<std::int64_t, Bytes>> after;
    for (const ReplayEntry e : *clip) after.emplace_back(e.dts, e.payload.toBytes());
    EXPECT(!before.empty());
    EXPECT(before == after);
}

void testStoredRingReturnsLocations() {
    ReplayRing ring(config(ReplayPayloadKind::Stored, 2000));
    for (std::int64_t n = 0; n < 100; ++n) {
        ReplayPacketMeta meta{kVideo, n, n, 1, n % 30 == 0};
        ring.pushStored(meta, StoredPayload{7, static_cast<std::uint64_t>(n) * 100, 40});
    }
    auto clip = ring.clipLast(milliseconds(1000));
    EXPECT(clip.has_value());
    if (!clip) return;
    const std::int64_t cut = 99 - clip->at(clip->size() - 1).dts;
    for (const ReplayEntry e : *clip) {
        EXPECT(e.kind == ReplayPayloadKind::Stored);
        EXPECT(e.payload.empty());
        EXPECT(e.stored.file == 7 && e.stored.len == 40);
        EXPECT(e.stored.offset == static_cast<std::uint64_t>(cut + e.dts) * 100);
        EXPECT(throwsWithRaw([&] { e.toPacket(); }, Status::InvalidState, MEDIAWAY_STATUS_INVALID_STATE));
    }
}

void testErrorsAreDistinct() {
    // Wrong payload kind, both ways: InvalidState.
    ReplayRing bytesRing(config(ReplayPayloadKind::Bytes, 1000));
    ReplayRing storedRing(config(ReplayPayloadKind::Stored, 1000));
    EXPECT(throwsWithRaw([&] { storedRing.push(videoPacket(0)); }, Status::InvalidState,
                         MEDIAWAY_STATUS_INVALID_STATE));
    EXPECT(throwsWithRaw(
        [&] { bytesRing.pushStored(ReplayPacketMeta{kVideo, 0, 0}, StoredPayload{0, 0, 1}); },
        Status::InvalidState, MEDIAWAY_STATUS_INVALID_STATE));

    // dts went backwards: routine, so tryPush says so without throwing, push throws.
    bytesRing.push(videoPacket(0));
    bytesRing.push(videoPacket(10));
    EXPECT(!bytesRing.tryPush(videoPacket(4)));
    EXPECT(throwsWithRaw([&] { bytesRing.push(videoPacket(4)); }, Status::MuxError,
                         MEDIAWAY_STATUS_INVALID_PACKET));
    EXPECT(bytesRing.tryPush(videoPacket(11)));  // and the ring carries on

    // A stream nobody added; the anchor added twice; a degenerate timebase.
    EXPECT(throwsWithRaw([&] { bytesRing.push(audioPacket(0)); }, Status::MuxError,
                         MEDIAWAY_STATUS_UNKNOWN_STREAM));
    EXPECT(throwsWithRaw([&] { bytesRing.addStream(kVideo, kFps); }, Status::MuxError,
                         MEDIAWAY_STATUS_INVALID_TRACK));
    EXPECT(throwsWithRaw([&] { bytesRing.addStream(kAudio, Rational{0, 1}); },
                         Status::InvalidArgument, MEDIAWAY_STATUS_INVALID_ARGUMENT));
    ReplayRingConfig bad = config(ReplayPayloadKind::Bytes, 1000);
    bad.anchorTimeBase = Rational{1, 0};
    EXPECT(throwsWithRaw([&] { ReplayRing r(bad); }, Status::InvalidArgument,
                         MEDIAWAY_STATUS_INVALID_ARGUMENT));

    // An out-of-range clip index.
    auto clip = bytesRing.clipLast(milliseconds(1000));
    EXPECT(clip.has_value());
    if (clip) {
        EXPECT(throwsWithRaw([&] { clip->at(9999); }, Status::InvalidArgument,
                             MEDIAWAY_STATUS_INVALID_ARGUMENT));
    }
}

// The muxer side: placements point at the real bytes, and recording them changes nothing.
Bytes muxAll(LiveMuxer& live, std::vector<Placement>* placements) {
    for (std::int64_t n = 0; n < 40; ++n) live.pushPacket(videoPacket(n));
    live.flush();
    Bytes file;
    for (Bytes chunk = live.pollBytes(); !chunk.empty(); chunk = live.pollBytes()) {
        file.insert(file.end(), chunk.begin(), chunk.end());
    }
    if (placements) *placements = live.pollPlacements();
    return file;
}

mediaway::VideoStreamInfo videoInfo() {
    return mediaway::VideoStreamInfo{0, mediaway::Codec::H264, kFps, 64, 64, {}};
}

void testPlacementsPointAtTheRealBytes() {
    Muxer recording = Muxer::withPlacements();
    EXPECT(recording.addVideoTrack(videoInfo()) == kVideo);
    LiveMuxer live = std::move(recording).begin();
    std::vector<Placement> placements;
    const Bytes withPlacements = muxAll(live, &placements);

    Muxer plain;
    plain.addVideoTrack(videoInfo());
    LiveMuxer plainLive = std::move(plain).begin();
    const Bytes withoutPlacements = muxAll(plainLive, nullptr);

    EXPECT(withPlacements == withoutPlacements);  // recording placements does not change the output
    EXPECT(placements.size() == 40);
    for (std::size_t i = 0; i < placements.size() && i < 40; ++i) {
        const Placement& p = placements[i];
        EXPECT(p.trackId == kVideo);
        EXPECT(p.dts == static_cast<std::int64_t>(i));
        EXPECT(p.len == 6);
        EXPECT(p.offset + p.len <= withPlacements.size());
        if (p.offset + p.len <= withPlacements.size()) {
            const Bytes at(withPlacements.begin() + static_cast<std::ptrdiff_t>(p.offset),
                           withPlacements.begin() + static_cast<std::ptrdiff_t>(p.offset + p.len));
            EXPECT(at == videoPacket(static_cast<std::int64_t>(i)).data);
        }
    }
}

// The documented contract (container.h, ADR-0009 §4): a WebM muxer never records, so it answers
// INVALID_STATE; a plain MP4 muxer, one not created with placements, records nothing and returns an
// empty array, the same answer the Rust API gives (indistinguishable from "nothing new yet").
void testNonRecordingMuxerPlacements() {
    Muxer plain;
    plain.addVideoTrack(videoInfo());
    LiveMuxer plainLive = std::move(plain).begin();
    EXPECT(plainLive.pollPlacements().empty());

    Muxer webm(Format::Webm);
    webm.addVideoTrack(mediaway::VideoStreamInfo{0, mediaway::Codec::Vp8, kFps, 64, 64, {}});
    LiveMuxer webmLive = std::move(webm).begin();
    EXPECT(throwsWithRaw([&] { webmLive.pollPlacements(); }, Status::InvalidState,
                         MEDIAWAY_STATUS_INVALID_STATE));
}

// Runs one test and reports whether it added any failure.
void run(const char* name, void (*test)()) {
    const int before = failures;
    test();
    std::cout << (failures == before ? "ok      " : "FAILED  ") << name << '\n';
}

}  // namespace

int main() {
    try {
        EXPECT(mediaway_container_ffi_abi_version() == 8);
        run("testClipStartsAtKeyframeRebasedToZero", testClipStartsAtKeyframeRebasedToZero);
        run("testNoClipBeforeAKeyframe", testNoClipBeforeAKeyframe);
        run("testClipIsAnOwnedSnapshot", testClipIsAnOwnedSnapshot);
        run("testStoredRingReturnsLocations", testStoredRingReturnsLocations);
        run("testErrorsAreDistinct", testErrorsAreDistinct);
        run("testPlacementsPointAtTheRealBytes", testPlacementsPointAtTheRealBytes);
        run("testNonRecordingMuxerPlacements", testNonRecordingMuxerPlacements);
    } catch (const mediaway::Error& e) {
        std::cerr << "unexpected mediaway::Error: " << e.what() << " (raw " << e.rawCode() << ")\n";
        return EXIT_FAILURE;
    }
    if (failures != 0) {
        std::cerr << failures << " expectation(s) failed\n";
        return EXIT_FAILURE;
    }
    std::cout << "replay_ring: ok\n";
    return EXIT_SUCCESS;
}
