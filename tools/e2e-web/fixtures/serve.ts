#!/usr/bin/env bun
/** Static server for Playwright (pkg + fixtures/app). */

import { join } from "node:path";

const root = join(import.meta.dir, "..");
const port = Number(process.env.E2E_PORT ?? 4173);

function resolvePath(pathname: string): string | null {
  if (pathname === "/" || pathname === "/index.html") {
    return join(root, "fixtures/app/index.html");
  }
  for (const crate of [
    "iso-bmff-wasm",
    "mediaway-encoder-web",
    "mediaway-decoder-web",
    "mediaway-device-web",
  ]) {
    const prefix = `/${crate}/`;
    if (pathname.startsWith(prefix)) {
      return join(root, "pkg", crate, pathname.slice(prefix.length));
    }
  }
  return null;
}

Bun.serve({
  port,
  async fetch(req) {
    const filePath = resolvePath(new URL(req.url).pathname);
    if (!filePath) {
      return new Response("not found", { status: 404 });
    }
    const file = Bun.file(filePath);
    if (!(await file.exists())) {
      return new Response(`missing ${filePath}`, { status: 404 });
    }
    return new Response(file);
  },
});

console.log(`e2e-web static server on http://127.0.0.1:${port}`);
