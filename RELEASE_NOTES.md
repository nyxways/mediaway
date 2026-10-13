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

### Changed

- Windows audio playback starts what is written within about 20 ms: with nothing to play it no longer fills its one-second buffer with silence that later sound had to wait behind

### Fixed

- H.264 CPU decode (`VideoOutputPreference::CpuFramesOk`) of a picture whose width or height is not a multiple of 16 returned no frames at all: the decoder's stream change was answered by resetting it, which dropped every buffered frame; it now takes the decoder's own NV12 output type
- Windows loopback capture (system and per-app sound) delivered sound after a silent stretch too early against wall-clock video: packets WASAPI marks silent were dropped uncounted, and an idle output device delivers no packets at all. Silent packets are now delivered as zeros and idle time is filled with zeros from the packets' QPC times, so a frame's `pts` (its sample index) follows the wall clock

### Removed

### Deprecated

### Breaking
