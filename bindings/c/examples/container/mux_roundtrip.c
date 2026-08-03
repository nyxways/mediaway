/*
 * mux_roundtrip.c — container mux + demux roundtrip.
 *
 * STATUS: REAL (link+run verified). The scenario table in bindings/c/README.md
 * marks mux_roundtrip "✅ link+run verified" — this file calls only the shipped
 * mediaway-ffi ABI, exactly as <mediaway/container.h> declares it.
 * Nothing here is aspirational.
 *
 * Flow: register an H.264 video track (id 0, time base 1/30 s) and an AAC
 * audio track (id 1, time base 1/48000 s) on a muxer, push 90 synthetic video
 * + 90 synthetic audio packets, flush, drain every muxed fragmented-MP4 byte,
 * then feed those same bytes into a demuxer and verify the packets come back
 * 1:1 (90 video, 90 audio).
 *
 * Build (see bindings/c/README.md "Building & verifying on Windows"):
 *   gcc -Icrates/mediaway-ffi/include bindings/c/examples/mux_roundtrip.c \
 *       -Ltarget/x86_64-pc-windows-gnu/debug -lmediaway_ffi -o mux_roundtrip.exe
 */

#include <mediaway/container.h>

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* CHECK: a call that should only fail on a programming bug — print and abort.
 * Expected conditions (none in this scenario) are handled inline instead. */
