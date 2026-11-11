# Mediaway v0.2.2

## What's new

### Added

- Capture a whole monitor through Windows Graphics Capture (`DesktopCaptureSource::Monitor`, `DesktopVideoCaptureConfig::monitor`), so a screen region can be cropped on the GPU with the pointer drawn in, which the DXGI screen path cannot do
- Windows audio playback can tell how much of what was written has been heard (`WindowsWasapiPlayback::played_frames`, from the device's own clock) and can drop everything not yet played at once (`WindowsWasapiPlayback::flush`), so a player can keep its picture on the sound and cut the sound on a seek
- A fixed-size capture canvas for Windows (`windows_desktop::Canvas`): each captured frame, whatever its size, is copied centered onto one texture on the GPU, so a window recording keeps one encoder size through resizes
- The Windows capture canvas can take a fixed part of each frame instead (`Canvas::place_texture_part`, `CanvasPlacement::part`): the part keeps its offset from the window's top-left through resizes and is black where a shrunk window no longer reaches, so a part of a window can be recorded while the window is covered
- The Windows capture canvas can place a region of each frame that itself changes size (`Canvas::place_texture_region`, `Canvas::place_pixels_region`, `CanvasPlacement::region`): the region is cut to what the frame holds and placed centered like a picture of its own size, so a recording of a part of a window can follow a rectangle that is moved or resized while the canvas, and so the encoder, keeps one size
- `iso_bmff::isobmff::write_progressive` writes the head of a non-fragmented MP4 (`ftyp`, a `moov` with real sample tables and an optional edit list per track, the `mdat` header) for samples all known before the file is written, the counterpart of `parse_stbl_samples`: a stream copy that starts at the keyframe before the moment it should show from can hide the packets before it with an edit list, which FFmpeg applies to a non-fragmented file and not to a fragmented one
- `iso_bmff::isobmff::write_indexed_moov` writes a `moov` with sample tables for samples already in a file, at absolute offsets and with a chunk for each run of adjacent samples: appended after a fragmented recording's last fragment it makes the file an ordinary MP4 (a duration for Windows and chat apps) without rewriting a sample
- `WindowsVideoEncoder::open_constant_quality` opens the CPU H.264 encoder with constant-quality rate control (`CODECAPI_AVEncCommonRateControlMode` = Quality, `CODECAPI_AVEncCommonQuality` 1 to 100) instead of a bitrate target: an encode whose output should stay close to its source (an export of an edit) gets the bits a busy picture needs and does not spend the ones a still picture does not; on four 5 to 7 s sources (a still screen, two 806x462 screens, a 1280x720 moving test pattern) quality 95 re-encoded at 47 to 53 dB PSNR against the source where a target of 0.1 bits per pixel gave 40 to 47 dB, with 3 to 11 % smaller files on the screens and larger ones on the busy pictures the target had starved

### Changed

- Windows audio playback starts what is written within about 20 ms: with nothing to play it no longer fills its one-second buffer with silence that later sound had to wait behind
- The Windows CPU H.264 encoder (the inbox software MFT, `VideoInputPreference::CpuUploadOk`) is asked for no B-frames: it used to reorder and stamp its first `pts` one frame late with the `dts` behind the `pts`, so an MP4 written from it started late and FFmpeg reported non-monotonic decode times at fragment boundaries. Its packets now have `pts == dts` and the first `pts` is the first input's time; on one 7 s 850x474 screen recording at the same target bitrate the file was 2 % smaller and 0.37 dB PSNR higher

### Fixed

- HEVC CPU decode (`VideoOutputPreference::CpuFramesOk`, Windows) of an MP4 whose samples carry no VPS/SPS/PPS, only the `hvcC` record does (an `hev1` file such as a 4K 60 fps game recording), accepted every packet and returned no frame, without an error: the inbox decoder reads the parameter sets from the stream, not from the sequence header it is configured with. A random-access picture (IRAP) with none of its own now gets the record's VPS/SPS/PPS in front of it; a picture that has them is left as it is. `iso_bmff::bitstream::hevc` gains `with_parameter_sets`
- H.264 CPU decode (`VideoOutputPreference::CpuFramesOk`) of a stream whose SPS crops at a left or top offset returned frames with only the crop's width and height decoded, from the coded picture's top-left, and the rest of the frame left as the buffer was: the inbox decoder reports the crop but writes it at the wrong place. Such an SPS (in the sequence header or in a packet) is now handed to the decoder without its crop, so the whole coded picture, which is what this path returns, is decoded. A crop of only the right and bottom edges (1080 lines coded as 1088) was never affected. `iso_bmff::bitstream::avc` gains `parse_sps_geometry`, `sps_with_crop` and `annex_b_replacing_sps` for reading and rewriting an SPS's crop
- H.264 CPU decode (`VideoOutputPreference::CpuFramesOk`) dropped frames of an MP4 without any error: an AVCC packet whose first NAL is 256 to 511 bytes long begins `00 00 01 xx`, which the Annex-B start-code probe took for a start code, so it reached the decoder with its length prefixes in it (26 of 218 frames of a libx264 screen recording never came out). A packet whose length prefixes walk exactly to its end is now length-prefixed (`iso_bmff::bitstream::is_length_prefixed`), for H.264 and HEVC
- H.264 CPU decode returned the last frame of a stream stamped with time 0 when it was an IDR picture held in the decoder until the drain (the inbox decoder holds back about 36 frames and stamped it 0, the first frame's time): a consumer that finds frames by time lost it. A frame stamped with a time no packet had now gets the earliest input time still waiting
- H.264 CPU decode (`VideoOutputPreference::CpuFramesOk`) of a picture whose width or height is not a multiple of 16 returned no frames at all: the decoder's stream change was answered by resetting it, which dropped every buffered frame; it now takes the decoder's own NV12 output type
- Windows loopback capture (system and per-app sound) delivered sound after a silent stretch too early against wall-clock video: packets WASAPI marks silent were dropped uncounted, and an idle output device delivers no packets at all. Silent packets are now delivered as zeros and idle time is filled with zeros from the packets' QPC times, so a frame's `pts` (its sample index) follows the wall clock
- The `esds` of an MP4 AAC track declared an `SLConfigDescriptor` it did not contain (the ES descriptor's length was three bytes longer than its content), the `mp4a` sample rate field held the track timescale truncated to 16 bits (16960 Hz for a microsecond timescale) instead of the audio rate, and a video track's `tkhd` had volume 1.0. The descriptors now carry their own counted lengths and the mandatory `SLConfigDescriptor`; the sample entry takes the rate and channel count from the `AudioSpecificConfig` (0 in the rate field above 65535 Hz, as FFmpeg writes it); only an audio track has a volume
- Vulkan Video (encoder and decoder) handed the driver uninitialised output structs when it listed the supported image formats (`vkGetPhysicalDeviceVideoFormatPropertiesKHR`): the driver reads each element's `p_next` before writing it, so HEVC and AV1 sessions crashed intermittently with an access violation on NVIDIA driver 621.01 (six tests aborted at random, H.264 happened not to). The elements are now initialised before the call

## Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config, and low-level APIs as first-class entry points. The workspace ships freestanding, independently versioned core crates (`iso-bmff`, `ebml-webm`, `flv-core`, `adts-core`, `ogg-core`, `riff-wave-core`, `mpeg-ts-core`, `mpeg-audio`, `iso-cenc`, `rtmp`, `rtp-core`) plus one `mediaway` umbrella with five capability crates (`container`, `encoder`, `decoder`, `device`, `sw`) and a single C ABI (`mediaway-ffi`).

This release is for the Rust crates and the Windows screen/window recording and editing path: whole-monitor capture, a fixed capture canvas, a progressive MP4 head and an indexed `moov`, constant-quality H.264 encode, and a run of H.264/HEVC decode, capture-clock and MP4 box fixes. `iso-bmff` moves to 0.1.3 because the new MP4 writers and the HEVC parameter-set helpers live there. The C ABI is unchanged from v0.2.1 (container 8, pipeline 7, device 2).

## Platforms

- Windows (win64): primary target and where this release's changes were verified. Capture of a whole monitor through Windows Graphics Capture, the fixed capture canvas, audio playback that starts within about 20 ms, constant-quality CPU H.264 encode, and H.264/HEVC CPU decode fixes. The D3D12 native decode paths are unchanged and still deliberately not hardware-run (known TDR on the existing D3D12 H.264 decode path).
- Linux: unchanged this release. VA-API, DMA-BUF Zero-Copy and the AMF encode
  backend are compile/test-verified on WSL2 only, with no real VA-API or AMD GPU
  hardware available. A capture region is refused with `Unsupported`.
- macOS / iOS: unchanged this release. The Apple AAC path is the only Apple backend that has run on a real Mac in CI; every other Apple backend (VideoToolbox H.264/HEVC/VP9/AV1/ProRes, camera, screen, Opus) is still compile- and lint-verified only.
- Android: unchanged. NDK `AMediaCodec` decode and Camera2/AAudio/`MediaProjection`
  capture remain authored without a device or emulator, and are not in CI.
- Web (wasm32): unchanged. `@mediaway/browser` ships `iso-bmff-wasm` and WebCodecs
  encode/decode, compile-verified only; the Opus-in-MP4 fix applies to its muxer.

## Codecs

- Encode: H.264 — NVENC, Vulkan Video, QuickSync (VPL), VA-API (GOP), AMF,
  Apple (unverified); HEVC — VA-API (GOP), AMF, Apple (unverified) and, on
  Windows, now muxed into playable MP4; VP9 — VA-API (narrow real-driver support,
  untested); AV1 — software (rav1e), AMF (untested), Vulkan (implemented but
  driver-blocked on this workspace's reference GPU); ProRes — Apple (unverified).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video (hardware-verified),
  VA-API (GOP, untested), D3D12 (HEVC, sans-io only), Apple (unverified); HEVC
  read back out of an MP4 now decodes on Windows; VP9 — VA-API (untested), Apple
  (unverified); AV1 — Vulkan (keyframe-only, hardware-verified), VA-API
  (keyframe-only, untested), D3D12 (keyframe-only, sans-io only), Apple
  (unverified); ProRes — Apple (unverified).
- Audio: Opus — Windows decode via Media Foundation, cross-platform software
  encode/decode (`unsafe-libopus`), native Apple encode/decode (unverified for Opus), and a
  real `Opus`/`dOps` MP4 track; AAC — Windows encode and now decode via Media
  Foundation (hardware-verified round trip), software encode (C# `AudioEncoder`),
  Apple encode/decode (verified on a real macOS runner: round trip to PCM); audio processing module (sonora). AAC decode
  and the Opus decode session are both reachable from all five native bindings
  through one C ABI call, but AAC is the OS's own decoder, so its samples can
  differ between hosts, unlike the software Opus decoder.
- Containers: ISOBMFF/MP4, WebM, FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE, MPEG
  audio — all verified playable in mpv; CENC encryption/decryption; RTMP
  (proposed, unpublished); `rtp-core` for RTP payloadization (H.264/HEVC). New:
  a replay ring over encoded packets and MP4 payload placements.

## Bindings

The native bindings (C, C#, Python, Node, C++) and `@mediaway/browser` carry no API change in this release: the C ABI is unchanged, and the packages are re-stamped with 0.2.2 from the workspace version. [`mediaway_ffi.h`](https://github.com/nyxways/mediaway/releases/tag/v0.2.2) and the CMake/CPack archives are on the GitHub Release page; [`Mediaway.*`](https://www.nuget.org/packages/Mediaway.Common) on NuGet, [`mediaway`](https://pypi.org/project/mediaway/) on PyPI and [`@mediaway/ffi`](https://www.npmjs.com/package/@mediaway/ffi), [`@mediaway/container`](https://www.npmjs.com/package/@mediaway/container), [`@mediaway/device`](https://www.npmjs.com/package/@mediaway/device), [`@mediaway/encoder`](https://www.npmjs.com/package/@mediaway/encoder), [`@mediaway/decoder`](https://www.npmjs.com/package/@mediaway/decoder), [`@mediaway/browser`](https://www.npmjs.com/package/@mediaway/browser) on npm. The Rust crates are on [crates.io](https://crates.io/crates/mediaway).

## Breaking changes

None. No public Rust type or C ABI version changed in this release; every addition is a new function, type or method.

## Maturity bar

Not production-ready. The changes in this release were verified on Windows hardware (the new capture, canvas, MP4 writer, encoder and decoder paths run against real files and the test machine's GPUs); treat every path without an explicit "hardware-verified" tag as unverified. The exceptions to read twice are unchanged from v0.2.1:

- **Apple VideoToolbox, camera, screen and Opus, Android, Linux VA-API/AMF and the D3D12 decode paths** are authored and compile- or test-verified only, with no real-hardware run. The Apple AAC path is the only Apple backend that has run on a real Mac in CI.
- **Bindings on Linux and macOS** are verified for the container capability at most.
- **AAC decode output is host-dependent** by construction: it is the OS's codec.
- **H.264/HEVC CPU decode on Windows** is the inbox Media Foundation software decoder, which has quirks (crop offsets, parameter sets, held-back frames) that this release works around; a stream shape not in the tests may expose another.

Costly paths (CPU readback, SW fallbacks, the throwaway sessions behind the capability probes, a window region away from the origin) are documented at each API (`docs/spec/caveats-and-clarity.md`). See `docs/spec/status.md`.
