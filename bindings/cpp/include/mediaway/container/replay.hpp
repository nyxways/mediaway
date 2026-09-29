/*
 * replay.hpp — ReplayRing / ReplayClip over the replay-ring C ABI
 * (crates/mediaway-ffi/adr/container/0009-replay-ring-c-abi.md).
 *
 * A rolling buffer of the last N milliseconds of encoded packets that cuts "the last M
 * milliseconds" at a keyframe, in decode order, as a packet sequence rebased to zero. Write a
 * clip out by pushing its entries through a container::Muxer (one addVideoTrack /
 * addAudioTrack per stream, then LiveMuxer::pushPacket per entry): the ring never writes a
 * file itself, and there is deliberately no "mux this clip" function.
 *
 * LIMIT: the C ABI has no video-packet source. encoder::EncodeSession muxes its encoder's packets
 * internally, and packet-level output exists only for the audio encoder. Feed the ring packets
 * from a Demuxer, from an AudioEncoder, or from an encoder you drive yourself.
 *
 * Errors are mediaway::Error. INVALID_STATE (a push of the wrong payload kind) is
 * Status::InvalidState; an unknown stream, a duplicate stream and a dts that went backwards are
 * Status::MuxError, told apart by rawCode() (MEDIAWAY_STATUS_UNKNOWN_STREAM,
 * MEDIAWAY_STATUS_INVALID_TRACK, MEDIAWAY_STATUS_INVALID_PACKET). An out-of-order dts is routine —
 * drop that packet and carry on — so tryPush()/tryPushStored() return false for it instead of
 * throwing.
 */

#ifndef MEDIAWAY_CONTAINER_REPLAY_HPP
#define MEDIAWAY_CONTAINER_REPLAY_HPP

#include <mediaway/container/detail.hpp>
#include <mediaway/core.hpp>

#include <chrono>
#include <cstddef>
#include <cstdint>
#include <iterator>
#include <memory>
#include <optional>

namespace mediaway {
namespace container {

/// What a ring holds for each packet.
enum class ReplayPayloadKind : std::uint32_t {
    /// The payload bytes: copied in by ReplayRing::push, shared by every clip by reference count.
    Bytes = MEDIAWAY_REPLAY_PAYLOAD_BYTES,
    /// Only where the bytes are (ReplayRing::pushStored): the caller keeps them in a file it
    /// already writes, and reads them back itself when it saves a clip.
    Stored = MEDIAWAY_REPLAY_PAYLOAD_STORED,
};

struct ReplayRingConfig {
    /// The stream whose keyframes decide where a clip may start (normally video).
    TrackId anchorStreamId;
    Rational anchorTimeBase;
    /// How much history to keep.
    std::chrono::milliseconds window;
    /// Evict oldest-GOP-first while more than this many payload bytes are held; 0 = no ceiling.
    /// The newest GOP is always kept, so one larger GOP can exceed it.
    std::uint64_t maxBytes = 0;
    ReplayPayloadKind payloadKind = ReplayPayloadKind::Bytes;
};

/// Where a packet's payload was stored on the caller's disk.
struct StoredPayload {
    std::uint32_t file;    ///< the caller's own id for the file the bytes are in
    std::uint64_t offset;  ///< byte offset of the payload in that file
    std::uint32_t len;     ///< payload length in bytes
};

/// A packet's metadata without its payload, the input to ReplayRing::pushStored.
struct ReplayPacketMeta {
    TrackId trackId;
    std::int64_t pts;
    std::int64_t dts;  ///< must not go backwards within a stream
    std::uint64_t duration = 0;
    bool keyframe = false;
    bool discard = false;
};

/// A non-owning view of bytes (C++17 has no std::span). Borrowed: see ReplayEntry::payload.
class ByteView {
public:
    ByteView() = default;
    ByteView(const std::uint8_t* data, std::size_t size) : data_(data), size_(size) {}

