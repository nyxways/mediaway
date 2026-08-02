# Workspace crate map

**Packaging v1:** [`crate-packaging`](../../../spec/crate-packaging.md) · ADR-0003 · ADR-0012.

| Member | Kind | Notes |
|--------|------|-------|
| `mediaway-common` | shared types | |
| `mediaway-common-ffi` | shared internal helper (rlib-only, no C ABI) | `Rational`/`CodecKind`/`GpuDeviceHandle`/`GpuBufferHandle` `#[repr(C)]` mirrors + buffer leak/reclaim helpers, consumed by the `*-ffi` crates ([ADR-0015](../../../adr/0015-common-ffi-unification.md); GPU handles: [device-ffi ADR-0003](../../../../crates/mediaway-device-ffi/adr/0003-gpu-handle-c-abi.md)) |
| `iso-cenc` | unprefixed ClearKey CENC | ADR-0011 |
| `iso-bmff` | unprefixed ISOBMFF/MP4 | sans-io mux+demux |
| `rtmp` | unprefixed RTMP publish client | Sans-io handshake (HMAC-SHA256 digest) + chunk stream + AMF0 command encode, raw `&[u8]` video/audio payloads (zero `mediaway-*` dep). **Implemented** — handshake digest-offset formula cross-checked against 3 independent implementations, not yet verified against a real server ([ADR-0001](../../../../crates/rtmp/adr/0001-rtmp-freestanding-core.md)) |
| `mediaway-container` | facade | traits + `mp4` over `iso-bmff` |
| `mediaway-container-ffi` | C ABI | first `*-ffi` crate; mux+demux, real-link-verified ([ADR](../../../../crates/mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md)) |
| `mediaway-pipeline` | facade-of-facades | `EncodeSession` + platform auto-dispatch ([ADR-0014](../../../adr/0014-pipeline-convenience-crate.md)) |
| `mediaway-pipeline-ffi` | C ABI | second `*-ffi` crate; auto encode, real-link-verified ([ADR](../../../../crates/mediaway-pipeline-ffi/adr/0001-auto-encode-c-abi.md)) |
| `mediaway-encoder` | facade | traits + `auto` types ([ADR-0003](../../../crates/mediaway-encoder/adr/0003-auto-encode.md)) |
| `mediaway-encoder-windows` | platform | WMF/DX11 encode + AutoVideoEncoder |
| `mediaway-encoder-linux` | platform | VA-API H.264 CPU-upload encode (`cros-libva`); zero HW verification |
| `mediaway-decoder` | facade | traits + `DecodeError` |
| `mediaway-decoder-windows` | platform | WMF/DX11 H.264 decode ZC out; `D3d11SharedDecodeBridge` D3D11→D3D12 share for `mediaway-wgpu` ([ADR-0003](../../../../crates/mediaway-decoder-windows/adr/0003-d3d11-shared-decode-bridge.md)) |
| `mediaway-decoder-web` | platform | WebCodecs `VideoDecoder` decode |
| `mediaway-device` | facade | capture (`CaptureError`) + playback (`AudioPlayback`/`PlaybackError`, [ADR-0004](../../../../crates/mediaway-device/adr/0004-audio-playback-traits.md)) traits |
| `mediaway-device-windows` | platform | DXGI Desktop Duplication screen ZC, WASAPI mic/loopback capture + WASAPI render playback ([ADR-0005](../../../../crates/mediaway-device-windows/adr/0005-wasapi-playback.md)) |
| `mediaway-device-linux` | platform | portal `ScreenCast` + PipeWire screen, CPU copy (`ashpd` + `pipewire`); zero session verification |
| `mediaway-device-web` | platform | getUserMedia / getDisplayMedia |
| `mediaway-device-ffi` | C ABI | third `*-ffi` crate; Camera video + Microphone/Loopback/ProcessLoopback audio capture, Screen/Window deferred ([ADR](../../../../crates/mediaway-device-ffi/adr/0001-capture-c-abi.md)) |
| `mediaway-encoder-web` | platform | WebCodecs encode |
| `mediaway-codec` | umbrella (planned) | re-export encoder (+ decoder); not a merge |
| `mediaway-*-ffi` (others) | C ABI (planned) | optional `mediaway-ffi` umbrella |
| `mediaway-wgpu` | GPU adapter | Windows DX12↔WMF `GpuCopy` bridges, hardware-tested: `wgpu::Texture`→H.264 encode ([ADR-0001](../../../../crates/mediaway-wgpu/adr/0001-dx12-hal-gpucopy-bridge.md)) and WMF decode output→`wgpu::Texture` NV12 ([ADR-0002](../../../../crates/mediaway-wgpu/adr/0002-decode-to-wgpu-texture-bridge.md)) |
| `mediaway-sw` | pure Rust sans-io SW | |
| `mediaway-sw-opus` | pure Rust SW, isolated unsafe | Opus encode+decode via `unsafe-libopus`; separate from `mediaway-sw` (`forbid(unsafe_code)`) since the dependency's own API is `unsafe fn` at every call site. `OpusEncoder`/`OpusDecoder` push/poll sessions implemented and round-trip tested; not yet wired into a public `mediaway-encoder`/`mediaway-decoder` trait ([ADR-0001](../../../../crates/mediaway-sw-opus/adr/0001-unsafe-libopus-encode-decode.md), Accepted) |
| `mediaway-audio-apm` | facade | Audio enhancement (AEC3/NS/AGC2/VAD) via `sonora`; one crate, no platform split (like `mediaway-wgpu`). `AudioProcessor` (`apm`) + `VoiceActivityDetector` (`vad`), catch-and-disable panic posture ([ADR-0001](../../../../crates/mediaway-audio-apm/adr/0001-sonora-audio-processing-adoption.md)) |
| `mediaway-test-media` | fixtures | |
| `mediaway-avcli` / `mediaway-avprobe` | bins (`tools/`) | use `mediaway-container` |
