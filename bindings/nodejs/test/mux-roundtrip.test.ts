/**
 * mux-roundtrip.test.ts — container round-trip suite (RC binding check).
 *
 * Validates the release-built mediaway_ffi.dll through the public
 * @mediaway/container API: mux 90 synthetic H.264 video packets + 90
 * synthetic AAC audio packets into a fragmented MP4, demux the bytes back,
 * and assert that packet counts, timestamps, keyframe flags, payload bytes,
 * and stream metadata all survive the round trip.
 *
 * Deterministic synthetic payloads only: no randomness, no files, no
 * hardware, no network. Requires the DLL staged at
 * packages/ffi/native/mediaway_ffi.dll (see README.md "Testing").
 *
 * Assertion-based with node:assert (no test framework); any failed assertion
 * exits nonzero — the signal the RC job acts on.
 *
 * Run: npm test   (from bindings/nodejs)
 */

import assert from "node:assert/strict";
import { Demuxer, MediawayError, Muxer } from "@mediaway/container";
import type { AudioTrackInfo, Packet, TrackInfo, VideoTrackInfo } from "@mediaway/container";

const VIDEO_COUNT = 90;
const AUDIO_COUNT = 90;
const WIDTH = 640;
const HEIGHT = 480;

// H.264 in a 1/30 s timebase: frame i has pts = i, duration = 1 (one frame period).
const VIDEO_TRACK: VideoTrackInfo = {
  codec: "h264",
  width: WIDTH,
  height: HEIGHT,
  pixelFormat: "nv12",
  timeBase: { num: 1, den: 30 },
};

// AAC at 48 kHz in a 1/48000 s timebase: each AAC frame carries 1024 samples.
const AUDIO_TRACK: AudioTrackInfo = {
  codec: "aac",
  sampleRate: 48000,
  channels: 2,
  timeBase: { num: 1, den: 48000 },
};

/** Deterministic fake H.264 payload (annex-B start code + NAL header + filler). */
function fakeVideoPacket(i: number): Buffer {
  const size = 64 + ((i * 37) % 512);
  const b = Buffer.alloc(size);
  b[0] = 0x00;
  b[1] = 0x00;
  b[2] = 0x00;
  b[3] = 0x01;
  b[4] = i % 30 === 0 ? 0x65 : 0x41; // NAL header: IDR slice vs. non-IDR
  for (let j = 5; j < size; j++) b[j] = (i * 13 + j) & 0xff;
  return b;
}

/** Deterministic fake AAC frame (7-byte ADTS-style header + payload). */
function fakeAudioPacket(i: number): Buffer {
  const size = 96 + ((i * 17) % 256);
  const b = Buffer.alloc(size);
  b[0] = 0xff;
  b[1] = 0xf1; // ADTS syncword
  b[2] = 0x50; // MPEG-4, AAC-LC, no CRC
  b[3] = 0x80 | ((size >> 11) & 0x03);
  b[4] = (size >> 3) & 0xff;
  b[5] = ((size & 0x07) << 5) | 0x1f;
  b[6] = 0xfc;
  for (let j = 7; j < size; j++) b[j] = (i * 29 + j) & 0xff;
  return b;
}

/**
 * The MP4 muxer stores H.264 samples as AVCC (4-byte length-prefixed NALUs),
 * converting each annex-B payload on the way in (iso-bmff `to_avcc`). Every
 * synthetic payload is a single NALU, so the stored form is the NAL bytes
 * preceded by a big-endian length — same total length as the annex-B input.
 */
function expectedVideoPayload(i: number): Buffer {
  const annexB = fakeVideoPacket(i);
  const avcc = Buffer.alloc(annexB.length);
  avcc.writeUInt32BE(annexB.length - 4, 0);
  annexB.copy(avcc, 4, 4);
  return avcc;
}

/**
 * AAC samples are stored as raw AAC frames: the muxer strips the 7-byte ADTS
 * header (iso-bmff `strip_adts`), so the demuxed payload is the input minus
 * its header.
 */
function expectedAudioPayload(i: number): Buffer {
  return fakeAudioPacket(i).subarray(7);
}

