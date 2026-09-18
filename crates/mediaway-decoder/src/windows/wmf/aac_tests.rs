#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::cast_precision_loss,
    reason = "unit tests may unwrap"
)]

use super::*;

/// `AudioSpecificConfig` for AAC-LC, 44.1 kHz, stereo — the same 2 bytes `iso-bmff`'s
/// `write_mp4a` writes as its `esds` default when a track carries no real config record.
///
/// Bit layout (ISO/IEC 14496-3): `audioObjectType` = 2 (AAC-LC, 5 bits),
/// `samplingFrequencyIndex` = 4 (44100 Hz, 4 bits), `channelConfiguration` = 2
/// (stereo, 4 bits), then three zero `GASpecificConfig` flags.
const ASC_AAC_LC_44100_STEREO: [u8; 2] = [0x12, 0x10];

/// Samples per channel in one AAC-LC frame. The MFT rejects the 960-sample variant
/// outright, so 1024 is the only frame length reachable here.
const AAC_FRAME_SAMPLES: u64 = 1024;

fn cfg() -> AacDecoderConfig {
    AacDecoderConfig::new(44_100, 2, Bytes::from(ASC_AAC_LC_44100_STEREO.to_vec()))
}

#[test]
fn user_data_blob_matches_the_documented_heaacwaveinfo_layout() {
    // Pure unit test — no MFT involved. Pins the byte layout against Microsoft's own
    // worked example in the AAC decoder reference, which gives
    // `{00 00 2a 00 00 00 00 00 00 00 00 00 11 b0}` for a 6-channel 48 kHz stream:
    // 12 bytes of `HEAACWAVEINFO` tail, then the 2-byte `AudioSpecificConfig`.
    let blob = user_data_blob(&ASC_AAC_LC_44100_STEREO);

    assert_eq!(blob.len(), HEAAC_WAVEINFO_TAIL_LEN + 2);
    assert_eq!(
        &blob[0..2],
        &0u16.to_le_bytes(),
        "wPayloadType = 0 (raw AAC)"
    );
    assert_eq!(
        &blob[2..4],
        &0x00FEu16.to_le_bytes(),
        "wAudioProfileLevelIndication = 0xFE (unspecified)"
    );
    assert_eq!(&blob[4..6], &[0, 0], "wStructType = 0");
    assert_eq!(&blob[6..8], &[0, 0], "wReserved1 = 0");
    assert_eq!(&blob[8..12], &[0, 0, 0, 0], "dwReserved2 = 0");
    assert_eq!(
        &blob[12..],
        &ASC_AAC_LC_44100_STEREO,
        "AudioSpecificConfig must follow the 12-byte tail verbatim"
    );
}

#[test]
fn rejects_empty_audio_specific_config() {
    // Guessing an ASC from rate/channels alone is wrong for SBR/PS streams, where the
    // media type describes the pre-tool core — so this must fail rather than synthesize.
    let bad = AacDecoderConfig::new(44_100, 2, Bytes::new());
    let err = WmfAacDecoder::open(&bad)
        .err()
        .expect("empty extra_data must be rejected");
    assert_eq!(err, DecodeError::Unsupported);
}

#[test]
fn rejects_invalid_config() {
    let bad = AacDecoderConfig {
        sample_rate: 0,
        channels: 2,
        time_base: Rational::new(1, 44_100),
        extra_data: Bytes::from(ASC_AAC_LC_44100_STEREO.to_vec()),
    };
    let err = WmfAacDecoder::open(&bad)
        .err()
        .expect("zero sample_rate must be rejected");
    assert_eq!(err, DecodeError::InvalidInput);
}

#[test]
fn open_or_skip_without_aac_decoder_mft() {
    let dec = match WmfAacDecoder::open(&cfg()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skip: WmfAacDecoder::open failed ({e:?}) — no inbox AAC decoder MFT?");
            return;
        }
    };
    assert_eq!(dec.stream_info().sample_rate(), Some(44_100));
    assert_eq!(dec.stream_info().channels(), Some(2));
    assert_eq!(dec.stream_info().codec(), CodecKind::Aac);
}

/// 20 ms of 440 Hz sine as interleaved stereo f32, `sample_rate` Hz.
fn sine_frame(sample_rate: u32, channels: u16, samples_per_channel: usize) -> AudioFrame {
    let mut pcm: Vec<f32> = Vec::with_capacity(samples_per_channel * usize::from(channels));
    for i in 0..samples_per_channel {
        let t = i as f32 / sample_rate as f32;
        let s = (t * 440.0_f32 * std::f32::consts::TAU).sin();
        for _ in 0..channels {
            pcm.push(s);
        }
    }
    let bytes: Vec<u8> = pcm.iter().flat_map(|f| f.to_le_bytes()).collect();
    AudioFrame {
        pts: 0,
        duration: samples_per_channel as u64,
        sample_rate,
        channels,
        format: SampleFormat::F32,
        data: bytes.into(),
    }
}

