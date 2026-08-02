# Introduction

Mediaway is a Rust media stack for encode, decode, mux/demux, and device capture
(camera, mic, screen) — built as **high-level pipelines composed from first-class
low-level surfaces**, not a monolith that hides them.

Three ideas run through the whole stack:

- **Zero-Copy paths.** Video moves through GPU handles (`GpuBufferHandle`) or
  shared CPU buffers wherever the platform allows it. When a copy, upload, or
  readback is unavoidable, the API names it — never a silent slow default.
- **Sans-IO cores.** Muxers, demuxers, and bitstream/timebase logic are pure
  state machines: you push bytes or packets in, poll bytes or packets out. No
  file handles or sockets inside the core, so the same logic runs unchanged on
  native hosts and in WASM.
- **Low-level APIs stay public.** `VideoEncoder`, `VideoDecoder`, `Muxer`,
  `Demuxer`, and friends are the real surface. `mediaway-pipeline`'s
  `EncodeSession` and `platform::Auto*` helpers are convenience wrappers over
  them, not a gate you have to go around.

## Is Mediaway ready for my project?

**Not for production yet.** Mediaway is early development (`0.x`), pre-1.0:
public APIs, crate layout, and backend behavior can change without a
deprecation cycle. See [Status & Stability](./project/status.md) for what that
means concretely and when to reconsider.

It's a good fit today for experimentation, integration spikes that can
tolerate breakage, and contributing to a platform still taking shape.

## Where to go next

- New to the crates? Start with [Installation](./getting-started/installation.md)
  and [Quick Start](./getting-started/quick-start.md).
- Want a worked example for a specific task? See [Guides](./guides/mux-demux.md).
- Need to know exactly what's implemented on your platform/codec/GPU combo?
  See [Reference](./reference/codec-support.md) — those tables are pulled
  directly from the project README, so they stay in sync automatically.

Design rationale beyond what this book covers lives in the repository's
[`docs/spec/`](https://github.com/nyxways/mediaway/tree/main/docs/spec) —
this book is the user-facing guide, `docs/spec/` is the engineering SSOT.
