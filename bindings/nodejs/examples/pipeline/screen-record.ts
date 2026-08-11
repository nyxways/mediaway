/**
 * screen-record.ts — screen + mic capture → H.264 + AAC encode → single
 * two-track MP4
 * Mirrors: examples/pipeline/screen_record.rs, bindings/c/examples/pipeline/screen_record.c
 *
 * Status: ✅ real ABI under it, as of the GPU device factory
 * (mediaway-device ADR-0007). Screen capture is GPU-only, Zero-Copy — a
 * `GpuDevice` (`@mediaway/device`) is created once and shared between capture
 * and the encoder's `gpuDevice` config field, and frames move straight from
 * capture into the encode session through the capture-to-encode bridge
 * (`EncodeSession.writeFrameFromDesktopCapture()`, adr/pipeline/0005) — no
 * `VideoFrame`/CPU pixel copy anywhere in the video path. Mic audio follows
 * the same real remux path `camera-record.ts` uses. No usable GPU/DDA path →
 * exit cleanly; no microphone / no audio backend → record video only.
 *
 * Run: npx tsx examples/pipeline/screen-record.ts
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
import { Demuxer, Muxer, type TrackInfo } from "@mediaway/container";
import {
  GpuDevice,
  openScreenCapture,
  openMicrophone,
  ScreenSession,
  MicSession,
  CaptureUnavailableError,
} from "@mediaway/device";

const RECORD_FRAMES = 90; // ~3 s at the negotiated screen capture rate
const VIDEO_POLL_TIMEOUT_MS = 10_000;
const VIDEO_POLL_INTERVAL_MS = 20;
const SCREEN_TIME_BASE = { num: 1, den: 30 };
const MIC_SAMPLE_RATE = 48_000;
const OUT = "out.mp4";

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

async function main(): Promise<void> {
  let gpuDevice: GpuDevice | undefined;
  let screen: ScreenSession | undefined;
  let mic: MicSession | undefined;
  try {
    // GPU device is required for Screen; no usable adapter → clean exit.
    try {
      gpuDevice = await GpuDevice.create();
    } catch (err) {
      if (err instanceof CaptureUnavailableError) {
        console.log(`no usable GPU device (${err.message}); nothing to record`);
        return;
      }
      throw err;
    }

    try {
      screen = await openScreenCapture({ timeBase: SCREEN_TIME_BASE, monitorIndex: 0, gpuDevice });
    } catch (err) {
      if (err instanceof CaptureUnavailableError) {
        console.log(`no screen capture available (${err.message}); nothing to record`);
        return;
      }
      throw err;
    }

    // Mic is optional: fall back to a screen-only recording when it is missing.
    try {
      mic = await openMicrophone({ sampleRate: MIC_SAMPLE_RATE, channels: 2 });
    } catch (err) {
      if (err instanceof CaptureUnavailableError) {
        console.log("no microphone; recording screen only");
      } else {
        throw err;
      }
    }

    const { width, height, timeBase } = screen;
    console.log(`screen negotiated: ${width}x${height} @ ${timeBase.den}/${timeBase.num} fps`);
    if (mic !== undefined) {
      console.log(`mic negotiated: ${mic.sampleRate} Hz, ${mic.channels} ch`);
    }

    let session: EncodeSession;
    try {
      const config = AutoVideoEncodeConfig.defaults("h264", width, height, timeBase);
      config.bitrateBps = 4_000_000; // screen content benefits from more headroom
      // DXGI Desktop Duplication delivers BGRA8 GPU textures, sharing the
      // same device the capture session was opened with — Zero-Copy end to
      // end via writeFrameFromDesktopCapture() below.
      config.gpuDevice = gpuDevice;
      config.pixelFormat = "bgra8";
      session = await openAutoEncoder(config);
    } catch (err) {
      // status 3 (NO_BACKEND) and 4 (UNSUPPORTED — this machine's encoder
      // doesn't accept this GPU-input config) are both graceful "nothing to
      // do here" outcomes, matching bindings/c/examples/pipeline/screen_record.c.
      if (err instanceof MediawayError && (err.status === 3 || err.status === 4)) {
        console.log(`no GPU-input encode backend available (status ${err.status}: ${err.message}); nothing to do`);
        return;
      }
      throw err;
    }

    let audioEncoder: AudioEncoder | undefined;
    if (mic !== undefined) {
      try {
        audioEncoder = await AudioEncoder.open({ sampleRate: mic.sampleRate, channels: mic.channels });
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
      while (recorded < RECORD_FRAMES && Date.now() - startedAt < VIDEO_POLL_TIMEOUT_MS) {
        const wrote = await session.writeFrameFromDesktopCapture(screen);
        if (wrote) {
          recorded++;
        } else {
          await sleep(VIDEO_POLL_INTERVAL_MS);
        }

        if (audioEncoder !== undefined) {
          while (mic !== undefined) {
            const audio = mic.pollFrame();
            if (audio === null) break;
            await audioEncoder.pushPcm({ pts: audio.pts, data: audio.data });
          }
        }
      }
      if (recorded < RECORD_FRAMES) {
        console.log(`screen capture stopped producing frames after ${recorded} — finishing with what we have`);
      }

      if (audioEncoder !== undefined) {
        await audioEncoder.flush();
        for (;;) {
          const packet = await audioEncoder.pollPacket();
          if (packet === null) break;
          audioPackets.push(packet);
        }
        audioInfo = await audioEncoder.streamInfo();
      }

      const mp4 = await session.finish();
      const haveAudio = audioPackets.length > 0 && audioInfo !== undefined && audioInfo.extraData.length > 0;

      let out: Buffer;
      if (haveAudio) {
        const demuxer = new Demuxer();
        demuxer.pushBytes(mp4);
        const streams = demuxer.streams();
        const video = streams.find((s): s is TrackInfo & { type: "video" } => s.type === "video");
        if (video === undefined) throw new MediawayError(10, "encode session fMP4 has no video stream");

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
          ? `recorded ${recorded} screen frames + ${audioPackets.length} AAC packets -> ${OUT} (${out.length} bytes, two tracks)`
          : `recorded ${recorded} screen frames -> ${OUT} (${out.length} bytes, video only)`
      );
    } finally {
      session.close(); // no-op after finish(); frees the handle on early-error paths
      audioEncoder?.close(); // always safe — no consumption trap (adr/0003)
    }
  } finally {
    // Capture sessions join their worker thread on close; ScreenSession also
    // closes an internally-created GpuDevice, but not a caller-supplied one
    // (owned here), so close it explicitly.
    await screen?.close();
    await mic?.close();
    gpuDevice?.close();
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
