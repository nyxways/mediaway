# rtmp

<p align="center">
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Sans-IO RTMP publish-client building blocks: the C0/C1/C2 ⇄ S0/S1/S2 handshake
(HMAC-SHA256 digest variant), the chunk stream encoder/decoder, and an AMF0 command
muxer (`connect` / `createStream` / `publish` / `onMetaData`). Feed it bytes, get chunked
RTMP bytes out — no sockets in the core.

## Quick start

```rust
use rtmp::{Handshake, Muxer};

// Handshake: feed server bytes, drain your own C0/C1/C2.
let mut handshake = Handshake::new();
while !handshake.is_complete() {
    handshake.feed_recv_bytes(&server_bytes)?;
    // take() the client bytes via handshake.pending_send()/advance_send()
}

// Publish flow: connect → createStream → publish → push media.
let mut muxer = Muxer::new(128);
let mut out = Vec::new();
muxer.write_connect("app", "rtmp://host/app", &mut out);
muxer.write_create_stream(&mut out);
muxer.write_publish("stream-key", &mut out);
muxer.push_video_data(&flv_tag_body, 0, &mut out); // already FLV-tag-shaped bytes
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| Handshake (HMAC-SHA256 digest variant) | 🆗 | Byte-level formula cross-checked against 3 independent implementations; **not yet exercised against a real RTMP server** |
| Chunk stream encode/decode | ✅ | Header types 0–3, extended timestamp, size-bounded fragmentation |
| AMF0 command mux (`connect`/`createStream`/`publish`/`onMetaData`) | ✅ | Encode only |
| AMF0/AMF3 decode | 🛠️ | Not needed for a publish-only client |
| Real-server interop gate | 👻 | No RTMP server/daemon available to run the otherwise-tested code against |
| RTMPS/TLS, server/play role, Enhanced RTMP v2 | 🛠️ | Out of v1 scope |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- Root [README](../../README.md) — container support matrix

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
