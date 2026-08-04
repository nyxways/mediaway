# mediaway-ffi

<p align="center">
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

The C ABI facade over Mediaway: one shared library (`mediaway_ffi.dll` /
`libmediaway_ffi.so` / `mediaway_ffi.dylib`) exporting the container, device, and
pipeline capabilities with hand-written headers under `include/mediaway/`. Opaque
handles, integer status codes, and explicit ownership rules — no panics across the
boundary.

## Quick start

```c
#include <mediaway/container.h>

mediaway_muxer_t *muxer = mediaway_muxer_create();

mediaway_video_track_info_t track = {
    .id = 0, .codec = MEDIAWAY_CODEC_H264,
    .time_base = { 1, 30 },
    .width = 1920, .height = 1080,
    /* .extra_data = … */
};
mediaway_muxer_add_video_track(muxer, &track);

mediaway_muxer_begin(muxer);
mediaway_muxer_push_packet(muxer, &packet_view);
mediaway_muxer_flush(muxer);

uint8_t *out = NULL; size_t out_len = 0;
mediaway_muxer_poll_bytes(muxer, &out, &out_len);
mediaway_buffer_free(out, out_len);

mediaway_muxer_close(muxer);
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| C ABI surface (headers + one cdylib) | ✅ | `common` / `container` / `device` / `pipeline` modules |
| Ownership + thread-safety contracts documented | ✅ | In the headers |
| Shipped/stable ABI | 🛠️ | No header/ABI release yet; pre-1.0, no stability promise |

## Docs

- Per-module docs and roadmaps: `docs/{common,container,device,pipeline}/`
- [`bindings/`](../../bindings/) — C/C++/C#/Python/Node.js consumers of this ABI
- Root [README](../../README.md) — FFI and bindings overview

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
