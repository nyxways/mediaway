# mediaway-encoder-linux

Linux **platform backend** for hardware encode (VA-API).

| Doc | Notes |
|-----|-------|
| [roadmap.md](docs/roadmap.md) | VA-API H.264 CPU-upload → Zero-Copy / multi-codec (future) |
| [adr/](adr/) | OS-specific decisions |

Apps that want VA-API depend on this crate directly. Traits: `mediaway-encoder`.

**Build deps:** `cros-libva` → `libva-dev` (Debian/Ubuntu) at build time; required by CI's Ubuntu job.

**Unverified on real hardware in the session that authored it** — see
[ADR-0001](adr/0001-vaapi-cros-libva-h264-cpu-upload.md) § Zero real-hardware
verification. Compile-verified on Linux; run the hardware-gated tests on a real
VA-API machine before relying on this backend.