/// The real verification: this workspace's own WMF AAC **encoder** produces a genuine
/// AAC-LC bitstream plus the `AudioSpecificConfig` it negotiated, and this decoder turns
/// it back into real PCM.
///
/// Encoding first is what makes this honest — there is no software AAC encoder in the
/// workspace to fall back on, and a hand-written `raw_data_block()` would prove only that
/// the MFT tolerates whatever was hand-written. This mirrors `opus_tests`'
/// `roundtrip_sw_encoded_sine_decodes_to_pcm_or_skip`.
#[test]
fn roundtrip_wmf_encoded_sine_decodes_to_real_pcm_or_skip() {
    use mediaway_encoder::windows::WindowsAudioEncoder;
    use mediaway_encoder::{AudioEncoder, AudioEncoderConfig};

    let sample_rate = 48_000_u32;
    let channels = 2_u16;
    let enc_cfg = AudioEncoderConfig::aac_stereo(sample_rate, Rational::new(1, sample_rate));
    let Ok(mut enc) = WindowsAudioEncoder::open(&enc_cfg) else {
        eprintln!("skip: no inbox WMF AAC encoder MFT");
        return;
    };

    // Feed enough PCM to be sure the encoder emits at least a couple of 1024-sample
    // AAC frames rather than buffering everything.
    let frame_samples = 4096_usize;
    enc.push_frame(&sine_frame(sample_rate, channels, frame_samples))
        .expect("aac encode push");
    enc.flush().expect("aac encode flush");

    let mut packets = Vec::new();
    while let Some(pkt) = enc.poll_packet().expect("aac encode poll") {
        packets.push(pkt);
    }
    assert!(
        !packets.is_empty(),
        "WMF AAC encoder produced no packets — cannot verify the decoder"
    );

    // The encoder reports the ASC it negotiated only after the output type is resolved,
    // which is exactly the record the decoder needs.
    let asc = enc.stream_info().extra_data().clone();
    assert!(
        !asc.is_empty(),
        "encoder produced no AudioSpecificConfig; decoder cannot be configured"
    );

    let dec_cfg = AacDecoderConfig::new(sample_rate, channels, asc);
    let Ok(mut dec) = WmfAacDecoder::open(&dec_cfg) else {
        eprintln!("skip: no inbox AAC decoder MFT");
        return;
    };

    let mut frames = 0_usize;
    let mut total_samples_per_channel = 0_u64;
    let mut peak = 0.0_f32;
    let mut consume = |frame: &AudioFrame| {
        assert_eq!(frame.format, SampleFormat::F32);
        assert_eq!(frame.channels, channels);
        assert_eq!(frame.sample_rate, sample_rate);
        assert_eq!(
            frame.data.len() % (4 * usize::from(channels)),
            0,
            "float32 stereo frame must be a whole number of interleaved samples"
        );
        let (samples, _) = frame.data.as_ref().as_chunks::<4>();
        for chunk in samples {
            peak = peak.max(f32::from_le_bytes(*chunk).abs());
        }
    };

    for pkt in &packets {
        dec.push_packet(pkt).expect("aac decode push");
        while let Some(frame) = dec.poll_frame().expect("aac decode poll") {
            consume(&frame);
            total_samples_per_channel += frame.duration;
            frames += 1;
        }
    }
    dec.flush().expect("aac decode flush");
    while let Some(frame) = dec.poll_frame().expect("aac decode poll") {
        consume(&frame);
        total_samples_per_channel += frame.duration;
        frames += 1;
    }

    assert!(frames >= 1, "expected at least one decoded PCM frame");
    // AAC-LC frames are 1024 samples/channel; the decoder must return whole frames.
    // Measured on a Windows 11 box: 4096 PCM samples/channel in -> exactly 4 AAC-LC
    // frames -> 4096 samples/channel back out. Asserting the exact round trip (rather
    // than a loose lower bound) is what would catch a decoder that drops or duplicates
    // a frame.
    assert_eq!(
        total_samples_per_channel, frame_samples as u64,
        "expected a sample-exact round trip of {AAC_FRAME_SAMPLES}-sample AAC-LC frames"
    );
    // A 440 Hz sine must come back as audible signal, not silence — this is what
    // separates "the MFT accepted our bytes" from "the MFT actually decoded them".
    assert!(
        peak > 0.01,
        "decoded PCM peaked at {peak}, which is silence — the bitstream was not decoded"
    );
}

/// Drives one packet through `dec` purely via the [`AudioDecoder`] trait bound — proves
/// the type satisfies the trait, not just its own inherent methods (same shape as
/// `opus_tests::decode_via_trait`).
#[test]
fn decodes_via_audio_decoder_trait_or_skip() {
    use mediaway_encoder::windows::WindowsAudioEncoder;
    use mediaway_encoder::{AudioEncoder, AudioEncoderConfig};

    fn drive<D: AudioDecoder>(dec: &mut D, packets: &[Packet]) -> usize {
        for pkt in packets {
            dec.push_packet(pkt).expect("push via trait");
        }
        dec.flush().expect("flush via trait");
        let mut frames = 0_usize;
        while dec.poll_frame().expect("poll via trait").is_some() {
            frames += 1;
        }
        frames
    }

    let sample_rate = 48_000_u32;
    let enc_cfg = AudioEncoderConfig::aac_stereo(sample_rate, Rational::new(1, sample_rate));
    let Ok(mut enc) = WindowsAudioEncoder::open(&enc_cfg) else {
        eprintln!("skip: no inbox WMF AAC encoder MFT");
        return;
    };
    enc.push_frame(&sine_frame(sample_rate, 2, 4096))
        .expect("aac encode push");
    enc.flush().expect("aac encode flush");
    let mut packets = Vec::new();
    while let Some(pkt) = enc.poll_packet().expect("aac encode poll") {
        packets.push(pkt);
    }
    let asc = enc.stream_info().extra_data().clone();
    if packets.is_empty() || asc.is_empty() {
        eprintln!("skip: WMF AAC encoder produced no usable stream");
        return;
    }

    let Ok(mut dec) = WmfAacDecoder::open(&AacDecoderConfig::new(sample_rate, 2, asc)) else {
        eprintln!("skip: no inbox AAC decoder MFT");
        return;
    };
    assert!(
        drive(&mut dec, &packets) >= 1,
        "expected at least one decoded PCM frame via trait"
    );
}
