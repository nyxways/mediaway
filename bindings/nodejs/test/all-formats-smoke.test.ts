/**
 * all-formats-smoke.test.ts — round-trips the 7 non-MP4 container formats
 * wired into this binding (WebM/Ogg/ADTS/FLV/MPEG-TS/MP3/WAV) against the
 * real mediaway_ffi.dll. Every payload/expected value mirrors a verified
 * round trip already checked in the Rust FFI smoke tests and the
 * C++/C#/Python bindings' own smoke tests — not invented here.
 *
 * Assertion-based with node:assert (no test framework); any failed
 * assertion exits nonzero.
 *
 * Run: npm test   (from bindings/nodejs)
 */

import assert from "node:assert/strict";
import {
  AUDIO_TRACK_ID,
  AdtsDemuxer,
  AdtsMuxer,
  Demuxer,
  FlvDemuxer,
  FlvMuxer,
  MediawayError,
  Mp3Demuxer,
  Mp3Muxer,
  Muxer,
  OggDemuxer,
  OggMuxer,
  TsDemuxer,
  TsMuxer,
  VIDEO_TRACK_ID,
  WavMuxer,
  parseWav,
  type AudioTrackInfo,
  type Packet,
  type TrackInfo,
  type VideoTrackInfo,
} from "@mediaway/container";

function smokeWebm(): void {
  const muxer = new Muxer("webm");
  try {
    // Track index starts at 1, not 0 — WebM/Matroska's TrackNumber must not be 0.
    const trackId = muxer.addVideoTrack({
      codec: "vp8",
      width: 64,
      height: 64,
      timeBase: { num: 1, den: 30 },
    } satisfies VideoTrackInfo);
    assert.equal(trackId, 1);

    let webmBytes = muxer.begin(); // returns the init segment (EBML header + segment info)
    for (let i = 0; i < 5; i++) {
      muxer.push({
        trackIndex: trackId,
        data: Buffer.alloc(16, 0xaa),
        pts: i,
        duration: 1,
        key: i === 0,
      } satisfies Packet);
      webmBytes = Buffer.concat([webmBytes, muxer.pollBytes()]);
    }
    muxer.flush();
    webmBytes = Buffer.concat([webmBytes, muxer.pollBytes()]);

    assert.ok(webmBytes.length > 4 && webmBytes[0] === 0x1a && webmBytes[1] === 0x45, "EBML magic present");

    const demuxer = new Demuxer("webm");
    try {
      demuxer.pushBytes(webmBytes);
      let count = 0;
      while (demuxer.pollPacket() !== null) count++;
      assert.equal(count, 5, "5 WebM packets recovered");
    } finally {
      demuxer.close();
    }
  } finally {
    muxer.close();
  }
}

function smokeOgg(): void {
  const head = Buffer.concat([
    Buffer.from("OpusHead", "latin1"),
    Buffer.from([1, 2, 0, 0]), // version, channels, pre-skip
    (() => {
      const b = Buffer.alloc(4);
      b.writeUInt32LE(48000, 0);
      return b;
    })(),
    Buffer.from([0, 0, 0]), // output gain, channel mapping family
  ]);

  const muxer = new OggMuxer(1);
  try {
    muxer.push({ trackIndex: 0, data: head, pts: 0, duration: 0, key: true } satisfies Packet);
    let oggBytes = muxer.pollBytes();
    muxer.push({
      trackIndex: 0,
      data: Buffer.from([1, 2, 3, 4]),
      pts: 960,
      duration: 0,
      key: true,
    } satisfies Packet);
    oggBytes = Buffer.concat([oggBytes, muxer.pollBytes()]);
    muxer.flush();

    assert.ok(oggBytes.length > 4 && oggBytes.subarray(0, 2).toString("latin1") === "Og", "capture pattern present");

    const demuxer = new OggDemuxer();
    try {
      demuxer.pushBytes(oggBytes);
      const packet = demuxer.pollPacket();
      assert.ok(packet !== null && packet.data.length === 4 && packet.pts === 960, "Opus packet recovered");
    } finally {
      demuxer.close();
    }
  } finally {
    muxer.close();
  }
}

function smokeAdts(): void {
  const rawAac = Buffer.alloc(100, 0xab);
  const muxer = new AdtsMuxer(44100, 2);
  try {
    for (let i = 0; i < 2; i++) {
      muxer.push({ trackIndex: 0, data: rawAac, pts: 0, duration: 0, key: true } satisfies Packet);
    }
    muxer.flush();
    const adtsBytes = muxer.pollBytes();
    assert.ok(
      adtsBytes.length > 2 && adtsBytes[0] === 0xff && (adtsBytes[1] & 0xf0) === 0xf0,
      "sync word present"
    );

    const demuxer = new AdtsDemuxer();
    try {
      demuxer.pushBytes(adtsBytes);
      let expectedPts = 0;
      for (let i = 0; i < 2; i++) {
        const packet = demuxer.pollPacket();
        assert.ok(packet !== null && packet.pts === expectedPts && packet.data.length === 100);
        expectedPts += 1024;
      }
    } finally {
      demuxer.close();
    }
  } finally {
    muxer.close();
  }
}

