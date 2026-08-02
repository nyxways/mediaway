# Sans-IO policy

Where Mediaway logic is **I/O-shaped** (bytes ↔ packets, timestamps, config), implement it **sans-io to the maximum practical extent**.

## Meaning

- Core types and state machines **do not** open files, sockets, OPFS, or GPU devices.
- Inputs/outputs are buffers, packet/frame structs, and caller-driven `push` / `pull` (or iterators).
- Files, network, OPFS, WASM FS, etc. are **adapters** outside the core (thin wrappers).

## Must be sans-io (default)

| Area | Crate / home |
|------|----------------|
| Container mux/demux (MP4) | `iso-bmff` (facade: `mediaway-container::mp4`) |
| Bitstream transforms (Annex-B ↔ AVCC, ADTS, …) | prefer `mediaway-common` or small helpers next to mux/demux |
| Timebase / interleave math | `mediaway-common` + mux |
| CLI / config parsing → structs | `mediaway-avcli` / `mediaway-avprobe` parse layer |
| Fixture byte generators (pure patterns) | `mediaway-test-media` generators |

These live in **dedicated sans-io crates** (e.g. `iso-bmff`) — not as modules inside platform backend crates. See [`crate-packaging.md`](crate-packaging.md).

## Not sans-io (platform adapters)

| Area | Why |
|------|-----|
| `mediaway-encoder` / `decoder` backends | OS/GPU codec sessions |
| `mediaway-device` | Capture devices and surfaces |
| `GpuBufferHandle` production/consumption at the device edge | Tied to GPU/OS lifetimes |

Those expose **traits** that speak frames/packets; the **session** is inherently platform I/O. Do not pretend WMF/WebCodecs are sans-io.

## Rules for agents / ADRs

1. New mux/demux/bitstream APIs: **sans-io first**; reject “just write to `File` in the core.”
2. Need a file path convenience? Add a **separate** helper module/feature (`std` adapter), not the core type.
3. Crate ADRs for mux/demux must state the sans-io boundary explicitly.
4. Vision alignment: Zero-Copy/HW + sans-io + first-class low-level surfaces ([`vision.md`](vision.md), [`api-layers.md`](api-layers.md)). Prefer streaming push/pull ([`async-and-streaming.md`](async-and-streaming.md)).