    const std::uint8_t* data() const noexcept { return data_; }
    std::size_t size() const noexcept { return size_; }
    bool empty() const noexcept { return size_ == 0; }
    const std::uint8_t* begin() const noexcept { return data_; }
    const std::uint8_t* end() const noexcept { return data_ + size_; }
    const std::uint8_t& operator[](std::size_t i) const noexcept { return data_[i]; }
    /// An owning copy.
    Bytes toBytes() const { return Bytes(begin(), end()); }

private:
    const std::uint8_t* data_ = nullptr;
    std::size_t size_ = 0;
};

/// One packet of a clip, with pts/dts rebased so the cut keyframe decodes at zero.
struct ReplayEntry {
    TrackId trackId;
    std::int64_t pts;
    std::int64_t dts;
    std::uint64_t duration;
    bool keyframe;
    bool discard;
    ReplayPayloadKind kind;
    /// Bytes ring only: BORROWED from the clip, valid until the ReplayClip is destroyed (or
    /// moved from). Empty for a Stored ring.
    ByteView payload;
    /// Stored ring only: where the bytes are. Zeroed for a Bytes ring.
    StoredPayload stored;

    /// A Packet (an owning copy of the payload) for LiveMuxer::pushPacket. Bytes ring only.
    /// `trackId` is kept as pushed, so push into the ring with the ids your muxer assigned
    /// (Muxer numbers tracks from 1 in registration order).
    Packet toPacket() const {
        if (kind != ReplayPayloadKind::Bytes) {
            detail::throwError(Status::InvalidState, MEDIAWAY_STATUS_INVALID_STATE,
                               "a Stored clip entry carries no payload bytes; read them from its file");
        }
        return Packet{trackId, pts, dts, keyframe, payload.toBytes()};
    }
};

namespace detail {

/// The replay ring's own messages over the shared container status enum: `INVALID_PACKET` means
/// "dts went backwards" here, not the muxer's "packet matches no track".
inline void checkReplay(mediaway_status_t st) {
    switch (st) {
        case MEDIAWAY_OK: return;
        case MEDIAWAY_STATUS_INVALID_PACKET:
            throwError(Status::MuxError, st, "dts went backwards; packets must arrive in decode order");
        case MEDIAWAY_STATUS_UNKNOWN_STREAM:
            throwError(Status::MuxError, st, "stream was not added to the replay ring");
        case MEDIAWAY_STATUS_INVALID_TRACK:
            throwError(Status::MuxError, st, "stream is already in the replay ring");
        case MEDIAWAY_STATUS_INVALID_STATE:
            throwError(Status::InvalidState, st, "wrong payload kind for this ring (push vs pushStored)");
        case MEDIAWAY_STATUS_INVALID_ARGUMENT:
            throwError(Status::InvalidArgument, st, "invalid argument (e.g. zero timebase)");
        default: checkContainer(st);
    }
}

inline std::chrono::milliseconds toMillis(std::uint64_t ms) {
    return std::chrono::milliseconds(static_cast<std::chrono::milliseconds::rep>(ms));
}

inline std::uint64_t fromMillis(std::chrono::milliseconds ms) {
    return ms.count() < 0 ? 0 : static_cast<std::uint64_t>(ms.count());
}

}  // namespace detail

/// A clip cut from a ReplayRing: an OWNED SNAPSHOT. It stays valid across further pushes and
/// after the ring is destroyed, because it holds its own copy of the metadata and shares the
/// payload bytes by reference count. (The Rust Clip borrows the ring; this one does not.)
class ReplayClip {
public:
    ~ReplayClip() = default;
    ReplayClip(ReplayClip&&) = default;
    ReplayClip& operator=(ReplayClip&&) = default;
    ReplayClip(const ReplayClip&) = delete;
    ReplayClip& operator=(const ReplayClip&) = delete;

    /// Number of packets.
    std::size_t size() const { return mediaway_replay_clip_packet_count(handle_.get()); }
    bool empty() const { return size() == 0; }

    /// From the cut keyframe to the newest packet pushed when the clip was taken.
    std::chrono::milliseconds duration() const {
        std::uint64_t ms = 0;
        detail::checkReplay(mediaway_replay_clip_duration_ms(handle_.get(), &ms));
        return detail::toMillis(ms);
    }

