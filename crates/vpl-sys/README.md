# vpl-sys

Minimal, unprefixed FFI core for a subset of Intel oneVPL (`libvpl`). Dynamic runtime
loading via `libloading` — no build-time link against an Intel import library. Freestanding
— no Mediaway types.

Real consumer: [`mediaway-encoder-quicksync`](../mediaway-encoder-quicksync/README.md), which
depends on this crate for its hardware-verified H.264/HEVC encode session.
