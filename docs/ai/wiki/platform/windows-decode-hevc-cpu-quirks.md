# Windows HEVC CPU decode: parameter sets must be in the stream

Found 2026-10-22 by a screen recorder (Recoway) that could not open a 4K 60 fps game recording
(HEVC Main, `hev1`, 1444 frames; FFmpeg decodes it). Handled in `mediaway-decoder`
(`src/windows/wmf/video_cpu.rs`), tests in `video_cpu_tests.rs` and `iso-bmff`'s `hevc_tests.rs`.

## Symptom

`WmfMultiCodecCpuDecoder` accepted every packet, drained cleanly and produced **zero frames**, no
error. The index of the file was fine (1444 frames), so a reader above it saw "no first frame".

## Cause

The file's `hev1` samples hold an SEI and the IDR only; VPS/SPS/PPS exist only in the `hvcC` record
(ISO/IEC 14496-15 lets `hev1` keep them in the samples, it does not require it). The decoder was
configured with them as an Annex-B sequence header (`resolve_framing`), but the inbox HEVC MFT
does not start from that: it wants them in front of the first picture.

Measured on the 4K file cut to 2 s with `ffmpeg -c:v copy`: no frame for any index. The same clip
with `-bsf:v hevc_mp4toannexb` (which puts VPS/SPS/PPS before each IRAP picture) decoded every
frame at 3840x2160 (coded 3840x2176), so the size is not the cause.

## Fix

`iso_bmff::bitstream::hevc::with_parameter_sets` puts the header in front of an IRAP picture
(`nal_unit_type` 16..=23) that has no VPS/SPS/PPS of its own; a picture with any of them, or a
non-IRAP picture, is passed on as it is. The decoder calls it per packet
(`with_stream_parameter_sets`), only for HEVC whose `extra_data` gave an Annex-B header.

Test: `hevc_samples_without_in_band_parameter_sets_decode` encodes with the HEVC MFT, strips the
VPS/SPS/PPS, length-prefixes the pictures, and asserts a frame comes out (fails without the fix;
skips where there is no HEVC encoder or decoder MFT).

## Not done

H.264 has the same shape for `avc1` files whose samples lack SPS/PPS; its caller prepends them
itself (Recoway's `avc::with_parameter_sets`) and this was not measured here.