function smokeFlv(): void {
  const muxer = new FlvMuxer();
  try {
    let flvBytes = muxer.writeHeader(true, true);

    muxer.addVideoTrack({
      codec: "h264",
      width: 1280,
      height: 720,
      timeBase: { num: 1, den: 1000 },
      extraData: Buffer.from([1, 0x42, 0, 0x1e, 0xff, 0xe1, 0, 0]),
    } satisfies VideoTrackInfo);
    muxer.addAudioTrack({
      codec: "aac",
      sampleRate: 44100,
      channels: 2,
      timeBase: { num: 1, den: 1000 },
      extraData: Buffer.from([0x12, 0x10]),
    } satisfies AudioTrackInfo);

    flvBytes = Buffer.concat([
      flvBytes,
      muxer.push({
        trackIndex: VIDEO_TRACK_ID,
        data: Buffer.from([0, 0, 0, 2, 0x65, 0x88]),
        pts: 45,
        duration: 33,
        key: true,
      } satisfies Packet),
    ]);
    flvBytes = Buffer.concat([
      flvBytes,
      muxer.push({
        trackIndex: AUDIO_TRACK_ID,
        data: Buffer.from([1, 2, 3, 4]),
        pts: 23,
        duration: 23,
        key: true,
      } satisfies Packet),
    ]);

    assert.equal(flvBytes.subarray(0, 3).toString("latin1"), "FLV", "file signature present");

    const demuxer = new FlvDemuxer();
    try {
      demuxer.pushBytes(flvBytes);
      let gotVideo = false;
      let gotAudio = false;
      for (let p = demuxer.pollPacket(); p !== null; p = demuxer.pollPacket()) {
        if (p.trackIndex === VIDEO_TRACK_ID) gotVideo = true;
        if (p.trackIndex === AUDIO_TRACK_ID) gotAudio = true;
      }
      assert.ok(gotVideo && gotAudio, "both tracks recovered");
    } finally {
      demuxer.close();
    }
  } finally {
    muxer.close();
  }
}

function smokeTs(): void {
  const videoPid = 0x100;
  const audioPid = 0x101;
  const streams = [
    { pid: videoPid, codec: "h264" as const },
    { pid: audioPid, codec: "aac" as const },
  ];

  const muxer = new TsMuxer(1, 0x1000, streams);
  try {
    let tsBytes = muxer.writePatPmt();
    tsBytes = Buffer.concat([tsBytes, muxer.writeAccessUnit(videoPid, Buffer.from([0, 0, 0, 1, 0x65, 0x88]), 90000, null, true)]);
    tsBytes = Buffer.concat([tsBytes, muxer.writeAccessUnit(videoPid, Buffer.from([0, 0, 0, 1, 0x41]), 90033, null, false)]);

    const demuxer = new TsDemuxer();
    try {
      demuxer.pushBytes(tsBytes);
      const packet = demuxer.pollPacket();
      assert.ok(packet !== null && packet.pts === 90000 && packet.key, "video access unit recovered");
    } finally {
      demuxer.close();
    }
  } finally {
    muxer.close();
  }

  // finish() recovers a trailing access unit with no confirming marker.
  const muxer2 = new TsMuxer(1, 0x1000, streams);
  const tail = Buffer.from([9, 9, 9]);
  let tsBytes2: Buffer;
  try {
    tsBytes2 = muxer2.writePatPmt();
    tsBytes2 = Buffer.concat([tsBytes2, muxer2.writeAccessUnit(videoPid, tail, 90000, null, true)]);
  } finally {
    muxer2.close();
  }

  const demuxer2 = new TsDemuxer();
  try {
    demuxer2.pushBytes(tsBytes2);
    assert.equal(demuxer2.pollPacket(), null, "no packet ready before finish()");
    const finished = demuxer2.finish();
    assert.equal(finished.length, 1, "finish() recovers exactly one trailing AU");
    assert.ok(finished[0].data.equals(tail), "finish() payload matches");
  } finally {
    demuxer2.close();
  }
}

function smokeMp3(): void {
  const muxer = new Mp3Muxer({ version: "mpeg1", bitrateKbps: 128, sampleRate: 44100, channelMode: "stereo" });
  try {
    // frame_len(false) for 128kbps/44100Hz = floor(144000*128/44100) = 417; body = 417-4 = 413.
    const body = Buffer.alloc(413, 0xab);
    const mp3Bytes = muxer.writeFrame(body, false);
    assert.equal(mp3Bytes[0], 0xff, "frame sync byte present");

    const demuxer = new Mp3Demuxer();
    try {
      demuxer.pushBytes(mp3Bytes);
      const packet = demuxer.pollPacket();
      assert.ok(packet !== null && packet.data.length === 413, "frame recovered");
    } finally {
      demuxer.close();
    }
  } finally {
    muxer.close();
  }
}

function smokeWav(): void {
  const pcm = Buffer.from([1, 2, 3, 4, 5, 6, 7, 8]);
  const muxer = new WavMuxer(44100, 2, 16);
  try {
    muxer.push({ trackIndex: 0, data: pcm, pts: 0, duration: 0, key: true } satisfies Packet);
    const wavBytes = muxer.finish();
    assert.ok(
      wavBytes.subarray(0, 1).toString("latin1") === "R" && wavBytes.subarray(8, 9).toString("latin1") === "W",
      "RIFF/WAVE header present"
    );

    const { info, packet } = parseWav(wavBytes);
    assert.equal(info.type, "audio");
    assert.equal((info as Extract<TrackInfo, { type: "audio" }>).sampleRate, 44100);
    assert.equal((info as Extract<TrackInfo, { type: "audio" }>).channels, 2);
    assert.ok(packet.data.equals(pcm), "parsed packet payload matches");

    // A second finish() fails honestly rather than corrupting anything.
    assert.throws(() => muxer.finish(), MediawayError, "second finish() must throw");
  } finally {
    muxer.close();
  }
}

function main(): void {
  smokeWebm();
  smokeOgg();
  smokeAdts();
  smokeFlv();
  smokeTs();
  smokeMp3();
  smokeWav();
  console.log("PASS: all 7 newly-wired container formats verified");
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
