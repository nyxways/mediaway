# Pipelines: Composing It All

The guides so far cover one capability at a time. Real applications compose
several — capture into encode, encode into mux, decode into edit into
re-encode. `mediaway-pipeline` supplies `EncodeSession` for the common
encode→mux case; everything past that is the low-level traits from the
earlier guides, wired together by your own code, exactly like the examples
below do.

## Encode to MP4

`EncodeSession` wraps one `VideoEncoder` + a single-track `mp4::Muxer`,
draining `poll_packet` into the muxer on every `write_frame` call so you
don't hand-write that loop:

```rust,ignore
let encoder = platform::AutoEncoder::open(&config)?;
let mut session = EncodeSession::open(encoder)?;

session.write_frame(&frame)?;
let mp4_bytes = session.finish()?; // flush + mux flush + poll_bytes
```

`EncodeSession` is generic over the encoder type — no `Box`/`dyn` overhead
beyond whatever `platform::AutoEncoder::open` itself returns. It's a
convenience layer, not a gate: the manual push/poll/mux loop from the
[Container](./container.md) and [Encode](./encode.md) guides stays fully
usable if you need something `EncodeSession` doesn't do (e.g. a second
track — see below).

Try it: `cargo run --example encode_to_mp4` —
[`examples/pipeline/encode_to_mp4.rs`](https://github.com/nyxways/mediaway/blob/main/examples/pipeline/encode_to_mp4.rs).

## Screen Recording — video + audio

`EncodeSession` is deliberately video-only, single-track — adding a second
(audio) track means composing it yourself against a shared `mp4::Muxer`,
the same pattern the workspace's own hardware-verified integration test
uses:

```rust,ignore
let mut open = Muxer::with_fragment_batch(2);
let video_track = open.add_track(video_encoder.stream_info().clone())?;
let audio_track = open.add_track(audio_encoder.stream_info().clone())?;
let mut mux = open.begin();

// … capture screen + mic, push into their respective encoders, poll packets,
// mux.push_packet each with the right stream_id …

mux.flush();
let mut bytes = Vec::new();
mux.poll_bytes(&mut bytes);
```

Screen and microphone capture come from `platform::ScreenCapture` /
`platform::Microphone`; audio encode has no cross-platform dispatcher yet,
so the example reaches for `mediaway_encoder_windows::WindowsAudioEncoder`
directly (it compiles everywhere, degrading gracefully off Windows — see
[Device](./device.md) for the same pattern applied to camera).

Try it: `cargo run --example screen_record` —
[`examples/pipeline/screen_record.rs`](https://github.com/nyxways/mediaway/blob/main/examples/pipeline/screen_record.rs)
produces `out_screen.mp4` with real captured audio muxed as a second track.
(Video frames are still a synthetic placeholder — the example's doc comment
explains why BGRA→NV12 conversion is a separately-tracked gap, not silently
skipped.)

## Trim & Splice

A non-linear edit built entirely from the low-level `VideoDecoder`/
`VideoEncoder` traits plus container mux/demux — no new `DecodeSession` or
`EditTimeline` abstraction. The shape:

1. Encode two short clips, mux each to fMP4.
2. Demux + decode each clip back to `Vec<VideoFrame>`.
3. **Trim** — slice the decoded frames by index/PTS; no new type needed.
4. **Splice** — `Iterator::chain` the trimmed segments, then renumber
   `pts`/`duration` contiguously (encoded timestamps must be monotonic).
5. Re-encode the spliced frames and mux the result.

```rust,ignore
let trimmed_1 = &decoded_1[1..decoded_1.len() - 1];
let trimmed_2 = &decoded_2[1..decoded_2.len() - 1];

let spliced: Vec<VideoFrame> = trimmed_1.iter().chain(trimmed_2.iter())
    .enumerate()
    .map(|(i, f)| VideoFrame { pts: i as i64, duration: 1, ..f.clone() })
    .collect();
```

Try it: `cargo run --example trim_and_splice` —
[`examples/pipeline/trim_and_splice.rs`](https://github.com/nyxways/mediaway/blob/main/examples/pipeline/trim_and_splice.rs).
Detail on what this composition surfaced (an AVCC-vs-Annex-B extradata bug):
[`docs/ai/wiki/pipeline/trim-and-splice.md`](https://github.com/nyxways/mediaway/blob/main/docs/ai/wiki/pipeline/trim-and-splice.md)
in the repository.
