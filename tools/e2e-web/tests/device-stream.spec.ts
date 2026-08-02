import { expect, test } from "@playwright/test";

test("device policy string is exposed", async ({ page }) => {
  await page.goto("/");
  await page.waitForFunction(() => window.mediawayE2e?.dev);
  const policy = await page.evaluate(() =>
    window.mediawayE2e.dev.device_selection_policy(),
  );
  expect(policy).toContain("browser picker");
  expect(policy).toContain("not supported");
});

test("fake getUserMedia yields a video track", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "fake media flags are Chromium-oriented");

  await page.goto("/");
  await page.waitForFunction(() => window.mediawayE2e?.dev);

  const tracks = await page.evaluate(async () => {
    const prefs = new window.mediawayE2e.dev.UserMediaPreferences(true, false);
    const stream = await window.mediawayE2e.dev.open_user_media(prefs);
    return window.mediawayE2e.dev.media_stream_video_track_count(stream);
  });
  expect(tracks).toBeGreaterThanOrEqual(1);
});

test("fake getDisplayMedia yields a video track", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "fake media flags are Chromium-oriented");

  await page.goto("/");
  await page.waitForFunction(() => window.mediawayE2e?.dev);

  const tracks = await page.evaluate(async () => {
    const prefs = new window.mediawayE2e.dev.DisplayCapturePreferences();
    const stream = await window.mediawayE2e.dev.open_display_capture(prefs);
    return window.mediawayE2e.dev.media_stream_video_track_count(stream);
  });
  expect(tracks).toBeGreaterThanOrEqual(1);
});
