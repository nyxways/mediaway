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

#define MEDIAWAY_CONTAINER_FFI_ABI_VERSION 5 /* bump on any breaking change; pre-1.0, no stability promise */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "common.h"

#ifdef __cplusplus
extern "C" {
#endif

/* ── Opaque handles ──────────────────────────────────────────────────────────────── */

/* No member list: layout is private to the Rust implementation and may change without
 * notice pre-1.0. Always access through the functions below. */
typedef struct mediaway_muxer mediaway_muxer_t;
typedef struct mediaway_demuxer mediaway_demuxer_t;

/* Dedicated Ogg handles (adr/0004-ogg-adts-c-abi.md) -- NOT reachable through
 * mediaway_muxer_t/mediaway_demuxer_t or mediaway_container_format_t: ogg::Muxer has no
 * track registration and no Open/Live typestate, so it does not fit the shared handles'
 * shape. */
typedef struct mediaway_ogg_muxer mediaway_ogg_muxer_t;
typedef struct mediaway_ogg_demuxer mediaway_ogg_demuxer_t;

/* Dedicated ADTS handles (adr/0004-ogg-adts-c-abi.md) -- same reasoning as the Ogg handles
 * above: adts::Muxer has no track registration and no Open/Live typestate. */
typedef struct mediaway_adts_muxer mediaway_adts_muxer_t;
typedef struct mediaway_adts_demuxer mediaway_adts_demuxer_t;

/* Dedicated FLV handles (adr/0005-flv-c-abi.md) -- flv::Muxer writes directly into a
 * caller-supplied buffer on every call instead of buffering for a separate poll_bytes step,
 * and has a fixed one-video/one-audio track slot instead of caller-assigned track ids. */
typedef struct mediaway_flv_muxer mediaway_flv_muxer_t;
typedef struct mediaway_flv_demuxer mediaway_flv_demuxer_t;

/* Dedicated MPEG-TS handles (adr/0006-mpeg-ts-c-abi.md) -- ts::Muxer::new takes the full
 * elementary stream list upfront (no add_track after construction); write_pat_pmt/
 * write_access_unit both write directly into a caller-supplied buffer with explicit
 * pts_90k/dts_90k clock values (the 90 kHz system clock is not a per-track timebase). */
typedef struct mediaway_ts_muxer mediaway_ts_muxer_t;
typedef struct mediaway_ts_demuxer mediaway_ts_demuxer_t;

/* ── Container format (adr/0003-multi-format-c-abi.md) ───────────────────────────── */

/* Which format mediaway_muxer_create_for_format/mediaway_demuxer_create_for_format open.
 * Only formats sharing MP4's multi-track, typestated (Open -> Live)
 * add_video_track/add_audio_track/begin/push_packet/poll_bytes/flush shape are listed
 * here. Ogg/ADTS (single implicit stream, no track registration) and FLV/MPEG-TS/MP3/WAV
 * (genuinely incompatible method shapes) are not reachable through this enum — see the
 * ADR's "Deferred" section. */
typedef enum mediaway_container_format {
    MEDIAWAY_CONTAINER_FORMAT_MP4 = 0,  /* the only format mediaway_muxer_create/
                                          * mediaway_demuxer_create (no _for_format suffix)
                                          * ever open */
    MEDIAWAY_CONTAINER_FORMAT_WEBM = 1,
} mediaway_container_format_t;

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
    MEDIAWAY_STATUS_UNSUPPORTED_CODEC = 9, /* track's codec has no encoding in the requested format */
    MEDIAWAY_STATUS_UNKNOWN_STREAM = 10,   /* push_packet's stream_id matches no registered track */
} mediaway_status_t;

/* ── Shared value types ──────────────────────────────────────────────────────────── */

/* mediaway_rational_t comes from common.h. */

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
    MEDIAWAY_CODEC_VP8 = 12,
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

/* One elementary stream registered in mediaway_ts_muxer_create's PMT. Input to muxer
 * construction only -- ts::Muxer::new takes the full stream list upfront (adr/0006). */
typedef struct mediaway_ts_elementary_stream {
    uint16_t pid;              /* 2..=0x1FFF; 0/1 reserved for PAT/CAT */
    mediaway_codec_kind_t codec; /* must be H264, HEVC, AAC, or MP3 */
} mediaway_ts_elementary_stream_t;

/* ── ABI version ─────────────────────────────────────────────────────────────────── */

/* Runtime counterpart to MEDIAWAY_CONTAINER_FFI_ABI_VERSION, for consumers that load
 * this library dynamically and never compile against this header. */
