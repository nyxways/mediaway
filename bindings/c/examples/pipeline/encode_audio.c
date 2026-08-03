/*
 * encode_audio.c — AAC audio encode of 96 synthetic F32 stereo frames → audio-only
 * fragmented MP4 (mediaway-pipeline-ffi ABI v2 + mediaway-container-ffi).
 *
 * STATUS: REAL (link+run verified). The scenario table in bindings/c/README.md
 * marks encode_audio "✅ link+run verified" — this file calls only the shipped
 * ABI, exactly as <mediaway/pipeline.h> and <mediaway/container.h> declare it.
 * Nothing here is aspirational.
 *
 * Flow: open the audio encoder (single step — the returned handle IS the encode
 * session; there is no intermediate handle and no consumption trap), push 96
 * frames of a 440 Hz sine (1024 samples @ 48 kHz ≈ 2 s of audio), flush, then
 * mux the polled AAC packets into an audio-only fragmented MP4. The track is
 * registered with the AudioSpecificConfig the encoder exposes via
 * mediaway_audio_encode_session_stream_info — the esds box an audio track needs
 * to be playable.
 *
 * AudioSpecificConfig availability: the WMF backend materializes it only after
 * the first PCM frame is pushed, so the call order is push → stream_info → mux
 * (adr/0003-auto-audio-encode-c-abi.md).
 *
 * Build (see bindings/c/README.md "Building & verifying on Windows"):
 *   gcc -Icrates/mediaway-pipeline-ffi/include -Icrates/mediaway-container-ffi/include \
 *       bindings/c/examples/pipeline/encode_audio.c \
 *       -Ltarget/x86_64-pc-windows-gnu/debug -lmediaway_pipeline_ffi -lmediaway_container_ffi \
 *       -o encode_audio.exe
 */

#include <mediaway/container.h>
#include <mediaway/pipeline.h>

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* CHECK-style macros: only for calls that should fail on a programming bug.
 * NO_BACKEND is handled explicitly, never through these. */
