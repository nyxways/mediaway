/*
 * replay_ring.c — keep the last N seconds of packets, save "the last M seconds" as a clip.
 *
 * STATUS: REAL (link+run verified) — calls only the shipped mediaway-ffi ABI, exactly as
 * <mediaway/container.h> declares it (adr/container/0009-replay-ring-c-abi.md).
 *
 * Flow:
 *   1. A BYTES ring keeps 3 s of synthetic packets (30 fps video, a keyframe every second, plus
 *      1024-sample AAC frames). mediaway_replay_ring_clip_last cuts the last 2 s at a keyframe, as an
 *      OWNED snapshot with pts/dts rebased so the cut keyframe decodes at zero.
 *   2. The clip's packets go through an ordinary muxer: the ring never writes a file, and there is
 *      deliberately no "mux this clip" function. The result is re-demuxed to prove it stands alone.
 *   3. STORED variant: a caller that already writes every packet to a file keeps only where each
 *      one landed (mediaway_muxer_create_with_placements + mediaway_muxer_poll_placements ->
 *      mediaway_replay_ring_push_stored) and reads the bytes back itself when it saves a clip.
 *
 * LIMIT: this ABI has no video-packet source. mediaway_encode_session muxes its encoder's packets
 * internally, so this feeds the ring synthetic packets; a real program feeds it packets from a
 * demuxer, from the audio encoder, or from an encoder it drives itself.
 *
 * Build (see bindings/c/README.md "Building & verifying on Windows"):
 *   gcc -Icrates/mediaway-ffi/include bindings/c/examples/container/replay_ring.c \
 *       -Ltarget/x86_64-pc-windows-gnu/debug -lmediaway_ffi -o replay_ring.exe
 */

