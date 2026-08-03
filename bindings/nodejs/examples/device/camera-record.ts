/**
 * camera-record.ts — camera + mic capture → H.264 + AAC encode → single
 * two-track MP4
 * Mirrors: examples/device/capture_camera.rs
 *
 * Status: ✅ real ABI under it.
 * The C ABI captures camera (CPU frames) and microphone (PCM), auto-encodes
 * both (audio encode is ABI v2, adr/0003-auto-audio-encode-c-abi.md), and the
 * container remux (demux the video session's fMP4, mux video + AAC with the
 * encoder's AudioSpecificConfig) produces ONE two-track MP4. The old
 * drain-and-discard gap is gone. No camera on this machine →
 * exit cleanly; no microphone / no audio backend → record video only.
 *
 * Flow: open camera index 0 at 1/30 s, open the mic at 48 kHz, query the
 * negotiated geometry + mic format, auto-encode ~3 s of polled camera frames
 * to H.264 fMP4 while pushing mic PCM into the audio encoder, then remux and
 * write out.mp4.
 *
 * Run: npx tsx examples/device/camera-record.ts
 */

import { writeFileSync } from "node:fs";

import {
  AudioEncoder,
  AutoVideoEncodeConfig,
  EncodeSession,
  openAutoEncoder,
  EncoderUnavailableError,
  MediawayError,
  type EncodedAudioPacket,
} from "@mediaway/encoder";
import {
  Demuxer,
  Muxer,
  type TrackInfo,
} from "@mediaway/container";
import {
  openCamera,
  openMicrophone,
  CameraSession,
  MicSession,
  CaptureUnavailableError,
} from "@mediaway/device";

const RECORD_MS = 3_000; // ~3 s of video at the negotiated camera rate
const MIC_SAMPLE_RATE = 48_000;
const OUT = "out.mp4";

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

async function main(): Promise<void> {
  let camera: CameraSession | undefined;
  let mic: MicSession | undefined;
  try {
    // Camera is required; no camera → clean exit (expected outcome, not an error).
    try {
      camera = await openCamera({ index: 0, timeBase: { num: 1, den: 30 } });
    } catch (err) {
      if (err instanceof CaptureUnavailableError) {
        console.log("no camera on this machine; nothing to record");
        return;
      }
      throw err;
    }

    // Mic is optional: fall back to a video-only recording when it is missing.
    try {
      mic = await openMicrophone({ sampleRate: MIC_SAMPLE_RATE, channels: 2 });
    } catch (err) {
      if (err instanceof CaptureUnavailableError) {
        console.log("no microphone; recording video only");
      } else {
        throw err;
      }
    }

    // Negotiated geometry is authoritative — the encoder must use it, not our request.
    const { width, height, pixelFormat, timeBase } = camera;
    console.log(`camera negotiated: ${width}x${height} ${pixelFormat} @ ${timeBase.den}/${timeBase.num} fps`);
    if (mic !== undefined) {
      console.log(`mic negotiated: ${mic.sampleRate} Hz, ${mic.channels} ch`);
    }

    let session: EncodeSession;
    try {
      const config = AutoVideoEncodeConfig.defaults("h264", width, height, timeBase);
      config.bitrateBps = 2_000_000;
      session = await openAutoEncoder(config);
    } catch (err) {
      if (err instanceof EncoderUnavailableError) {
        console.log("no encoder backend on this machine; nothing to do");
        return;
      }
      throw err;
    }

    // Audio encoder at the mic's negotiated format (a mono mic is not the AAC
    // sugar's default stereo).
    let audioEncoder: AudioEncoder | undefined;
    if (mic !== undefined) {
      try {
        audioEncoder = await AudioEncoder.open({
          sampleRate: mic.sampleRate,
          channels: mic.channels,
        });
      } catch (err) {
        if (err instanceof EncoderUnavailableError) {
          console.log("no audio encoder backend; recording video only");
        } else {
          throw err;
        }
      }
    }

    let recorded = 0;
    let audioPackets: EncodedAudioPacket[] = [];
    let audioInfo: Awaited<ReturnType<AudioEncoder["streamInfo"]>> | undefined;
    try {
      const startedAt = Date.now();
      while (Date.now() - startedAt < RECORD_MS) {
        const frame = camera.pollFrame();
        if (frame === null) {
          await sleep(5); // no frame pending yet — poll again shortly
          continue;
        }
        await session.writeFrame(frame); // wrapper converts camera formats as needed
        recorded++;

        if (audioEncoder !== undefined) {
          while (mic !== undefined) {
            const audio = mic.pollFrame();
            if (audio === null) break;
            await audioEncoder.pushPcm({ pts: audio.pts, data: audio.data });
          }
        }
      }

      // Finish audio encode BEFORE the audio encoder goes out of scope — it
      // must stay open to poll packets and query the ASC.
      if (audioEncoder !== undefined) {
        await audioEncoder.flush();
        for (;;) {
          const packet = await audioEncoder.pollPacket();
          if (packet === null) break;
          audioPackets.push(packet);
        }
        audioInfo = await audioEncoder.streamInfo(); // ASC materialized after the first push
      }

      const mp4 = await session.finish();
      const haveAudio = audioPackets.length > 0 && audioInfo !== undefined &&
        audioInfo.extraData.length > 0;

      // Remux video + AAC into one two-track MP4.
      let out: Buffer;
      if (haveAudio) {
        const demuxer = new Demuxer();
        demuxer.pushBytes(mp4);
        const streams = demuxer.streams();
        const video = streams.find((s): s is TrackInfo & { type: "video" } => s.type === "video");
        if (video === undefined) {
          throw new MediawayError(10, "encode session fMP4 has no video stream");
        }

        const muxer = new Muxer();
        const videoTrack = muxer.addVideoTrack({
          codec: video.codec as "h264",
          width: video.width,
          height: video.height,
          timeBase: video.timeBase,
          extraData: (video as { extraData?: Buffer }).extraData,
        });
        const audioTrack = muxer.addAudioTrack({
          codec: "aac",
          sampleRate: audioInfo.sampleRate,
          channels: audioInfo.channels,
          timeBase: { num: 1, den: audioInfo.sampleRate },
          extraData: audioInfo.extraData,
        });
        muxer.begin();
        for (;;) {
          const packet = demuxer.pollPacket();
          if (packet === null) break;
          muxer.push({ ...packet, trackIndex: videoTrack });
        }
        for (const packet of audioPackets) {
          muxer.push({
            trackIndex: audioTrack,
            pts: packet.pts,
            duration: packet.duration,
            data: packet.data,
            key: packet.keyframe,
          });
        }
        muxer.flush();
        out = muxer.pollBytes();
      } else {
        out = mp4;
      }

      writeFileSync(OUT, out);
      console.log(
        haveAudio
          ? `recorded ${recorded} frames + ${audioPackets.length} AAC packets in ~${RECORD_MS} ms -> ${OUT} (${out.length} bytes, two tracks)`
          : `recorded ${recorded} frames in ~${RECORD_MS} ms -> ${OUT} (${out.length} bytes, video only)`
      );
    } finally {
      session.close(); // no-op after finish(); frees the handle on early-error paths
      audioEncoder?.close(); // always safe — no consumption trap (adr/0003)
    }
  } finally {
    // Capture sessions join their worker thread on close.
    await camera?.close();
    await mic?.close();
  }
}

main().catch((err) => {
  if (err instanceof MediawayError) {
    console.error(`mediaway error (status ${err.status}): ${err.message}`);
  } else {
    console.error(err);
  }
  process.exitCode = 1;
});
