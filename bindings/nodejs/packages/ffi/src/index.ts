/**
 * @mediaway/ffi — internal koffi glue over Mediaway's C ABI.
 *
 * Not part of the public DX contract (bindings/nodejs/README.md): this package
 * exists only so @mediaway/container, @mediaway/encoder, and @mediaway/device
 * share one set of struct layouts and function bindings. Everything here maps
 * 1:1 to `crates/mediaway-*-ffi/include/mediaway/*.h`.
 *
 * Ownership rules (from the headers):
 *   - Borrowed inputs (extra_data, packet payload, push_bytes data, frame
 *     bytes) are caller-owned, valid for the call only — we copy in/out.
 *   - Owned outputs (poll_bytes buffers, demuxed packets/stream info, finish
 *     buffers, polled device frames) MUST be released through the matching
 *     `_free` — the public packages do this automatically.
 */

import koffi, { type TypeObject } from "koffi";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// ── Library discovery ──────────────────────────────────────────────────────────
// The cdylibs are Rust build artifacts, not installed system libraries. Search:
//   1. $MEDIAWAY_FFI_DIR
//   2. <this package>/native        (DLLs bundled at pack time — the npm distribution)
//   3. <repo root>/target/x86_64-pc-windows-gnu/debug   (GNU toolchain, dev runs)
//   4. <repo root>/target/debug                          (host/MSVC toolchain)
//   5. cwd
const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..", "..", "..", "..");
const searchDirs = [
  process.env.MEDIAWAY_FFI_DIR ?? "",
  path.resolve(here, "..", "native"),
  path.join(repoRoot, "target", "x86_64-pc-windows-gnu", "debug"),
  path.join(repoRoot, "target", "debug"),
  process.cwd(),
];

export function findLibrary(name: string): string {
  for (const dir of searchDirs) {
    if (!dir) continue;
    const candidate = path.join(dir, name);
    if (fs.existsSync(candidate)) return candidate;
  }
  throw new Error(
    `cannot find ${name}; set $MEDIAWAY_FFI_DIR or build the -ffi crates`
  );
}

function load(name: string) {
  return koffi.load(findLibrary(name));
}

export const containerLib = load("mediaway_ffi.dll");
export const pipelineLib = load("mediaway_ffi.dll");
export const deviceLib = load("mediaway_ffi.dll");

// ── Structs (layouts mirror the headers exactly) ───────────────────────────────

export const MwRational = koffi.struct("MwRational", {
  num: "uint64",
  den: "uint32",
});

export const MwVideoTrackInfo = koffi.struct("MwVideoTrackInfo", {
  id: "uint32",
  codec: "int32",
  time_base: MwRational,
  width: "uint32",
  height: "uint32",
  extra_data: "uint8_t *",
  extra_data_len: "size_t",
});

export const MwAudioTrackInfo = koffi.struct("MwAudioTrackInfo", {
  id: "uint32",
  codec: "int32",
  time_base: MwRational,
  sample_rate: "uint32",
  channels: "uint16",
  extra_data: "uint8_t *",
  extra_data_len: "size_t",
});

export const MwPacketView = koffi.struct("MwPacketView", {
  stream_id: "uint32",
  pts: "int64",
  dts: "int64",
  duration: "uint64",
  is_keyframe: "bool",
  is_discard: "bool",
  payload: "uint8_t *",
  payload_len: "size_t",
});

export const MwPacket = koffi.struct("MwPacket", {
  stream_id: "uint32",
  pts: "int64",
  dts: "int64",
  duration: "uint64",
  is_keyframe: "bool",
  is_discard: "bool",
  payload: "uint8_t *",
  payload_len: "size_t",
});

export const MwTsElementaryStream = koffi.struct("MwTsElementaryStream", {
  pid: "uint16",
  codec: "int32",
});

export const MwMp3FrameHeader = koffi.struct("MwMp3FrameHeader", {
  version: "int32",
  bitrate_kbps: "uint16",
  sample_rate: "uint32",
  channel_mode: "int32",
});

export const MwWaveFormat = koffi.struct("MwWaveFormat", {
  sample_format: "int32",
  channels: "uint16",
  sample_rate: "uint32",
  bits_per_sample: "uint16",
});

