import { expect, test } from "@playwright/test";

/**
 * @mediaway/browser (bindings/browser/packages/browser, ADR-0020 + ADR-0022) E2E.
 *
 * The wasm mux/demux roundtrip runs on any browser. The WebCodecs H.264/AAC
 * encode and decode (EncodeSession / DecodeSession) paths only run where the
 * browser ships real codecs — Playwright's bundled Chromium lacks H.264/AAC
 * WebCodecs backends, so those tests skip there and only execute on the
 * `msedge-real` project (same skip pattern as webcodecs-fmp4.spec.ts).
 */

test("browser package: wasm mux/demux roundtrip", async ({ page }) => {
  await page.goto("/browser-package.html");
  await page.waitForFunction(() => window.mediawayE2e?.browserPkg);

  const result = await page.evaluate(() => window.mediawayE2e.browserPkg);

  expect(result.error).toBeUndefined();
  expect(result.mux.recovered).toBe(180); // 90 h264 + 90 aac
  expect(result.mux.bytes).toBeGreaterThan(0);
  expect(result.mux.streams).toEqual(["h264", "aac"]);
});

test("browser package: WebCodecs H.264 encode to fMP4", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "WebCodecs H.264 is Chromium-first");

  await page.goto("/browser-package.html");
  await page.waitForFunction(() => window.mediawayE2e?.browserPkg);

  const result = await page.evaluate(() => window.mediawayE2e.browserPkg);
  expect(result.error).toBeUndefined();

  if (result.video.skipped !== undefined) {
    test.skip(true, result.video.skipped);
  }
  expect(result.video.error).toBeUndefined();
  expect(result.video.packets).toBeGreaterThan(0);
  expect(result.video.codecs).toContain("h264");
});

test("browser package: WebCodecs AAC encode to fMP4", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "WebCodecs AAC is Chromium-first");

  await page.goto("/browser-package.html");
  await page.waitForFunction(() => window.mediawayE2e?.browserPkg);

  const result = await page.evaluate(() => window.mediawayE2e.browserPkg);
  expect(result.error).toBeUndefined();

  if (result.audio.skipped !== undefined) {
    test.skip(true, result.audio.skipped);
  }
  expect(result.audio.error).toBeUndefined();
  expect(result.audio.packets).toBeGreaterThan(0);
  expect(result.audio.codecs).toContain("aac");
});

test("browser package: DecodeSession H.264 decode round trip", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "WebCodecs H.264 is Chromium-first");

  await page.goto("/browser-package.html");
  await page.waitForFunction(() => window.mediawayE2e?.browserPkg);

  const result = await page.evaluate(() => window.mediawayE2e.browserPkg);
  expect(result.error).toBeUndefined();

  if (result.decodedVideo.skipped !== undefined) {
    test.skip(true, result.decodedVideo.skipped);
  }
  expect(result.decodedVideo.error).toBeUndefined();
  // One decoded VideoFrame per demuxed packet (no B-frames, flush emits the
  // last one), and the decoded visible geometry matches the 64x64 encode
  // config. Assert display* — coded* is bitstream-aligned and encoders pad
  // it (e.g. Edge emits 64x66 coded for 64x64 input).
  expect(result.decodedVideo.frames).toBe(result.decodedVideo.packets);
  expect(result.decodedVideo.displayWidth).toBe(64);
  expect(result.decodedVideo.displayHeight).toBe(64);
});

test("browser package: DecodeSession AAC decode round trip", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "WebCodecs AAC is Chromium-first");

  await page.goto("/browser-package.html");
  await page.waitForFunction(() => window.mediawayE2e?.browserPkg);

  const result = await page.evaluate(() => window.mediawayE2e.browserPkg);
  expect(result.error).toBeUndefined();

  if (result.decodedAudio.skipped !== undefined) {
    test.skip(true, result.decodedAudio.skipped);
  }
  expect(result.decodedAudio.error).toBeUndefined();
  expect(result.decodedAudio.frames).toBeGreaterThan(0);
  expect(result.decodedAudio.sampleRate).toBe(48000);
  expect(result.decodedAudio.numberOfChannels).toBe(1);
});
