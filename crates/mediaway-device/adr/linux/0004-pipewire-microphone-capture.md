# ADR-0004: Direct `PipeWire` audio stream for Linux microphone capture

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-linux`

## Context

`AudioCapture`/microphone had no Linux backend (ADR-0001 explicitly deferred
it, stage 3 of `docs/roadmap.md`). This crate already depends on `pipewire`
0.10 for screen/window capture's video stream. `PipeWire` is also the
dominant modern desktop Linux audio server (replacing/subsuming PulseAudio
on current GNOME/KDE/Fedora/Ubuntu defaults), reachable via ALSA-compat,
PulseAudio-compat, or its own native client API.

## Decision

> [`LinuxMicrophoneCapture::open`](../src/mic.rs) connects **directly** to
> the local `PipeWire` daemon socket — `pw::context::ContextBox::connect(None)`
> — no `xdg-desktop-portal` involved. Builds an `Audio`/`Capture` stream
> (`MEDIA_ROLE => "Communication"`), negotiates `F32LE` interleaved PCM
> (leaving rate/channels unset in the format offer so the daemon picks
> whatever the default source graph is already running at, read back from
> the negotiated format), and pumps `AudioFrame`s into the same bounded,
> drop-oldest queue shape `mediaway-device-windows` `wasapi.rs` uses
> (`PCM_QUEUE_CAP = 64`).
>
> Only `AudioCaptureSource::Microphone { device_index: 0 }` (the default
> source) is supported this session — a nonzero index, `Loopback`, or
> `ProcessLoopback` all return `CaptureError::Unsupported`. Only
> `SampleFormat::F32` is accepted (same restriction `wasapi.rs` already
> applies — no S16/S32 conversion path this session).

### Why no portal for audio, unlike screen/window capture

Regular `PipeWire` clients (`pw-record`, VoIP apps, screen-recorders that
also grab system audio) connect to the daemon's local Unix socket directly
and capture audio with no portal-mediated consent step — unlike
`ScreenCast`, there is no `org.freedesktop.portal.*` interface that gates
"connect an `Audio`/`Capture` stream to the default source" behind a picker
or permission prompt on current desktop Linux. This is a real, load-bearing
difference from the screen/window capture recipe (ADR-0001/0003), not an
oversight: adding a portal round trip here would imitate a consent flow that
doesn't exist for this operation, the same way faking `PermissionState::Granted`
without a real check would be dishonest in the other direction.

### Why `PipeWire` directly, not ALSA

| Option | Verdict |
|--------|---------|
| `pipewire` crate (already a dependency) | **Chosen.** Zero new Cargo dependencies. Captures the modern desktop stack (PipeWire is the default audio server on current GNOME/KDE/Fedora/Ubuntu, subsuming PulseAudio) rather than a lower layer most desktop audio no longer talks to directly. Reuses the exact main-loop/stream/`process`-callback pattern `screencast.rs` already established and this session hardened. |
| `alsa` crate (raw ALSA, `libasound2`) | Considered, not chosen: a new Cargo dependency plus a new system runtime library (`libasound.so.2`, LGPL, dynamically linked — same license shape as the already-accepted `libpipewire-0.3`/`libv4l2` pattern, so not a license blocker, but still a second audio stack this crate would then own). On a `PipeWire`-routed desktop, ALSA capture usually still ends up going through PipeWire's ALSA-compat shim anyway, so opening ALSA directly would not reach a genuinely different signal path on the primary target desktop. Confirmed this session: WSL2 has no `alsa.pc` (`libasound2-dev` not installed) while `libpipewire-0.3-dev` already is — reusing the already-provisioned dependency is also the pragmatic choice here. |

No new dependency was added for this decision — `pipewire` 0.10's
dependency review already happened in ADR-0001 and is unchanged (this ADR
uses the same crate, a different API surface within it — `Context::connect`
instead of `Context::connect_fd`, and an `Audio` stream instead of a `Video`
one).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Portal-mediated audio via a hypothetical future `org.freedesktop.portal.Microphone`-style interface | Does not exist today — there is no such portal interface to call. Revisit if/when one ships. |
| Target a specific non-default `PipeWire` node (`PW_KEY_TARGET_OBJECT`) | Deferred — `device_index` stays `0`-only this session, matching the Windows backend's own "default endpoint only, nonzero index unsupported" restriction for its first slice. |
| System audio / monitor-port loopback via `STREAM_CAPTURE_SINK` | Deferred (`AudioCaptureSource::Loopback` stays `Unsupported`) — real, tractable follow-up (the `pipewire` audio-capture example even shows the property to flip), just out of this session's stated scope (camera/mic/window, not loopback). |

## Consequences

### Positive

- Zero new Cargo dependencies; reuses and hardens the exact `pipewire`
  main-loop/stream pattern `screencast.rs`/`window.rs` already use.
- No `unsafe` in `mic.rs` — same property as the rest of this crate's
  `pipewire`-based code (the crate's Rust API surface used here needs no
  `unsafe` at the call site).
- Honestly documents the real absence of an OS-level consent gate for
  microphone capture on `PipeWire`-based desktop Linux, rather than
  approximating a permission model that doesn't exist here.

### Negative / Trade-offs

- No portal-style user-visible mic indicator/consent UI comes for free the
  way screen sharing gets one — a caller wanting a "mic is live" affordance
  must build it itself; this backend cannot rely on the OS providing one.
- Only the default source, only `F32`, no loopback — real scope limits
  carried into this first slice, matching the equivalent Windows limits.

## Zero runtime hardware/session verification this session

**No real `PipeWire` daemon was exercised.** WSL2 Ubuntu 24.04 has
`libpipewire-0.3-dev` installed (`pkg-config --modversion libpipewire-0.3`
reports `1.0.5`, confirming headers are present) but **no running `PipeWire`
daemon** (confirmed this session — same gap ADR-0001 already documents for
the screen-capture path, which needs the daemon too). Verification this
session was **compile-only**. The hardware/session-gated tests
(`mic_tests.rs`) are written to run the real path and are expected to
**skip** here for exactly this reason.

## Addendum (2026-08-19): `Select::Id` node targeting

Closes the "target a specific non-default `PipeWire` node (`PW_KEY_TARGET_OBJECT`)" row in §
Alternatives Considered above. `DeviceId` gained a fourth, Linux-specific `PipeWire(String)`
variant (`from_pipewire_node_name`/`as_pipewire_node_name`, `mediaway-device` `device_id.rs`,
tag prefix `pipewire:`) wrapping a `node.name`. `LinuxMicrophoneCapture::open` accepts
`Select::Id(DeviceId::from_pipewire_node_name(name))` and sets `name` as the stream's
`PW_KEY_TARGET_OBJECT` property (real, confirmed constant — `pipewire` crate's own `keys.rs`,
gated behind the crate's `v0_3_44` Cargo feature, now enabled in `mediaway-device`'s
`Cargo.toml`; PipeWire 0.3.44 released 2021, well below any mainstream distro's shipped
version). PipeWire resolves the name match server-side; this crate does not verify it against a
live enumeration first — no `PipeWire` node enumeration exists in this crate (a caller passes a
name it already has, e.g. from `pw-cli ls Node` or its own tooling).

**Still not closed:** `Select::NameContains` stays `Unsupported` — resolving a substring match
would need a real enumeration step this crate does not have, and guessing would be a behavior
difference from every other backend's `NameContains` semantics, not a shortcut worth taking.
`STREAM_CAPTURE_SINK` loopback (this ADR's other deferred row) is untouched by this addendum.

Real compile **and** unit-test verification this time (unlike the rest of this ADR): WSL2's
working `libpipewire-0.3` link let `cargo check`/`clippy`/`test` all run for real against the
actual `pipewire` crate and its `v0_3_44`-gated `TARGET_OBJECT` constant — a real, caught
compile error (wrong feature not enabled) was fixed before this addendum was written, not
guessed past. `open_microphone_capture_or_skip`'s own daemon-reachability gap (§ above) is
unchanged; the new `Select::Id`/`Select::NameContains` validation paths run and pass as real
unit tests (`mic_tests.rs`) since they return before ever touching PipeWire.

## References

- [ADR-0001](0001-portal-pipewire-screen-capture.md) — the `pipewire`
  dependency review this ADR relies on unchanged.
- Windows precedent: [`mediaway-device-windows/adr/0002-wasapi-capture.md`](../../mediaway-device-windows/adr/0002-wasapi-capture.md)
- `pipewire` audio-capture example: <https://gitlab.freedesktop.org/pipewire/pipewire-rs/-/blob/main/pipewire/examples/audio-capture.rs>
