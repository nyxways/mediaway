import { expect, test } from "@playwright/test";

// Mirrors `crates/mediaway-pipeline/tests/trim_and_splice_windows.rs`: two synthetic clips
// with distinct per-frame luma (so segment identity/order survives compression) are encoded,
// decoded, trimmed (drop first/last frame), spliced (concat + renumber timestamps),
// re-encoded, decoded again, and checked for frame count + monotonic timestamps + per-frame
// luma close to expected.
//
// Difference from the Rust/Windows test: no fMP4 mux/demux round trip here. This test proves
// the encode/decode/trim/splice leg directly at the `EncodedVideoChunk` level
// (`mediaway-encoder-web` + `mediaway-decoder-web`), which is the leg that previously had zero
// decode-side coverage. `iso-bmff` now writes a real `vp09`/`vpcC` sample entry for VP9
// (crate-local ADR-0002) — see `wasm-mux-roundtrip.spec.ts`'s "vp09 sample entry" test for the
// container-level proof; it isn't combined with this WebCodecs-level test because
// `iso-bmff-wasm`'s mux/demux is pure sans-io logic with synthetic sample bytes, independent
// of this browser's real WebCodecs VP9 encode/decode support.
//
// Codec choice: Playwright's bundled Chromium build's WebCodecs H.264 *decode* is unsupported
// (see `docs/ai/wiki/decode/web-video-decode.md`), so on the "chromium" project this test
// probes VP9/VP8/AV1/H.264 in that order and uses the first codec with both encode + decode
// support, skipping honestly only if none work. The "msedge-real" project runs against the
// machine's real, separately-installed Microsoft Edge (see playwright.config.ts), which has a
// genuine H.264 WebCodecs backend — there this test pins the codec to H.264 only, so a pass
// actually proves the real H.264 encode/decode/trim/splice path instead of silently falling
// back to VP9 (see docs/ai/wiki/encode/web-real-chrome-bugs.md).

const WIDTH = 64;
const HEIGHT = 64;
const FRAME_DURATION_US = 33_333; // ~30fps, matches mediaway-encoder-web's smoke framerate.
const BITRATE_BPS = 500_000;
const LUMA_TOLERANCE = 20; // Same tolerance as the Rust reference test's `mean_luma` check.
const CANDIDATE_CODECS = ["vp09.00.10.08", "vp8", "av01.0.04M.08", "avc1.42E01E"];
const H264_CODEC = "avc1.42E01E";
const REAL_H264_PROJECT = "msedge-real";

const LUMA_A = [10, 30, 50, 70, 90, 110];
const LUMA_B = [130, 150, 170, 190, 210, 230];

interface PipelineResult {
  frameCount: number;
  timestampsUs: number[];
  meanLumas: number[];
}

interface PipelineError {
  error: string;
}