#define CHECK_PIPE(call)                                                            \
    do {                                                                            \
        mediaway_pipeline_status_t _st_ = (call);                                   \
        if (_st_ != MEDIAWAY_PIPELINE_STATUS_OK) {                                  \
            fprintf(stderr,                                                         \
                    "CHECK failed: %s -> mediaway_pipeline_status_t %d (%s:%d)\n",   \
                    #call, (int)_st_, __FILE__, __LINE__);                          \
            exit(EXIT_FAILURE);                                                     \
        }                                                                           \
    } while (0)

#define CHECK_CT(call)                                                              \
    do {                                                                            \
        mediaway_status_t _st_ = (call);                                            \
        if (_st_ != MEDIAWAY_OK) {                                           \
            fprintf(stderr,                                                         \
                    "CHECK failed: %s -> mediaway_status_t %d (%s:%d)\n",            \
                    #call, (int)_st_, __FILE__, __LINE__);                          \
            exit(EXIT_FAILURE);                                                     \
        }                                                                           \
    } while (0)

#define SAMPLE_RATE 48000   /* Hz */
#define CHANNELS 2          /* stereo */
#define FRAME_SAMPLES 1024  /* samples per pushed frame (~21 ms) */
#define FRAME_COUNT 96      /* ~2.0 s of audio */

/* One interleaved F32 stereo frame of a deterministic 440 Hz sine. No
 * microphone needed, so the example is hermetic and fast. */
static void fill_sine_frame(uint8_t *out, int frame_index) {
    float *f = (float *)out;
    for (int s = 0; s < FRAME_SAMPLES; s++) {
        float t = (float)(frame_index * FRAME_SAMPLES + s) / (float)SAMPLE_RATE;
        float v = sinf(2.0f * 3.14159265f * 440.0f * t);
        for (int c = 0; c < CHANNELS; c++) {
            *f++ = v;
        }
    }
}

int main(void) {
    if (mediaway_pipeline_ffi_abi_version() != MEDIAWAY_PIPELINE_FFI_ABI_VERSION) {
        fprintf(stderr, "pipeline ABI version mismatch — rebuild against the shipped headers\n");
        return EXIT_FAILURE;
    }

    /* ---- Open the audio encoder (single step) ------------------------------ */

    mediaway_rational_t audio_tb = {1, SAMPLE_RATE}; /* 48 kHz sample clock */
    mediaway_audio_encode_config_t config =
        mediaway_audio_encode_config_aac(SAMPLE_RATE, audio_tb);

    mediaway_audio_encode_session_t *session = NULL;
    mediaway_pipeline_status_t pst = mediaway_audio_encoder_open(&config, &session);
    if (pst == MEDIAWAY_PIPELINE_STATUS_NO_BACKEND) {
        printf("no audio encode backend compiled in (NO_BACKEND) — exiting gracefully\n");
        return EXIT_SUCCESS;
    }
    CHECK_PIPE(pst);
    if (session == NULL) {
        fprintf(stderr, "mediaway_audio_encoder_open returned OK with NULL session\n");
        return EXIT_FAILURE;
    }

    /* ---- Push synthetic PCM ------------------------------------------------ */

    uint8_t *pcm = (uint8_t *)malloc(FRAME_SAMPLES * CHANNELS * sizeof(float));
    if (pcm == NULL) {
        fprintf(stderr, "out of memory allocating a PCM frame\n");
        mediaway_audio_encode_session_close(session);
        return EXIT_FAILURE;
    }
    for (int i = 0; i < FRAME_COUNT; i++) {
        fill_sine_frame(pcm, i);
        mediaway_audio_frame_view_t frame = {
            .pts = (int64_t)i * FRAME_SAMPLES,
            .duration = FRAME_SAMPLES,
            .sample_rate = SAMPLE_RATE,
            .channels = CHANNELS,
            .sample_format = MEDIAWAY_SAMPLE_FORMAT_F32,
            .data = pcm,              /* borrowed for the call only */
            .data_len = (size_t)FRAME_SAMPLES * CHANNELS * sizeof(float),
        };
        CHECK_PIPE(mediaway_audio_encode_session_push_pcm(session, &frame));
    }
    CHECK_PIPE(mediaway_audio_encode_session_flush(session));
    free(pcm);

    /* ---- Poll encoded packets ---------------------------------------------- */

    enum { MAX_PACKETS = 128 };
    mediaway_audio_packet_t packets[MAX_PACKETS];
    size_t packet_count = 0;
    for (;;) {
        mediaway_audio_packet_t packet;
        bool has_packet = false;
        pst = mediaway_audio_encode_session_poll_packet(session, &packet, &has_packet);
        CHECK_PIPE(pst);
        if (!has_packet) {
            break; /* false is the valid "nothing ready" result, not an error */
        }
        if (packet_count >= MAX_PACKETS) {
            fprintf(stderr, "too many audio packets (%zu) — raise MAX_PACKETS\n", packet_count);
            mediaway_pipeline_ffi_packet_free(&packet);
            mediaway_audio_encode_session_close(session);
            return EXIT_FAILURE;
        }
        packets[packet_count++] = packet;
    }
    if (packet_count == 0) {
        fprintf(stderr, "encoder produced no packets for %d PCM frames\n", FRAME_COUNT);
        mediaway_audio_encode_session_close(session);
        return EXIT_FAILURE;
    }

    /* ---- Stream info: the AudioSpecificConfig the track needs ------------- */
    /* Only populated after the first pushed frame — we pushed above. */
    mediaway_audio_stream_info_t info;
    pst = mediaway_audio_encode_session_stream_info(session, &info);
    CHECK_PIPE(pst);
    if (info.extra_data_len == 0) {
        fprintf(stderr, "stream info carries no AudioSpecificConfig — cannot register track\n");
        mediaway_audio_encode_session_close(session);
        return EXIT_FAILURE;
    }
    printf("encoded %zu AAC packet(s), ASC %zu bytes\n", packet_count, info.extra_data_len);

    /* ---- Mux an audio-only fragmented MP4 ---------------------------------- */

    mediaway_muxer_t *muxer = mediaway_muxer_create();
    if (muxer == NULL) {
        fprintf(stderr, "mediaway_muxer_create returned NULL\n");
        mediaway_audio_encode_session_close(session);
        return EXIT_FAILURE;
    }
    /* Container codec values are mirrored 1:1 with the pipeline enum — the cast
     * is a compile-time convenience, not a renumbering. */
    mediaway_audio_track_info_t atrack = {
        .id = 0,
        .codec = (mediaway_codec_kind_t)info.codec,
        .time_base = audio_tb,
        .sample_rate = info.sample_rate,
        .channels = info.channels,
        .extra_data = info.extra_data, /* borrowed for add_audio_track only */
        .extra_data_len = info.extra_data_len,
    };
    CHECK_CT(mediaway_muxer_add_audio_track(muxer, &atrack));
    CHECK_CT(mediaway_muxer_begin(muxer));

    for (size_t i = 0; i < packet_count; i++) {
        mediaway_packet_view_t view = {
            .stream_id = 0,
            .pts = packets[i].pts,
            .dts = packets[i].dts,
            .duration = packets[i].duration,
            .is_keyframe = packets[i].is_keyframe,
            .is_discard = packets[i].is_discard,
            .payload = packets[i].payload, /* borrowed for push_packet only */
            .payload_len = packets[i].payload_len,
        };
        CHECK_CT(mediaway_muxer_push_packet(muxer, &view));
    }
    CHECK_CT(mediaway_muxer_flush(muxer));

    uint8_t *out_data = NULL;
    size_t out_len = 0;
    pst = mediaway_muxer_poll_bytes(muxer, &out_data, &out_len);
    CHECK_PIPE(pst);
    if (out_len == 0 || out_data == NULL) {
        fprintf(stderr, "mediaway_muxer_poll_bytes returned OK with empty output\n");
        mediaway_muxer_close(muxer);
        mediaway_audio_encode_session_close(session);
        return EXIT_FAILURE;
    }
    printf("muxed %zu AAC packet(s) into %zu bytes of audio-only fragmented MP4\n",
           packet_count, out_len);

    /* ---- Cleanup ----------------------------------------------------------- */
    /* Owned outputs: released with the matching frees. Track registration is
     * done (ASC copied into the moov), so info can go now. */
    mediaway_pipeline_ffi_stream_info_free(&info);
    for (size_t i = 0; i < packet_count; i++) {
        mediaway_pipeline_ffi_packet_free(&packets[i]);
    }
    mediaway_muxer_close(muxer);
    mediaway_audio_encode_session_close(session); /* always safe — no consumption trap */
    mediaway_buffer_free(out_data, out_len); /* container-ffi's own free for muxer_poll_bytes output */
    return EXIT_SUCCESS;
}
