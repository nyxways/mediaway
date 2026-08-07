/*
 * ts.hpp — TsMuxer/TsDemuxer over the dedicated mediaway_ts_muxer_t/
 * mediaway_ts_demuxer_t handles (adr/container/0006-mpeg-ts-c-abi.md):
 * elementary streams are registered at muxer construction (no add_track
 * after), writeAccessUnit takes raw 90 kHz pts/dts, and TsDemuxer::finish()
 * returns an owned array of packets — the only multi-packet demux call in
 * this crate.
 */

#ifndef MEDIAWAY_CONTAINER_TS_HPP
#define MEDIAWAY_CONTAINER_TS_HPP

#include <mediaway/container/detail.hpp>
#include <mediaway/core.hpp>

#include <memory>
#include <optional>
#include <vector>

namespace mediaway {
namespace container {

/// One elementary stream registered in TsMuxer's PMT at construction.
struct ElementaryStream {
    std::uint16_t pid;  // 2..=0x1FFF; 0/1 reserved for PAT/CAT
    Codec codec;         // must be H264, Hevc, Aac, or Mp3
};

/// A live MPEG-TS mux session for one program's elementary streams. Unlike
/// every other muxer class, the stream list is fixed at construction — there
/// is no add_track step at all.
class TsMuxer {
public:
    /// Throws Error(Status::InvalidArgument) for an invalid PID or unsupported
    /// codec — the ABI constructor has no status side channel, so a bad
    /// input and a caught panic collapse to the same NULL.
    TsMuxer(std::uint16_t programNumber, std::uint16_t pmtPid,
            const std::vector<ElementaryStream>& streams)
        : handle_(createHandle(programNumber, pmtPid, streams), &mediaway_ts_muxer_close) {
        if (!handle_) {
            detail::throwError(Status::InvalidArgument, MEDIAWAY_STATUS_INVALID_ARGUMENT,
                               "invalid PID/codec in elementary stream list, or muxer "
                               "creation panicked");
        }
    }
    ~TsMuxer() = default;
    TsMuxer(TsMuxer&&) = default;
    TsMuxer& operator=(TsMuxer&&) = default;
    TsMuxer(const TsMuxer&) = delete;
    TsMuxer& operator=(const TsMuxer&) = delete;

    /// Write PAT + PMT packets. Call once at the start and periodically
    /// thereafter — real players expect PAT/PMT to repeat.
    Bytes writePatPmt() {
        std::uint8_t* data = nullptr;
        std::size_t len = 0;
        detail::checkContainer(mediaway_ts_muxer_write_pat_pmt(handle_.get(), &data, &len));
        if (len == 0) return {};
        Bytes out(data, data + len);
        mediaway_buffer_free(data, len);
        return out;
    }

    /// Packetize one access unit for `pid` into PES + TS packets. `pts90k`/
    /// `dts90k` are the real MPEG-TS 90 kHz clock values, not a track's own
    /// timebase — `dts90k = std::nullopt` means "no DTS" (video commonly
    /// omits it when PTS == DTS).
    Bytes writeAccessUnit(std::uint16_t pid, const Bytes& data, std::uint64_t pts90k,
                          std::optional<std::uint64_t> dts90k, bool randomAccess) {
        std::uint8_t* out = nullptr;
        std::size_t len = 0;
        detail::checkContainer(mediaway_ts_muxer_write_access_unit(
            handle_.get(), pid, data.empty() ? nullptr : data.data(), data.size(), pts90k,
            dts90k.has_value(), dts90k.value_or(0), randomAccess, &out, &len));
        if (len == 0) return {};
        Bytes result(out, out + len);
        mediaway_buffer_free(out, len);
        return result;
    }

private:
    static mediaway_ts_muxer_t* createHandle(std::uint16_t programNumber, std::uint16_t pmtPid,
                                             const std::vector<ElementaryStream>& streams) {
        std::vector<mediaway_ts_elementary_stream_t> raw;
        raw.reserve(streams.size());
        for (const auto& s : streams) {
            mediaway_ts_elementary_stream_t r{};
            r.pid = s.pid;
            r.codec = static_cast<mediaway_codec_kind_t>(detail::toAbiCodec(s.codec));
            raw.push_back(r);
        }
        return mediaway_ts_muxer_create(programNumber, pmtPid, raw.data(), raw.size());
    }

