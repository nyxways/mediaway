import { defineConfig, devices } from "@playwright/test";

const baseURL = "http://127.0.0.1:4173";

// Shared by both Chromium-family projects below.
const commonArgs = [
  "--use-fake-ui-for-media-stream",
  "--use-fake-device-for-media-stream",
  "--enable-unsafe-webgpu",
];

export default defineConfig({
  testDir: "./tests",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: "list",
  use: {
    baseURL,
    trace: "on-first-retry",
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        launchOptions: {
          // "chromium" (not the default headless "chromium-headless-shell") — the shell
          // build has no GPU process/compositor, so `navigator.gpu` is never defined on it.
          // The full build supports WebGPU headless (verified empirically on this machine).
          // This bundled build has NO real H.264/AAC WebCodecs backend at all — H.264/AAC
          // specs honestly skip (or fall back to VP9) here. See the "msedge-real" project
          // below for codec-accurate coverage.
          channel: "chromium",
          args: commonArgs,
        },
      },
    },
    {
      name: "msedge-real",
      // Real, separately-installed system Microsoft Edge (Chromium-based, ships with
      // Windows) — has a genuine H.264/AAC WebCodecs encode+decode backend, unlike
      // Playwright's bundled Chromium above. `channel: "msedge"` resolves to the
      // system-installed binary (no extra Playwright browser download); verified present at
      // `C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe` on this machine. See
      // `docs/ai/wiki/encode/web-real-chrome-bugs.md` for bugs only reachable this way.
      use: {
        ...devices["Desktop Chrome"],
        launchOptions: {
          channel: "msedge",
          args: commonArgs,
        },
      },
    },
  ],
  webServer: {
    command: "bun run fixtures/serve.ts",
    url: baseURL,
    reuseExistingServer: !process.env.CI,
    cwd: import.meta.dir,
  },
});
