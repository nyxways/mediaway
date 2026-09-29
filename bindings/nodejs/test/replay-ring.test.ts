/**
 * replay-ring.test.ts — replay ring + MP4 payload placements through the public
 * @mediaway/container API (adr/container/0009-replay-ring-c-abi.md).
 *
 * Hermetic: synthetic packets, no files, no hardware. Layout pins are the numbers from a gcc
 * probe of the real crates/mediaway-ffi/include/mediaway/container.h (sizeof/offsetof), so a
 * reordered koffi field fails here instead of corrupting a call.
 *
 * Opt-out of nothing: needs only the native library. Run: bun run test (from bindings/nodejs).
 */

import assert from "node:assert/strict";
import { container, replayStructLayout } from "@mediaway/ffi";
import {
  Demuxer,
  MediawayError,
  Muxer,
  ReplayOutOfOrderError,
  ReplayPayloadKindError,
  ReplayRing,
  ReplayUnknownStreamError,
} from "@mediaway/container";
import type { Packet, ReplayBytesEntry } from "@mediaway/container";

const VIDEO = 1;
const AUDIO = 2;
const FPS = { num: 1, den: 30 };
const AUDIO_RATE = { num: 1, den: 48000 };

/** Frame `n` of a 30 fps stream: keyframe every 30 frames, payload `[n % 256] x 4`. */
function videoFrame(n: number) {
  return {
    streamId: VIDEO,
    pts: n,
    duration: 1,
    key: n % 30 === 0,
    data: Buffer.alloc(4, n % 256),
  };
}

function audioFrame(k: number) {
  return { streamId: AUDIO, pts: k * 1024, duration: 1024, key: true, data: Buffer.alloc(3, 0xa0) };
}

// ── layout + ABI ───────────────────────────────────────────────────────────────

function testLayoutAndAbi(): void {
  assert.equal(container.abiVersion(), 8, "container ABI version");
  // gcc probe of the real container.h.
  const l = replayStructLayout();
  assert.deepEqual(l.replayRingConfig, {
    size: 48,
    offsets: { anchor_stream_id: 0, anchor_time_base: 8, window_ms: 24, max_bytes: 32, payload_kind: 40 },
  });
  assert.deepEqual(l.packetMeta, {
    size: 40,
    offsets: { stream_id: 0, pts: 8, dts: 16, duration: 24, is_keyframe: 32, is_discard: 33 },
  });
  assert.deepEqual(l.storedPayload, { size: 24, offsets: { file: 0, offset: 8, len: 16 } });
  assert.deepEqual(l.replayClipEntry, {
    size: 80,
    offsets: {
      stream_id: 0, pts: 8, dts: 16, duration: 24, is_keyframe: 32, is_discard: 33,
      payload_kind: 36, payload: 40, payload_len: 48, stored_file: 56, stored_offset: 64, stored_len: 72,
    },
  });
  assert.deepEqual(l.placement, { size: 32, offsets: { track_id: 0, dts: 8, offset: 16, len: 24 } });
  console.log("ok  layout pins + container ABI 8");
}

// ── ring behaviour ─────────────────────────────────────────────────────────────

