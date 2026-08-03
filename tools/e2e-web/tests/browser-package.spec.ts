import { expect, test } from "@playwright/test";

/**
 * @mediaway/browser (bindings/browser/packages/browser, ADR-0020) E2E.
 *
 * The wasm mux/demux roundtrip runs on any browser. The WebCodecs H.264/AAC
 * encode paths only run where the browser ships real codecs — Playwright's
 * bundled Chromium lacks H.264/AAC WebCodecs backends, so those tests skip
 * there and only execute on the `msedge-real` project (same skip pattern as
 * webcodecs-fmp4.spec.ts).
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
