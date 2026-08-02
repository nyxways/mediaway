import { expect, test } from "@playwright/test";

test("webcodecs A/V fMP4 smoke", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "WebCodecs smoke is Chromium-first");

  await page.goto("/");
  await page.waitForFunction(() => window.mediawayE2e?.enc);

  const supported = await page.evaluate(async () =>
    window.mediawayE2e.enc.is_webcodecs_av_supported(),
  );
  test.skip(!supported, "WebCodecs H.264/AAC not supported in this Chromium build");

  const packetCount = await page.evaluate(async () => {
    const bytes = await window.mediawayE2e.enc.webcodecs_av_fmp4_smoke();
    return window.mediawayE2e.enc.fmp4_packet_count(bytes);
  });
  expect(packetCount).toBeGreaterThanOrEqual(2);
});

test("webcodecs GPU-resident video fMP4 smoke (WebGPU canvas source)", async ({
  page,
  browserName,
}) => {
  test.skip(browserName !== "chromium", "WebGPU smoke is Chromium-first");

  await page.goto("/");
  await page.waitForFunction(() => window.mediawayE2e?.enc);

  const supported = await page.evaluate(async () =>
    window.mediawayE2e.enc.is_webgpu_video_frame_supported(),
  );
  test.skip(
    !supported,
    "WebGPU device (navigator.gpu adapter/device) or WebCodecs H.264 not available " +
      "in this Chromium build/config",
  );

  const packetCount = await page.evaluate(async () => {
    const bytes = await window.mediawayE2e.enc.webcodecs_gpu_video_fmp4_smoke();
    return window.mediawayE2e.enc.fmp4_packet_count(bytes);
  });
  expect(packetCount).toBeGreaterThanOrEqual(1);
});