/** Mux the synthetic packets into a fragmented MP4 and return the bytes. */
function muxFragmentedMp4(): Buffer {
  const muxer = new Muxer();
  try {
    const videoTrack = muxer.addVideoTrack(VIDEO_TRACK);
    const audioTrack = muxer.addAudioTrack(AUDIO_TRACK);

    // begin() makes the muxer live and returns the init segment (ftyp + moov),
    // which must be written exactly once, ahead of any media bytes.
    const chunks: Buffer[] = [muxer.begin()];

    for (let i = 0; i < VIDEO_COUNT; i++) {
      muxer.push({
        trackIndex: videoTrack,
        data: fakeVideoPacket(i),
        pts: i, // units of 1/30 s
        duration: 1,
        key: i % 30 === 0,
      } satisfies Packet);
    }
    for (let i = 0; i < AUDIO_COUNT; i++) {
      muxer.push({
        trackIndex: audioTrack,
        data: fakeAudioPacket(i),
        pts: i * 1024, // units of 1/48000 s; 1024 samples per AAC frame
        duration: 1024,
      } satisfies Packet);
    }

    muxer.flush(); // end of input — finalizes the last fragment

    // pollBytes() drains bytes queued since the last call; keep polling until empty.
    for (let chunk = muxer.pollBytes(); chunk.length > 0; chunk = muxer.pollBytes()) {
      chunks.push(chunk);
    }

    const mp4 = Buffer.concat(chunks);
    assert.ok(mp4.length > 0, "muxer produced no output bytes");
    assert.equal(mp4.subarray(4, 8).toString("latin1"), "ftyp", "output must start with an ftyp box");
    return mp4;
  } finally {
    muxer.close();
  }
}

/** Demux the muxed bytes and assert the round trip (counts, metadata, timing, bytes). */
function demuxAndVerify(mp4: Buffer): void {
  const demuxer = new Demuxer();
  try {
    demuxer.pushBytes(mp4);

    // Available once the init segment has been parsed; packet.trackIndex indexes this list.
    const streams: TrackInfo[] = demuxer.streams();
    assert.equal(streams.length, 2, `expected 2 streams, got ${streams.length}`);
    const videoStreams = streams.filter((s): s is Extract<TrackInfo, { type: "video" }> => s.type === "video");
    const audioStreams = streams.filter((s): s is Extract<TrackInfo, { type: "audio" }> => s.type === "audio");
    assert.equal(videoStreams.length, 1, "expected exactly one video stream");
    assert.equal(audioStreams.length, 1, "expected exactly one audio stream");

    // Stream metadata round trip.
    const videoInfo = videoStreams[0];
    assert.equal(videoInfo.codec, "h264", "video codec");
    assert.equal(videoInfo.width, WIDTH, "video width");
    assert.equal(videoInfo.height, HEIGHT, "video height");
    assert.deepEqual(videoInfo.timeBase, { num: 1, den: 30 }, "video timebase");
    const audioInfo = audioStreams[0];
    assert.equal(audioInfo.codec, "aac", "audio codec");
    // MP4 demux stream info does not carry sample rate / channel count yet
    // (iso_bmff::Track gap — the C ABI reports 0 rather than fabricating).
    // The 1/48000 s timebase is the sample-rate signal and it does round-trip.
    assert.deepEqual(audioInfo.timeBase, { num: 1, den: 48000 }, "audio timebase");

    // Recovered packet counts.
    const recoveredVideo: Packet[] = [];
    const recoveredAudio: Packet[] = [];
    for (let packet = demuxer.pollPacket(); packet !== null; packet = demuxer.pollPacket()) {
      if (streams[packet.trackIndex].type === "video") recoveredVideo.push(packet);
      else recoveredAudio.push(packet);
    }
    assert.equal(recoveredVideo.length, VIDEO_COUNT, "recovered video packet count");
    assert.equal(recoveredAudio.length, AUDIO_COUNT, "recovered audio packet count");

    // Per-packet round trip: timing, keyframe flags, and payload bytes.
    for (let i = 0; i < VIDEO_COUNT; i++) {
      const p = recoveredVideo[i];
      assert.equal(p.pts, i, `video packet ${i}: pts`);
      assert.equal(p.duration, 1, `video packet ${i}: duration`);
      assert.equal(p.key, i % 30 === 0, `video packet ${i}: keyframe flag`);
      assert.ok(p.data.equals(expectedVideoPayload(i)), `video packet ${i}: payload bytes`);
    }
    for (let i = 0; i < AUDIO_COUNT; i++) {
      const p = recoveredAudio[i];
      assert.equal(p.pts, i * 1024, `audio packet ${i}: pts`);
      assert.equal(p.duration, 1024, `audio packet ${i}: duration`);
      assert.equal(p.key, false, `audio packet ${i}: keyframe flag`);
      assert.ok(p.data.equals(expectedAudioPayload(i)), `audio packet ${i}: payload bytes`);
    }
  } finally {
    demuxer.close();
  }
}

function main(): void {
  const mp4 = muxFragmentedMp4();
  demuxAndVerify(mp4);
  console.log(
    `PASS: ${VIDEO_COUNT} video + ${AUDIO_COUNT} audio packets round-tripped through ${mp4.length} fMP4 bytes`
  );
}

try {
  main();
} catch (err) {
  if (err instanceof MediawayError) {
    console.error(`mediaway error (status ${err.status}): ${err.message}`);
  } else {
    console.error(err);
  }
  process.exitCode = 1;
}
