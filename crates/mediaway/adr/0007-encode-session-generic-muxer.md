# ADR-0007: `EncodeSession` generic over its muxer

- **Status**: Accepted
- **Date**: 2026-09-18
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: mediaway (+ `mediaway-container` for the traits)

## Context

`EncodeSession` was hardcoded to one container:

```rust
pub struct EncodeSession<E: VideoEncoder> {
    muxer: mp4::Muxer<mp4::Live>,
    ...
}
```

`mediaway-container` ships a WebM muxer with the same typestate shape (`Muxer<Open>` →
`add_track` → `begin()` → `Muxer<Live>`, `Mux` implemented on the live state), and
`mediaway-ffi` already lets a C caller pick between MP4 and WebM at the *container* layer.
The facade above them could not. A caller wanting VP9/Opus in WebM had to drop out of
`EncodeSession` and drive `webm::Muxer` by hand — re-writing the `poll_packet` drain loop,
the track-id assignment, and the audio-track flush that ADR-0003 exists to provide.

That is the same shape of gap ADR-0006 closed: a convenience layer that **hides** a
low-level capability instead of composing it, against `docs/spec/api-layers.md`.

Two smaller limitations came from the same hardcoding:

- `mp4::Muxer::with_fragment_batch` was unreachable through the facade. `EncodeSession`
  always constructed `mp4::Muxer::new()`, so fragment cadence — which decides how often
  `poll_bytes` yields anything, and therefore the memory/latency trade-off ADR-0006 exists
  to give callers — was fixed at the default 30.
- `PipelineError::Mux` named `mp4::Error` directly, so every downstream `match` was written
  against one container's error type.

## Decision

> We make `EncodeSession` generic over its muxer, with fragmented MP4 as the default type
> parameter, and introduce `MuxOpen` in `mediaway-container` to name the shared
> track-registration shape.

```rust
pub trait MuxOpen {
    type Live: Mux<Error = Self::Error>;
    type Error;
    const FIRST_TRACK_ID: u32 = 0;

    fn add_track(&mut self, track: StreamInfo) -> Result<u32, Self::Error>;
    fn begin(self) -> Self::Live;
}

pub struct EncodeSession<E: VideoEncoder, M: MuxOpen = mp4::Muxer<mp4::Open>> { ... }
```

Implemented for `mp4::Muxer<Open>` and `webm::Muxer<Open>`.

### Constructors

| Constructor | Container |
|---|---|
| `open(encoder)` / `open_with_audio(encoder, audio)` | fragmented MP4 |
| `open_in(muxer, encoder)` / `open_in_with_audio(muxer, encoder, audio)` | whatever muxer is passed |

`open`/`open_with_audio` keep their exact previous signatures **and meaning**, so no
existing call site changed.

### Four decisions worth recording

**1. The MP4 constructors live in a concrete impl block, not the generic one.**

A struct's default type parameter is *not* consulted when inferring an associated function
call. `EncodeSession::open(encoder)` against a generic `open` fails with `E0283: type
annotations needed for EncodeSession<_, _>` — measured, not assumed: the first
implementation put `open` on the generic impl behind a `M: Default` bound and broke
`examples/pipeline/encode_to_mp4.rs`. Pinning `M` in the impl header
(`impl<E: VideoEncoder> EncodeSession<E, mp4::Muxer<mp4::Open>>`) lets inference resolve
`M` from the impl instead.

Consequence: other containers go through `open_in`, which names the muxer explicitly.
`EncodeSession::open_in(webm::Muxer::new(), encoder)` reads better than
`EncodeSession::<_, webm::Muxer<webm::Open>>::open(encoder)` would have anyway, so this
costs nothing beyond the asymmetry itself.

**2. `FIRST_TRACK_ID` is an associated const, because track numbering is a container rule.**