    /// Packet `index`, in decode order across streams. Throws Error(Status::InvalidArgument)
    /// when out of range. For a Bytes ring, `payload` is borrowed from this clip.
    ReplayEntry at(std::size_t index) const {
        mediaway_replay_clip_entry_t raw{};
        detail::checkReplay(mediaway_replay_clip_packet_at(handle_.get(), index, &raw));
        ReplayEntry entry{};
        entry.trackId = raw.stream_id;
        entry.pts = raw.pts;
        entry.dts = raw.dts;
        entry.duration = raw.duration;
        entry.keyframe = raw.is_keyframe;
        entry.discard = raw.is_discard;
        entry.kind = static_cast<ReplayPayloadKind>(raw.payload_kind);
        if (entry.kind == ReplayPayloadKind::Bytes) {
            entry.payload = ByteView(raw.payload, raw.payload_len);
        } else {
            entry.stored = StoredPayload{raw.stored_file, raw.stored_offset, raw.stored_len};
        }
        return entry;
    }
    ReplayEntry operator[](std::size_t index) const { return at(index); }

    /// Forward iteration over entries, in decode order. Entries are produced by value.
    class const_iterator {
    public:
        using iterator_category = std::input_iterator_tag;
        using value_type = ReplayEntry;
        using difference_type = std::ptrdiff_t;
        using pointer = const ReplayEntry*;
        using reference = ReplayEntry;

        struct ArrowProxy {
            ReplayEntry entry;
            const ReplayEntry* operator->() const { return &entry; }
        };

        ReplayEntry operator*() const { return clip_->at(index_); }
        ArrowProxy operator->() const { return ArrowProxy{clip_->at(index_)}; }
        const_iterator& operator++() {
            ++index_;
            return *this;
        }
        const_iterator operator++(int) {
            const_iterator previous = *this;
            ++index_;
            return previous;
        }
        friend bool operator==(const const_iterator& a, const const_iterator& b) {
            return a.clip_ == b.clip_ && a.index_ == b.index_;
        }
        friend bool operator!=(const const_iterator& a, const const_iterator& b) { return !(a == b); }

    private:
        friend class ReplayClip;
        const_iterator(const ReplayClip* clip, std::size_t index) : clip_(clip), index_(index) {}
        const ReplayClip* clip_;
        std::size_t index_;
    };
    const_iterator begin() const { return const_iterator(this, 0); }
    const_iterator end() const { return const_iterator(this, size()); }

private:
    friend class ReplayRing;
    explicit ReplayClip(mediaway_replay_clip_t* handle) : handle_(handle, &mediaway_replay_clip_free) {}
    std::unique_ptr<mediaway_replay_clip_t, void (*)(mediaway_replay_clip_t*)> handle_;
};

/// A replay ring. Thread-confined like every handle: do not call one ring from two threads at once.
class ReplayRing {
public:
    /// Throws Error(Status::InvalidArgument) for a zero timebase numerator or denominator.
    explicit ReplayRing(const ReplayRingConfig& config) : handle_(nullptr, &mediaway_replay_ring_close) {
        mediaway_replay_ring_config_t raw{};
        raw.anchor_stream_id = config.anchorStreamId;
        raw.anchor_time_base = {config.anchorTimeBase.num, config.anchorTimeBase.den};
        raw.window_ms = detail::fromMillis(config.window);
        raw.max_bytes = config.maxBytes;
        raw.payload_kind = static_cast<mediaway_replay_payload_kind_t>(config.payloadKind);
        mediaway_replay_ring_t* ring = nullptr;
        detail::checkReplay(mediaway_replay_ring_create(&raw, &ring));
        handle_.reset(ring);
    }
    ~ReplayRing() = default;
    ReplayRing(ReplayRing&&) = default;
    ReplayRing& operator=(ReplayRing&&) = default;
    ReplayRing(const ReplayRing&) = delete;
    ReplayRing& operator=(const ReplayRing&) = delete;

    /// Carry another stream (e.g. audio), cut by time to match the anchor.
    void addStream(TrackId streamId, Rational timeBase) {
        detail::checkReplay(
            mediaway_replay_ring_add_stream(handle_.get(), streamId, {timeBase.num, timeBase.den}));
    }