export const MwStreamInfo = koffi.struct("MwStreamInfo", {
  id: "uint32",
  codec: "int32",
  time_base: MwRational,
  has_geometry: "bool",
  width: "uint32",
  height: "uint32",
  sample_rate: "uint32",
  channels: "uint16",
  extra_data: "uint8_t *",
  extra_data_len: "size_t",
});

export const MwGpuDeviceHandle = koffi.struct("MwGpuDeviceHandle", {
  kind: "int32",
  native: "uintptr_t",
  webgpu_device_id: "uint64",
});

export const MwGpuBufferHandle = koffi.struct("MwGpuBufferHandle", {
  kind: "int32",
  native_a: "uintptr_t",
  native_b: "uintptr_t",
  subresource: "uint32",
  webgpu_texture_id: "uint64",
});

export const MwEncConfig = koffi.struct("MwEncConfig", {
  codec: "int32",
  width: "uint32",
  height: "uint32",
  time_base: MwRational,
  bitrate_bps: "uint32",
  pixel_format: "int32",
  gpu_device: MwGpuDeviceHandle,
});

export const MwPipelineFrame = koffi.struct("MwPipelineFrame", {
  pts: "int64",
  duration: "uint64",
  width: "uint32",
  height: "uint32",
  pixel_format: "int32",
  storage_kind: "int32",
  raw_bytes: "uint8_t *",
  raw_bytes_len: "size_t",
  gpu_buffer: MwGpuBufferHandle,
});

export const MwCameraConfig = koffi.struct("MwCameraConfig", {
  device_index: "uint32",
  time_base: MwRational,
});

export const MwCameraFrame = koffi.struct("MwCameraFrame", {
  pts: "int64",
  duration: "uint64",
  width: "uint32",
  height: "uint32",
  pixel_format: "int32",
  data: "uint8_t *",
  data_len: "size_t",
});

export const MwDesktopConfig = koffi.struct("MwDesktopConfig", {
  source_kind: "int32",
  source_index: "uint32",
  time_base: MwRational,
  gpu_device: MwGpuDeviceHandle,
});

export const MwDesktopFrame = koffi.struct("MwDesktopFrame", {
  pts: "int64",
  duration: "uint64",
  width: "uint32",
  height: "uint32",
  pixel_format: "int32",
  storage_kind: "int32",
  data: "uint8_t *",
  data_len: "size_t",
  gpu_buffer: MwGpuBufferHandle,
});

export const MwAudioConfig = koffi.struct("MwAudioConfig", {
  device_index: "uint32",
  time_base: MwRational,
  sample_format: "int32",
});

export const MwAudioFrame = koffi.struct("MwAudioFrame", {
  pts: "int64",
  duration: "uint64",
  sample_rate: "uint32",
  channels: "uint16",
  sample_format: "int32",
  data: "uint8_t *",
  data_len: "size_t",
});

export const MwAudioEncodeConfig = koffi.struct("MwAudioEncodeConfig", {
  codec: "int32",
  sample_rate: "uint32",
  channels: "uint16",
  sample_format: "int32",
  time_base: MwRational,
  bitrate_bps: "uint32",
});

export const MwAudioFrameView = koffi.struct("MwAudioFrameView", {
  pts: "int64",
  duration: "uint64",
  sample_rate: "uint32",
  channels: "uint16",
  sample_format: "int32",
  data: "uint8_t *",
  data_len: "size_t",
});

export const MwAudioPacket = koffi.struct("MwAudioPacket", {
  pts: "int64",
  dts: "int64",
  duration: "uint64",
  is_keyframe: "bool",
  is_discard: "bool",
  payload: "uint8_t *",
  payload_len: "size_t",
});

export const MwAudioStreamInfo = koffi.struct("MwAudioStreamInfo", {
  codec: "int32",
  time_base: MwRational,
  sample_rate: "uint32",
  channels: "uint16",
  extra_data: "uint8_t *",
  extra_data_len: "size_t",
});

// ── Container functions ────────────────────────────────────────────────────────

