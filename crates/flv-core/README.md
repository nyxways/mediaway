# flv-core

<p align="center">
  <a href="https://docs.rs/flv-core"><img src="https://img.shields.io/docsrs/flv-core" alt="docs.rs"></a>
  <a href="https://crates.io/crates/flv-core"><img src="https://img.shields.io/crates/v/flv-core.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Sans-IO FLV (Flash Video) tag framing: the file header and the tag /
`PreviousTagSize` structure, incrementally. FLV tags are independently appendable — the
muxer has no `finish()` — and the demuxer is a byte-boundary-safe
`push_bytes`/`poll_tag` reader. Codec-specific sub-framing inside a tag's payload
(`AudioTagHeader`/`VideoTagHeader`) stays opaque here; `mediaway-container::flv` builds
and reads those bytes. No I/O in the core.

## Quick start

```rust
use flv_core::{Demuxer, Muxer, Tag, TagType};

let mut muxer = Muxer::new();
let mut flv = Vec::new();
muxer.write_header(true, true, &mut flv); // has_audio, has_video
muxer.write_tag(&Tag {
    tag_type: TagType::Video,
    timestamp_ms: 0,
    data: flv_tag_body.into(),
}, &mut flv)?;

let mut demuxer = Demuxer::new();
demuxer.push_bytes(&flv);
while let Some(tag) = demuxer.poll_tag()? { /* tag.tag_type, tag.data */ }
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| Mux (file header + tags, self-trailing sizes) | ✅ | No `finish()` — tags are appendable |
| Demux (incremental, boundary-safe `DataOffset`) | ✅ | Tested byte-by-byte across header/tag boundaries |
| Codec sub-framing (`AACPacketType`, `AVCPacketType`, CTS) | 🛠️ | `Tag::data` stays opaque here; `mediaway-container::flv` handles it |
| Script data (AMF0/AMF3) parsing | 🛠️ | Framed but not decoded |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway-container`](../mediaway-container/) — Mediaway-typed `flv` surface over this crate
- Root [README](../../README.md) — container support matrix

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