    /// Bytes ring: add a packet, then evict what fell out of the window. The payload is COPIED in
    /// (the C ABI borrows it for the call only); every clip then shares that copy by reference
    /// count. Anchor packets before the anchor's first keyframe are dropped without error.
    /// Throws on an unknown stream, on a Stored ring, and when this stream's dts went backwards
    /// (the packet is not added); use tryPush() to treat that last case as routine.
    /// `duration` (in the stream's timebase) is metadata for clip readers; 0 = unknown.
    void push(const Packet& packet, std::uint64_t duration = 0) {
        detail::checkReplay(pushRaw(packet, duration));
    }

    /// push(), but returns false instead of throwing when the packet's dts went backwards
    /// (the packet is not added; carry on).
    bool tryPush(const Packet& packet, std::uint64_t duration = 0) {
        const mediaway_status_t st = pushRaw(packet, duration);
        if (st == MEDIAWAY_STATUS_INVALID_PACKET) return false;
        detail::checkReplay(st);
        return true;
    }

    /// Stored ring: add a packet's metadata and where its bytes are. The ring never reads the
    /// file and holds a few dozen bytes per packet. Take file/offset/len from
    /// LiveMuxer::pollPlacements when you write every polled muxer byte to a file.
    void pushStored(const ReplayPacketMeta& meta, const StoredPayload& stored) {
        detail::checkReplay(pushStoredRaw(meta, stored));
    }

    /// pushStored(), but false (instead of a throw) for a dts that went backwards.
    bool tryPushStored(const ReplayPacketMeta& meta, const StoredPayload& stored) {
        const mediaway_status_t st = pushStoredRaw(meta, stored);
        if (st == MEDIAWAY_STATUS_INVALID_PACKET) return false;
        detail::checkReplay(st);
        return true;
    }

    /// The longest clip clipLast() can return right now; zero before the anchor's first keyframe.
    std::chrono::milliseconds span() {
        std::uint64_t ms = 0;
        detail::checkReplay(mediaway_replay_ring_span_ms(handle_.get(), &ms));
        return detail::toMillis(ms);
    }

    /// Cut the last `span` of every stream at a keyframe: starting at the latest anchor keyframe
    /// at or before `newest - span`, so up to one keyframe interval EARLIER than asked, never
    /// later (with less than `span` held it starts at the oldest keyframe). nullopt until the
    /// anchor's first keyframe has been pushed. The clip is an owned snapshot.
    std::optional<ReplayClip> clipLast(std::chrono::milliseconds span) {
        mediaway_replay_clip_t* clip = nullptr;
        bool has = false;
        detail::checkReplay(
            mediaway_replay_ring_clip_last(handle_.get(), detail::fromMillis(span), &clip, &has));
        if (!has || clip == nullptr) return std::nullopt;
        return ReplayClip(clip);
    }

private:
    mediaway_status_t pushRaw(const Packet& packet, std::uint64_t duration) {
        mediaway_packet_view_t raw{};
        raw.stream_id = packet.trackId;
        raw.pts = packet.pts;
        raw.dts = packet.dts;
        raw.duration = duration;
        raw.is_keyframe = packet.keyframe;
        raw.is_discard = false;
        raw.payload = packet.data.empty() ? nullptr : packet.data.data();
        raw.payload_len = packet.data.size();
        return mediaway_replay_ring_push(handle_.get(), &raw);
    }

    mediaway_status_t pushStoredRaw(const ReplayPacketMeta& meta, const StoredPayload& stored) {
        mediaway_packet_meta_t rawMeta{};
        rawMeta.stream_id = meta.trackId;
        rawMeta.pts = meta.pts;
        rawMeta.dts = meta.dts;
        rawMeta.duration = meta.duration;
        rawMeta.is_keyframe = meta.keyframe;
        rawMeta.is_discard = meta.discard;
        mediaway_stored_payload_t rawStored{};
        rawStored.file = stored.file;
        rawStored.offset = stored.offset;
        rawStored.len = stored.len;
        return mediaway_replay_ring_push_stored(handle_.get(), &rawMeta, &rawStored);
    }

    std::unique_ptr<mediaway_replay_ring_t, void (*)(mediaway_replay_ring_t*)> handle_;
};

}  // namespace container
}  // namespace mediaway

#endif  // MEDIAWAY_CONTAINER_REPLAY_HPP