uint32_t mediaway_container_ffi_abi_version(void);

/* ── Muxer (requires the `mux` Cargo feature, on by default) ────────────────────── */

/* Create a muxer in the track-registration (Open) state. Returns NULL only on a caught
 * panic during construction (defensive; should not happen in practice). */
mediaway_muxer_t *mediaway_muxer_create(void);

/* Create a muxer in the track-registration (Open) state for `format`. A separate function
 * rather than a parameter on mediaway_muxer_create — adding one to an already-shipped
 * zero-argument function would silently break every existing binding's call at the ABI
 * level, not just source. Returns NULL for an unrecognized format or a caught panic. */
mediaway_muxer_t *mediaway_muxer_create_for_format(mediaway_container_format_t format);

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

/* Create a new, empty demuxer for `format` — see mediaway_muxer_create_for_format's doc
 * comment for why this is a separate function. Returns NULL for an unrecognized format or
 * a caught panic. */
mediaway_demuxer_t *mediaway_demuxer_create_for_format(mediaway_container_format_t format);

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

/* Set the ClearKey decryption key applied to ALL encrypted tracks on this demuxer. MP4
 * only -- a WebM-backed demuxer has no CENC/ClearKey support and returns
 * MEDIAWAY_STATUS_INVALID_STATE (adr/0003-multi-format-c-abi.md). One
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

/* ── Ogg muxer/demuxer (adr/0004-ogg-adts-c-abi.md; requires `mux`/`demux` respectively) ── */

/* Dedicated handles, not mediaway_muxer_t/mediaway_demuxer_t: Ogg has no track-registration
 * step and no Open/Live typestate -- mediaway_ogg_muxer_create is immediately ready for
 * push_packet. Reuses mediaway_packet_view_t/mediaway_packet_t/mediaway_stream_info_t
 * (already codec-agnostic) and mediaway_buffer_free/mediaway_packet_free/
 * mediaway_stream_info_free (same ownership shape as the MP4/WebM handles). */

/* Open a mux session for logical bitstream `serial`. Returns NULL only on a caught panic
 * during construction (defensive; should not happen in practice). */
mediaway_ogg_muxer_t *mediaway_ogg_muxer_create(uint32_t serial);

/* Write one Ogg page containing packet's payload. packet->pts becomes the page's
 * granule_position; packet->is_discard becomes the page's eos flag. Fails with
 * MEDIAWAY_STATUS_INVALID_DATA when the payload exceeds a single Ogg page's capacity (this
 * mux always emits one page per packet). */
mediaway_status_t mediaway_ogg_muxer_push_packet(mediaway_ogg_muxer_t *muxer,
                                                  const mediaway_packet_view_t *packet);

/* No-op -- every push_packet call already wrote a complete, independently valid Ogg page.
 * Exposed for shape parity with mediaway_muxer_flush. */
mediaway_status_t mediaway_ogg_muxer_flush(mediaway_ogg_muxer_t *muxer);

/* Drain whatever muxed Ogg page bytes are ready right now into an owned buffer.
 * *out_data == NULL && *out_len == 0 is a valid "nothing ready" result, not an error.
 * Release the buffer with mediaway_buffer_free. */
mediaway_status_t mediaway_ogg_muxer_poll_bytes(mediaway_ogg_muxer_t *muxer, uint8_t **out_data,
                                                 size_t *out_len);

/* Close and free an Ogg muxer handle. Always safe to call, including on a poisoned handle. */
void mediaway_ogg_muxer_close(mediaway_ogg_muxer_t *muxer);

/* Create a new, empty Ogg demuxer. Returns NULL only on a caught panic during
 * construction. */
mediaway_ogg_demuxer_t *mediaway_ogg_demuxer_create(void);

/* Feed Ogg-container bytes into the demuxer. `data` is a borrowed buffer, valid for the
 * call only. */
mediaway_status_t mediaway_ogg_demuxer_push_bytes(mediaway_ogg_demuxer_t *demuxer,
                                                    const uint8_t *data, size_t len);

/* Number of logical bitstreams discovered so far -- 0 or 1 (Ogg carries a single logical
 * bitstream per this facade). Read-only; returns 0 on a null/poisoned handle or a caught
 * panic. */
size_t mediaway_ogg_demuxer_stream_count(const mediaway_ogg_demuxer_t *demuxer);

/* Get stream info by index (always index 0 once the identification-header packet has been
 * recognized). On success, release *out_info with mediaway_stream_info_free. */
