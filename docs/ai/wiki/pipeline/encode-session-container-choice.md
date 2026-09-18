# `EncodeSession` container choice

`EncodeSession<E: VideoEncoder, M: MuxOpen = mp4::Muxer<mp4::Open>>` — ADR-0007 (`mediaway`).

| Constructor | Container |
|---|---|
| `open` / `open_with_audio` | fragmented MP4 (unchanged signatures and meaning) |
| `open_in` / `open_in_with_audio` | whatever `MuxOpen` is passed |

```rust
EncodeSession::open_in(webm::Muxer::new(), vp9_encoder)?;             // WebM
EncodeSession::open_in(mp4::Muxer::with_fragment_batch(5), enc)?;     // MP4, custom cadence
```

## Three traps this hit, worth not re-deriving

**A default type parameter does not survive associated-function inference.** A generic
`EncodeSession::open(enc)` fails with `E0283`, so the MP4 constructors live in a *concrete*
`impl<E> EncodeSession<E, mp4::Muxer<mp4::Open>>` block. That is why there is an
`open`/`open_in` asymmetry rather than one generic constructor.

**Matroska reserves `TrackNumber` 0, and encoders default to `id: 0`.** Hence
`MuxOpen::FIRST_TRACK_ID` (mp4 `0`, webm `1`); `open_in` clamps upward only, so MP4 output
is unchanged. Found by the first run of the WebM round-trip test, not by reading specs.

**`set_track_extra_data` (ADR-0005's late-config backfill) is on `Mux` with a no-op
default.** WebM writes `CodecPrivate` at `begin()` and cannot honour it, so a late-config
backend (`VideoToolbox`) + `webm::Muxer` yields a file with no config record. Pair those
with `mp4::Muxer`. See [encode-session-byte-output](encode-session-byte-output.md) for the
streaming exits this builds on.

## Errors

`PipelineError::Mux` wraps `mediaway_container::ContainerError` (closed, `#[non_exhaustive]`),
not `mp4::Error` — matching on a container's own error is one level deeper than before.
`MuxOpen` itself carries no `Into<ContainerError>` bound; that lives on `EncodeSession`'s
impl, so `MuxOpen` stays implementable by third parties for their own generic code.

Verification is a real round trip through `webm::Demuxer` (track codec + packet count), not
an EBML magic-byte check — `src/session_tests.rs` `mod container_choice`.