test("web decode/trim/splice/re-encode round trip (WebCodecs)", async ({
  page,
  browserName,
}, testInfo) => {
  test.skip(browserName !== "chromium", "WebCodecs pipeline is Chromium-first");

  // Real Edge has a genuine H.264 backend — pin to H.264 so a pass proves the real codec
  // path instead of silently accepting whichever candidate the bundled-Chromium fallback
  // loop would have picked (typically VP9).
  const isRealH264Project = testInfo.project.name === REAL_H264_PROJECT;
  const candidates = isRealH264Project ? [H264_CODEC] : CANDIDATE_CODECS;

  await page.goto("/");
  await page.waitForFunction(() => window.mediawayE2e?.enc && window.mediawayE2e?.dec);

  const codec = await page.evaluate(
    async ({ candidates, width, height }) => {
      for (const candidate of candidates) {
        const canEncode =
          await window.mediawayE2e.enc.is_webcodecs_video_codec_supported(candidate);
        const canDecode = await window.mediawayE2e.dec.is_webcodecs_video_decode_supported(
          candidate,
          width,
          height,
        );
        if (canEncode && canDecode) {
          return candidate;
        }
      }
      return null;
    },
    { candidates, width: WIDTH, height: HEIGHT },
  );

  test.skip(
    codec === null,
    `No WebCodecs codec with both encode + decode support in this browser ` +
      `(tried: ${candidates.join(", ")})`,
  );

  if (isRealH264Project) {
    expect(codec).toBe(H264_CODEC);
  }

  const result = await page.evaluate(
    async ({ codec, width, height, frameDurationUs, bitrateBps, lumaA, lumaB }) => {
      const { enc, dec } = window.mediawayE2e;

      function flattenChunks(chunks: EncodedVideoChunksHandle) {
        const n = chunks.chunk_count;
        const parts: Uint8Array[] = [];
        const offsets = new Uint32Array(n);
        const lengths = new Uint32Array(n);
        const timestamps = new Float64Array(n);
        const isKey = new Uint8Array(n);
        let total = 0;
        for (let i = 0; i < n; i += 1) {
          const part = chunks.data(i);
          parts.push(part);
          offsets[i] = total;
          lengths[i] = part.length;
          total += part.length;
          timestamps[i] = chunks.timestamp_us(i);
          isKey[i] = chunks.is_key(i) ? 1 : 0;
        }
        const data = new Uint8Array(total);
        let pos = 0;
        for (const part of parts) {
          data.set(part, pos);
          pos += part.length;
        }
        return { data, offsets, lengths, timestamps, isKey };
      }

      function meanLuma(plane: Uint8Array): number {
        if (plane.length === 0) {
          return 0;
        }
        let sum = 0;
        for (let i = 0; i < plane.length; i += 1) {
          sum += plane[i];
        }
        return sum / plane.length;
      }

      async function encodeClip(lumas: number[]) {
        const timestampsUs = Float64Array.from(lumas.map((_, i) => i * frameDurationUs));
        return enc.encode_video_frames(
          codec,
          width,
          height,
          bitrateBps,
          Uint8Array.from(lumas),
          timestampsUs,
        );
      }

      async function decodeChunks(chunks: EncodedVideoChunksHandle) {
        const flat = flattenChunks(chunks);
        const frames = await dec.decode_video_chunks(
          codec,
          width,
          height,
          chunks.description,
          flat.data,
          flat.offsets,
          flat.lengths,
          flat.timestamps,
          flat.isKey,
        );
        const out: { timestampUs: number; meanLuma: number }[] = [];
        for (let i = 0; i < frames.frame_count; i += 1) {
          out.push({ timestampUs: frames.timestamp_us(i), meanLuma: meanLuma(frames.luma_plane(i)) });
        }
        out.sort((a, b) => a.timestampUs - b.timestampUs);
        return out;
      }

      const chunksA = await encodeClip(lumaA);
      const chunksB = await encodeClip(lumaB);
      const decodedA = await decodeChunks(chunksA);
      const decodedB = await decodeChunks(chunksB);

      if (decodedA.length !== lumaA.length || decodedB.length !== lumaB.length) {
        return {
          error:
            `decoded frame count mismatch: A=${decodedA.length}/${lumaA.length} ` +
            `B=${decodedB.length}/${lumaB.length}`,
        };
      }

      // Trim: drop the first and last frame of each decoded clip.
      const trimmedA = decodedA.slice(1, decodedA.length - 1);
      const trimmedB = decodedB.slice(1, decodedB.length - 1);

      // Splice: concatenate the trimmed segments. Re-encoding needs a per-frame scalar luma
      // (encode_video_frames' wasm boundary takes solid-luma inputs, not raw pixel buffers —
      // see decode_video_chunks' doc comment on why decoded pixel data can't cross back into
      // a *different* wasm module as a rich VideoFrame type), so each spliced frame is
      // re-synthesized from the rounded mean of its own decoded luma plane.
      const spliced = trimmedA.concat(trimmedB);
      const splicedLumas = spliced.map((frame) => Math.round(frame.meanLuma));

      const outputChunks = await encodeClip(splicedLumas);
      const decodedOutput = await decodeChunks(outputChunks);

      return {
        frameCount: decodedOutput.length,
        timestampsUs: decodedOutput.map((frame) => frame.timestampUs),
        meanLumas: decodedOutput.map((frame) => frame.meanLuma),
      };
    },
    {
      codec: codec as string,
      width: WIDTH,
      height: HEIGHT,
      frameDurationUs: FRAME_DURATION_US,
      bitrateBps: BITRATE_BPS,
      lumaA: LUMA_A,
      lumaB: LUMA_B,
    },
  );

  if ("error" in (result as PipelineError | PipelineResult)) {
    throw new Error((result as PipelineError).error);
  }
  const pipeline = result as PipelineResult;

  const expectedTrimmedLumas = [...LUMA_A.slice(1, -1), ...LUMA_B.slice(1, -1)];
  expect(pipeline.frameCount).toBe(expectedTrimmedLumas.length);

  let lastTimestampUs = -Infinity;
  for (const timestampUs of pipeline.timestampsUs) {
    expect(timestampUs).toBeGreaterThanOrEqual(lastTimestampUs);
    lastTimestampUs = timestampUs;
  }

  pipeline.meanLumas.forEach((luma, i) => {
    expect(Math.abs(luma - expectedTrimmedLumas[i])).toBeLessThanOrEqual(LUMA_TOLERANCE);
  });
});