mediaway_status_t mediaway_ogg_demuxer_stream_at(const mediaway_ogg_demuxer_t *demuxer,
                                                  size_t index,
                                                  mediaway_stream_info_t *out_info);

/* Pop the next demuxed packet, if any is ready. *out_has_packet == false is a valid
 * "nothing ready" result, not an error. When true, release *out_packet with
 * mediaway_packet_free. */
mediaway_status_t mediaway_ogg_demuxer_poll_packet(mediaway_ogg_demuxer_t *demuxer,
                                                    mediaway_packet_t *out_packet,
                                                    bool *out_has_packet);

/* Close and free an Ogg demuxer handle. Always safe to call, including on a poisoned
 * handle. */
void mediaway_ogg_demuxer_close(mediaway_ogg_demuxer_t *demuxer);

/* ── ADTS muxer/demuxer (adr/0004-ogg-adts-c-abi.md; requires `mux`/`demux` respectively) ── */

/* Same dedicated-handle reasoning as the Ogg section above: no track-registration step, no
 * Open/Live typestate. Reuses mediaway_packet_view_t/mediaway_packet_t/
 * mediaway_stream_info_t and the shared frees. */

/* Open a mux session for `sample_rate` (must be a standard ADTS rate) / `channels`.
 * Returns NULL for a non-standard sample_rate OR a caught panic during construction --
 * both collapse to NULL; there is no status side channel on this constructor. */
mediaway_adts_muxer_t *mediaway_adts_muxer_create(uint32_t sample_rate, uint8_t channels);

/* Append one AAC frame (raw, ADTS header added) from packet's payload. Fails with
 * MEDIAWAY_STATUS_INVALID_PACKET if the payload is too large for ADTS's 13-bit
 * frame-length field. */
mediaway_status_t mediaway_adts_muxer_push_packet(mediaway_adts_muxer_t *muxer,
                                                   const mediaway_packet_view_t *packet);

/* No-op -- ADTS frames are independently appendable; nothing is buffered beyond what
 * poll_bytes already exposes. Exposed for shape parity with mediaway_muxer_flush. */
mediaway_status_t mediaway_adts_muxer_flush(mediaway_adts_muxer_t *muxer);

/* Drain whatever muxed ADTS bytes are ready right now into an owned buffer.
 * *out_data == NULL && *out_len == 0 is a valid "nothing ready" result, not an error.
 * Release the buffer with mediaway_buffer_free. */
mediaway_status_t mediaway_adts_muxer_poll_bytes(mediaway_adts_muxer_t *muxer,
                                                  uint8_t **out_data, size_t *out_len);

/* Close and free an ADTS muxer handle. Always safe to call, including on a poisoned
 * handle. */
void mediaway_adts_muxer_close(mediaway_adts_muxer_t *muxer);

/* Create a new, empty ADTS demuxer. Returns NULL only on a caught panic during
 * construction. */
mediaway_adts_demuxer_t *mediaway_adts_demuxer_create(void);

/* Feed ADTS elementary-stream bytes into the demuxer. `data` is a borrowed buffer, valid
 * for the call only. */
mediaway_status_t mediaway_adts_demuxer_push_bytes(mediaway_adts_demuxer_t *demuxer,
                                                     const uint8_t *data, size_t len);

/* Number of streams discovered so far -- 0 or 1 (ADTS carries a single implicit stream,
 * recognized once the first frame's header has been parsed). Read-only; returns 0 on a
 * null/poisoned handle or a caught panic. */
size_t mediaway_adts_demuxer_stream_count(const mediaway_adts_demuxer_t *demuxer);

/* Get stream info by index (always index 0 once the first frame has been parsed). On
 * success, release *out_info with mediaway_stream_info_free. */
mediaway_status_t mediaway_adts_demuxer_stream_at(const mediaway_adts_demuxer_t *demuxer,
                                                   size_t index,
                                                   mediaway_stream_info_t *out_info);

/* Pop the next demuxed packet (one AAC frame), if any is ready. pts/duration are
 * synthesized from a running 1024-samples-per-frame count -- ADTS carries no per-frame
 * timing of its own. *out_has_packet == false is a valid "nothing ready" result, not an
 * error. When true, release *out_packet with mediaway_packet_free. */
mediaway_status_t mediaway_adts_demuxer_poll_packet(mediaway_adts_demuxer_t *demuxer,
                                                     mediaway_packet_t *out_packet,
                                                     bool *out_has_packet);

