#!/usr/bin/env bun
/**
 * Build the Mediaway.* NuGet packages: stage the native DLLs, then `dotnet
 * pack` each of the eight packages.
 *
 * Usage:
 *   bun tools/scripts/package-csharp.ts [--release]
 *
 * Output: bindings/csharp/dist/*.nupkg — each nupkg carries the managed
 * assembly plus runtimes/win-x64/native/*.dll and a build targets file that
 * copies the DLLs into the consumer's output (see
 * bindings/csharp/src/Directory.Build.targets).
 */

import { $ } from "bun";
import { join } from "node:path";

const root = join(import.meta.dir, "..", "..");
const csharp = join(root, "bindings", "csharp");
const release = process.argv.includes("--release");

const dllScript = join(root, "tools", "scripts", "copy-native-dlls.ts").replaceAll("\\", "/");
const dllArgs = [dllScript];
if (release) dllArgs.push("--release");
await $`bun ${dllArgs}`.quiet();

const projects = [
  "Mediaway.Common",
  "Mediaway.Container",
  "Mediaway.Device",
  "Mediaway.Device.Audio",
  "Mediaway.Device.Camera",
  "Mediaway.Device.Desktop",
  "Mediaway.Device.Hotplug",
  "Mediaway.Pipeline",
];

for (const p of projects) {
  const csproj = join(csharp, "src", p, `${p}.csproj`);
  await $`dotnet pack ${csproj} -c Release -o dist`.cwd(csharp);
  console.log(`packed ${p}.nupkg`);
}
