/**
 * Mux + demux roundtrip — aspirational quick-start example.
 *
 * ASPIRATIONAL EXAMPLE: no `@mediaway/container` npm package exists yet. This
 * file shows the target ergonomics for a future Node.js binding over
 * Mediaway's C ABI (napi-rs native addon under the hood, wrapped in
 * idiomatic Node/TypeScript: typed structs, `Error` subclasses, explicit
 * `close()`). See ../README.md and docs/spec/c-ffi.md.
 *
 * Mirrors examples/mux_roundtrip.rs: register one H.264 video track and one
 * AAC audio track, push fake packets for a simulated 3-second clip, flush,
 * and read the fragmented MP4 bytes back with a streaming demuxer.
 *
 * Run (once the real package exists):
 *   npx tsx mux-roundtrip.ts
 */

import {
  type AudioTrackInfo,
  Demuxer,
  Muxer,
  type Packet,
  type Rational,
  type VideoTrackInfo,
} from "@mediaway/container";

/** Parameters for the synthetic clip we mux. */
interface ClipPlan {
  readonly fps: Rational;
  readonly audioTimeBase: Rational;
  readonly frameCount: number; // 90 frames @ 30 fps = 3 s
  readonly keyframeInterval: number;
  readonly samplesPerAudioFrame: number;
}

const PLAN: ClipPlan = {
  fps: { num: 1, den: 30 },
  audioTimeBase: { num: 1, den: 48_000 },
  frameCount: 90,
  keyframeInterval: 30,
  samplesPerAudioFrame: 1_600,
};

/** Mux one video + one audio track into fragmented MP4 bytes. */
async function buildFmp4(plan: ClipPlan): Promise<Buffer> {
  const muxer = new Muxer();

  try {
    // ── 1. Register tracks (open state) ──────────────────────────────────
    const videoTrack: VideoTrackInfo = {
      kind: "video",
      codec: "h264",
      timeBase: plan.fps,
      width: 1920,
      height: 1080,
      extraData: new Uint8Array(0),
    };
    const videoId = muxer.addTrack(videoTrack);

    const audioTrack: AudioTrackInfo = {
      kind: "audio",
      codec: "aac",
      timeBase: plan.audioTimeBase,
      sampleRate: 48_000,
      channels: 2,
      extraData: new Uint8Array(0),
    };
    const audioId = muxer.addTrack(audioTrack);

    // ── 2. Transition to a live session — track registration closes here ──
    muxer.begin();

    for (let i = 0; i < plan.frameCount; i++) {
      const videoPacket: Packet = {
        streamId: videoId,
        pts: i,
        dts: i,
        duration: 1,
        isKeyframe: i % plan.keyframeInterval === 0,
        isDiscard: false,
        payload: Uint8Array.from([0x00, 0x00, 0x00, 0x01]), // placeholder NAL unit
      };
      muxer.pushPacket(videoPacket);

      const audioPacket: Packet = {
        streamId: audioId,
        pts: i * plan.samplesPerAudioFrame,
        dts: i * plan.samplesPerAudioFrame,
        duration: plan.samplesPerAudioFrame,
        isKeyframe: true,
        isDiscard: false,
        payload: Uint8Array.from([0xff, 0xf1]), // placeholder ADTS-ish frame
      };
      muxer.pushPacket(audioPacket);
    }

    // "Finish" — finalizing the last fragment can plausibly block, so it's async.
    await muxer.flush();

    // ── 3. Pull bytes — caller owns I/O, the muxer never touches disk ──────
    return muxer.pollBytes();
  } finally {
    muxer.close();
  }
}

/** Feed muxed bytes into a demuxer and count video vs. audio packets. */
function demuxAndCount(data: Buffer): { video: number; audio: number } {
  const demuxer = new Demuxer();

  try {
    demuxer.pushBytes(data);

    const streams = demuxer.streams;
    console.log(`mux_roundtrip: demuxer sees ${streams.length} stream(s)`);
    for (const stream of streams) {
      const geometry =
        stream.width !== undefined && stream.height !== undefined
          ? `${stream.width}x${stream.height}`
          : "no geometry";
      console.log(`  stream ${stream.id} — ${stream.codec} (${geometry})`);
    }

    let video = 0;
    let audio = 0;
    let packet: Packet | undefined;
    while ((packet = demuxer.pollPacket()) !== undefined) {
      const stream = streams.find((s) => s.id === packet!.streamId);
      if (stream?.codec === "h264") {
        video++;
      } else {
        audio++;
      }
    }
    return { video, audio };
  } finally {
    demuxer.close();
  }
}

async function main(): Promise<void> {
  const fmp4Bytes = await buildFmp4(PLAN);
  console.log(`mux_roundtrip: ${PLAN.frameCount} frames -> ${fmp4Bytes.length} bytes of fMP4`);

  const { video, audio } = demuxAndCount(fmp4Bytes);
  console.log(`mux_roundtrip: recovered ${video} video + ${audio} audio packets`);
  console.log("mux_roundtrip: OK");
}

await main();
