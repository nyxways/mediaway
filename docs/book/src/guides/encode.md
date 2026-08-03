# Encode

The low-level surface is the `VideoEncoder` trait: push frames in, poll
packets out. `mediaway`'s `platform::AutoEncoder` picks the best
backend available on the current platform (Windows WMF today; VA-API on
Linux) and hands you back something that implements it.

## Push / poll, not call-and-block

```rust,ignore
let config = AutoVideoEncodeConfig {
    bitrate_bps: 1_000_000,
    ..AutoVideoEncodeConfig::new(CodecKind::H264, 320, 240, Rational::new(1, 30))
};
let mut encoder = platform::AutoEncoder::open(&config)?;

encoder.push_frame(&frame)?;
while let Some(packet) = encoder.poll_packet()? {
    // packet.payload is compressed bitstream data
}
```

An encoder may buffer internally (B-frame reordering, rate control lookahead)
— `push_frame` doesn't promise a packet back immediately, which is why the
poll loop runs after every push, not just once at the end.

## Flushing

When you're done pushing frames, flush and drain whatever's still buffered:

```rust,ignore
encoder.flush()?;
while let Some(packet) = encoder.poll_packet()? {
    // final packets
}
```

## Zero-Copy vs CPU upload

`AutoVideoEncodeConfig`'s `max_path_class` controls how far up the cost
ladder the encoder is allowed to go: `ZeroCopy` (GPU handle straight into the
hardware encoder, no payload `memcpy`) down through `CpuUpload` (a CPU
buffer gets uploaded to the GPU encoder session) — never a silent slow
default. Passing a `gpu_device` (e.g.
`GpuDeviceHandle::DirectX11(handle)`) is what makes the Zero-Copy path
reachable; without one, `CpuUpload` is as far as it goes.

## Try it

```bash
cargo run --example encode_h264
```

[`examples/encode/encode_h264.rs`](https://github.com/nyxways/mediaway/blob/main/examples/encode/encode_h264.rs)
is the complete, compiling version — encoder in isolation, no muxing, no
capture, so you can see exactly what push/poll/flush produces on its own.

For turning that packet stream into a playable file, see
[Pipelines](./pipelines.md#encode-to-mp4).
