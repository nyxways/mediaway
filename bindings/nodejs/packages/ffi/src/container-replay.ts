/**
 * Replay ring + MP4 payload placements (adr/container/0009-replay-ring-c-abi.md).
 *
 * Split out of index.ts, which sits under the workspace's 1000-line source cap. This module
 * imports only leaf modules (koffi, loader), never index.ts, so there is no import cycle:
 * index.ts calls {@link createReplayBindings} once, after it has defined `MwRational` and
 * `MwPacketView`, whose koffi names the signatures below refer to.
 */

import koffi, { type TypeObject } from "koffi";
import { containerLib } from "./loader.js";

/** `mediaway_replay_payload_kind_t`. */
export const REPLAY_PAYLOAD_BYTES = 0;
export const REPLAY_PAYLOAD_STORED = 1;

/** `mediaway_replay_ring_config_t`. */
export interface RawReplayRingConfig {
  anchor_stream_id: number;
  anchor_time_base: { num: bigint | number; den: number };
  window_ms: bigint | number;
  max_bytes: bigint | number;
  payload_kind: number;
}

/** `mediaway_packet_meta_t`. */
export interface RawPacketMeta {
  stream_id: number;
  pts: bigint | number;
  dts: bigint | number;
  duration: bigint | number;
  is_keyframe: boolean;
  is_discard: boolean;
}

/** `mediaway_stored_payload_t`. */
export interface RawStoredPayload {
  file: number;
  offset: bigint | number;
  len: number;
}

/** `mediaway_replay_clip_entry_t`. `payload` is BORROWED from the clip. */
export interface RawReplayClipEntry {
  stream_id: number;
  pts: bigint;
  dts: bigint;
  duration: bigint;
  is_keyframe: boolean;
  is_discard: boolean;
  payload_kind: number;
  payload: unknown;
  payload_len: number;
  stored_file: number;
  stored_offset: bigint;
  stored_len: number;
}

/** `mediaway_placement_t`. */
export interface RawPlacement {
  track_id: number;
  dts: bigint;
  offset: bigint;
  len: number;
}

/**
 * Declare the replay structs and bind the functions.
 *
 * `rational` is index.ts's `MwRational` (the struct is registered there once; koffi rejects a
 * second definition under the same name).
 */
export function createReplayBindings(rational: TypeObject) {
  const MwReplayRingConfig = koffi.struct("MwReplayRingConfig", {
    anchor_stream_id: "uint32",
    anchor_time_base: rational,
    window_ms: "uint64",
    max_bytes: "uint64",
    payload_kind: "int32",
  });
  const MwPacketMeta = koffi.struct("MwPacketMeta", {
    stream_id: "uint32",
    pts: "int64",
    dts: "int64",
    duration: "uint64",
    is_keyframe: "bool",
    is_discard: "bool",
  });
  const MwStoredPayload = koffi.struct("MwStoredPayload", {
    file: "uint32",
    offset: "uint64",
    len: "uint32",
  });
  const MwReplayClipEntry = koffi.struct("MwReplayClipEntry", {
    stream_id: "uint32",
    pts: "int64",
    dts: "int64",
    duration: "uint64",
    is_keyframe: "bool",
    is_discard: "bool",
    payload_kind: "int32",
    payload: "uint8_t *",
    payload_len: "size_t",
    stored_file: "uint32",
    stored_offset: "uint64",
    stored_len: "uint32",
  });
  const MwPlacement = koffi.struct("MwPlacement", {
    track_id: "uint32",
    dts: "int64",
    offset: "uint64",
    len: "uint32",
  });

  return {
    structs: { MwReplayRingConfig, MwPacketMeta, MwStoredPayload, MwReplayClipEntry, MwPlacement },
    ringCreate: containerLib.func(
      "int mediaway_replay_ring_create(MwReplayRingConfig *config, _Out_ void **out_ring)"
    ),
    ringAddStream: containerLib.func(
      "int mediaway_replay_ring_add_stream(void *ring, uint32_t stream_id, MwRational time_base)"
    ),
    ringPush: containerLib.func("int mediaway_replay_ring_push(void *ring, MwPacketView *packet)"),
    ringPushStored: containerLib.func(
      "int mediaway_replay_ring_push_stored(void *ring, MwPacketMeta *meta, MwStoredPayload *stored)"
    ),
    ringSpanMs: containerLib.func(
      "int mediaway_replay_ring_span_ms(void *ring, _Out_ uint64_t *out_span_ms)"
    ),
    ringClipLast: containerLib.func(
      "int mediaway_replay_ring_clip_last(void *ring, uint64_t span_ms, _Out_ void **out_clip, _Out_ bool *out_has)"
    ),
    ringClose: containerLib.func("void mediaway_replay_ring_close(void *ring)"),
    clipPacketCount: containerLib.func("size_t mediaway_replay_clip_packet_count(void *clip)"),
    clipDurationMs: containerLib.func(
      "int mediaway_replay_clip_duration_ms(void *clip, _Out_ uint64_t *out_ms)"
    ),
    clipPacketAt: containerLib.func(
      "int mediaway_replay_clip_packet_at(void *clip, size_t index, _Out_ MwReplayClipEntry *out_entry)"
    ),
    clipFree: containerLib.func("void mediaway_replay_clip_free(void *clip)"),
    muxerCreateWithPlacements: containerLib.func("void *mediaway_muxer_create_with_placements()"),
    muxerPollPlacements: containerLib.func(
      "int mediaway_muxer_poll_placements(void *muxer, _Out_ MwPlacement **out_rows, _Out_ size_t *out_count)"
    ),
    placementsFree: containerLib.func("void mediaway_placements_free(MwPlacement *rows, size_t count)"),
  };
}

/**
 * `sizeof` and member offsets of the replay structs as koffi lays them out, for the test that
 * pins them against a gcc probe of the real header.
 */
export function replayLayout(structs: ReturnType<typeof createReplayBindings>["structs"]) {
  const describe = (type: TypeObject, members: string[]) => ({
    size: koffi.sizeof(type) as number,
    offsets: Object.fromEntries(members.map((m) => [m, koffi.offsetof(type, m) as number])),
  });
  return {
    replayRingConfig: describe(structs.MwReplayRingConfig, [
      "anchor_stream_id", "anchor_time_base", "window_ms", "max_bytes", "payload_kind",
    ]),
    packetMeta: describe(structs.MwPacketMeta, [
      "stream_id", "pts", "dts", "duration", "is_keyframe", "is_discard",
    ]),
    storedPayload: describe(structs.MwStoredPayload, ["file", "offset", "len"]),
    replayClipEntry: describe(structs.MwReplayClipEntry, [
      "stream_id", "pts", "dts", "duration", "is_keyframe", "is_discard", "payload_kind",
      "payload", "payload_len", "stored_file", "stored_offset", "stored_len",
    ]),
    placement: describe(structs.MwPlacement, ["track_id", "dts", "offset", "len"]),
  };
}