export const container = {
  abiVersion: containerLib.func("uint32_t mediaway_container_ffi_abi_version()"),
  muxerCreate: containerLib.func("void *mediaway_muxer_create()"),
  muxerCreateForFormat: containerLib.func("void *mediaway_muxer_create_for_format(int format)"),
  muxerCreateWithBatch:
    containerLib.func("void *mediaway_muxer_create_with_fragment_batch(size_t batch)"),
  muxerAddVideoTrack: containerLib.func(
    "int mediaway_muxer_add_video_track(void *muxer, MwVideoTrackInfo *info)"
  ),
  muxerAddAudioTrack: containerLib.func(
    "int mediaway_muxer_add_audio_track(void *muxer, MwAudioTrackInfo *info)"
  ),
  muxerBegin: containerLib.func("int mediaway_muxer_begin(void *muxer)"),
  muxerPushPacket: containerLib.func(
    "int mediaway_muxer_push_packet(void *muxer, MwPacketView *packet)"
  ),
  muxerFlush: containerLib.func("int mediaway_muxer_flush(void *muxer)"),
  muxerPollBytes: containerLib.func(
    "int mediaway_muxer_poll_bytes(void *muxer, _Out_ uint8_t **out_data, _Out_ size_t *out_len)"
  ),
  muxerClose: containerLib.func("void mediaway_muxer_close(void *muxer)"),
  demuxerCreate: containerLib.func("void *mediaway_demuxer_create()"),
  demuxerCreateForFormat: containerLib.func("void *mediaway_demuxer_create_for_format(int format)"),
  demuxerPushBytes: containerLib.func(
    "int mediaway_demuxer_push_bytes(void *demuxer, uint8_t *data, size_t len)"
  ),
  demuxerStreamCount: containerLib.func("size_t mediaway_demuxer_stream_count(void *demuxer)"),
  demuxerStreamAt: containerLib.func(
    "int mediaway_demuxer_stream_at(void *demuxer, size_t index, _Out_ MwStreamInfo *out_info)"
  ),
  demuxerPollPacket: containerLib.func(
    "int mediaway_demuxer_poll_packet(void *demuxer, _Out_ MwPacket *out_packet, _Out_ bool *out_has)"
  ),
  demuxerSetDecryptionKey: containerLib.func(
    "int mediaway_demuxer_set_decryption_key(void *demuxer, uint8_t *key, size_t key_len)"
  ),
  demuxerClearDecryptionKey:
    containerLib.func("int mediaway_demuxer_clear_decryption_key(void *demuxer)"),
  demuxerClose: containerLib.func("void mediaway_demuxer_close(void *demuxer)"),
  bufferFree: containerLib.func("void mediaway_buffer_free(uint8_t *data, size_t len)"),
  packetFree: containerLib.func("void mediaway_packet_free(MwPacket *packet)"),
  streamInfoFree: containerLib.func("void mediaway_stream_info_free(MwStreamInfo *info)"),

  // ── Ogg (adr/container/0004) ──────────────────────────────────────────────
  oggMuxerCreate: containerLib.func("void *mediaway_ogg_muxer_create(uint32_t serial)"),
  oggMuxerPushPacket: containerLib.func(
    "int mediaway_ogg_muxer_push_packet(void *muxer, MwPacketView *packet)"
  ),
  oggMuxerFlush: containerLib.func("int mediaway_ogg_muxer_flush(void *muxer)"),
  oggMuxerPollBytes: containerLib.func(
    "int mediaway_ogg_muxer_poll_bytes(void *muxer, _Out_ uint8_t **out_data, _Out_ size_t *out_len)"
  ),
  oggMuxerClose: containerLib.func("void mediaway_ogg_muxer_close(void *muxer)"),
  oggDemuxerCreate: containerLib.func("void *mediaway_ogg_demuxer_create()"),
  oggDemuxerPushBytes: containerLib.func(
    "int mediaway_ogg_demuxer_push_bytes(void *demuxer, uint8_t *data, size_t len)"
  ),
  oggDemuxerStreamCount: containerLib.func("size_t mediaway_ogg_demuxer_stream_count(void *demuxer)"),
  oggDemuxerStreamAt: containerLib.func(
    "int mediaway_ogg_demuxer_stream_at(void *demuxer, size_t index, _Out_ MwStreamInfo *out_info)"
  ),
  oggDemuxerPollPacket: containerLib.func(
    "int mediaway_ogg_demuxer_poll_packet(void *demuxer, _Out_ MwPacket *out_packet, _Out_ bool *out_has)"
  ),
  oggDemuxerClose: containerLib.func("void mediaway_ogg_demuxer_close(void *demuxer)"),

  // ── ADTS (adr/container/0004) ─────────────────────────────────────────────
  adtsMuxerCreate: containerLib.func("void *mediaway_adts_muxer_create(uint32_t sample_rate, uint8_t channels)"),
  adtsMuxerPushPacket: containerLib.func(
    "int mediaway_adts_muxer_push_packet(void *muxer, MwPacketView *packet)"
  ),
  adtsMuxerFlush: containerLib.func("int mediaway_adts_muxer_flush(void *muxer)"),
  adtsMuxerPollBytes: containerLib.func(
    "int mediaway_adts_muxer_poll_bytes(void *muxer, _Out_ uint8_t **out_data, _Out_ size_t *out_len)"
  ),
  adtsMuxerClose: containerLib.func("void mediaway_adts_muxer_close(void *muxer)"),
  adtsDemuxerCreate: containerLib.func("void *mediaway_adts_demuxer_create()"),
  adtsDemuxerPushBytes: containerLib.func(
    "int mediaway_adts_demuxer_push_bytes(void *demuxer, uint8_t *data, size_t len)"
  ),
  adtsDemuxerStreamCount: containerLib.func("size_t mediaway_adts_demuxer_stream_count(void *demuxer)"),
  adtsDemuxerStreamAt: containerLib.func(
    "int mediaway_adts_demuxer_stream_at(void *demuxer, size_t index, _Out_ MwStreamInfo *out_info)"
  ),
  adtsDemuxerPollPacket: containerLib.func(
    "int mediaway_adts_demuxer_poll_packet(void *demuxer, _Out_ MwPacket *out_packet, _Out_ bool *out_has)"
  ),
  adtsDemuxerClose: containerLib.func("void mediaway_adts_demuxer_close(void *demuxer)"),

  // ── FLV (adr/container/0005) ──────────────────────────────────────────────
  flvMuxerCreate: containerLib.func("void *mediaway_flv_muxer_create()"),
  flvMuxerWriteHeader: containerLib.func(
    "int mediaway_flv_muxer_write_header(void *muxer, bool has_audio, bool has_video, _Out_ uint8_t **out_data, _Out_ size_t *out_len)"
  ),
  flvMuxerAddVideoTrack: containerLib.func(
    "int mediaway_flv_muxer_add_video_track(void *muxer, MwVideoTrackInfo *info)"
  ),
  flvMuxerAddAudioTrack: containerLib.func(
    "int mediaway_flv_muxer_add_audio_track(void *muxer, MwAudioTrackInfo *info)"
  ),
  flvMuxerPushPacket: containerLib.func(
    "int mediaway_flv_muxer_push_packet(void *muxer, MwPacketView *packet, _Out_ uint8_t **out_data, _Out_ size_t *out_len)"
  ),
  flvMuxerClose: containerLib.func("void mediaway_flv_muxer_close(void *muxer)"),
  flvDemuxerCreate: containerLib.func("void *mediaway_flv_demuxer_create()"),
  flvDemuxerPushBytes: containerLib.func(
    "int mediaway_flv_demuxer_push_bytes(void *demuxer, uint8_t *data, size_t len)"
  ),
  flvDemuxerStreamCount: containerLib.func("size_t mediaway_flv_demuxer_stream_count(void *demuxer)"),
  flvDemuxerStreamAt: containerLib.func(
    "int mediaway_flv_demuxer_stream_at(void *demuxer, size_t index, _Out_ MwStreamInfo *out_info)"
  ),
  flvDemuxerPollPacket: containerLib.func(
    "int mediaway_flv_demuxer_poll_packet(void *demuxer, _Out_ MwPacket *out_packet, _Out_ bool *out_has)"
  ),
  flvDemuxerClose: containerLib.func("void mediaway_flv_demuxer_close(void *demuxer)"),

  // ── MPEG-TS (adr/container/0006) ──────────────────────────────────────────
  tsMuxerCreate: containerLib.func(
    "void *mediaway_ts_muxer_create(uint16_t program_number, uint16_t pmt_pid, MwTsElementaryStream *streams, size_t stream_count)"
  ),
  tsMuxerWritePatPmt: containerLib.func(
    "int mediaway_ts_muxer_write_pat_pmt(void *muxer, _Out_ uint8_t **out_data, _Out_ size_t *out_len)"
  ),
  tsMuxerWriteAccessUnit: containerLib.func(
    "int mediaway_ts_muxer_write_access_unit(void *muxer, uint16_t pid, uint8_t *data, size_t data_len, uint64_t pts_90k, bool has_dts, uint64_t dts_90k, bool random_access, _Out_ uint8_t **out_data, _Out_ size_t *out_len)"
  ),
  tsMuxerClose: containerLib.func("void mediaway_ts_muxer_close(void *muxer)"),
  tsDemuxerCreate: containerLib.func("void *mediaway_ts_demuxer_create()"),
  tsDemuxerPushBytes: containerLib.func(
    "int mediaway_ts_demuxer_push_bytes(void *demuxer, uint8_t *data, size_t len)"
  ),
  tsDemuxerStreamCount: containerLib.func("size_t mediaway_ts_demuxer_stream_count(void *demuxer)"),
  tsDemuxerStreamAt: containerLib.func(
    "int mediaway_ts_demuxer_stream_at(void *demuxer, size_t index, _Out_ MwStreamInfo *out_info)"
  ),
  tsDemuxerPollPacket: containerLib.func(
    "int mediaway_ts_demuxer_poll_packet(void *demuxer, _Out_ MwPacket *out_packet, _Out_ bool *out_has)"
  ),
  tsDemuxerFinish: containerLib.func(
    "int mediaway_ts_demuxer_finish(void *demuxer, _Out_ MwPacket **out_packets, _Out_ size_t *out_count)"
  ),
  tsDemuxerFinishFree: containerLib.func("void mediaway_ts_demuxer_finish_free(MwPacket *packets, size_t count)"),
  tsDemuxerClose: containerLib.func("void mediaway_ts_demuxer_close(void *demuxer)"),

  // ── MP3 (adr/container/0007) ──────────────────────────────────────────────
  mp3MuxerCreate: containerLib.func("void *mediaway_mp3_muxer_create(MwMp3FrameHeader *header)"),
  mp3MuxerWriteFrame: containerLib.func(
    "int mediaway_mp3_muxer_write_frame(void *muxer, uint8_t *frame_body, size_t frame_body_len, bool padding, _Out_ uint8_t **out_data, _Out_ size_t *out_len)"
  ),
  mp3MuxerClose: containerLib.func("void mediaway_mp3_muxer_close(void *muxer)"),
  mp3DemuxerCreate: containerLib.func("void *mediaway_mp3_demuxer_create()"),
  mp3DemuxerPushBytes: containerLib.func(
    "int mediaway_mp3_demuxer_push_bytes(void *demuxer, uint8_t *data, size_t len)"
  ),
  mp3DemuxerStreamCount: containerLib.func("size_t mediaway_mp3_demuxer_stream_count(void *demuxer)"),
  mp3DemuxerStreamAt: containerLib.func(
    "int mediaway_mp3_demuxer_stream_at(void *demuxer, size_t index, _Out_ MwStreamInfo *out_info)"
  ),
  mp3DemuxerPollPacket: containerLib.func(
    "int mediaway_mp3_demuxer_poll_packet(void *demuxer, _Out_ MwPacket *out_packet, _Out_ bool *out_has)"
  ),
  mp3DemuxerClose: containerLib.func("void mediaway_mp3_demuxer_close(void *demuxer)"),

  // ── WAV (adr/container/0008) ──────────────────────────────────────────────
  wavMuxerCreate: containerLib.func(
    "void *mediaway_wav_muxer_create(uint32_t sample_rate, uint16_t channels, uint16_t bits_per_sample)"
  ),
  wavMuxerCreateWithFormat: containerLib.func(
    "void *mediaway_wav_muxer_create_with_format(MwWaveFormat *format)"
  ),
  wavMuxerPushPacket: containerLib.func(
    "int mediaway_wav_muxer_push_packet(void *muxer, MwPacketView *packet)"
  ),
  wavMuxerFinish: containerLib.func(
    "int mediaway_wav_muxer_finish(void *muxer, _Out_ uint8_t **out_data, _Out_ size_t *out_len)"
  ),
  wavMuxerClose: containerLib.func("void mediaway_wav_muxer_close(void *muxer)"),
  wavParse: containerLib.func(
    "int mediaway_wav_parse(uint8_t *data, size_t data_len, _Out_ MwStreamInfo *out_info, _Out_ MwPacket *out_packet)"
  ),
};