/* Close and free an ADTS demuxer handle. Always safe to call, including on a poisoned
 * handle. */
void mediaway_adts_demuxer_close(mediaway_adts_demuxer_t *demuxer);

/* ── FLV muxer/demuxer (adr/0005-flv-c-abi.md; requires `mux`/`demux` respectively) ─── */

/* Create a new FLV mux session. Call mediaway_flv_muxer_write_header before any tag.
 * Returns NULL only on a caught panic during construction. */
mediaway_flv_muxer_t *mediaway_flv_muxer_create(void);

/* Write the FLV file header, declaring whether audio/video tags follow. Unlike
 * mediaway_muxer_poll_bytes, this writes its output directly rather than buffering
 * internally -- the returned buffer holds exactly the header bytes from this call. Release
 * with mediaway_buffer_free. */
mediaway_status_t mediaway_flv_muxer_write_header(mediaway_flv_muxer_t *muxer, bool has_audio,
                                                   bool has_video, uint8_t **out_data,
                                                   size_t *out_len);

/* Register the video track. FLV has exactly one video slot (no track-id field in the
 * format itself) -- info->id is ignored; video/audio are distinguished by which
 * add_*_track function was called, matching mediaway_flv_demuxer_t's fixed stream ids.
 * Only H264 is a recognized video codec (MEDIAWAY_STATUS_UNSUPPORTED_CODEC otherwise). */
mediaway_status_t mediaway_flv_muxer_add_video_track(mediaway_flv_muxer_t *muxer,
                                                      const mediaway_video_track_info_t *info);

/* Register the audio track. FLV has exactly one audio slot -- same info->id-ignored
 * reasoning as add_video_track. AAC and MP3 are the recognized audio codecs
 * (MEDIAWAY_STATUS_UNSUPPORTED_CODEC otherwise). */
mediaway_status_t mediaway_flv_muxer_add_audio_track(mediaway_flv_muxer_t *muxer,
                                                      const mediaway_audio_track_info_t *info);

/* Mux one packet. Writes the track's sequence-header tag first (once, only for codecs
 * that have one) then the data tag, directly into a freshly allocated output buffer -- no
 * separate poll step. packet->stream_id selects video (0) vs. audio (1) and must have a
 * matching add_*_track call already made, else MEDIAWAY_STATUS_UNKNOWN_STREAM. Release the
 * buffer with mediaway_buffer_free. */
mediaway_status_t mediaway_flv_muxer_push_packet(mediaway_flv_muxer_t *muxer,
                                                  const mediaway_packet_view_t *packet,
                                                  uint8_t **out_data, size_t *out_len);

/* Close and free an FLV muxer handle. Always safe to call, including on a poisoned
 * handle. */
void mediaway_flv_muxer_close(mediaway_flv_muxer_t *muxer);

/* Create a new, empty FLV demuxer. Returns NULL only on a caught panic during
 * construction. */
mediaway_flv_demuxer_t *mediaway_flv_demuxer_create(void);

/* Feed FLV-container bytes into the demuxer. `data` is a borrowed buffer, valid for the
 * call only. */
mediaway_status_t mediaway_flv_demuxer_push_bytes(mediaway_flv_demuxer_t *demuxer,
                                                    const uint8_t *data, size_t len);

/* Number of streams recognized so far -- 0, 1, or 2 (fixed video-then-audio slots).
 * Read-only; returns 0 on a null/poisoned handle or a caught panic. */
size_t mediaway_flv_demuxer_stream_count(const mediaway_flv_demuxer_t *demuxer);

/* Get stream info by index. On success, release *out_info with
 * mediaway_stream_info_free. */
mediaway_status_t mediaway_flv_demuxer_stream_at(const mediaway_flv_demuxer_t *demuxer,
                                                  size_t index,
                                                  mediaway_stream_info_t *out_info);

/* Pop the next demuxed packet, if any is ready. Sequence-header tags (AVC/AAC config)
 * update the matching stream's extra_data internally and are not themselves returned as
 * packets. *out_has_packet == false is a valid "nothing ready" result, not an error. When
 * true, release *out_packet with mediaway_packet_free. */
mediaway_status_t mediaway_flv_demuxer_poll_packet(mediaway_flv_demuxer_t *demuxer,
                                                    mediaway_packet_t *out_packet,
                                                    bool *out_has_packet);

/* Close and free an FLV demuxer handle. Always safe to call, including on a poisoned
 * handle. */
void mediaway_flv_demuxer_close(mediaway_flv_demuxer_t *demuxer);

