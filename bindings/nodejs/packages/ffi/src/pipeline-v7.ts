/**
 * Pipeline ABI v7 additions — `crates/mediaway-ffi/adr/pipeline/0007-stream-bytes-aac-decode-
 * support-probe.md`: streaming fMP4 bytes, AAC decode, and the capability probes.
 *
 * Split out of index.ts for the workspace's 1000-line source cap. It imports only leaf modules
 * (`rational.ts`, `loader.ts`), never `index.ts`, so `index.ts` can re-export it without a cycle.
 *
 * Ownership (from `pipeline.h`):
 *   - `mediaway_encode_session_poll_bytes` returns an OWNED buffer (release it with
 *     `pipeline.bufferFree`); nothing ready is `NULL` / `0` and needs no free.
 *   - `mediaway_encoder_support_at` returns an OWNED array; release it with
 *     `mediaway_encoder_support_free`, passing back the same pointer and count.
 *   - `mediaway_audio_decode_config_t.extra_data` is BORROWED, valid for `open()` only.
 */

import koffi from "koffi";
import { pipelineLib } from "./loader.js";
import { MwRational } from "./rational.js";

/** `mediaway_audio_decode_config_t` — Opus passes `extra_data = NULL`, AAC the raw ASC. */
export const MwAudioDecodeConfig = koffi.struct("MwAudioDecodeConfig", {
  codec: "int32", // Opus (5) or AAC (4)
  sample_rate: "uint32",
  channels: "uint16",
  time_base: MwRational,
  extra_data: "uint8_t *", // borrowed, open-call only; AAC's AudioSpecificConfig
  extra_data_len: "size_t",
});

/** `mediaway_encoder_capability_t` — one backend row of `mediaway_encoder_support_at`. */
export const MwEncoderCapability = koffi.struct("MwEncoderCapability", {
  backend: "int32",
  state: "int32",
  path_class: "int32", // meaningful only when state == SUPPORTED
});

/** `mediaway_encoder_capability_t` as decoded by koffi. */
export interface RawEncoderCapability {
  backend: number;
  state: number;
  path_class: number;
}

/** Codec numbers the probes and AAC decode accept (`mediaway_pipeline_codec_kind_t`). */
export const PIPELINE_CODEC = { h264: 0, hevc: 1, av1: 2, vp9: 3, aac: 4, opus: 5 } as const;

export const pipelineV7 = {
  audioDecodeConfigAac: pipelineLib.func(
    "MwAudioDecodeConfig mediaway_audio_decode_config_aac(uint32_t sample_rate, uint16_t channels, MwRational time_base, uint8_t *extra_data, size_t extra_data_len)"
  ),
  sessionPollBytes: pipelineLib.func(
    "int mediaway_encode_session_poll_bytes(void *session, _Out_ uint8_t **out_data, _Out_ size_t *out_len)"
  ),
  encoderSupportAt: pipelineLib.func(
    "int mediaway_encoder_support_at(int codec, uint32_t width, uint32_t height, _Out_ void **out_rows, _Out_ size_t *out_count)"
  ),
  encoderSupportFree: pipelineLib.func("void mediaway_encoder_support_free(void *rows, size_t count)"),
  decoderSupport: pipelineLib.func(
    "int mediaway_decoder_support(int codec, _Out_ int *out_state)"
  ),
};

/**
 * Size and field offsets of a registered struct, for pinning a mirror against the C header's
 * layout in a test. `fields` are read in the order given.
 */
export function structLayout(type: unknown, fields: readonly string[]): { size: number; offsets: Record<string, number> } {
  const offsets: Record<string, number> = {};
  for (const field of fields) offsets[field] = koffi.offsetof(type, field);
  return { size: koffi.sizeof(type), offsets };
}