// ── Pipeline functions ─────────────────────────────────────────────────────────

export const pipeline = {
  abiVersion: pipelineLib.func("uint32_t mediaway_pipeline_ffi_abi_version()"),
  encConfigNew: pipelineLib.func(
    "MwEncConfig mediaway_auto_video_encode_config_new(int codec, uint32_t width, uint32_t height, MwRational time_base)"
  ),
  encConfigH264: pipelineLib.func(
    "MwEncConfig mediaway_auto_video_encode_config_h264(uint32_t width, uint32_t height, MwRational time_base)"
  ),
  autoEncoderOpen: pipelineLib.func(
    "int mediaway_auto_encoder_open(MwEncConfig *config, _Out_ void **out_encoder)"
  ),
  autoEncoderClose: pipelineLib.func("void mediaway_auto_encoder_close(void *encoder)"),
  sessionOpen: pipelineLib.func(
    "int mediaway_encode_session_open(void *encoder, _Out_ void **out_session)"
  ),
  sessionWriteFrame: pipelineLib.func(
    "int mediaway_encode_session_write_frame(void *session, MwPipelineFrame *frame)"
  ),
  sessionFinish: pipelineLib.func(
    "int mediaway_encode_session_finish(void *session, _Out_ uint8_t **out_data, _Out_ size_t *out_len)"
  ),
  sessionClose: pipelineLib.func("void mediaway_encode_session_close(void *session)"),
  bufferFree: pipelineLib.func("void mediaway_pipeline_ffi_buffer_free(uint8_t *data, size_t len)"),
  audioConfigAac: pipelineLib.func(
    "MwAudioEncodeConfig mediaway_audio_encode_config_aac(uint32_t sample_rate, MwRational time_base)"
  ),
  audioEncoderOpen: pipelineLib.func(
    "int mediaway_audio_encoder_open(MwAudioEncodeConfig *config, _Out_ void **out_session)"
  ),
  audioPushPcm: pipelineLib.func(
    "int mediaway_audio_encode_session_push_pcm(void *session, MwAudioFrameView *frame)"
  ),
  audioPollPacket: pipelineLib.func(
    "int mediaway_audio_encode_session_poll_packet(void *session, _Out_ MwAudioPacket *out_packet, _Out_ bool *out_has)"
  ),
  audioFlush: pipelineLib.func("int mediaway_audio_encode_session_flush(void *session)"),
  audioStreamInfo: pipelineLib.func(
    "int mediaway_audio_encode_session_stream_info(void *session, _Out_ MwAudioStreamInfo *out_info)"
  ),
  audioSessionClose: pipelineLib.func("void mediaway_audio_encode_session_close(void *session)"),
  pipelinePacketFree: pipelineLib.func("void mediaway_pipeline_ffi_packet_free(MwAudioPacket *packet)"),
  pipelineStreamInfoFree: pipelineLib.func(
    "void mediaway_pipeline_ffi_stream_info_free(MwAudioStreamInfo *info)"
  ),
};

