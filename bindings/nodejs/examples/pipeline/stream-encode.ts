/**
 * stream-encode.ts — auto video encode, streamed to disk as it is produced
 * Mirrors: examples/pipeline/encode_to_mp4.rs (the streaming exit of `EncodeSession`)
 *
 * Status: ✅ real ABI under it (pipeline ABI v7, crates/mediaway-ffi/adr/pipeline/0007).
 * `encode-to-mp4.ts` holds the whole recording in RAM until `finish()`. A long capture
 * should not: call `pollBytes()` as you go and append each chunk to the file, so memory is
 * bounded by the poll cadence instead of the recording's length. `finish()` then returns only
 * the tail that was not polled yet, which is appended last.
 *
 * Flow: encode 300 synthetic grey NV12 frames with the auto-selected H.264 encoder, polling
 * after every frame, and write the chunks to out_stream.mp4 (or argv[2]) as they arrive. The
 * output is a normal MP4; the demux at the end proves every frame is in it. When no encoder
 * backend exists we catch EncoderUnavailableError and exit cleanly.
 *
 * Run: npx tsx examples/pipeline/stream-encode.ts [out.mp4]
 */

import { closeSync, openSync, readFileSync, writeSync } from "node:fs";

import {
  AutoVideoEncodeConfig,
  EncodeSession,
  EncoderUnavailableError,
  MediawayError,
  openAutoEncoder,
} from "@mediaway/encoder";
import { Demuxer, Rational } from "@mediaway/container";

const CODEC = "h264";
const WIDTH = 640;
const HEIGHT = 480;
const TIME_BASE: Rational = { num: 1, den: 30 };
const FRAME_COUNT = 300; // 10 s at 30 fps
const OUT = process.argv[2] ?? "out_stream.mp4";

async function main(): Promise<void> {
  let session: EncodeSession;
  try {
    session = await openAutoEncoder(AutoVideoEncodeConfig.defaults(CODEC, WIDTH, HEIGHT, TIME_BASE));
  } catch (err) {
    if (err instanceof EncoderUnavailableError) {
      console.log(`no ${CODEC} encoder backend on this machine; nothing to do`);
      return;
    }
    throw err;
  }

  const fd = openSync(OUT, "w");
  let written = 0;
  let chunks = 0;
  const append = (bytes: Buffer): void => {
    if (bytes.length === 0) return; // nothing ready yet — not an error
    writeSync(fd, bytes);
    written += bytes.length;
    chunks++;
  };

  try {
    const greyFrame = Buffer.alloc((WIDTH * HEIGHT * 3) / 2, 0x80);
    for (let i = 0; i < FRAME_COUNT; i++) {
      await session.writeFrame({
        pts: i,
        duration: 1,
        width: WIDTH,
        height: HEIGHT,
        pixelFormat: "nv12",
        data: greyFrame,
      });
      append(await session.pollBytes()); // memory stays bounded by this cadence
    }
    const polled = written;
    append(await session.finish()); // only what was not polled: the tail
    console.log(
      `streamed ${FRAME_COUNT} frames (${CODEC} ${WIDTH}x${HEIGHT}) -> ${OUT}: ` +
        `${written} bytes in ${chunks} writes (${polled} while encoding, ${written - polled} at finish)`
    );
  } finally {
    closeSync(fd);
    session.close(); // idempotent; frees the handle if an error aborted the encode
  }

  // The concatenation of every chunk is a complete MP4: demux it and count the frames.
  const demuxer = new Demuxer("mp4");
  demuxer.pushBytes(readFileSync(OUT));
  let packets = 0;
  while (demuxer.pollPacket()) packets++;
  demuxer.close();
  console.log(`demuxed ${packets} packets from ${OUT} (expected ${FRAME_COUNT})`);
  if (packets !== FRAME_COUNT) process.exitCode = 1;
}

main().catch((err) => {
  if (err instanceof MediawayError) {
    console.error(`mediaway error (status ${err.status}): ${err.message}`);
  } else {
    console.error(err);
  }
  process.exitCode = 1;
});