function testClipShape(): void {
  const ring = ReplayRing.create({ anchorStreamId: VIDEO, anchorTimeBase: FPS, windowMs: 2000, payload: "bytes" });
  ring.addStream(AUDIO, AUDIO_RATE);
  assert.equal(ring.clipLast(2000), null, "no clip before the first keyframe");
  assert.equal(ring.spanMs(), 0);

  for (let n = 0; n < 180; n++) {
    ring.push(videoFrame(n));
    if (n % 2 === 0) ring.push(audioFrame((n / 2) * 3));
  }
  assert.ok(ring.spanMs() >= 2000, `ring holds at least its window, holds ${ring.spanMs()} ms`);

  const clip = ring.clipLast(2000);
  assert.ok(clip);
  const list = clip.entries();
  const video = list.filter((e) => e.streamId === VIDEO);
  assert.ok(video.length > 0 && list.some((e) => e.streamId === AUDIO), "video and audio were cut together");

  assert.equal(video[0].isKeyframe, true, "clip starts at a keyframe");
  assert.equal(video[0].dts, 0, "the cut keyframe decodes at zero");
  assert.equal(video[0].pts, 0);
  const cut = 180 - video.length;
  assert.equal(cut % 30, 0, `clip starts on a GOP boundary, frame ${cut}`);
  for (const e of video) {
    assert.equal(e.kind, "bytes");
    assert.deepEqual((e as ReplayBytesEntry).payload, Buffer.alloc(4, (cut + e.dts) % 256), `payload of frame ${cut + e.dts}`);
  }
  for (let i = 1; i < video.length; i++) assert.equal(video[i].dts, video[i - 1].dts + 1);
  assert.ok(clip.durationMs >= 2000, `clip is at least as long as asked: ${clip.durationMs} ms`);
  assert.equal(clip.length, list.length);
  assert.equal([...clip].length, list.length, "iterable form agrees with entries()");

  clip.close();
  clip.close(); // idempotent
  assert.throws(() => clip.length, MediawayError, "a closed clip refuses reads");
  ring.close();
  ring.close();
  console.log(`ok  clip: ${video.length} video + ${list.length - video.length} audio entries, starts at frame ${cut}`);
}

function testClipIsAnOwnedSnapshot(): void {
  const ring = ReplayRing.create({ anchorStreamId: VIDEO, anchorTimeBase: FPS, windowMs: 1000, payload: "bytes" });
  for (let n = 0; n < 90; n++) ring.push(videoFrame(n));
  const clip = ring.clipLast(1000);
  assert.ok(clip);
  const before = clip.entries();

  // Evict everything the clip was cut from (300 pushes), then drop the ring itself.
  for (let n = 90; n < 390; n++) ring.push(videoFrame(n));
  ring.close();

  assert.deepEqual(clip.entries(), before, "a clip survives further pushes and its ring closing");
  clip.close();
  console.log(`ok  snapshot: ${before.length} entries survived 300 more pushes and ring.close()`);
}

function testStoredRing(): void {
  const ring = ReplayRing.create({ anchorStreamId: VIDEO, anchorTimeBase: FPS, windowMs: 2000, payload: "stored" });
  assert.equal(ring.payload, "stored");
  for (let n = 0; n < 100; n++) {
    ring.pushStored(
      { streamId: VIDEO, pts: n, duration: 1, key: n % 30 === 0 },
      { file: 7, offset: n * 100, length: 40 }
    );
  }
  const clip = ring.clipLast(1000);
  assert.ok(clip);
  const list = clip.entries();
  const cut = 99 - list[list.length - 1].dts;
  for (const e of list) {
    assert.equal(e.kind, "stored");
    assert.deepEqual(e.kind === "stored" && e.stored, { file: 7, offset: (cut + e.dts) * 100, length: 40 });
  }
  clip.close();
  ring.close();
  console.log(`ok  stored ring: ${list.length} locations, no bytes`);
}

function testErrors(): void {
  const bytesRing = ReplayRing.create({ anchorStreamId: VIDEO, anchorTimeBase: FPS, windowMs: 1000, payload: "bytes" });
  const storedRing = ReplayRing.create({ anchorStreamId: VIDEO, anchorTimeBase: FPS, windowMs: 1000, payload: "stored" });

  // dts went backwards: a droppable error; the ring is still usable.
  bytesRing.push(videoFrame(0));
  bytesRing.push(videoFrame(10));
  assert.throws(() => bytesRing.push(videoFrame(4)), ReplayOutOfOrderError);
  bytesRing.push(videoFrame(11));
  assert.throws(() => bytesRing.push(videoFrame(4)), (e) => e instanceof MediawayError && e.status === 4);

  assert.throws(() => bytesRing.push(audioFrame(0)), ReplayUnknownStreamError, "a stream nobody added");
  assert.throws(() => bytesRing.addStream(VIDEO, FPS), (e) => e instanceof MediawayError && e.status === 3, "the anchor is already there");
  assert.throws(() => bytesRing.addStream(AUDIO, { num: 0, den: 1 }), (e) => e instanceof MediawayError && e.status === 1, "degenerate timebase");

  // Wrong payload kind, both directions.
  assert.throws(() => storedRing.push(videoFrame(0)), ReplayPayloadKindError);
  assert.throws(
    () => bytesRing.pushStored({ streamId: VIDEO, pts: 0, duration: 1, key: true }, { file: 0, offset: 0, length: 1 }),
    ReplayPayloadKindError
  );
  assert.throws(
    () => ReplayRing.create({ anchorStreamId: VIDEO, anchorTimeBase: { num: 1, den: 0 }, windowMs: 1, payload: "bytes" }),
    (e) => e instanceof MediawayError && e.status === 1,
    "a zero-denominator anchor timebase"
  );

  bytesRing.close();
  storedRing.close();
  assert.throws(() => bytesRing.spanMs(), MediawayError, "a closed ring refuses calls");
  console.log("ok  errors: out-of-order, unknown stream, wrong payload kind, bad timebase");
}