#include <mediaway/container.h>

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(call)                                                              \
    do {                                                                         \
        mediaway_status_t _st_ = (call);                                         \
        if (_st_ != MEDIAWAY_OK) {                                               \
            fprintf(stderr, "CHECK failed: %s -> mediaway_status_t %d (%s:%d)\n", \
                    #call, (int)_st_, __FILE__, __LINE__);                       \
            exit(EXIT_FAILURE);                                                  \
        }                                                                        \
    } while (0)

#define VIDEO 0 /* stream ids double as muxer track ids here */
#define AUDIO 1
#define SECONDS 6
#define VIDEO_FRAMES (SECONDS * 30)
#define MAX_PACKETS 1024
#define VIDEO_LEN 64
#define AUDIO_LEN 32

/* One synthetic packet: a video frame or an AAC frame, with its payload. */
typedef struct {
    mediaway_packet_meta_t meta;
    uint8_t payload[VIDEO_LEN];
    size_t payload_len;
} synth_t;

/* Fill `out` with SECONDS of interleaved video + audio in decode order; return the count. */
static size_t produce(synth_t *out) {
    size_t n = 0;
    int64_t audio = 0;
    for (int64_t f = 0; f < VIDEO_FRAMES; f++) {
        synth_t *v = &out[n++];
        v->meta = (mediaway_packet_meta_t){VIDEO, f, f, 1, f % 30 == 0, false};
        v->payload_len = VIDEO_LEN;
        memset(v->payload, 0xB0 + (int)(f % 64), VIDEO_LEN);
        while (audio * 1024 * 30 <= f * 48000) {
            synth_t *a = &out[n++];
            a->meta = (mediaway_packet_meta_t){AUDIO, audio * 1024, audio * 1024, 1024, true, false};
            a->payload_len = AUDIO_LEN;
            memset(a->payload, 0xA0, AUDIO_LEN);
            audio++;
        }
    }
    return n;
}

static mediaway_packet_view_t view_of(const synth_t *p) {
    mediaway_packet_view_t v = {
        .stream_id = p->meta.stream_id,
        .pts = p->meta.pts,
        .dts = p->meta.dts,
        .duration = p->meta.duration,
        .is_keyframe = p->meta.is_keyframe,
        .is_discard = false,
        .payload = p->payload,
        .payload_len = p->payload_len,
    };
    return v;
}

static mediaway_muxer_t *begin_muxer(mediaway_muxer_t *muxer) {
    if (muxer == NULL) {
        fprintf(stderr, "muxer creation returned NULL (caught panic)\n");
        exit(EXIT_FAILURE);
    }
    mediaway_video_track_info_t video = {.id = VIDEO, .codec = MEDIAWAY_CODEC_H264,
                                         .time_base = {1, 30}, .width = 640, .height = 480};
    mediaway_audio_track_info_t audio = {.id = AUDIO, .codec = MEDIAWAY_CODEC_AAC,
                                         .time_base = {1, 48000}, .sample_rate = 48000, .channels = 2};
    CHECK(mediaway_muxer_add_video_track(muxer, &video));
    CHECK(mediaway_muxer_add_audio_track(muxer, &audio));
    CHECK(mediaway_muxer_begin(muxer));
    return muxer;
}

/* Everything the muxer has produced so far, appended to *buf (caller frees). */
static void drain(mediaway_muxer_t *muxer, uint8_t **buf, size_t *len) {
    for (;;) {
        uint8_t *chunk = NULL;
        size_t chunk_len = 0;
        CHECK(mediaway_muxer_poll_bytes(muxer, &chunk, &chunk_len));
        if (chunk_len == 0) break;
        *buf = (uint8_t *)realloc(*buf, *len + chunk_len);
        memcpy(*buf + *len, chunk, chunk_len);
        *len += chunk_len;
        mediaway_buffer_free(chunk, chunk_len);
    }
}

static mediaway_replay_ring_t *make_ring(mediaway_replay_payload_kind_t kind) {
    mediaway_replay_ring_config_t config = {
        .anchor_stream_id = VIDEO,
        .anchor_time_base = {1, 30},
        .window_ms = 3000,
        .max_bytes = 0, /* no byte ceiling */
        .payload_kind = kind,
    };
    mediaway_replay_ring_t *ring = NULL;
    CHECK(mediaway_replay_ring_create(&config, &ring));
    CHECK(mediaway_replay_ring_add_stream(ring, AUDIO, (mediaway_rational_t){1, 48000}));
    return ring;
}

int main(void) {
    if (mediaway_container_ffi_abi_version() != MEDIAWAY_CONTAINER_FFI_ABI_VERSION) {
        fprintf(stderr, "ABI version mismatch: header %d, library %u\n",
                MEDIAWAY_CONTAINER_FFI_ABI_VERSION, mediaway_container_ffi_abi_version());
        return EXIT_FAILURE;
    }

    static synth_t packets[MAX_PACKETS];
    const size_t total = produce(packets);

    /* ---- 1. BYTES ring: push, then cut the last 2 s ---------------------------------------- */
    mediaway_replay_ring_t *ring = make_ring(MEDIAWAY_REPLAY_PAYLOAD_BYTES);
    for (size_t i = 0; i < total; i++) {
        mediaway_packet_view_t v = view_of(&packets[i]); /* the payload is COPIED into the ring */
        CHECK(mediaway_replay_ring_push(ring, &v));
    }
    uint64_t span_ms = 0;
    CHECK(mediaway_replay_ring_span_ms(ring, &span_ms));

    mediaway_replay_clip_t *clip = NULL;
    bool has_clip = false;
    CHECK(mediaway_replay_ring_clip_last(ring, 2000, &clip, &has_clip));
    if (!has_clip) {
        fprintf(stderr, "no clip: the ring has not seen a keyframe\n");
        return EXIT_FAILURE;
    }
    const size_t clip_len = mediaway_replay_clip_packet_count(clip);
    uint64_t clip_ms = 0;
    CHECK(mediaway_replay_clip_duration_ms(clip, &clip_ms));
    mediaway_replay_clip_entry_t first;
    CHECK(mediaway_replay_clip_packet_at(clip, 0, &first));
    printf("ring holds %llu ms; clip: %zu packets, %llu ms, first dts %lld (keyframe: %s)\n",
           (unsigned long long)span_ms, clip_len, (unsigned long long)clip_ms,
           (long long)first.dts, first.is_keyframe ? "true" : "false");

    /* ---- 2. Write the clip through an ordinary muxer, then re-demux it ---------------------- */
    mediaway_muxer_t *muxer = begin_muxer(mediaway_muxer_create());
    for (size_t i = 0; i < clip_len; i++) {
        mediaway_replay_clip_entry_t e;
        CHECK(mediaway_replay_clip_packet_at(clip, i, &e));
        mediaway_packet_view_t v = {.stream_id = e.stream_id, .pts = e.pts, .dts = e.dts,
                                    .duration = e.duration, .is_keyframe = e.is_keyframe,
                                    .is_discard = e.is_discard,
                                    /* BORROWED from the clip; valid until it is freed */
                                    .payload = e.payload, .payload_len = e.payload_len};
        CHECK(mediaway_muxer_push_packet(muxer, &v));
    }
    CHECK(mediaway_muxer_flush(muxer));
    uint8_t *mp4 = NULL;
    size_t mp4_len = 0;
    drain(muxer, &mp4, &mp4_len);
    mediaway_muxer_close(muxer);

    FILE *f = fopen("replay_clip_c.mp4", "wb");
    if (f != NULL) {
        fwrite(mp4, 1, mp4_len, f);
        fclose(f);
    }

    mediaway_demuxer_t *demuxer = mediaway_demuxer_create();
    CHECK(mediaway_demuxer_push_bytes(demuxer, mp4, mp4_len));
    size_t recovered = 0;
    for (;;) {
        mediaway_packet_t p;
        bool has = false;
        CHECK(mediaway_demuxer_poll_packet(demuxer, &p, &has));
        if (!has) break;
        recovered++;
        mediaway_packet_free(&p);
    }
    mediaway_demuxer_close(demuxer);
    printf("replay_clip_c.mp4: %zu bytes, re-demuxed %zu packets (clip has %zu)\n", mp4_len,
           recovered, clip_len);
    if (recovered != clip_len) {
        fprintf(stderr, "the standalone file does not hold every clip packet\n");
        return EXIT_FAILURE;
    }

    /* ---- 3. STORED ring: keep only where each packet landed --------------------------------- */
    mediaway_muxer_t *recording = begin_muxer(mediaway_muxer_create_with_placements());
    for (size_t i = 0; i < total; i++) {
        mediaway_packet_view_t v = view_of(&packets[i]);
        CHECK(mediaway_muxer_push_packet(recording, &v));
    }
    CHECK(mediaway_muxer_flush(recording));
    uint8_t *disk = NULL; /* stands in for the recording file */
    size_t disk_len = 0;
    drain(recording, &disk, &disk_len);
    mediaway_placement_t *rows = NULL;
    size_t row_count = 0;
    CHECK(mediaway_muxer_poll_placements(recording, &rows, &row_count));
    mediaway_muxer_close(recording);

    mediaway_replay_ring_t *stored = make_ring(MEDIAWAY_REPLAY_PAYLOAD_STORED);
    for (size_t i = 0; i < total; i++) {
        /* Placements are in WRITE order, and a fragment writes its samples grouped by track, not
         * in push order: with more than one track, find each by (track_id, dts). */
        const mediaway_placement_t *at = NULL;
        for (size_t r = 0; r < row_count && at == NULL; r++) {
            if (rows[r].track_id == packets[i].meta.stream_id && rows[r].dts == packets[i].meta.dts) {
                at = &rows[r];
            }
        }
        if (at == NULL) {
            fprintf(stderr, "no placement for packet %zu\n", i);
            return EXIT_FAILURE;
        }
        mediaway_stored_payload_t where = {.file = 0, .offset = at->offset, .len = at->len};
        CHECK(mediaway_replay_ring_push_stored(stored, &packets[i].meta, &where));
    }
    mediaway_placements_free(rows, row_count);

    mediaway_replay_clip_t *stored_clip = NULL;
    CHECK(mediaway_replay_ring_clip_last(stored, 2000, &stored_clip, &has_clip));
    if (!has_clip || mediaway_replay_clip_packet_count(stored_clip) != clip_len) {
        fprintf(stderr, "the STORED ring did not cut the same clip\n");
        return EXIT_FAILURE;
    }
    for (size_t i = 0; i < clip_len; i++) {
        mediaway_replay_clip_entry_t b, s;
        CHECK(mediaway_replay_clip_packet_at(clip, i, &b));
        CHECK(mediaway_replay_clip_packet_at(stored_clip, i, &s));
        if (s.stored_len != b.payload_len ||
            memcmp(disk + s.stored_offset, b.payload, b.payload_len) != 0) {
            fprintf(stderr, "packet %zu: the stored location does not read back the same bytes\n", i);
            return EXIT_FAILURE;
        }
    }
    printf("STORED ring: same %zu-packet clip; every location reads back the BYTES ring's payload\n",
           clip_len);

    mediaway_replay_clip_free(stored_clip);
    mediaway_replay_ring_close(stored);
    mediaway_replay_clip_free(clip);
    mediaway_replay_ring_close(ring);
    free(disk);
    free(mp4);
    return EXIT_SUCCESS;
}
