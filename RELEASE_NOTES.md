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

### Removed

### Deprecated

### Breaking
