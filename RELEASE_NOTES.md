# Mediaway release notes

<!-- Accumulate development changes under ## Unreleased below. See
     docs/ai/wiki/meta/release-notes.md and AGENTS.md § 10 (Release notes).
     Finalize with /release-notes <version>; reset with /release-notes reset. -->

## Unreleased

### Added

- Capture a whole monitor through Windows Graphics Capture (`DesktopCaptureSource::Monitor`, `DesktopVideoCaptureConfig::monitor`), so a screen region can be cropped on the GPU with the pointer drawn in, which the DXGI screen path cannot do
- Windows audio playback can tell how much of what was written has been heard (`WindowsWasapiPlayback::played_frames`, from the device's own clock) and can drop everything not yet played at once (`WindowsWasapiPlayback::flush`), so a player can keep its picture on the sound and cut the sound on a seek
- A fixed-size capture canvas for Windows (`windows_desktop::Canvas`): each captured frame, whatever its size, is copied centered onto one texture on the GPU, so a window recording keeps one encoder size through resizes
- The Windows capture canvas can take a fixed part of each frame instead (`Canvas::place_texture_part`, `CanvasPlacement::part`): the part keeps its offset from the window's top-left through resizes and is black where a shrunk window no longer reaches, so a part of a window can be recorded while the window is covered
- `iso_bmff::isobmff::write_progressive` writes the head of a non-fragmented MP4 (`ftyp`, a `moov` with real sample tables and an optional edit list per track, the `mdat` header) for samples all known before the file is written, the counterpart of `parse_stbl_samples`: a stream copy that starts at the keyframe before the moment it should show from can hide the packets before it with an edit list, which FFmpeg applies to a non-fragmented file and not to a fragmented one
- `iso_bmff::isobmff::write_indexed_moov` writes a `moov` with sample tables for samples already in a file, at absolute offsets and with a chunk for each run of adjacent samples: appended after a fragmented recording's last fragment it makes the file an ordinary MP4 (a duration for Windows and chat apps) without rewriting a sample
- `WindowsVideoEncoder::open_constant_quality` opens the CPU H.264 encoder with constant-quality rate control (`CODECAPI_AVEncCommonRateControlMode` = Quality, `CODECAPI_AVEncCommonQuality` 1 to 100) instead of a bitrate target: an encode whose output should stay close to its source (an export of an edit) gets the bits a busy picture needs and does not spend the ones a still picture does not; on four 5 to 7 s sources (a still screen, two 806x462 screens, a 1280x720 moving test pattern) quality 95 re-encoded at 47 to 53 dB PSNR against the source where a target of 0.1 bits per pixel gave 40 to 47 dB, with 3 to 11 % smaller files on the screens and larger ones on the busy pictures the target had starved

### Changed

- Windows audio playback starts what is written within about 20 ms: with nothing to play it no longer fills its one-second buffer with silence that later sound had to wait behind
- The Windows CPU H.264 encoder (the inbox software MFT, `VideoInputPreference::CpuUploadOk`) is asked for no B-frames: it used to reorder and stamp its first `pts` one frame late with the `dts` behind the `pts`, so an MP4 written from it started late and FFmpeg reported non-monotonic decode times at fragment boundaries. Its packets now have `pts == dts` and the first `pts` is the first input's time; on one 7 s 850x474 screen recording at the same target bitrate the file was 2 % smaller and 0.37 dB PSNR higher

### Fixed

- H.264 CPU decode (`VideoOutputPreference::CpuFramesOk`) of a picture whose width or height is not a multiple of 16 returned no frames at all: the decoder's stream change was answered by resetting it, which dropped every buffered frame; it now takes the decoder's own NV12 output type
- Windows loopback capture (system and per-app sound) delivered sound after a silent stretch too early against wall-clock video: packets WASAPI marks silent were dropped uncounted, and an idle output device delivers no packets at all. Silent packets are now delivered as zeros and idle time is filled with zeros from the packets' QPC times, so a frame's `pts` (its sample index) follows the wall clock
- The `esds` of an MP4 AAC track declared an `SLConfigDescriptor` it did not contain (the ES descriptor's length was three bytes longer than its content), the `mp4a` sample rate field held the track timescale truncated to 16 bits (16960 Hz for a microsecond timescale) instead of the audio rate, and a video track's `tkhd` had volume 1.0. The descriptors now carry their own counted lengths and the mandatory `SLConfigDescriptor`; the sample entry takes the rate and channel count from the `AudioSpecificConfig` (0 in the rate field above 65535 Hz, as FFmpeg writes it); only an audio track has a volume

### Removed

### Deprecated

### Breaking
