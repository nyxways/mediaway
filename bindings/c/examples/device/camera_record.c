/*
 * camera_record.c — camera + mic capture → auto H.264 + AAC encode → single
 * two-track fragmented MP4 (mediaway-device-ffi + mediaway-pipeline-ffi ABI v2
 * + mediaway-container-ffi).
 *
 * STATUS: REAL (link+run verified). The scenario table in bindings/c/README.md
 * marks camera_record "✅ link+run verified" — this file calls only the shipped
 * ABI, exactly as the three shipped headers declare it. Nothing aspirational.
 *
 * Flow: open the default camera and microphone, encode ~3 s of camera frames
 * with the auto H.264 encoder (its internal MP4 session — video-only by
 * design), meanwhile encode mic PCM frames with the audio encode session
 * (ABI v2, adr/0003), then REMUX: demux the video session's fragmented MP4,
 * mux video + AAC audio into one two-track fragmented MP4, registering the
 * audio track with the AudioSpecificConfig the audio encoder exposes via
 * mediaway_audio_encode_session_stream_info.
 *
 * Before ABI v2 this example drained mic frames unencoded (the drain-only gap
 * that adr/0003 resolves). NO_BACKEND / missing devices are expected, graceful
 * outcomes — a missing mic degrades to video-only, never a crash.
 *
 * Build (see bindings/c/README.md "Building & verifying on Windows"):
 *   gcc -Icrates/mediaway-device-ffi/include -Icrates/mediaway-pipeline-ffi/include \
 *       -Icrates/mediaway-container-ffi/include bindings/c/examples/device/camera_record.c \
 *       -Ltarget/x86_64-pc-windows-gnu/debug -lmediaway_device_ffi -lmediaway_pipeline_ffi \
 *       -lmediaway_container_ffi -o camera_record.exe
 */

#include <mediaway/container.h>
#include <mediaway/device.h>
#include <mediaway/pipeline.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define RECORD_VIDEO_FRAMES 90 /* ~3 s at 30 fps */
#define CAPTURE_TIMEOUT_MS 5000

/* CHECK-style macros: only for calls that should fail on a programming bug.
 * NO_BACKEND / device-unavailable are handled explicitly, never through these. */