// ── Device functions ───────────────────────────────────────────────────────────

export const device = {
  abiVersion: deviceLib.func("uint32_t mediaway_device_ffi_abi_version()"),
  cameraConfigDefault: deviceLib.func(
    "MwCameraConfig mediaway_camera_capture_config_default(uint32_t device_index, MwRational time_base)"
  ),
  cameraOpen: deviceLib.func(
    "int mediaway_camera_capture_open(MwCameraConfig *config, _Out_ void **out_capture)"
  ),
  cameraGeometry: deviceLib.func(
    "int mediaway_camera_capture_geometry(void *capture, _Out_ uint32_t *out_width, _Out_ uint32_t *out_height)"
  ),
  cameraPollFrame: deviceLib.func(
    "int mediaway_camera_capture_poll_frame(void *capture, _Out_ MwCameraFrame *out_frame, _Out_ bool *out_has)"
  ),
  cameraPollFrameBlocking: deviceLib.func(
    "int mediaway_camera_capture_poll_frame_blocking(void *capture, uint32_t timeout_ms, _Out_ MwCameraFrame *out_frame)"
  ),
  cameraReleaseFrame: deviceLib.func("int mediaway_camera_capture_release_frame(void *capture)"),
  cameraClose: deviceLib.func("int mediaway_camera_capture_close(void *capture)"),
  cameraFrameFree: deviceLib.func("void mediaway_camera_frame_free(MwCameraFrame *frame)"),
  desktopConfigScreen: deviceLib.func(
    "MwDesktopConfig mediaway_desktop_capture_config_screen(uint32_t output_index, MwRational time_base, MwGpuDeviceHandle gpu_device)"
  ),
  desktopOpen: deviceLib.func(
    "int mediaway_desktop_capture_open(MwDesktopConfig *config, _Out_ void **out_capture)"
  ),
  desktopGeometry: deviceLib.func(
    "int mediaway_desktop_capture_geometry(void *capture, _Out_ uint32_t *out_width, _Out_ uint32_t *out_height)"
  ),
  desktopPollFrame: deviceLib.func(
    "int mediaway_desktop_capture_poll_frame(void *capture, _Out_ MwDesktopFrame *out_frame, _Out_ bool *out_has)"
  ),
  desktopReleaseFrame: deviceLib.func("int mediaway_desktop_capture_release_frame(void *capture)"),
  desktopClose: deviceLib.func("int mediaway_desktop_capture_close(void *capture)"),
  desktopFrameFree: deviceLib.func("void mediaway_desktop_frame_free(MwDesktopFrame *frame)"),
  audioConfigMicrophone: deviceLib.func(
    "MwAudioConfig mediaway_audio_capture_config_microphone(MwRational time_base)"
  ),
  audioOpen: deviceLib.func(
    "int mediaway_audio_capture_open(MwAudioConfig *config, _Out_ void **out_capture)"
  ),
  audioFormat: deviceLib.func(
    "int mediaway_audio_capture_format(void *capture, _Out_ uint32_t *out_sample_rate, _Out_ uint16_t *out_channels)"
  ),
  audioPollFrame: deviceLib.func(
    "int mediaway_audio_capture_poll_frame(void *capture, _Out_ MwAudioFrame *out_frame, _Out_ bool *out_has)"
  ),
  audioClose: deviceLib.func("int mediaway_audio_capture_close(void *capture)"),
  audioFrameFree: deviceLib.func("void mediaway_audio_frame_free(MwAudioFrame *frame)"),
};

