import { expect, test } from "@playwright/test";

test("wasm mux demux smoke", async ({ page }) => {
  await page.goto("/");
  await page.waitForFunction(() => window.mediawayE2e?.iso);
  const count = await page.evaluate(() =>
    window.mediawayE2e.iso.wasm_mux_demux_smoke(),
  );
  expect(count).toBeGreaterThanOrEqual(2);
});

test("wasm mux produces ftyp", async ({ page }) => {
  await page.goto("/");
  await page.waitForFunction(() => window.mediawayE2e?.iso);
  const head = await page.evaluate(() => {
    const bytes = window.mediawayE2e.iso.wasm_mux_av_bytes();
    const slice = bytes.slice(4, 8);
    return String.fromCharCode(...slice);
  });
  expect(head).toBe("ftyp");
});

// Proves crate-local ADR-0002 (`crates/iso-bmff/adr/0002-vp9-sample-entry.md`): a VP9 track
// muxes with a real `vp09`/`vpcC` sample entry (not `avc1`) and demuxes back with the correct
// codec. Pure sans-io iso-bmff logic compiled to wasm — no dependency on this browser's
// WebCodecs VP9 support, unlike decode-trim-splice.spec.ts.
test("wasm mux/demux round trip recognizes vp09 sample entry", async ({ page }) => {
  await page.goto("/");
  await page.waitForFunction(() => window.mediawayE2e?.iso);

  const hasVp09Tag = await page.evaluate(() => {
    const bytes = window.mediawayE2e.iso.wasm_mux_vp9_bytes();
    for (let i = 0; i + 4 <= bytes.length; i += 1) {
      const tag = String.fromCharCode(...bytes.slice(i, i + 4));
      if (tag === "vp09") return true;
    }
    return false;
  });
  expect(hasVp09Tag).toBe(true);

  const smoke = await page.evaluate(() => window.mediawayE2e.iso.wasm_mux_vp9_demux_smoke());
  expect(smoke).toBe("vp9:1");
});
