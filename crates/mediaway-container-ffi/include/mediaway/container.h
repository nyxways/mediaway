/*
 * container.h — mediaway-container-ffi: C ABI facade over Mediaway's fragmented-MP4
 * mux/demux core (mediaway-container / iso-bmff).
 *
 * Hand-written (not cbindgen-generated) — see adr/0001-mp4-mux-demux-c-abi.md §8.
 * Design rules: docs/spec/c-ffi.md (ADR-0004).
 *
 * Ownership summary (see adr/0001-mp4-mux-demux-c-abi.md §6 for the full rationale):
 *   - Input buffers (mediaway_*_track_info_t.extra_data, mediaway_packet_view_t.payload,
 *     mediaway_demuxer_push_bytes's data) are caller-owned borrows, valid for the
 *     duration of the call only.
 *   - Output buffers (mediaway_muxer_poll_bytes, mediaway_packet_t.payload,
 *     mediaway_stream_info_t.extra_data) are library-owned and MUST be released through
 *     the matching `_free` function.
 *
 * Thread safety: every handle (mediaway_muxer_t*, mediaway_demuxer_t*) is
 * thread-confined by convention, not internally synchronized. A handle may be moved to
 * another thread, but calling two functions on the SAME handle concurrently from
 * different threads without external synchronization is a data race (undefined
 * behavior), not merely wrong output.
 *
 * Some symbols in this header require this crate to have been built with the
 * corresponding Cargo feature (`mux` and/or `demux`, both on by default) — a slim build
 * genuinely does not export the other side's symbols; a link error, not a runtime
 * error, is the result of calling one from a slim build that lacks it.
 */

#ifndef MEDIAWAY_CONTAINER_H
#define MEDIAWAY_CONTAINER_H

#define MEDIAWAY_CONTAINER_FFI_ABI_VERSION 0 /* bump on any breaking change; pre-1.0, no stability promise */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Opaque handles ──────────────────────────────────────────────────────────────── */

/* No member list: layout is private to the Rust implementation and may change without
 * notice pre-1.0. Always access through the functions below. */
typedef struct mediaway_muxer mediaway_muxer_t;
typedef struct mediaway_demuxer mediaway_demuxer_t;

/* ── Status codes ────────────────────────────────────────────────────────────────── */

typedef enum mediaway_status {
    MEDIAWAY_OK = 0,
    MEDIAWAY_STATUS_INVALID_ARGUMENT = 1, /* null pointer, out-of-range index, mismatched ptr/len */
    MEDIAWAY_STATUS_INVALID_STATE = 2,    /* typestate violation: add_track on Live, push/flush/poll on Open */
    MEDIAWAY_STATUS_INVALID_TRACK = 3,    /* invalid or duplicate track id */
    MEDIAWAY_STATUS_INVALID_PACKET = 4,   /* Sample does not match a registered track / bad framing */
    MEDIAWAY_STATUS_INVALID_DATA = 5,     /* truncated or malformed ISOBMFF data */
    MEDIAWAY_STATUS_UNKNOWN_ERROR = 6,    /* reserved for a future error variant */
    MEDIAWAY_STATUS_INTERNAL_PANIC = 7,   /* this call caught a Rust panic; the handle is now poisoned */
    MEDIAWAY_STATUS_HANDLE_POISONED = 8,  /* a previous call already poisoned this handle; call refused */
} mediaway_status_t;

/* ── Shared value types ──────────────────────────────────────────────────────────── */

typedef struct mediaway_rational {
    uint64_t num;
    uint32_t den; /* must be non-zero */
} mediaway_rational_t;

typedef enum mediaway_codec_kind {
    MEDIAWAY_CODEC_H264 = 0,
    MEDIAWAY_CODEC_HEVC = 1,
    MEDIAWAY_CODEC_AV1 = 2,
    MEDIAWAY_CODEC_VP9 = 3,
    MEDIAWAY_CODEC_AAC = 4,
    MEDIAWAY_CODEC_OPUS = 5,
    MEDIAWAY_CODEC_MP3 = 6,
    MEDIAWAY_CODEC_VORBIS = 7,
    MEDIAWAY_CODEC_WEBVTT = 8,
    MEDIAWAY_CODEC_TX3G = 9,
    MEDIAWAY_CODEC_RAW_VIDEO = 10,
    MEDIAWAY_CODEC_RAW_AUDIO = 11,
} mediaway_codec_kind_t; /* pre-1.0 — values may be renumbered */