// ── placements ─────────────────────────────────────────────────────────────────

/** Deterministic fake H.264 payload (annex-B start code + NAL header + filler). */
function fakeVideo(i: number): Buffer {
  const size = 64 + ((i * 37) % 512);
  const b = Buffer.alloc(size);
  b.writeUInt32BE(1, 0);
  b[4] = i % 30 === 0 ? 0x65 : 0x41;
  for (let j = 5; j < size; j++) b[j] = (i * 13 + j) & 0xff;
  return b;
}

/** Deterministic fake AAC frame with a 7-byte ADTS header. */
function fakeAudio(i: number): Buffer {
  const size = 96 + ((i * 17) % 256);
  const b = Buffer.alloc(size);
  b[0] = 0xff;
  b[1] = 0xf1;
  b[2] = 0x50;
  b[3] = 0x80 | ((size >> 11) & 0x03);
  b[4] = (size >> 3) & 0xff;
  b[5] = ((size & 0x07) << 5) | 0x1f;
  b[6] = 0xfc;
  for (let j = 7; j < size; j++) b[j] = (i * 29 + j) & 0xff;
  return b;
}

/** Mux 60 video + 60 audio packets; return every output byte and the placements (if recorded). */
function muxAll(muxer: Muxer, takePlacements: boolean) {
  const video = muxer.addVideoTrack({ codec: "h264", width: 640, height: 480, timeBase: FPS });
  const audio = muxer.addAudioTrack({ codec: "aac", sampleRate: 48000, channels: 2, timeBase: AUDIO_RATE });
  const chunks: Buffer[] = [muxer.begin()];
  const pushed: { track: number; index: number; dts: number }[] = [];
  for (let i = 0; i < 60; i++) {
    muxer.push({ trackIndex: video, data: fakeVideo(i), pts: i, duration: 1, key: i % 30 === 0 } satisfies Packet);
    pushed.push({ track: video, index: i, dts: i });
  }
  for (let i = 0; i < 60; i++) {
    muxer.push({ trackIndex: audio, data: fakeAudio(i), pts: i * 1024, duration: 1024, key: true } satisfies Packet);
    pushed.push({ track: audio, index: i, dts: i * 1024 });
  }
  muxer.flush();
  chunks.push(muxer.pollBytes());
  const placements = takePlacements ? muxer.pollPlacements() : [];
  return { file: Buffer.concat(chunks), placements, video, audio };
}