Matroska reserves `TrackNumber` 0. Encoders default to `id: 0`. So the *unchanged* session
logic — pass the encoder's stream info through, renumber a two-track session to 0/1 — works
on MP4 and fails on WebM with `InvalidTrackNumber`, for a reason having nothing to do with
the caller. This was found by the WebM round-trip test on its first run, not reasoned out
in advance.

`open_in` clamps upward (`if info.id() < M::FIRST_TRACK_ID`) rather than overwriting, so an
encoder that deliberately chose an id keeps it, and MP4 behaviour is bit-identical to
before (`FIRST_TRACK_ID = 0` never clamps).

**3. `set_track_extra_data` moves onto `Mux` with a no-op default.**

ADR-0005's late-known extra-data backfill is MP4-specific. Putting it on `Mux` with a
default that ignores the call keeps `drain` container-agnostic.

**The cost is real and is not hidden:** WebM writes `CodecPrivate` into `Tracks` in the EBML
header at `begin()`, so it *cannot* honour a late backfill and silently drops it. A
late-config encoder backend (e.g. `VideoToolbox`, which derives SPS/PPS internally) paired
with `webm::Muxer` therefore produces a file with no codec configuration record. The trait
method documents this and says to pair those backends with `mp4::Muxer`. Failing loudly
instead was considered and rejected: the call is already a legitimate no-op on MP4 once
`moov` is written, so an error would fire on the common path too.

**4. `PipelineError::Mux` wraps a new closed `ContainerError`.**

The alternatives were a generic `PipelineError<M>` — viral through every signature that
mentions it — or `Box<dyn Error>`, which makes every mux failure untypeable by the caller.
`ContainerError` is `#[non_exhaustive]` so adding a container stays non-breaking, and
`mediaway-container` owns every container mediaway ships.

**This is a breaking change** for code matching `PipelineError::Mux(mp4::Error::…)`, which
now matches one level deeper. Pre-1.0, and the alternative was leaving the facade welded to
one container.

## Consequences

- WebM sessions work through the facade: `open_in(webm::Muxer::new(), vp9_encoder)`,
  verified by round-tripping the output back through `webm::Demuxer` — one VP9 track, every
  packet recovered — rather than by sniffing the EBML magic, which would pass on a
  well-headed empty file.
- Fragment cadence is now reachable (`open_in(mp4::Muxer::with_fragment_batch(n), …)`), with
  a test asserting a small batch flushes more bytes than the default at the same frame count.
- A codec the container cannot carry fails at `open_in` (H.264 into WebM →
  `ContainerError::Webm(UnsupportedCodec(H264))`) rather than producing an unplayable file.
- `MuxOpen` is deliberately *not* bounded by `Into<ContainerError>`; that bound sits on
  `EncodeSession`'s impl instead. `MuxOpen` stays a structural trait a third party can
  implement for their own generic code — they just cannot feed it to `EncodeSession`, which
  is the documented price of a closed error enum.
- `mediaway-ffi` gained `From<ContainerError> for MediawayPipelineStatus`. Non-MP4 mux
  errors map to `UnknownError`: every C encode entry point opens the default MP4 session, so
  a precise mapping would be untestable. A future C entry point that lets the caller select a
  container is the change that must add real variants.
- Not done: `ContainerFormat` (which also pairs muxer and demuxer) was left alone. It is
  gated on `all(feature = "mux", feature = "demux")` and only MP4 implements it, so building
  on it would have forced encode-only users to enable demux.

## Alternatives considered

- **`Box<dyn Mux>`** — rejected: `Mux` has an associated `Error`, so it is not object-safe
  without erasing the error anyway, and mediaway's rule 2 prefers static dispatch here.
- **A `ContainerKind` enum with runtime dispatch inside `EncodeSession`** — rejected: it
  forces every container's code into every build regardless of features, and makes
  `add_track`'s per-container rules a runtime match instead of a type.
- **Leaving `open`/`open_with_audio` generic and updating call sites** — rejected once the
  inference failure was measured; it would have made every caller in every downstream
  project write a turbofish for the overwhelmingly common case.
