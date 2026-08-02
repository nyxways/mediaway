# mediaway-encoder-vulkan

Cross-platform **Vulkan Video** encode backend (`VK_KHR_video_encode_queue`), bound via
[`vulkanalia`](https://crates.io/crates/vulkanalia) (migrated from `ash`, which had no
AV1 bindings — see ADR-0001's migration addendum).

| Doc | Notes |
|-----|-------|
| [roadmap.md](docs/roadmap.md) | Stage-by-stage status: probe → session → `VideoEncoder` → codecs |
| [adr/](adr/) | Bindings choice, crate placement, scope |

Unlike `mediaway-encoder-windows` / `mediaway-encoder-linux`, this crate is **not**
OS-suffixed: Vulkan Video is one portable API reachable from Windows, Linux, and
Android alike (mirrors `mediaway-encoder-nvenc`'s vendor/framework-axis placement,
not an OS backend). See [ADR-0001](adr/0001-vulkan-video-encode-ash-probe.md).

**`VulkanVideoEncoder` (a real, reusable `mediaway_encoder::VideoEncoder` impl) is
hardware-verified for H.264 and HEVC** — instance/device/video session/session
parameters/images/buffers/command pool/fence/query pool all persist across
`push_frame` calls, byte-exact packets via `VK_QUERY_TYPE_VIDEO_ENCODE_FEEDBACK_KHR`,
output cross-checked against a system-FFmpeg oracle decode. VP9 has no Vulkan
Video extension at all.
AV1 is implemented (`av1_params.rs`/`session_command_av1.rs`) but currently blocked
on an RTX 4090: session/session-parameters/sequence-header
are all real and hardware-verified, but per-frame `vkCmdEncodeVideoKHR` output is not
a valid OBU stream — independently confirmed to be a driver-maturity limitation
(FFmpeg's own `av1_vulkan` encoder produces AV1 `dav1d` itself rejects on the same
test machine), not this crate's bug. See ADR-0001's AV1 addendum for the full trail. Run
`cargo test -p mediaway-encoder-vulkan -- --nocapture` on a real machine to
reproduce.