/* ── MPEG-TS muxer/demuxer (adr/0006-mpeg-ts-c-abi.md; requires `mux`/`demux` respectively) ── */

/* Start a mux session for one program's elementary streams. pmt_pid and every stream's pid
 * must be in 2..=0x1FFF; every stream's codec must be H264/HEVC/AAC/MP3. Returns NULL for
 * an invalid PID, an unsupported codec, or a caught panic during construction -- all three
 * collapse to NULL (no status side channel on this constructor). `streams` is a borrowed
 * array, valid for the call only. */
mediaway_ts_muxer_t *mediaway_ts_muxer_create(uint16_t program_number, uint16_t pmt_pid,
                                               const mediaway_ts_elementary_stream_t *streams,
                                               size_t stream_count);

/* Write PAT + PMT packets into a freshly allocated output buffer. Call once at the start
 * and periodically thereafter -- real players expect PAT/PMT to repeat. Release with
 * mediaway_buffer_free. */
mediaway_status_t mediaway_ts_muxer_write_pat_pmt(mediaway_ts_muxer_t *muxer,
                                                   uint8_t **out_data, size_t *out_len);

/* Packetize one access unit for `pid` into PES + TS packets, written into a freshly
 * allocated output buffer. pts_90k/dts_90k are the real MPEG-TS 90 kHz clock values, not a
 * track's own timebase-relative units; has_dts == false means "no DTS". Release the buffer
 * with mediaway_buffer_free. */
mediaway_status_t mediaway_ts_muxer_write_access_unit(mediaway_ts_muxer_t *muxer, uint16_t pid,
                                                        const uint8_t *data, size_t data_len,
                                                        uint64_t pts_90k, bool has_dts,
                                                        uint64_t dts_90k, bool random_access,
                                                        uint8_t **out_data, size_t *out_len);

/* Close and free an MPEG-TS muxer handle. Always safe to call, including on a poisoned
 * handle. */
void mediaway_ts_muxer_close(mediaway_ts_muxer_t *muxer);

/* Create a new, empty MPEG-TS demuxer. Returns NULL only on a caught panic during
 * construction. */
mediaway_ts_demuxer_t *mediaway_ts_demuxer_create(void);

/* Feed bytes (need not be 188-byte aligned across calls). */
mediaway_status_t mediaway_ts_demuxer_push_bytes(mediaway_ts_demuxer_t *demuxer,
                                                   const uint8_t *data, size_t len);

/* Number of streams whose stream_type maps to a recognized codec (H264/HEVC/AAC/MP3).
 * Empty until poll_packet has actually consumed the PMT packet (lazy PSI parsing).
 * Read-only; returns 0 on a null/poisoned handle or a caught panic. */
size_t mediaway_ts_demuxer_stream_count(const mediaway_ts_demuxer_t *demuxer);

/* Get stream info by index (id is the TS PID). On success, release *out_info with
 * mediaway_stream_info_free. */
mediaway_status_t mediaway_ts_demuxer_stream_at(const mediaway_ts_demuxer_t *demuxer,
                                                 size_t index,
                                                 mediaway_stream_info_t *out_info);

/* Pop the next demuxed packet, if any is ready. A PID with no recognized codec mapping is
 * silently skipped. *out_has_packet == false is a valid "nothing ready" result, not an
 * error. When true, release *out_packet with mediaway_packet_free. */
mediaway_status_t mediaway_ts_demuxer_poll_packet(mediaway_ts_demuxer_t *demuxer,
                                                   mediaway_packet_t *out_packet,
                                                   bool *out_has_packet);

/* Force-emit whatever is still accumulating per PID -- call once at the end of a stream so
 * the very last access unit per PID isn't lost (MPEG-TS only confirms a PES boundary once
 * the next packet on the same PID starts). *out_packets/*out_count describe an owned array
 * (possibly empty), released with mediaway_ts_demuxer_finish_free -- NOT
 * mediaway_packet_free, which only frees one packet, not an array. */
mediaway_status_t mediaway_ts_demuxer_finish(mediaway_ts_demuxer_t *demuxer,
                                              mediaway_packet_t **out_packets,
                                              size_t *out_count);

/* Free an array returned by mediaway_ts_demuxer_finish. */
void mediaway_ts_demuxer_finish_free(mediaway_packet_t *packets, size_t count);

/* Close and free an MPEG-TS demuxer handle. Always safe to call, including on a poisoned
 * handle. */
void mediaway_ts_demuxer_close(mediaway_ts_demuxer_t *demuxer);

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
