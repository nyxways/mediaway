/*
 * mux_roundtrip.c — Mediaway container mux + demux roundtrip (fragmented MP4).
 *
 * This now matches the real C ABI in `crates/mediaway-container-ffi`
 * (`include/mediaway/container.h`, `adr/0001-mp4-mux-demux-c-abi.md`) — the
 * crate is not yet built as a packaged/distributed release, so this is still
 * a documentation example rather than a shipped sample, but the function
 * names, struct layouts, and ownership rules below are the real ABI, not
 * aspirational guesses. It mirrors examples/mux_roundtrip.rs. Type-checked
 * against the real header (`gcc -fsyntax-only`) and, in this dev environment,
 * also built + linked against the `x86_64-pc-windows-gnu` target output and
 * run: 90 video + 90 audio packets recovered exactly (see
 * `adr/0001-mp4-mux-demux-c-abi.md` and the crate's `docs/roadmap.md` for the
 * verification recipe).
 *
 * The core never touches files or sockets: the caller always owns byte I/O,
 * pulling muxed bytes out of the muxer and pushing them into the demuxer.
 */

#include <mediaway/container.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

/* Check a mediaway_status_t after every call; no exceptions/panics cross the
 * C ABI, so this is the idiomatic error-handling shape for this layer. */
#define CHECK(call)                                                          \
    do {                                                                     \
        mediaway_status_t _st = (call);                                     \
        if (_st != MEDIAWAY_OK) {                                            \
            fprintf(stderr, "%s failed: status %d\n", #call, (int)_st);      \
            exit(EXIT_FAILURE);                                              \
        }                                                                    \
    } while (0)

int main(void) {
    const uint32_t frame_count = 90; /* 3 s at 30 fps */
    static const uint8_t video_payload[4] = {0x00, 0x00, 0x00, 0x01};
    static const uint8_t audio_payload[2] = {0xff, 0xf1};

    /* ── 1. Create muxer, register tracks (Open state) ─────────────────── */
    mediaway_muxer_t *muxer = mediaway_muxer_create();
    if (muxer == NULL) {
        fprintf(stderr, "mediaway_muxer_create failed\n");
        return EXIT_FAILURE;
    }

    /* Track ids are caller-assigned (the library only validates uniqueness), so they are
     * chosen up front instead of being filled in by add_*_track. */
    const uint32_t video_track_id = 0;
    const uint32_t audio_track_id = 1;

    mediaway_video_track_info_t video_info = {
        .id = video_track_id,
        .codec = MEDIAWAY_CODEC_H264,
        .time_base = {.num = 1, .den = 30},
        .width = 1920,
        .height = 1080,
        .extra_data = NULL,
        .extra_data_len = 0,
    };
    CHECK(mediaway_muxer_add_video_track(muxer, &video_info));

    mediaway_audio_track_info_t audio_info = {
        .id = audio_track_id,
        .codec = MEDIAWAY_CODEC_AAC,
        .time_base = {.num = 1, .den = 48000},
        .sample_rate = 48000,
        .channels = 2,
        .extra_data = NULL,
        .extra_data_len = 0,
    };
    CHECK(mediaway_muxer_add_audio_track(muxer, &audio_info));

    /* ── 2. Transition to streaming (Live state) ────────────────────────── */
    /* Track registration closes here; packet submission begins. */
    CHECK(mediaway_muxer_begin(muxer));

    for (uint32_t i = 0; i < frame_count; i++) {
        /* mediaway_packet_view_t: borrowed input view, valid for the call only. */
        mediaway_packet_view_t video_packet = {
            .stream_id = video_track_id,
            .pts = (int64_t)i,
            .dts = (int64_t)i,
            .duration = 1,
            .is_keyframe = (i % 30 == 0),
            .is_discard = false,
            .payload = video_payload,
            .payload_len = sizeof(video_payload),
        };
        CHECK(mediaway_muxer_push_packet(muxer, &video_packet));

        mediaway_packet_view_t audio_packet = {
            .stream_id = audio_track_id,
            .pts = (int64_t)i * 1600,
            .dts = (int64_t)i * 1600,
            .duration = 1600,
            .is_keyframe = true,
            .is_discard = false,
            .payload = audio_payload,
            .payload_len = sizeof(audio_payload),
        };
        CHECK(mediaway_muxer_push_packet(muxer, &audio_packet));
    }

    CHECK(mediaway_muxer_flush(muxer));

    /* ── 3. Pull bytes — caller decides where they go (no file inside) ──── */
    uint8_t *mp4_bytes = NULL;
    size_t mp4_len = 0;
    CHECK(mediaway_muxer_poll_bytes(muxer, &mp4_bytes, &mp4_len));
    printf("mux_roundtrip: %u frames -> %zu bytes of fMP4\n", frame_count, mp4_len);

    mediaway_muxer_close(muxer);

    /* ── 4. Demux the same bytes back ───────────────────────────────────── */
    mediaway_demuxer_t *demuxer = mediaway_demuxer_create();
    if (demuxer == NULL) {
        fprintf(stderr, "mediaway_demuxer_create failed\n");
        mediaway_buffer_free(mp4_bytes, mp4_len);
        return EXIT_FAILURE;
    }

    CHECK(mediaway_demuxer_push_bytes(demuxer, mp4_bytes, mp4_len));
    mediaway_buffer_free(mp4_bytes, mp4_len);

    size_t stream_count = mediaway_demuxer_stream_count(demuxer);
    printf("mux_roundtrip: demuxer sees %zu stream(s)\n", stream_count);
    for (size_t i = 0; i < stream_count; i++) {
        /* mediaway_stream_info_t: owned output, released via mediaway_stream_info_free. */
        mediaway_stream_info_t stream;
        CHECK(mediaway_demuxer_stream_at(demuxer, i, &stream));
        if (stream.has_geometry) {
            printf("  stream %u - codec %d %ux%u\n", stream.id, (int)stream.codec,
                   stream.width, stream.height);
        } else {
            printf("  stream %u - codec %d (no geometry)\n", stream.id, (int)stream.codec);
        }
        mediaway_stream_info_free(&stream);
    }

    uint32_t n_video = 0;
    uint32_t n_audio = 0;
    for (;;) {
        /* mediaway_packet_t: owned output, released via mediaway_packet_free. */
        mediaway_packet_t packet;
        bool has_packet = false;
        CHECK(mediaway_demuxer_poll_packet(demuxer, &packet, &has_packet));
        if (!has_packet) {
            break;
        }
        if (packet.stream_id == video_track_id) {
            n_video++;
        } else {
            n_audio++;
        }
        mediaway_packet_free(&packet);
    }
    printf("mux_roundtrip: recovered %u video + %u audio packets\n", n_video, n_audio);

    mediaway_demuxer_close(demuxer);
    return EXIT_SUCCESS;
}