// ── Copy helpers ───────────────────────────────────────────────────────────────

/** Copy `len` bytes out of a koffi pointer/opaque handle. */
export function copyBytes(ptr: unknown, len: number): Buffer {
  if (!ptr || len <= 0) return Buffer.alloc(0);
  return Buffer.from(koffi.decode(ptr, "uint8_t", len));
}

/**
 * Decode `count` consecutive struct instances out of a pointer to an owned
 * native array (e.g. `mediaway_ts_demuxer_finish`'s `out_packets`) — the only
 * multi-element owned-array output shape in this crate. Free the underlying
 * array with the matching `_finish_free` after copying out what's needed.
 */
export function decodeArray<T>(ptr: unknown, type: TypeObject, count: number): T[] {
  if (!ptr || count <= 0) return [];
  return koffi.decode(ptr, type, count) as unknown as T[];
}

// ── ABI mirror types ───────────────────────────────────────────────────────────
// TypeScript mirrors of the koffi structs above (fields match the C headers
// verbatim). koffi populates/reads plain JS objects with these shapes; the
// mirror types keep call sites type-safe without depending on koffi's own
// struct typing.

/** `mediaway_rational_t` (u64 num / u32 den). */
export interface RawRational {
  num: bigint | number;
  den: number;
}

/** `mediaway_video_track_info_t` / `mediaway_audio_track_info_t`. */
export interface RawVideoTrackInfo {
  id: number;
  codec: number;
  time_base: RawRational;
  width: number;
  height: number;
  extra_data: unknown;
  extra_data_len: number;
}

