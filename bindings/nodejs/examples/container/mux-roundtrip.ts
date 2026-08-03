/**
 * mux-roundtrip.ts — container roundtrip
 * Mirrors: examples/container/mux_demux_mp4.rs
 *
 * Status: ✅ real ABI under it.
 * The C ABI's container capability (mux fMP4 + demux, mediaway_ffi) is
 * fully implemented; this file is the DX contract the @mediaway/container package
 * targets. The muxer is sans-io: it never touches files, the caller owns byte I/O.
 *
 * Flow: mux 90 synthetic H.264 video packets + 90 synthetic AAC audio packets into a
 * fragmented MP4, poll the bytes out, demux the same bytes, and verify the recovered
 * packet counts.
 *
 * Run (once @mediaway/* packages exist): npx tsx examples/mux-roundtrip.ts
 */

import {
  Muxer,
  Demuxer,
  Packet,
  TrackInfo,
  VideoTrackInfo,
  AudioTrackInfo,
  MediawayError,
} from "@mediaway/container";

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

function main(): void {
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
    console.log(`muxed ${VIDEO_COUNT} video + ${AUDIO_COUNT} audio packets into ${mp4.length} bytes`);

    // --- demux the same bytes ---
    const demuxer = new Demuxer();
    try {
      demuxer.pushBytes(mp4);

      // Available once the init segment has been parsed; packet.trackIndex indexes this list.
      const streams: TrackInfo[] = demuxer.streams();
      const videoStreams = streams.filter((s) => s.type === "video").length;
      const audioStreams = streams.filter((s) => s.type === "audio").length;
      console.log(`streams: ${streams.length} (${videoStreams} video, ${audioStreams} audio)`);

      let recoveredVideo = 0;
      let recoveredAudio = 0;
      for (let packet = demuxer.pollPacket(); packet !== null; packet = demuxer.pollPacket()) {
        if (streams[packet.trackIndex].type === "video") recoveredVideo++;
        else recoveredAudio++;
      }
      console.log(`recovered ${recoveredVideo} video + ${recoveredAudio} audio packets`);
    } finally {
      demuxer.close();
    }
  } finally {
    muxer.close();
  }
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
