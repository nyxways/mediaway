import { expect, test } from "@playwright/test";

// Explicit per-codec HEVC/AV1/VP9 support matrix, on both the "chromium" (Playwright's
// bundled Chromium, no real H.264/AAC WebCodecs backend — see
// docs/ai/wiki/decode/web-video-decode.md) and "msedge-real" (this machine's real,
// separately-installed Microsoft Edge, see playwright.config.ts) projects.
//
// Unlike `decode-trim-splice.spec.ts` (which picks the *first* candidate codec with both
// encode + decode support and moves on), this spec checks and reports each codec
// individually and does not fall back — a codec reporting "not supported" here is a real,
// evidence-backed negative finding (support-check returned `false`, or a real WebCodecs
// `configure()`/`encode()`/`decode()` call threw), not an assumption. See
// `docs/ai/wiki/decode/web-video-decode.md` for known-good candidate codec strings and prior
// findings this spec extends.
//
// `is_webcodecs_video_codec_supported` (mediaway-encoder) does not trust
// `isConfigSupported` alone — it also runs one real throwaway encode (see wasm.rs's
// `video_codec_supported` doc comment), so a `true` result here already reflects a real
// encoder round trip, not just a capability query. This spec goes one step further per
// codec: on every combination that reports both encode + decode support, it runs a full
// multi-frame encode -> decode round trip and checks frame count + timestamps + luma to make
// sure the codec is not just "configurable" but actually produces correct output.

const WIDTH = 64;
const HEIGHT = 64;
const FRAME_DURATION_US = 33_333; // ~30fps, matches mediaway-encoder's smoke framerate.
const BITRATE_BPS = 500_000;
const LUMA_TOLERANCE = 20; // Same tolerance as the Rust reference test's `mean_luma` check.
const LUMAS = [10, 30, 50, 70, 90, 110];

interface Codec {
  name: string;
  codec: string;
}

// Codec strings: HEVC Main profile/tier/level 3.1 (`hev1.1.6.L93.B0`), AV1 Main profile
// level 4.0 (`av01.0.04M.08`), and VP9 profile 0 level 1.0 (`vp09.00.10.08`) — the latter two
// are the same candidate strings already proven to parse correctly by
// `decode-trim-splice.spec.ts`.
const CODECS: Codec[] = [
  { name: "HEVC", codec: "hev1.1.6.L93.B0" },
  { name: "AV1", codec: "av01.0.04M.08" },
  { name: "VP9", codec: "vp09.00.10.08" },
];

interface PipelineResult {
  frameCount: number;
  timestampsUs: number[];
  meanLumas: number[];
}

interface PipelineError {
  error: string;
}

for (const { name, codec } of CODECS) {
  test(`codec support matrix: ${name} (${codec})`, async ({ page }, testInfo) => {
    await page.goto("/");
    await page.waitForFunction(() => window.mediawayE2e?.enc && window.mediawayE2e?.dec);

    const support = await page.evaluate(
      async ({ codec, width, height }) => {
        const encodeSupported =
          await window.mediawayE2e.enc.is_webcodecs_video_codec_supported(codec);
        const decodeSupported = await window.mediawayE2e.dec.is_webcodecs_video_decode_supported(
          codec,
          width,
          height,
        );
        return { encodeSupported, decodeSupported };
      },
      { codec, width: WIDTH, height: HEIGHT },
    );

    // Evidence line for the "list" reporter's output / CI logs, independent of skip/pass.
    console.log(
      `[${testInfo.project.name}] ${name} (${codec}): ` +
        `encodeSupported=${support.encodeSupported} decodeSupported=${support.decodeSupported}`,
    );

    test.skip(
      !support.encodeSupported || !support.decodeSupported,
      `${name} (${codec}) not fully supported on project "${testInfo.project.name}": ` +
        `encodeSupported=${support.encodeSupported}, decodeSupported=${support.decodeSupported} ` +
        `(from is_webcodecs_video_codec_supported/is_webcodecs_video_decode_supported)`,
    );

    // Both encode and decode report supported: run a real encode -> decode round trip and
    // let any actual WebCodecs error (configure/encode/decode/flush) fail the test loudly
    // rather than being silently swallowed, so a "supported but broken" codec surfaces as a
    // real failure, distinct from an honest "not supported" skip above.
    const result = await page.evaluate(
      async ({ codec, width, height, frameDurationUs, bitrateBps, lumas }) => {
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

        const timestampsUs = Float64Array.from(lumas.map((_, i) => i * frameDurationUs));
        const chunks = await enc.encode_video_frames(
          codec,
          width,
          height,
          bitrateBps,
          Uint8Array.from(lumas),
          timestampsUs,
        );

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
          out.push({
            timestampUs: frames.timestamp_us(i),
            meanLuma: meanLuma(frames.luma_plane(i)),
          });
        }
        out.sort((a, b) => a.timestampUs - b.timestampUs);

        if (out.length !== lumas.length) {
          return {
            error: `decoded frame count mismatch: got ${out.length}, expected ${lumas.length}`,
          };
        }

        return {
          frameCount: out.length,
          timestampsUs: out.map((frame) => frame.timestampUs),
          meanLumas: out.map((frame) => frame.meanLuma),
        };
      },
      {
        codec,
        width: WIDTH,
        height: HEIGHT,
        frameDurationUs: FRAME_DURATION_US,
        bitrateBps: BITRATE_BPS,
        lumas: LUMAS,
      },
    );

    if ("error" in (result as PipelineError | PipelineResult)) {
      throw new Error(
        `${name} (${codec}) on "${testInfo.project.name}" reported supported but round trip ` +
          `failed: ${(result as PipelineError).error}`,
      );
    }
    const pipeline = result as PipelineResult;

    expect(pipeline.frameCount).toBe(LUMAS.length);

    let lastTimestampUs = -Infinity;
    for (const timestampUs of pipeline.timestampsUs) {
      expect(timestampUs).toBeGreaterThanOrEqual(lastTimestampUs);
      lastTimestampUs = timestampUs;
    }

    pipeline.meanLumas.forEach((luma, i) => {
      expect(Math.abs(luma - LUMAS[i])).toBeLessThanOrEqual(LUMA_TOLERANCE);
    });
  });
}
