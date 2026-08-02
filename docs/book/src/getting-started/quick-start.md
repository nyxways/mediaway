# Quick Start

The fastest way to see Mediaway work is the mux/demux roundtrip: register
tracks, push packets, pull bytes out as fragmented MP4, then demux the same
bytes back. It's pure Rust — no OS codec, no `unsafe`, runs on every platform.

```bash
cargo run --example mux_roundtrip
```

Expected output looks like:

```text
mux_roundtrip: 90 frames → NNNN bytes of fMP4
mux_roundtrip: demuxer sees 2 stream(s)
  stream 0 — H264 1920×1080
  stream 1 — Aac (no geometry)
mux_roundtrip: recovered 90 video + 90 audio packets
mux_roundtrip: OK
```

Full source and a line-by-line walkthrough:
[Mux + Demux Roundtrip](../guides/mux-demux.md).

## Next steps

- Want to actually encode video (not just mux pre-made bytes)? See
  [Encode to MP4](../guides/encode-to-mp4.md).
- Want to capture the screen and encode it live? See
  [Screen Recording](../guides/screen-recording.md).
- Want to edit — trim and splice clips using the low-level decoder/encoder
  traits directly? See [Decode, Trim & Splice](../guides/trim-and-splice.md).
- Need the exact platform/codec support matrix? See
  [Codec Support](../reference/codec-support.md).
