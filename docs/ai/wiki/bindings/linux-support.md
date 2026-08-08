# Linux support for language bindings (2026-08-08)

All 4 C-ABI bindings (C++/C#/Python/Node.js) verified against a real Linux
build, via WSL2. Scope: container capability only (pure CPU, no hardware) —
device/pipeline capture/encode remain Windows-hardware-verified only; Linux
device/pipeline status is unchanged (see
[`platform/linux-encode.md`](../platform/linux-encode.md) /
[`linux-decode.md`](../platform/linux-decode.md)).

`mediaway-ffi` itself needed **zero Rust changes** — `cargo build -p
mediaway-ffi --all-features` already produces a clean `libmediaway_ffi.so`
exporting all 158 `mediaway_*` symbols. All the work was in each binding's
native-library discovery layer.

## What changed per binding

- **Python** (`_ffi.py`): library filename was hardcoded to `mediaway_ffi.dll`.
  Added `_library_filename()` (`platform.system()` → `.dll`/`libmediaway_ffi.so`;
  raises on anything else — no macOS claim).
- **Node.js** (`ffi/src/index.ts`): same fix via `process.platform` →
  `libraryFilename()`. C++'s headers needed no changes at all — pure portable
  C++, no `_WIN32`/`windows.h` anywhere.
- **C#** (`Mediaway.Container.Tests.csproj`): `NativeMethods.cs`'s
  `LibraryName = "mediaway_ffi"` already had no extension, so .NET's own
  per-platform `DllImport` resolution needed no code change — only the test
  project's DLL-staging `<None>` items needed a `libmediaway_ffi.so` sibling
  set (release-staged `runtime/linux-x64/native/` first, `target/debug/`
  dev-fallback second).

## Real bugs found

1. **npm exact-version cross-package pins** (`@mediaway/container`/`device`/
   `encoder`'s `package.json`): each declared `"@mediaway/ffi": "0.1.0"` —
   an exact pin — while the actual workspace package was already `0.1.1`.
   npm's workspace linker silently falls back to fetching the real
   **published** `@mediaway/ffi@0.1.0` from the npm registry instead of
   symlinking the local (fresher, format-wiring-complete) source whenever a
   sibling's declared version doesn't satisfy the local one — this is not
   Linux-specific at all, it would eventually have bitten anyone running a
   plain `npm install` on any platform once the version drifted. Fixed by
   switching every internal `@mediaway/*` cross-dependency to a caret range
   (`^0.1.0`).
2. **WSL2-behind-a-shared-Windows-filesystem `cargo build` collision**: a
   plain `cargo build` on the WSL2 side (no `--target`) writes to the SAME
   `target/debug/` a Windows-host build also uses — since both this repo's
   Windows Bash tool and WSL2 mount the same `E:`/`/mnt/e` filesystem, this
   silently corrupted the Windows build's own `mediaway_ffi.dll` output on a
   subsequent Windows rebuild (missing/stale `.dll`, `DllNotFoundException`
   in `dotnet test`). Fixed for testing purposes by building Linux
   explicitly to an isolated triple directory instead:
   `cargo build -p mediaway-ffi --all-features --target
   x86_64-unknown-linux-gnu` → `target/x86_64-unknown-linux-gnu/debug/`.
   Real (non-WSL2) Linux hosts/CI never hit this — their native `cargo
   build` has no Windows host to collide with. C#'s csproj gained this path
   as a third fallback candidate for the same WSL2-dev-loop reason; Python/
   Node deliberately did **not** gain it — their `$MEDIAWAY_FFI_DIR`/
   `process.env.MEDIAWAY_FFI_DIR` override already covers this case without
   adding a WSL2-specific path to the shipped search list.

## npm workspace testing gotcha

Running `npm install <single-package>` inside a shared `node_modules` (even
with `--no-save --ignore-scripts`) can silently prune the *other* platform's
already-installed optional native binary (`@koromix/koffi-win32-x64` vs.
`-linux-x64`) as a side effect of npm recalculating the whole tree, **and**
can materialize stale nested `packages/<name>/node_modules/@mediaway/*`
copies that shadow the correct workspace symlink for that one package. For
genuine dual-platform testing, install into a **separate, platform-isolated
copy** of `bindings/nodejs` (e.g. rsync minus `node_modules`/`dist` into a
WSL2-native path) rather than reusing the Windows-side `node_modules` from
WSL2, or vice versa.
