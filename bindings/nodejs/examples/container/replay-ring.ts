/**
 * replay-ring.ts — keep the last N seconds of encoded packets, save "the last M seconds"
 * Mirrors: mediaway_container::replay::ReplayRing (adr/container/0009-replay-ring-c-abi.md)
 *
 * Flow: 8 s of synthetic 30 fps video (a keyframe every 30 frames) + audio is pushed into a
 * ring that keeps 3 s. `clipLast(2000)` cuts at a keyframe, and a fresh Muxer writes the clip as
 * a standalone MP4, which is demuxed again to prove it holds exactly the clip.
 *
 * LIMIT: the C ABI has no video-packet source. `EncodeSession` muxes its encoder's packets
 * internally, and packet-level output exists only for the audio encoder. Feed a ring packets from
 * a Demuxer, from the audio encoder, or from an encoder you drive yourself. This example
 * fabricates them, as mux-roundtrip.ts does.
 *
 * Run: node node_modules/tsx/dist/cli.mjs examples/container/replay-ring.ts   (from bindings/nodejs)
 */

import assert from "node:assert/strict";
import { Demuxer, Muxer, ReplayRing, type ReplayBytesEntry } from "@mediaway/container";

const FPS = { num: 1, den: 30 };
const AUDIO_RATE = { num: 1, den: 48000 };
const VIDEO = 1;
const AUDIO = 2;

/** Fake H.264 access unit: annex-B start code + NAL header (IDR on a keyframe) + filler. */
function videoPayload(n: number): Buffer {
  const b = Buffer.alloc(80 + (n % 40));
  b.writeUInt32BE(1, 0);
  b[4] = n % 30 === 0 ? 0x65 : 0x41;
  for (let j = 5; j < b.length; j++) b[j] = (n * 7 + j) & 0xff;
  return b;
}

/** Fake AAC frame with a 7-byte ADTS header. */
function audioPayload(k: number): Buffer {
  const b = Buffer.alloc(100);
  b[0] = 0xff;
  b[1] = 0xf1;
  b[2] = 0x50;
  b[3] = 0x80;
  b[4] = 100 >> 3;
  b[5] = ((100 & 7) << 5) | 0x1f;
  b[6] = 0xfc;
  for (let j = 7; j < b.length; j++) b[j] = (k * 3 + j) & 0xff;
  return b;
}

const ring = ReplayRing.create({
  anchorStreamId: VIDEO,
  anchorTimeBase: FPS,
  windowMs: 3000,
  payload: "bytes",
});
ring.addStream(AUDIO, AUDIO_RATE);

// 8 s of "live" input; the ring keeps only the newest ~3 s (whole GOPs).
const FRAMES = 240;
let nextAudio = 0;
for (let n = 0; n < FRAMES; n++) {
  ring.push({ streamId: VIDEO, pts: n, duration: 1, key: n % 30 === 0, data: videoPayload(n) });
  // 48 kHz AAC is 1024 samples per packet; frame n is at n * 1600 samples, so push every audio
  // packet that starts by then, keeping both streams on the same clock.
  while (nextAudio * 1024 <= n * 1600) {
    const k = nextAudio++;
    ring.push({ streamId: AUDIO, pts: k * 1024, duration: 1024, key: true, data: audioPayload(k) });
  }
}
console.log(`ring holds ${ring.spanMs()} ms of history`);

// "Save the last 2 seconds": cut at a keyframe, at most one GOP earlier than asked.
const clip = ring.clipLast(2000);
assert.ok(clip, "the ring holds keyframes, so it must cut a clip");
const entries = clip.entries();
const video = entries.filter((e) => e.streamId === VIDEO) as ReplayBytesEntry[];
console.log(`clip: ${clip.length} packets, ${clip.durationMs} ms, ${video.length} video frames`);
assert.equal(video[0].isKeyframe, true);
assert.equal(video[0].dts, 0, "the cut keyframe decodes at zero");

// The clip is an owned snapshot: the ring can keep running, or be closed, while we write it out.
ring.close();

// Write it as a standalone MP4. Track ids follow registration order (1, 2), matching the ring's
// stream ids above.
const muxer = new Muxer();
assert.equal(muxer.addVideoTrack({ codec: "h264", width: 640, height: 480, timeBase: FPS }), VIDEO);
assert.equal(muxer.addAudioTrack({ codec: "aac", sampleRate: 48000, channels: 2, timeBase: AUDIO_RATE }), AUDIO);
const chunks: Buffer[] = [muxer.begin()];
for (const e of entries as ReplayBytesEntry[]) {
  muxer.push({ trackIndex: e.streamId, data: e.payload, pts: e.pts, dts: e.dts, duration: e.duration, key: e.isKeyframe });
}
muxer.flush();
chunks.push(muxer.pollBytes());
muxer.close();
clip.close();
const mp4 = Buffer.concat(chunks);

// Prove it: demux the file and count what came back.
const demux = new Demuxer();
demux.pushBytes(mp4);
demux.streams(); // maps packet.trackIndex onto this list: 0 = video, 1 = audio
const back = { video: 0, audio: 0, firstKey: undefined as boolean | undefined };
for (let p = demux.pollPacket(); p; p = demux.pollPacket()) {
  if (p.trackIndex === 0) {
    back.firstKey ??= p.key;
    back.video++;
  } else {
    back.audio++;
  }
}
demux.close();
assert.equal(back.video, video.length, "every clip video frame is in the file");
assert.equal(back.audio, entries.length - video.length, "every clip audio packet is in the file");
assert.equal(back.firstKey, true, "the file starts on a keyframe");
console.log(`standalone MP4: ${mp4.length} bytes, ${back.video} video + ${back.audio} audio packets, starts on a keyframe`);