/* Input to mediaway_muxer_add_video_track — borrowed extra_data, valid for the call only. */
typedef struct mediaway_video_track_info {
    uint32_t id; /* caller-assigned; unique per muxer */
    mediaway_codec_kind_t codec;
    mediaway_rational_t time_base;
    uint32_t width;
    uint32_t height;
    const uint8_t *extra_data; /* borrowed; NULL iff extra_data_len == 0 */
    size_t extra_data_len;
} mediaway_video_track_info_t;

/* Input to mediaway_muxer_add_audio_track — borrowed extra_data, valid for the call only. */
typedef struct mediaway_audio_track_info {
    uint32_t id; /* caller-assigned; unique per muxer */
    mediaway_codec_kind_t codec;
    mediaway_rational_t time_base;
    uint32_t sample_rate;
    uint16_t channels;
    const uint8_t *extra_data; /* borrowed; NULL iff extra_data_len == 0 */
    size_t extra_data_len;
} mediaway_audio_track_info_t;

/* Input to mediaway_muxer_push_packet — borrowed view, no free function. */
typedef struct mediaway_packet_view {
    uint32_t stream_id;
    int64_t pts;
    int64_t dts;
    uint64_t duration;
    bool is_keyframe;
    bool is_discard;
    const uint8_t *payload; /* borrowed; valid for the call only */
    size_t payload_len;
} mediaway_packet_view_t;

/* Output of mediaway_demuxer_poll_packet — owned; release with mediaway_packet_free. */
typedef struct mediaway_packet {
    uint32_t stream_id;
    int64_t pts;
    int64_t dts;
    uint64_t duration;
    bool is_keyframe;
    bool is_discard;
    uint8_t *payload; /* owned */
    size_t payload_len;
} mediaway_packet_t;

/* Output of mediaway_demuxer_stream_at — owned extra_data; release with
 * mediaway_stream_info_free. */
typedef struct mediaway_stream_info {
    uint32_t id;
    mediaway_codec_kind_t codec;
    mediaway_rational_t time_base;
    bool has_geometry;
    uint32_t width;       /* valid only if has_geometry */
    uint32_t height;      /* valid only if has_geometry */
    uint32_t sample_rate; /* 0 if not applicable */
    uint16_t channels;    /* 0 if not applicable */
    uint8_t *extra_data;  /* owned */
    size_t extra_data_len;
} mediaway_stream_info_t;

/* ── ABI version ─────────────────────────────────────────────────────────────────── */

/* Runtime counterpart to MEDIAWAY_CONTAINER_FFI_ABI_VERSION, for consumers that load
 * this library dynamically and never compile against this header. */
uint32_t mediaway_container_ffi_abi_version(void);

/* ── Muxer (requires the `mux` Cargo feature, on by default) ────────────────────── */

/* Create a muxer in the track-registration (Open) state. Returns NULL only on a caught
 * panic during construction (defensive; should not happen in practice). */
mediaway_muxer_t *mediaway_muxer_create(void);

/* Create a muxer in the track-registration (Open) state with a custom samples-per-fragment
 * batch size. batch == 0 is NOT rejected: it is passed straight through to the core, which
 * clamps it to 1 itself, so there is no diagnostic for a caller passing 0 by mistake.
 * Otherwise identical to mediaway_muxer_create: returns NULL only on a caught panic. */
mediaway_muxer_t *mediaway_muxer_create_with_fragment_batch(size_t batch);

/* Register a video track. Fails with MEDIAWAY_STATUS_INVALID_STATE once the muxer has
 * already begun streaming (see mediaway_muxer_begin). */
mediaway_status_t mediaway_muxer_add_video_track(mediaway_muxer_t *muxer,
                                                  const mediaway_video_track_info_t *info);

/* Register an audio (or subtitle) track. Same state rule as add_video_track. */
mediaway_status_t mediaway_muxer_add_audio_track(mediaway_muxer_t *muxer,
                                                  const mediaway_audio_track_info_t *info);

/* Close track registration and start accepting packets. Consumes the Open state;
 * subsequent add_*_track calls fail with MEDIAWAY_STATUS_INVALID_STATE. */
mediaway_status_t mediaway_muxer_begin(mediaway_muxer_t *muxer);

/* Push one packet. Requires the muxer to already be Live (see mediaway_muxer_begin). */
mediaway_status_t mediaway_muxer_push_packet(mediaway_muxer_t *muxer,
                                              const mediaway_packet_view_t *packet);

