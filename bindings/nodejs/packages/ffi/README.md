# @mediaway/ffi

Internal [koffi](https://koffi.dev/) bindings for **Mediaway's unified C ABI**
(`mediaway_ffi.dll`). This package owns the native binary: the DLL lives in
`native/` and is loaded once per process.

> **You usually do not need this package directly.** The high-level packages
> `@mediaway/container`, `@mediaway/device`, and `@mediaway/encoder` build on
> it and re-export everything you need. This package exists so the DLL is
> shipped and loaded exactly once.

## Install

```bash
npm install @mediaway/ffi
```

Requires Windows x64 (the packaged DLL is `win-x64`). The `.dll` is resolved
next to the package via `findLibrary()` — no PATH or system install needed.

## What is inside

- `mediaway_ffi.dll` — the merged C ABI (container + device + pipeline
  capabilities) from the [Mediaway](https://github.com/nyxways/mediaway)
  workspace.
- koffi `LibraryHandle`s + struct types: `containerLib`, `deviceLib`,
  `pipelineLib`, and the `Mw*` type objects they expose.

## Minimal use

```ts
import { findLibrary, containerLib } from "@mediaway/ffi";

// Absolute path to the DLL inside this package (used by the loader).
const dll = findLibrary("mediaway_ffi");
console.log("DLL:", dll);

// The library handles are ready after import — call C functions directly:
const abiVersion = containerLib.mediaway_container_ffi_abi_version();
console.log("ABI version:", abiVersion);
```

## License

MIT OR Apache-2.0. Part of the [Mediaway](https://github.com/nyxways/mediaway)
project — pre-1.0, APIs may change.