function testPlacements(): void {
  const plain = new Muxer();
  const plainOut = muxAll(plain, false);
  plain.close();

  const recording = Muxer.createWithPlacements();
  const { file, placements, video, audio } = muxAll(recording, true);
  recording.close();

  assert.deepEqual(file, plainOut.file, "recording placements must not change the output bytes");
  assert.equal(placements.length, 120, "one placement per sample");

  // Every placement points at the bytes as written: the AVCC form of the video packet, and the
  // AAC frame minus its ADTS header.
  let videoSeen = 0;
  let audioSeen = 0;
  for (const p of placements) {
    const bytes = file.subarray(p.offset, p.offset + p.length);
    if (p.trackIndex === video) {
      const annexB = fakeVideo(videoSeen++);
      const avcc = Buffer.alloc(annexB.length);
      avcc.writeUInt32BE(annexB.length - 4, 0);
      annexB.copy(avcc, 4, 4);
      assert.deepEqual(bytes, avcc, `video placement ${videoSeen - 1}`);
    } else {
      assert.equal(p.trackIndex, audio);
      assert.deepEqual(bytes, fakeAudio(audioSeen++).subarray(7), `audio placement ${audioSeen - 1}`);
    }
  }
  assert.equal(videoSeen + audioSeen, 120);

  // A WebM muxer records none, and cannot be asked to.
  assert.throws(() => new Muxer("webm", { placements: true }), MediawayError);
  const webm = new Muxer("webm");
  assert.throws(() => webm.pollPlacements(), (e) => e instanceof MediawayError && e.status === 2);
  webm.close();
  console.log(`ok  placements: ${placements.length} rows point at the real bytes; output identical to a plain muxer`);
}

/** The full C ABI loop: mux -> demux -> ring (stored, fed from placements) -> same clip as bytes. */
function testStoredMatchesBytes(): void {
  const recording = Muxer.createWithPlacements();
  const { file, placements, video } = muxAll(recording, true);
  recording.close();

  const bytesRing = ReplayRing.create({ anchorStreamId: video, anchorTimeBase: FPS, windowMs: 1500, payload: "bytes" });
  const storedRing = ReplayRing.create({ anchorStreamId: video, anchorTimeBase: FPS, windowMs: 1500, payload: "stored" });
  const videoPlacements = placements.filter((p) => p.trackIndex === video);
  for (let i = 0; i < videoPlacements.length; i++) {
    const p = videoPlacements[i];
    const meta = { streamId: video, pts: i, duration: 1, key: i % 30 === 0 };
    bytesRing.push({ ...meta, data: file.subarray(p.offset, p.offset + p.length) });
    storedRing.pushStored(meta, { file: 1, offset: p.offset, length: p.length });
  }
  const a = bytesRing.clipLast(1000)!.entries();
  const b = storedRing.clipLast(1000)!.entries();
  assert.equal(a.length, b.length);
  for (let i = 0; i < a.length; i++) {
    const x = a[i] as ReplayBytesEntry;
    const y = b[i];
    assert.equal(y.kind, "stored");
    assert.deepEqual([x.pts, x.dts, x.isKeyframe], [y.pts, y.dts, y.isKeyframe]);
    if (y.kind === "stored") {
      assert.deepEqual(file.subarray(y.stored.offset, y.stored.offset + y.stored.length), x.payload, "stored location reads back the bytes the bytes-ring holds");
    }
  }
  bytesRing.close();
  storedRing.close();
  // Sanity that the file demuxes: the ring's inputs were real muxer output.
  const d = new Demuxer();
  d.pushBytes(file);
  assert.equal(d.streams().length, 2);
  d.close();
  console.log(`ok  stored ring cuts the same ${a.length}-packet clip as the bytes ring`);
}

/**
 * The documented contract (container.h, ADR-0009 §4): a WebM muxer never records and an Open muxer
 * has not started, both INVALID_STATE; a plain MP4 muxer, one not created with placements, records
 * nothing and returns an empty array, the same answer the Rust API gives.
 */
function testPlainMuxerContract(): void {
  const plain = new Muxer();
  muxAll(plain, false);
  // A plain MP4 muxer records nothing and returns an empty array, as the Rust API does.
  assert.deepEqual(plain.pollPlacements(), [], "a plain MP4 muxer records none");
  plain.close();
  console.log("ok  plain MP4 muxer returns no placements");
}

testLayoutAndAbi();
testClipShape();
testClipIsAnOwnedSnapshot();
testStoredRing();
testErrors();
testPlacements();
testStoredMatchesBytes();
testPlainMuxerContract();
console.log("PASS: replay ring + payload placements (container ABI 8)");
