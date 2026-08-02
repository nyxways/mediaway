# Decode

The mirror of encode: `VideoDecoder`'s `push_packet` / `poll_frame` / `flush`
shape, dispatched cross-platform through `platform::AutoDecoder`.

## Push / poll

```rust,ignore
let config = VideoDecoderConfig {
    extra_data,
    output: VideoOutputPreference::CpuFramesOk,
    ..VideoDecoderConfig::h264(320, 240, Rational::new(1, 30))
};
let mut decoder = platform::AutoDecoder::open(&config)?;

decoder.push_packet(&packet)?;
while let Some(frame) = decoder.poll_frame()? {
    // decoded pixel data
}
```

Same reasoning as encode: a decoder may hold packets internally waiting for
a full frame's worth of data (B-frame reordering again), so a packet going
in doesn't promise a frame coming out on that same call.

## `extra_data` matters

`extra_data` carries the codec's out-of-band configuration — SPS/PPS for
H.264, wrapped as `avcC`. Get this from wherever the bitstream came from
(a demuxed `StreamInfo::Video::extra_data`, or an encoder's own
`stream_info()` if you're decoding what you just encoded, as the example
below does).

## Zero-Copy output

Like encode, `VideoDecoderConfig::output` controls the cost ladder:
`ZeroCopyGpu` keeps decoded frames GPU-resident (paired with a `gpu_device`);
`CpuFramesOk` accepts a CPU readback when there's no GPU path, or none is
needed.

## Try it

```bash
cargo run --example decode_h264
```

[`examples/decode/decode_h264.rs`](https://github.com/nyxways/mediaway/blob/main/examples/decode/decode_h264.rs)
is the complete, compiling version. It encodes a few frames first purely to
have real H.264 bytes to feed the decoder — that setup step isn't the point
of the example, decode is.

For a full edit built from decode + encode together, see
[Pipelines](./pipelines.md#trim--splice).