    std::unique_ptr<mediaway_ts_muxer_t, void (*)(mediaway_ts_muxer_t*)> handle_;
};

/// A streaming MPEG-TS demuxer: feed bytes (need not be 188-byte aligned
/// across calls), poll recognized streams (H264/HEVC video, AAC/MP3 audio)
/// and packets.
class TsDemuxer {
public:
    TsDemuxer() : handle_(mediaway_ts_demuxer_create(), &mediaway_ts_demuxer_close) {
        if (!handle_) {
            detail::throwError(Status::Panic, MEDIAWAY_STATUS_INTERNAL_PANIC,
                               "MPEG-TS demuxer creation panicked");
        }
    }
    ~TsDemuxer() = default;
    TsDemuxer(TsDemuxer&&) = default;
    TsDemuxer& operator=(TsDemuxer&&) = default;
    TsDemuxer(const TsDemuxer&) = delete;
    TsDemuxer& operator=(const TsDemuxer&) = delete;

    void pushBytes(const Bytes& bytes) {
        detail::checkContainer(mediaway_ts_demuxer_push_bytes(handle_.get(), bytes.data(), bytes.size()));
    }

    /// Streams whose stream_type maps to a recognized codec; `id` is the TS
    /// PID. Empty until pollPacket() has actually consumed the PMT packet
    /// (this crate parses PAT/PMT lazily).
    std::vector<StreamInfo> streams() const {
        std::vector<StreamInfo> out;
        const std::size_t count = mediaway_ts_demuxer_stream_count(handle_.get());
        out.reserve(count);
        for (std::size_t i = 0; i < count; ++i) {
            mediaway_stream_info_t raw{};
            detail::checkContainer(mediaway_ts_demuxer_stream_at(handle_.get(), i, &raw));
            Bytes extra(raw.extra_data, raw.extra_data + raw.extra_data_len);
            mediaway_stream_info_free(&raw);
            out.emplace_back(detail::toStreamInfo(raw, std::move(extra)));
        }
        return out;
    }

    /// The next demuxed packet, if any is ready. A PID with no recognized
    /// codec mapping is silently skipped.
    std::optional<Packet> pollPacket() {
        mediaway_packet_t raw{};
        bool has = false;
        detail::checkContainer(mediaway_ts_demuxer_poll_packet(handle_.get(), &raw, &has));
        if (!has) return std::nullopt;
        Bytes payload(raw.payload, raw.payload + raw.payload_len);
        mediaway_packet_free(&raw);
        return detail::toPacket(raw, std::move(payload));
    }

    /// Force-emit whatever is still accumulating per PID — call once at the
    /// end of a stream so the very last access unit per PID isn't lost (PES
    /// boundaries are only confirmed once the *next* packet on the same PID
    /// starts). Unlike pollPacket(), this can return more than one packet.
    std::vector<Packet> finish() {
        mediaway_packet_t* packets = nullptr;
        std::size_t count = 0;
        detail::checkContainer(mediaway_ts_demuxer_finish(handle_.get(), &packets, &count));
        std::vector<Packet> out;
        out.reserve(count);
        for (std::size_t i = 0; i < count; ++i) {
            Bytes payload(packets[i].payload, packets[i].payload + packets[i].payload_len);
            out.push_back(detail::toPacket(packets[i], std::move(payload)));
        }
        mediaway_ts_demuxer_finish_free(packets, count);
        return out;
    }

private:
    std::unique_ptr<mediaway_ts_demuxer_t, void (*)(mediaway_ts_demuxer_t*)> handle_;
};

}  // namespace container
}  // namespace mediaway

#endif  // MEDIAWAY_CONTAINER_TS_HPP
