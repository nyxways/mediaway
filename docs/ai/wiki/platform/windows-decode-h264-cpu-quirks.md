# Windows H.264 CPU decode: three quirks of the inbox MFT

Found 2026-10-14 by a screen recorder (Recoway) reading its own and libx264 MP4 files back.
Both showed as "frame N could not be decoded"; both are handled in `mediaway-decoder`
(`src/windows/wmf/{shared,h264}.rs`), tests in `h264_tests.rs` and `iso-bmff`'s `avc_tests.rs`.

## 1. A start-code probe cannot tell AVCC from Annex-B

An AVCC packet whose first NAL is 256 to 511 bytes long begins `00 00 01 xx` (1 byte: `00 00 00 01`),
which is an Annex-B start code. `is_annex_b` said yes, the packet went to the MFT with its length
prefixes still in it, and the MFT **silently dropped the frame** (no error, no output: 26 of 218
frames of a libx264 screen recording; typical for tiny P pictures of a still desktop).
`iso_bmff::bitstream::is_length_prefixed` walks the lengths instead: a packet whose prefixes land
exactly on its last byte is AVCC. `packet_to_sample` tries it first, then the start-code probe.

## 2. An output frame stamped with a time no packet had

The default (non low-latency) MFT holds back about 36 frames (output #1 after 45 inputs on one
file). At the drain the **last frame can come out stamped 0** (`GetSampleTime` is `Ok(0)`, not an
error), which is also the first frame's time; a consumer that finds frames by time loses it
(the last frame of a 241-frame 60 fps recording, an IDR picture). `InFlight` in `h264.rs` keeps the
times of packets not yet seen on an output and gives such a frame the earliest one (decoder output
is in display order).

Not done, measured: `CODECAPI_AVLowLatencyMode` removes the 36-frame hold and the bad stamp, but
sequential decode got 35 to 75 % slower (debug build, 150 to 180 frames, 720p and 806x462; the
inbox decoder seems to give up frame-parallel decoding), so it is not the default.

## 3. A crop at a left or top offset leaves part of the picture undecoded

Found 2026-10-14 by the same recorder, whose window recordings crop a fixed-size canvas to the
window's picture with the SPS's `frame_cropping` rectangle. Measured with a scratch probe driving
the MFT directly on a 640 x 400 stream cropped to 480 x 360 at (80, 20): the output type is
640 x 400, stride 640, with the minimum display, geometric and pan-scan apertures all at (80, 20),
480 x 360, but only the **top-left 480 x 360 of the coded picture is written** (identical to
FFmpeg's `-flags2 +ignorecrop` decode there); the 80 columns right and 20 rows below are left as
the buffer was. A 1-D (`MFCreateMemoryBuffer`) and a 2-D (`MFCreate2DMediaBuffer`) output buffer
behave the same. A crop of the right and bottom edges only (1080 lines coded as 1088) is not
affected, which is why the common case never shows it.

The CPU path returns the coded picture and passes no aperture on, so `without_offset_crop` in
`h264.rs` rewrites such an SPS (sequence header and in-band, before the first slice of each access
unit) to crop nothing before the MFT sees it (`iso_bmff::bitstream::avc::{sps_with_crop,
annex_b_replacing_sps}`). Test: `a_crop_at_an_offset_still_decodes_the_whole_coded_picture_on_the_cpu`.
The DX11 path is untouched and was not measured.
