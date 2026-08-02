# mediaway-decoder-linux

Linux **platform backend** for hardware video decode (VA-API).

| Doc | Notes |
|-----|-------|
| [roadmap.md](docs/roadmap.md) | VA-API H.264 CPU-output decode → Zero-Copy / multi-codec (future) |
| [adr/](adr/) | OS-specific decisions |

**Build deps:** `cros-libva` → `libva-dev` (Debian/Ubuntu) at build time; required by CI's Ubuntu job.

Apps that want VA-API decode depend on this crate directly. Traits: `mediaway-decoder`.

**Unverified on real hardware in the session that authored it** — see
[ADR-0001](adr/0001-vaapi-h264-cpu-out.md) § Zero real-hardware verification.
Compile-verified on Linux; run the hardware-gated tests on a real VA-API machine
before relying on this backend.