#define CHECK(call)                                                              \
    do {                                                                         \
        mediaway_status_t _st_ = (call);                                         \
        if (_st_ != MEDIAWAY_OK) {                                               \
            fprintf(stderr, "CHECK failed: %s -> mediaway_status_t %d (%s:%d)\n", \
                    #call, (int)_st_, __FILE__, __LINE__);                       \
            exit(EXIT_FAILURE);                                                  \
        }                                                                        \
    } while (0)

#define VIDEO_FRAMES 90 /* 90 frames at 1/30 s = 3 s of video */
#define AUDIO_FRAMES 90 /* 90 AAC frames of 1024 samples at 48 kHz */
#define VIDEO_TRACK_ID 0
#define AUDIO_TRACK_ID 1

/* Synthetic payload placeholders: an Annex-B start code for H.264 video and
 * the ADTS sync word for AAC audio. These are stand-ins — the roundtrip only
 * exercises framing and byte transport, not codec validity. */
static const uint8_t video_payload[4] = {0x00, 0x00, 0x00, 0x01};
static const uint8_t audio_payload[2] = {0xff, 0xf1};

int main(void) {
    if (mediaway_container_ffi_abi_version() != MEDIAWAY_CONTAINER_FFI_ABI_VERSION) {
        fprintf(stderr,
                "ABI version mismatch: header %d, library %u — rebuild against "
                "the same mediaway-ffi\n",
                MEDIAWAY_CONTAINER_FFI_ABI_VERSION,
                mediaway_container_ffi_abi_version());
        return EXIT_FAILURE;
    }

    /* ---- Mux side ------------------------------------------------------- */

    mediaway_muxer_t *muxer = mediaway_muxer_create();
    if (muxer == NULL) {
        fprintf(stderr, "mediaway_muxer_create returned NULL (caught panic)\n");
        return EXIT_FAILURE;
    }

    mediaway_video_track_info_t video_track = {
        .id = VIDEO_TRACK_ID,
        .codec = MEDIAWAY_CODEC_H264,
        .time_base = {1, 30}, /* 30 fps */
        .width = 640,
        .height = 480,
        .extra_data = NULL,   /* synthetic stream: no decoder config to carry */
        .extra_data_len = 0,  /* borrowed input; NULL iff len == 0 */
    };
    CHECK(mediaway_muxer_add_video_track(muxer, &video_track));

    mediaway_audio_track_info_t audio_track = {
        .id = AUDIO_TRACK_ID,
        .codec = MEDIAWAY_CODEC_AAC,
        .time_base = {1, 48000}, /* 48 kHz sample clock */
        .sample_rate = 48000,
        .channels = 2,
        .extra_data = NULL,
        .extra_data_len = 0,
    };
    CHECK(mediaway_muxer_add_audio_track(muxer, &audio_track));

    CHECK(mediaway_muxer_begin(muxer)); /* closes track registration; muxer is now Live */

    /* 90 synthetic video packets: one 1/30 s tick each, first is a keyframe. */
    mediaway_packet_view_t packet = {0};
    for (int i = 0; i < VIDEO_FRAMES; i++) {
        packet.stream_id = VIDEO_TRACK_ID;
        packet.pts = i;
        packet.dts = i;
        packet.duration = 1; /* one tick of the 1/30 s video time base */
        packet.is_keyframe = (i == 0);
        packet.is_discard = false;
        packet.payload = video_payload; /* borrowed, valid for the call only */
        packet.payload_len = sizeof(video_payload);
        CHECK(mediaway_muxer_push_packet(muxer, &packet));
    }

    /* 90 synthetic audio packets: 1024-sample AAC frames at 48 kHz. */
    for (int i = 0; i < AUDIO_FRAMES; i++) {
        packet.stream_id = AUDIO_TRACK_ID;
        packet.pts = i * 1024;
        packet.dts = i * 1024;
        packet.duration = 1024;
        packet.is_keyframe = true; /* AAC frames are independently decodable */
        packet.is_discard = false;
        packet.payload = audio_payload;
        packet.payload_len = sizeof(audio_payload);
        CHECK(mediaway_muxer_push_packet(muxer, &packet));
    }

    CHECK(mediaway_muxer_flush(muxer));

    /* Drain everything the muxer produced into one contiguous buffer so the
     * demuxer sees the complete stream in a single push. */
    uint8_t *mp4 = NULL;
    size_t mp4_len = 0;
    for (;;) {
        uint8_t *chunk = NULL;
        size_t chunk_len = 0;
        CHECK(mediaway_muxer_poll_bytes(muxer, &chunk, &chunk_len));
        if (chunk_len == 0) {
            break; /* NULL/0 is the valid "nothing ready" result, not an error */
        }
        uint8_t *grown = (uint8_t *)realloc(mp4, mp4_len + chunk_len);
        if (grown == NULL) {
            fprintf(stderr, "out of memory growing muxed buffer\n");
            exit(EXIT_FAILURE);
        }
        mp4 = grown;
        memcpy(mp4 + mp4_len, chunk, chunk_len);
        mp4_len += chunk_len;
        /* Owned output: released with mediaway_buffer_free. */
        mediaway_buffer_free(chunk, chunk_len);
    }
    if (mp4_len == 0) {
        fprintf(stderr, "flush produced no bytes — mux feature enabled in the build?\n");
        exit(EXIT_FAILURE);
    }

    mediaway_muxer_close(muxer); /* muxer done: close + free the handle */
    printf("muxed %zu bytes of fragmented MP4 (%d video + %d audio packets)\n",
           mp4_len, VIDEO_FRAMES, AUDIO_FRAMES);

    /* ---- Demux side ----------------------------------------------------- */

    mediaway_demuxer_t *demuxer = mediaway_demuxer_create();
    if (demuxer == NULL) {
        fprintf(stderr, "mediaway_demuxer_create returned NULL (caught panic)\n");
        free(mp4);
        return EXIT_FAILURE;
    }

    /* Borrowed input: the core copies synchronously before returning. */
    CHECK(mediaway_demuxer_push_bytes(demuxer, mp4, mp4_len));

    const size_t stream_count = mediaway_demuxer_stream_count(demuxer);
    printf("demuxer reports %zu stream(s)\n", stream_count);
    for (size_t idx = 0; idx < stream_count; idx++) {
        mediaway_stream_info_t info;
        CHECK(mediaway_demuxer_stream_at(demuxer, idx, &info));
        printf("  stream %zu: id=%u codec=%d time_base=%llu/%u\n", idx, info.id,
               (int)info.codec, (unsigned long long)info.time_base.num,
               (unsigned int)info.time_base.den);
        /* Owned extra_data: released with mediaway_stream_info_free. */
        mediaway_stream_info_free(&info);
    }

    /* Poll packets until the demuxer reports none ready. */
    int video_recovered = 0;
    int audio_recovered = 0;
    int other_recovered = 0;
    mediaway_packet_t out_packet;
    bool has_packet = false;
    for (;;) {
        CHECK(mediaway_demuxer_poll_packet(demuxer, &out_packet, &has_packet));
        if (!has_packet) {
            break; /* false is the valid "nothing ready" result */
        }
        if (out_packet.stream_id == VIDEO_TRACK_ID) {
            video_recovered++;
        } else if (out_packet.stream_id == AUDIO_TRACK_ID) {
            audio_recovered++;
        } else {
            other_recovered++;
        }
        /* Owned payload: released with mediaway_packet_free. */
        mediaway_packet_free(&out_packet);
    }

    mediaway_demuxer_close(demuxer);
    free(mp4); /* our own accumulated buffer, not an ABI-owned output */

    printf("recovered %d video packet(s), %d audio packet(s)\n",
           video_recovered, audio_recovered);

    /* The roundtrip must be lossless — a mismatch here is a real bug. */
    if (video_recovered != VIDEO_FRAMES || audio_recovered != AUDIO_FRAMES ||
        other_recovered != 0) {
        fprintf(stderr,
                "roundtrip mismatch: expected %d video / %d audio, got %d / %d "
                "(+%d unknown stream(s))\n",
                VIDEO_FRAMES, AUDIO_FRAMES, video_recovered, audio_recovered,
                other_recovered);
        return EXIT_FAILURE;
    }

    printf("roundtrip OK: %d video + %d audio packets survived mux + demux\n",
           video_recovered, audio_recovered);
    return EXIT_SUCCESS;
}