/* Flush any pending fragments so they become available via mediaway_muxer_poll_bytes. */
mediaway_status_t mediaway_muxer_flush(mediaway_muxer_t *muxer);

/* Drain whatever muxed container bytes are ready right now into an owned buffer.
 * *out_data == NULL && *out_len == 0 is a valid "nothing ready" result, not an error.
 * Release the buffer with mediaway_buffer_free. */
mediaway_status_t mediaway_muxer_poll_bytes(mediaway_muxer_t *muxer, uint8_t **out_data,
                                             size_t *out_len);

/* Close and free a muxer handle. Always safe to call, including on a poisoned handle. */
void mediaway_muxer_close(mediaway_muxer_t *muxer);

/* ── Demuxer (requires the `demux` Cargo feature, on by default) ─────────────────── */

/* Create a new, empty demuxer. Returns NULL only on a caught panic during construction. */
mediaway_demuxer_t *mediaway_demuxer_create(void);

/* Feed container bytes into the demuxer. `data` is a borrowed buffer, valid for the call
 * only; the core copies it synchronously before returning. */
mediaway_status_t mediaway_demuxer_push_bytes(mediaway_demuxer_t *demuxer, const uint8_t *data,
                                               size_t len);

/* Number of streams/tracks discovered so far. Read-only; returns 0 on a null/poisoned
 * handle or a caught panic (no error code available on this read-only call shape). */
size_t mediaway_demuxer_stream_count(const mediaway_demuxer_t *demuxer);

/* Get stream/track info by index (0 <= index < mediaway_demuxer_stream_count(...)).
 * On success, release *out_info with mediaway_stream_info_free. */
mediaway_status_t mediaway_demuxer_stream_at(const mediaway_demuxer_t *demuxer, size_t index,
                                              mediaway_stream_info_t *out_info);

/* Pop the next demuxed packet, if any is ready. *out_has_packet == false is a valid
 * "nothing ready" result, not an error. When true, release *out_packet with
 * mediaway_packet_free. */
mediaway_status_t mediaway_demuxer_poll_packet(mediaway_demuxer_t *demuxer,
                                                mediaway_packet_t *out_packet,
                                                bool *out_has_packet);

/* Set the ClearKey decryption key applied to ALL encrypted tracks on this demuxer. One
 * demuxer-wide key, no per-track/KID scoping: a key that is wrong for even one track in
 * multi-KID content -- or no key set at all -- silently yields garbage or raw-ciphertext
 * sample payloads marked as ordinary (non-discarded) packets, not an error. Timing
 * contract: decrypt runs synchronously inside mediaway_demuxer_push_bytes, so this call
 * only affects samples drained from SUBSEQUENT push_bytes calls; it does not
 * retroactively re-decrypt packets already sitting in the poll queue. `key` is a borrowed
 * buffer, valid for the call only; key_len must be exactly 16, else
 * MEDIAWAY_STATUS_INVALID_ARGUMENT. */
mediaway_status_t mediaway_demuxer_set_decryption_key(mediaway_demuxer_t *demuxer,
                                                       const uint8_t *key, size_t key_len);

/* Clear a previously set ClearKey decryption key. Same timing contract as
 * mediaway_demuxer_set_decryption_key: only affects samples drained from subsequent
 * push_bytes calls. */
mediaway_status_t mediaway_demuxer_clear_decryption_key(mediaway_demuxer_t *demuxer);

/* Close and free a demuxer handle. Always safe to call, including on a poisoned handle. */
void mediaway_demuxer_close(mediaway_demuxer_t *demuxer);

/* ── Shared frees ────────────────────────────────────────────────────────────────── */

/* Free a buffer returned by mediaway_muxer_poll_bytes. Requires the `mux` feature. */
void mediaway_buffer_free(uint8_t *data, size_t len);

/* Free a packet returned by mediaway_demuxer_poll_packet. Nulls payload/payload_len
 * afterward, making a double-free a visible no-op. Requires the `demux` feature. */
void mediaway_packet_free(mediaway_packet_t *packet);

/* Free stream info returned by mediaway_demuxer_stream_at. Nulls extra_data/
 * extra_data_len afterward, making a double-free a visible no-op. Requires the `demux`
 * feature. */
void mediaway_stream_info_free(mediaway_stream_info_t *info);

#ifdef __cplusplus
}
#endif

#endif /* MEDIAWAY_CONTAINER_H */
