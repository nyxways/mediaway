# AAC in MP4: `mp4a` / `esds` (`iso-bmff`)

`crates/iso-bmff/src/isobmff/sample_entry.rs::write_mp4a`, the one writer behind the fragmented
init `moov`, `write_progressive` and `write_indexed_moov`.

## What a strict reader checks

- **`esds`** (ISO/IEC 14496-14 § 3.1.2): `ES_Descriptor` (0x03) holding `DecoderConfigDescriptor`
  (0x04, with the `DecoderSpecificInfo` 0x05 = the `AudioSpecificConfig`) and the mandatory
  `SLConfigDescriptor` (0x06, one byte, 2). Every declared length must equal the bytes under it.
  `write_descriptor` counts the length from the bytes written, so it cannot disagree; before
  2026-10-14 a hand-counted length included an SLConfig that was never written (FFmpeg tolerates it,
  Chromium's parser may not).
- **`samplerate`** is 16.16 fixed point (14496-12 § 12.2.3): the real audio rate, from the
  `AudioSpecificConfig` (`bitstream::aac::parse_audio_specific_config`), never the track timescale.
  It was `time_base.den << 16`, i.e. a microsecond timescale wrote 16960 Hz. Above 65535 Hz (88.2 and
  96 kHz) the integer part cannot hold it: 0 is written, as FFmpeg does, and a reader takes the rate
  from the config. `channelcount` comes from the config too (default stereo).
- **`tkhd` volume** (14496-12 § 8.3.2) is 1.0 for an audio track and 0 for the others.

Tests: `sample_entry_tests.rs` (strict descriptor walk, rate and channels) and
`progressive_tests.rs::only_an_audio_track_has_a_volume`.

## Not decided here

`DecoderConfigDescriptor` still declares `bufferSizeDB` 0 and 128 kbps max/average whatever the
encoder produced (readers ignore both); `ES_ID` is the track id plus one.