#define CHECK_DEV(call)                                                             \
    do {                                                                            \
        mediaway_device_status_t _st_ = (call);                                     \
        if (_st_ != MEDIAWAY_DEVICE_STATUS_OK) {                                    \
            fprintf(stderr,                                                         \
                    "CHECK failed: %s -> mediaway_device_status_t %d (%s:%d)\n",     \
                    #call, (int)_st_, __FILE__, __LINE__);                          \
            exit(EXIT_FAILURE);                                                     \
        }                                                                           \
    } while (0)

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
        if (_st_ != MEDIAWAY_OK) {                                                  \
            fprintf(stderr,                                                         \
                    "CHECK failed: %s -> mediaway_status_t %d (%s:%d)\n",            \
                    #call, (int)_st_, __FILE__, __LINE__);                          \
            exit(EXIT_FAILURE);                                                     \
        }                                                                           \
    } while (0)

/* Expected "device not there / not usable" results at open time — the example
 * exits gracefully on these instead of treating them as bugs. The remaining
 * statuses (invalid argument, poisoned handle, invalid input, internal panic)
 * mean the caller misused the API, so they fail loudly. */
static int is_capture_unavailable(mediaway_device_status_t st) {
    switch (st) {
    case MEDIAWAY_DEVICE_STATUS_INVALID_ARGUMENT:
    case MEDIAWAY_DEVICE_STATUS_HANDLE_POISONED:
    case MEDIAWAY_DEVICE_STATUS_INVALID_INPUT:
    case MEDIAWAY_DEVICE_STATUS_INTERNAL_PANIC:
        return 0; /* API misuse / poisoned handle: fail loudly */
    default:
        return 1; /* no device, no backend, blocked, or backend failure */
    }
}

int main(void) {
    if (mediaway_device_ffi_abi_version() != MEDIAWAY_DEVICE_FFI_ABI_VERSION ||
        mediaway_pipeline_ffi_abi_version() != MEDIAWAY_PIPELINE_FFI_ABI_VERSION ||
        mediaway_container_ffi_abi_version() != MEDIAWAY_CONTAINER_FFI_ABI_VERSION) {
        fprintf(stderr, "ABI version mismatch — rebuild against the shipped headers\n");
        return EXIT_FAILURE;
    }

    /* ---- Camera video --------------------------------------------------- */

    mediaway_rational_t video_tb = {1, 30}; /* 30 fps capture */
    mediaway_camera_capture_config_t vconfig =
        mediaway_camera_capture_config_default(/* device_index */ 0, video_tb);

    mediaway_camera_capture_t *vcapture = NULL;
    mediaway_device_status_t st = mediaway_camera_capture_open(&vconfig, &vcapture);
    if (st != MEDIAWAY_DEVICE_STATUS_OK) {
        if (is_capture_unavailable(st)) {
            printf("no camera available (mediaway_camera_capture_open -> status %d) — "
                   "exiting gracefully\n",
                   (int)st);
            return EXIT_SUCCESS;
        }
        fprintf(stderr, "camera open failed with unexpected status %d\n", (int)st);
        return EXIT_FAILURE;
    }

    /* ---- Microphone audio ------------------------------------------------ */

    mediaway_rational_t audio_tb = {1, 48000}; /* 48 kHz sample clock */
    mediaway_audio_capture_config_t aconfig =
        mediaway_audio_capture_config_microphone(audio_tb);

    bool have_mic = false;
    mediaway_audio_capture_t *acapture = NULL;
    st = mediaway_audio_capture_open(&aconfig, &acapture);
    if (st == MEDIAWAY_DEVICE_STATUS_OK) {
        have_mic = true;
    } else if (is_capture_unavailable(st)) {
        printf("no microphone available (status %d) — recording video only\n", (int)st);
    } else {
        fprintf(stderr, "mic open failed with unexpected status %d\n", (int)st);
        mediaway_camera_capture_close(vcapture); /* blocking: joins the backend worker */
        return EXIT_FAILURE;
    }

    /* ---- Negotiated formats ---------------------------------------------- */

    uint32_t width = 0;
    uint32_t height = 0;
    CHECK_DEV(mediaway_camera_capture_geometry(vcapture, &width, &height));
    printf("camera negotiated %ux%u\n", width, height);

    uint32_t sample_rate = 0;
    uint16_t channels = 0;
    if (have_mic) {
        CHECK_DEV(mediaway_audio_capture_format(acapture, &sample_rate, &channels));
        printf("mic negotiated %u Hz, %u channel(s)\n", sample_rate, channels);
    }

    /* ---- Video encoder at the real negotiated geometry -------------------- */

    mediaway_auto_video_encode_config_t econfig =
        mediaway_auto_video_encode_config_h264(width, height, video_tb);

    mediaway_auto_encoder_t *encoder = NULL;
    mediaway_pipeline_status_t pst = mediaway_auto_encoder_open(&econfig, &encoder);
    if (pst == MEDIAWAY_PIPELINE_STATUS_NO_BACKEND) {
        printf("no encode backend compiled in (NO_BACKEND) — exiting gracefully\n");
        mediaway_camera_capture_close(vcapture);
        if (have_mic) {
            mediaway_audio_capture_close(acapture);
        }
        return EXIT_SUCCESS;
    }
    CHECK_PIPE(pst);

    /* Consumes `encoder` UNCONDITIONALLY — never mediaway_auto_encoder_close
     * on it after this, regardless of the returned status. */
    mediaway_encode_session_t *session = NULL;
    pst = mediaway_encode_session_open(encoder, &session);
    if (pst == MEDIAWAY_PIPELINE_STATUS_NO_BACKEND) {
        printf("no encode backend compiled in (NO_BACKEND at session open) — "
               "exiting gracefully\n");
        mediaway_camera_capture_close(vcapture);
        if (have_mic) {
            mediaway_audio_capture_close(acapture);
        }
        return EXIT_SUCCESS;
    }
    CHECK_PIPE(pst);

    /* ---- Audio encoder (single step — the session IS the encoder) -------- */
    /* Channels/rate are the mic's negotiated ones, not hard-coded stereo. */
    mediaway_audio_encode_session_t *asession = NULL;
    if (have_mic) {
        mediaway_audio_encode_config_t aenc_config = {
            .codec = MEDIAWAY_PIPELINE_CODEC_AAC,
            .sample_rate = sample_rate,
            .channels = channels,
            .sample_format = MEDIAWAY_SAMPLE_FORMAT_F32,
            .time_base = audio_tb,
            .bitrate_bps = 0, /* backend default */
        };
        pst = mediaway_audio_encoder_open(&aenc_config, &asession);
        if (pst == MEDIAWAY_PIPELINE_STATUS_NO_BACKEND) {
            printf("no audio encode backend compiled in (NO_BACKEND) — "
                   "recording video only\n");
            asession = NULL;
        } else if (pst != MEDIAWAY_PIPELINE_STATUS_OK) {
            printf("audio encoder open failed (status %d) — recording video only\n",
                   (int)pst);
            asession = NULL;
        }
    }

    /* ---- Record ~3 s ------------------------------------------------------ */

    int video_frames = 0;
    int audio_pushed = 0;
    mediaway_camera_frame_t vframe;
    while (video_frames < RECORD_VIDEO_FRAMES) {
        /* Block up to 5 s for the next camera frame; TIMEOUT means the camera
         * stopped producing — finish with what we have. */
        st = mediaway_camera_capture_poll_frame_blocking(vcapture, CAPTURE_TIMEOUT_MS,
                                                        &vframe);
        if (st == MEDIAWAY_DEVICE_STATUS_TIMEOUT) {
            printf("camera stopped producing frames after %d — finishing with "
                   "what we have\n",
                   video_frames);
            break;
        }
        CHECK_DEV(st);

        /* Camera frames are CPU-storage by definition (no storage_kind field on
         * mediaway_camera_frame_t — every shipped Camera backend is CPU-only). */
        mediaway_video_frame_t input = {
            .pts = vframe.pts,
            .duration = vframe.duration,
            .width = vframe.width,
            .height = vframe.height,
            .pixel_format = vframe.pixel_format,
            .storage_kind = MEDIAWAY_VIDEO_FRAME_STORAGE_CPU,
            .raw_bytes = vframe.data,   /* borrowed for the call only */
            .raw_bytes_len = vframe.data_len,
            /* gpu_buffer unused for CPU storage (zeroed) */
        };
        CHECK_PIPE(mediaway_encode_session_write_frame(session, &input));
        video_frames++;

        /* release_frame before the next frame-acquiring poll (a documented
         * no-op on the Camera backend, required by the contract regardless). */
        CHECK_DEV(mediaway_camera_capture_release_frame(vcapture));
        /* Owned CPU buffer: released with mediaway_camera_frame_free. */
        mediaway_camera_frame_free(&vframe);

        /* Encode any mic chunks that arrived meanwhile. */
        if (asession != NULL) {
            mediaway_device_audio_frame_t aframe;
            for (;;) {
                bool has_audio = false;
                st = mediaway_audio_capture_poll_frame(acapture, &aframe, &has_audio);
                CHECK_DEV(st);
                if (!has_audio) {
                    break; /* false is the valid "no frames yet" result */
                }
                /* Mic PCM is F32 interleaved (device.h truth: only F32 accepted)
                 * — feed it straight into the F32 audio encoder. */
                mediaway_audio_frame_view_t ainput = {
                    .pts = aframe.pts,
                    .duration = aframe.duration,
                    .sample_rate = aframe.sample_rate,
                    .channels = aframe.channels,
                    .sample_format = aframe.sample_format, /* F32 by contract */
                    .data = aframe.data, /* borrowed for the call only */
                    .data_len = aframe.data_len,
                };
                CHECK_PIPE(mediaway_audio_encode_session_push_pcm(asession, &ainput));
                audio_pushed++;
                /* Owned PCM buffer: released with mediaway_audio_frame_free. */
                mediaway_audio_frame_free(&aframe);
            }
        }
    }

    /* ---- Finish video + audio encode -------------------------------------- */

    int video_bytes = 0;
    uint8_t *vout = NULL;
    size_t vout_len = 0;
    if (video_frames == 0) {
        printf("no video frames captured — abandoning the session without output\n");
        mediaway_encode_session_close(session); /* abandon path: always safe */
    } else {
        /* finish() consumes `session` UNCONDITIONALLY — never
         * mediaway_encode_session_close after it. Output is library-owned;
         * release with mediaway_pipeline_ffi_buffer_free. */
        pst = mediaway_encode_session_finish(session, &vout, &vout_len);
        CHECK_PIPE(pst);
        if (vout_len == 0 || vout == NULL) {
            fprintf(stderr,
                    "mediaway_encode_session_finish returned OK with empty output\n");
            mediaway_camera_capture_close(vcapture);
            if (have_mic) {
                mediaway_audio_capture_close(acapture);
            }
            return EXIT_FAILURE;
        }
        video_bytes = (int)vout_len;
    }

    /* Poll the AAC packets; bail to video-only on zero packets. */
    enum { MAX_AUDIO_PACKETS = 512 };
    mediaway_audio_packet_t apackets[MAX_AUDIO_PACKETS];
    size_t apacket_count = 0;
    mediaway_audio_stream_info_t ainfo;
    bool have_audio_track = false;
    if (asession != NULL) {
        pst = mediaway_audio_encode_session_flush(asession);
        CHECK_PIPE(pst);
        for (;;) {
            mediaway_audio_packet_t packet;
            bool has_packet = false;
            pst = mediaway_audio_encode_session_poll_packet(asession, &packet, &has_packet);
            CHECK_PIPE(pst);
            if (!has_packet) {
                break;
            }
            if (apacket_count >= MAX_AUDIO_PACKETS) {
                fprintf(stderr, "too many audio packets — raise MAX_AUDIO_PACKETS\n");
                mediaway_pipeline_ffi_packet_free(&packet);
                break;
            }
            apackets[apacket_count++] = packet;
        }
        /* AudioSpecificConfig materializes after the first pushed frame. */
        pst = mediaway_audio_encode_session_stream_info(asession, &ainfo);
        CHECK_PIPE(pst);
        have_audio_track = apacket_count > 0 && ainfo.extra_data_len > 0;
        if (!have_audio_track && apacket_count > 0) {
            printf("audio encoder produced packets but no AudioSpecificConfig — "
                   "recording video only\n");
        }
    }

    /* ---- Remux: one two-track fragmented MP4 ------------------------------ */

    if (video_bytes > 0 && have_audio_track) {
        /* Demux the video session's fMP4 to get its packets + track info. */
        mediaway_demuxer_t *demuxer = mediaway_demuxer_create();
        if (demuxer == NULL) {
            fprintf(stderr, "mediaway_demuxer_create returned NULL\n");
            return EXIT_FAILURE;
        }
        CHECK_CT(mediaway_demuxer_push_bytes(demuxer, vout, vout_len));

        mediaway_stream_info_t vinfo;
        pst = mediaway_demuxer_stream_at(demuxer, 0, &vinfo);
        CHECK_PIPE(pst);

        mediaway_muxer_t *muxer = mediaway_muxer_create();
        if (muxer == NULL) {
            fprintf(stderr, "mediaway_muxer_create returned NULL\n");
            return EXIT_FAILURE;
        }

        /* Video track from the demuxed stream info (avcC SPS/PPS included). */
        mediaway_video_track_info_t vtrack = {
            .id = vinfo.id,
            .codec = vinfo.codec,
            .time_base = vinfo.time_base,
            .width = vinfo.width,
            .height = vinfo.height,
            .extra_data = vinfo.extra_data, /* borrowed for add_video_track only */
            .extra_data_len = vinfo.extra_data_len,
        };
        /* Audio track: id 1 (video owns whatever the demuxer assigned), with
         * the encoder's AudioSpecificConfig. */
        mediaway_audio_track_info_t atrack = {
            .id = 1,
            .codec = (mediaway_codec_kind_t)ainfo.codec,
            .time_base = ainfo.time_base,
            .sample_rate = ainfo.sample_rate,
            .channels = ainfo.channels,
            .extra_data = ainfo.extra_data, /* borrowed for add_audio_track only */
            .extra_data_len = ainfo.extra_data_len,
        };
        CHECK_CT(mediaway_muxer_add_video_track(muxer, &vtrack));
        CHECK_CT(mediaway_muxer_add_audio_track(muxer, &atrack));
        CHECK_CT(mediaway_muxer_begin(muxer));

        /* Re-push every demuxed video packet (stream_id preserved). */
        for (;;) {
            mediaway_packet_t packet;
            bool has_packet = false;
            pst = mediaway_demuxer_poll_packet(demuxer, &packet, &has_packet);
            CHECK_PIPE(pst);
            if (!has_packet) {
                break;
            }
            mediaway_packet_view_t view = {
                .stream_id = packet.stream_id,
                .pts = packet.pts,
                .dts = packet.dts,
                .duration = packet.duration,
                .is_keyframe = packet.is_keyframe,
                .is_discard = packet.is_discard,
                .payload = packet.payload, /* borrowed for push_packet only */
                .payload_len = packet.payload_len,
            };
            CHECK_CT(mediaway_muxer_push_packet(muxer, &view));
            mediaway_packet_free(&packet);
        }
        /* Then the AAC packets (audio track id 1). */
        for (size_t i = 0; i < apacket_count; i++) {
            mediaway_packet_view_t view = {
                .stream_id = 1,
                .pts = apackets[i].pts,
                .dts = apackets[i].dts,
                .duration = apackets[i].duration,
                .is_keyframe = apackets[i].is_keyframe,
                .is_discard = apackets[i].is_discard,
                .payload = apackets[i].payload, /* borrowed for push_packet only */
                .payload_len = apackets[i].payload_len,
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
            return EXIT_FAILURE;
        }

        printf("recorded %d video frame(s) (%ux%u) + %zu AAC packet(s) "
               "(%u Hz, %u ch) -> %zu bytes of two-track fragmented MP4\n",
               video_frames, width, height, apacket_count, ainfo.sample_rate,
               ainfo.channels, out_len);

        /* Owned outputs: released with the matching frees. */
        mediaway_buffer_free(out_data, out_len);
        mediaway_stream_info_free(&vinfo);
        mediaway_muxer_close(muxer);
        mediaway_demuxer_close(demuxer);
    } else if (video_bytes > 0) {
        printf("recorded %d video frame(s) (%ux%u) -> %zu bytes of video-only "
               "fragmented MP4\n",
               video_frames, width, height, vout_len);
        if (have_mic) {
            printf("mic was available but audio encoding was not (no audio "
                   "backend / no AAC output) — video-only result\n");
        }
    }

    /* ---- Cleanup ----------------------------------------------------------- */

    if (asession != NULL) {
        if (have_audio_track) {
            mediaway_pipeline_ffi_stream_info_free(&ainfo);
            for (size_t i = 0; i < apacket_count; i++) {
                mediaway_pipeline_ffi_packet_free(&apackets[i]);
            }
        }
        mediaway_audio_encode_session_close(asession); /* always safe — no consumption trap */
    }
    if (video_bytes > 0) {
        mediaway_pipeline_ffi_buffer_free(vout, vout_len);
    }
    /* Closing a capture joins the backend's worker thread: can block for up to
     * one frame/period interval (documented cost, not just a pointer free). */
    CHECK_DEV(mediaway_camera_capture_close(vcapture));
    if (have_mic) {
        CHECK_DEV(mediaway_audio_capture_close(acapture));
    }
    return EXIT_SUCCESS;
}