export interface RawAudioTrackInfo {
  id: number;
  codec: number;
  time_base: RawRational;
  sample_rate: number;
  channels: number;
  extra_data: unknown;
  extra_data_len: number;
}

/** `mediaway_packet_view_t` (muxer input). */
export interface RawPacketView {
  stream_id: number;
  pts: bigint | number;
  dts: bigint | number;
  duration: bigint | number;
  is_keyframe: boolean;
  is_discard: boolean;
  payload: unknown;
  payload_len: number;
}

/** `mediaway_stream_info_t` (demuxer output; geometry vs. audio via has_geometry). */
export interface RawStreamInfo {
  id: number;
  codec: number;
  time_base: RawRational;
  has_geometry: boolean;
  width: number;
  height: number;
  sample_rate: number;
  channels: number;
  extra_data: unknown;
  extra_data_len: number;
}

/** `mediaway_packet_t` (demuxer output). */
export interface RawPacket {
  stream_id: number;
  pts: bigint | number;
  dts: bigint | number;
  duration: bigint | number;
  is_keyframe: boolean;
  is_discard: boolean;
  payload: unknown;
  payload_len: number;
}

/** `mediaway_ts_elementary_stream_t` (MPEG-TS muxer construction input). */
export interface RawTsElementaryStream {
  pid: number;
  codec: number;
}

