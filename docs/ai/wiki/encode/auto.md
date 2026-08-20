# Auto encode

High-level UX: push frames → Mediaway **auto-selects** a path and labels it.

| Item | Location |
|------|----------|
| Types | `mediaway_encoder::auto` (path / policy / `AutoVideoEncodeConfig::new`) |
| Windows session | `mediaway_encoder_windows::auto::AutoVideoEncoder::open` |
| ADR (surface) | [`0003-auto-encode.md`](../../../../crates/mediaway-encoder/adr/0003-auto-encode.md) |
| ADR (preference) | [`0004-backend-preference.md`](../../../../crates/mediaway-encoder/adr/0004-backend-preference.md) |
| Hierarchy | [backend-preference](backend-preference.md) |
| Labels | `zc` · `copy` · `upload` · `readback` · `sw` |

```text
App (Windows)
  → AutoVideoEncodeConfig::new(codec, width, height, time_base)
  → AutoVideoEncoder::open(&cfg)
  → path_class()  // ZeroCopy | CpuUpload today
  → push_frame / poll_packet / flush  // VideoEncoder
```

Target Auto order: GraphicsApi ZC → labeled GPU costs → OsCpu → (policy) Sw;  
VendorHw explicit / not default #1. See [backend-preference](backend-preference.md).

No free `auto::open`; no baked-in 1080p preset.  

Windows `AutoVideoEncoder::open` order: `DirectX11` ZC → `DirectX12` GpuCopy
(`D3d12SharedEncodeBridge` bridge to native D3D11, one GPU→GPU copy/frame) →
CPU upload → honest `EncodeError::NoBackend` when only Readback/SW remain
allowed (neither has a backend: no DX11 readback exists yet; `mediaway-sw` is
still an empty placeholder). A foreign GPU device kind this crate can't bridge
records `Unsupported` and still tries CPU upload if allowed.

`mediaway::platform::AutoEncoder::open` (cross-platform facade, 2026-08-20): Linux/Apple now
also try `ZeroCopyGpu` first when `gpu_device` is `Some(_)`, falling back to `CpuUploadOk` — no
`GpuCopy` middle tier on either (neither backend has one). See
`crates/mediaway/adr/0004-autoencoder-zerocopy-linux-apple.md`.

Planned: preference types fully wired; DX11 readback backend; `mediaway-sw`
encoder; `mediaway-codec` re-exports.
