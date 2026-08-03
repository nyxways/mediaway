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

import koffi from "koffi";
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

export const containerLib = load("mediaway_container_ffi.dll");
export const pipelineLib = load("mediaway_pipeline_ffi.dll");
export const deviceLib = load("mediaway_device_ffi.dll");

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
  duration: bigint | number;
  is_keyframe: boolean;
  is_discard: boolean;
  payload: unknown;
  payload_len: number;
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