/** `mediaway_mp3_frame_header_t` (MP3 muxer construction input). */
export interface RawMp3FrameHeader {
  version: number;
  bitrate_kbps: number;
  sample_rate: number;
  channel_mode: number;
}

/** `mediaway_wave_format_t` (WAV muxer construction input). */
export interface RawWaveFormat {
  sample_format: number;
  channels: number;
  sample_rate: number;
  bits_per_sample: number;
}

/** ABI output structs — fields are filled by the ABI call; callers construct
 * them empty (`{} as RawX`) and read the fields after the call. */

/** `mediaway_camera_frame_t` / `mediaway_audio_frame_t` (device outputs). */
export interface RawCameraFrame {
  pts: bigint | number;
  duration: bigint | number;
  width: number;
  height: number;
  pixel_format: number;
  data: unknown;
  data_len: number;
}

export interface RawAudioFrame {
  pts: bigint | number;
  duration: bigint | number;
  sample_rate: number;
  channels: number;
  sample_format: number;
  data: unknown;
  data_len: number;
}

/** `mediaway_pipeline_frame_t` (encode session input). */
export interface RawPipelineFrame {
  pts: bigint | number;
  duration: bigint | number;
  width: number;
  height: number;
  pixel_format: number;
  storage_kind: number;
  raw_bytes: unknown;
  raw_bytes_len: number;
  gpu_buffer: RawGpuBufferHandle;
}

/** `mediaway_gpu_buffer_handle_t`. */
export interface RawGpuBufferHandle {
  kind: number;
  native_a: number | bigint;
  native_b: number | bigint;
  subresource: number;
  webgpu_texture_id: number | bigint;
}

/** Audio ABI v2 config / frame view / packet (ADR-0003). */
export interface RawAudioEncodeConfig {
  codec: number;
  sample_rate: number;
  channels: number;
  sample_format: number;
  time_base: RawRational;
  bitrate_bps: number;
}

/** `mediaway_audio_stream_info_t` (encoder output). */
export interface RawAudioStreamInfo {
  codec: number;
  time_base: RawRational;
  sample_rate: number;
  channels: number;
  extra_data: unknown;
  extra_data_len: number;
}

export interface RawAudioFrameView {
  pts: bigint | number;
  duration: bigint | number;
  sample_rate: number;
  channels: number;
  sample_format: number;
  data: unknown;
  data_len: number;
}

export interface RawAudioPacket {
  pts: bigint | number;
  dts: bigint | number;
  duration: bigint | number;
  is_keyframe: boolean;
  is_discard: boolean;
  payload: unknown;
  payload_len: number;
}
