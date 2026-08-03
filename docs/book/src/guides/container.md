# Container: Mux + Demux

`mediaway-container` wraps eight freestanding, sans-io container cores
(`iso-bmff`, `ebml-webm`, `riff-wave-core`, `adts-core`, `mpeg-audio`, `ogg`, `flv`,
`mpeg-ts-core`) behind one shape: register tracks, push packets, poll bytes out —
and the mirror for demux. None of it touches a file handle or socket; I/O is
entirely the caller's job.

This guide walks the MP4 (`mp4::Muxer`/`mp4::Demuxer`) shape, which every
other format variant follows closely.

## Muxing is a typestate

`Muxer::new()` starts in an `Open` state, where you register every track
you're going to write:

```rust,ignore
let mut muxer = mp4::Muxer::new();
let video_track = muxer.add_track(StreamInfo::Video {
    id: 0,
    codec: CodecKind::H264,
    time_base: Rational::new(1, 30),
    geometry: VideoGeometry { width: 1920, height: 1080 },
    extra_data: Bytes::new(),
})?;
```

`muxer.begin()` transitions `Open` → `Live`. That's a real type change, not a
runtime flag — once you call it, `add_track` is no longer callable on the
result. The compiler enforces "register tracks, then stream packets," not a
convention you have to remember.

```rust,ignore
let mut muxer = muxer.begin();
```

## Streaming packets, streaming bytes

Each `Packet` carries `pts`/`dts`/`duration` in the track's own time base, an
`is_keyframe` flag, and the payload bytes:

```rust,ignore
muxer.push_packet(&Packet {
    stream_id: video_track,
    pts: 0,
    dts: 0,
    duration: 1,
    is_keyframe: true,
    is_discard: false,
    payload: encoded_bytes,
})?;
muxer.flush();

let mut mp4_bytes = Vec::new();
muxer.poll_bytes(&mut mp4_bytes);
```

`poll_bytes` drains whatever the muxer has ready into a buffer you own —
write it to a file, stream it over a socket, hand it to another crate. The
muxer never makes that choice for you.

## Demuxing is the mirror

```rust,ignore
let mut demux = mp4::Demuxer::new();
demux.push_bytes(&mp4_bytes);

for stream in demux.streams() {
    println!("stream {} — {:?}", stream.id(), stream.codec());
}

while let Some(packet) = demux.poll_packet() {
    // route by packet.stream_id
}
```

`push_bytes` can be called incrementally as bytes arrive — over a network,
say — not just once with a whole buffer like the snippet above.

## Extra data (`avcC`) for H.264

Notice the `extra_data: Bytes::new()` above — for H.264 tracks you can leave
it empty. The muxer derives a proper `avcC` record from the first packet's
in-band SPS/PPS, rather than requiring the caller to pre-assemble one.

## Try it

```bash
cargo run --example mux_demux_mp4
```

[`examples/container/mux_demux_mp4.rs`](https://github.com/nyxways/mediaway/blob/main/examples/container/mux_demux_mp4.rs)
is the full, compiling version of everything above, including audio (AAC)
as a second track — see it run.

## Other formats

Same marks, same shape, different framing quirks — WAV needs a known-upfront
size, MP3 needs an explicit padding bit, MPEG-TS uses a fixed 90 kHz clock.
See [Container Support](../reference/container-support.md) for the full
format matrix, and `mediaway-container/adr/0002` for why a few formats don't
fit the shared `Mux`/`Demux` trait shape exactly.
