//! Real-macOS tests for the `AudioConverter` AAC decoder.
//!
//! The Apple audio backends had no tests at all until this file: CI only linted them, so their
//! first real run was the release pipeline's RC gate, where `AacDecoder::open` failed on a real
//! `macos-14` runner. The `OSStatus` behind that failure is not carried by `DecodeError::Backend`,
//! so [`open_variants_report_their_os_status`] calls `AudioConverter` directly with each candidate
//! configuration and prints what the OS answers.

#![cfg(test)]
#![allow(
    unsafe_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    clippy::suboptimal_flops,
    reason = "real-hardware test that reports what the OS answers"
)]

use std::ptr::NonNull;

use mediaway_common::{Bytes, Rational};
use objc2_audio_toolbox::{
    AudioConverterDispose, AudioConverterNew, AudioConverterRef, AudioConverterSetProperty,
    kAudioConverterDecompressionMagicCookie,
};
use objc2_core_audio_types::{
    AudioStreamBasicDescription, kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked,
    kAudioFormatLinearPCM, kAudioFormatMPEG4AAC,
};

use super::{AacDecoder, AacDecoderConfig, esds_cookie};
use crate::AudioDecoder;

const RATE: u32 = 44_100;
const CHANNELS: u32 = 2;
/// AAC-LC, 44.1 kHz, stereo — the `AudioSpecificConfig` `platform::decoder_support` probes with.
const ASC: [u8; 2] = [0x12, 0x10];
/// `kMPEG4Object_AAC_LC`, the value Core Audio documents for an AAC-LC `mFormatFlags`.
const MPEG4_OBJECT_AAC_LC: u32 = 2;

fn aac_format(
    flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
) -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        mSampleRate: f64::from(RATE),
        mFormatID: kAudioFormatMPEG4AAC,
        mFormatFlags: flags,
        mBytesPerPacket: bytes_per_packet,
        mFramesPerPacket: frames_per_packet,
        mBytesPerFrame: 0,
        mChannelsPerFrame: CHANNELS,
        mBitsPerChannel: 0,
        mReserved: 0,
    }
}

fn pcm_format() -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        mSampleRate: f64::from(RATE),
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
        mBytesPerPacket: CHANNELS * 4,
        mFramesPerPacket: 1,
        mBytesPerFrame: CHANNELS * 4,
        mChannelsPerFrame: CHANNELS,
        mBitsPerChannel: 32,
        mReserved: 0,
    }
}

/// The experiment's own, independently written copy of the cookie the decoder builds. It is kept so
/// [`the_decoder_builds_the_cookie_the_os_accepted`] compares two implementations rather than one
/// against itself.
fn experiment_esds_cookie(asc: &[u8]) -> Vec<u8> {
    let dsi = {
        let mut v = vec![0x05, u8::try_from(asc.len()).unwrap()];
        v.extend_from_slice(asc);
        v
    };
    let dcd_body = {
        // objectTypeIndication 0x40 (MPEG-4 audio), streamType 0x15 (audio), bufferSizeDB,
        // maxBitrate, avgBitrate.
        let mut v = vec![0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        v.extend_from_slice(&dsi);
        v
    };
    let mut dcd = vec![0x04, u8::try_from(dcd_body.len()).unwrap()];
    dcd.extend_from_slice(&dcd_body);
    let slc = [0x06, 0x01, 0x02];
    let esd_body = {
        // ES_ID (2 bytes) + flags (1 byte) + DecoderConfigDescriptor + SLConfigDescriptor.
        let mut v = vec![0, 0, 0];
        v.extend_from_slice(&dcd);
        v.extend_from_slice(&slc);
        v
    };
    let mut esd = vec![0x03, u8::try_from(esd_body.len()).unwrap()];
    esd.extend_from_slice(&esd_body);
    esd
}

/// Tries one `(source format, cookie)` pair and returns the two `OSStatus` values, `New` then
/// `SetProperty` (`None` when there was no converter to set it on).
fn try_variant(source: &AudioStreamBasicDescription, cookie: &[u8]) -> (i32, Option<i32>) {
    let destination = pcm_format();
    let mut converter: AudioConverterRef = std::ptr::null_mut();
    // SAFETY: both descriptions are fully initialised locals that outlive the call.
    let created = unsafe {
        AudioConverterNew(
            NonNull::from(source),
            NonNull::from(&destination),
            NonNull::from(&mut converter),
        )
    };
    if created != 0 || converter.is_null() {
        return (created, None);
    }
    let mut bytes = cookie.to_vec();
    // SAFETY: `converter` was just created; `bytes` is a live buffer for this synchronous call.
    let set = unsafe {
        AudioConverterSetProperty(
            converter,
            kAudioConverterDecompressionMagicCookie,
            u32::try_from(bytes.len()).unwrap(),
            NonNull::new(bytes.as_mut_ptr()).unwrap().cast(),
        )
    };
    // SAFETY: `converter` is valid and not yet disposed.
    let _ = unsafe { AudioConverterDispose(converter) };
    (created, Some(set))
}

/// Prints what the OS answers for each candidate configuration. It asserts nothing about which one
/// works: it exists to make the failure diagnosable from a CI log, and the decoder's own test below
/// asserts the outcome once a variant is chosen.
#[test]
fn open_variants_report_their_os_status() {
    let raw = ASC.to_vec();
    let esds = experiment_esds_cookie(&ASC);
    eprintln!("esds cookie ({} bytes): {esds:02x?}", esds.len());

    let sources = [
        (
            "flags=0, bytesPerPacket=0, framesPerPacket=1024 (current)",
            aac_format(0, 0, 1024),
        ),
        (
            "flags=AAC_LC, bytesPerPacket=0, framesPerPacket=1024",
            aac_format(MPEG4_OBJECT_AAC_LC, 0, 1024),
        ),
        ("flags=0, framesPerPacket=0", aac_format(0, 0, 0)),
        (
            "flags=AAC_LC, framesPerPacket=0",
            aac_format(MPEG4_OBJECT_AAC_LC, 0, 0),
        ),
    ];
    for (label, source) in &sources {
        for (cookie_label, cookie) in [("raw ASC", &raw), ("esds", &esds)] {
            let (created, set) = try_variant(source, cookie);
            eprintln!(
                "VARIANT {label} | cookie={cookie_label} => AudioConverterNew={created}, SetProperty={set:?}"
            );
        }
    }
}

/// The decoder as shipped, opened the way `platform::decoder_support` and the C ABI open it.
#[test]
fn the_decoder_opens_with_a_real_audio_specific_config() {
    let config = AacDecoderConfig::new(
        RATE,
        u16::try_from(CHANNELS).unwrap(),
        Rational::new(1, RATE),
        Bytes::from(ASC.to_vec()),
    );
    let opened = AacDecoder::open(&config);
    eprintln!(
        "AacDecoder::open => {:?}",
        opened.as_ref().map(|_| "Ok").map_err(|e| format!("{e:?}"))
    );
    assert!(
        opened.is_ok(),
        "the Apple AAC decoder must open on a real macOS runner"
    );
}

/// The cookie the decoder builds is the one the OS accepted in the experiment above, byte for byte,
/// and stays valid when the descriptor grows past one length byte.
#[test]
fn the_decoder_builds_the_cookie_the_os_accepted() {
    assert_eq!(esds_cookie(&ASC), experiment_esds_cookie(&ASC));
    assert_eq!(esds_cookie(&ASC).len(), 27);

    // A 200-byte ASC (a program config element can be this large) needs a two-byte length.
    let big = vec![0x5a; 200];
    let cookie = esds_cookie(&big);
    assert_eq!(cookie[0], 0x03, "starts with the ES_Descriptor tag");
    assert!(
        cookie[1] & 0x80 != 0,
        "a length above 127 continues into a second byte"
    );
    assert!(
        cookie.windows(big.len()).any(|w| w == big.as_slice()),
        "the ASC is carried verbatim"
    );
}

/// The Apple AAC encoder and this decoder, end to end: PCM in, AAC out, PCM back. This is the check
/// that `open` succeeding is not enough — and it also reports what the encoder exposes as the
/// stream's `extra_data`, because the decoder takes the bare `AudioSpecificConfig` and Core Audio's
/// encoder-side magic cookie may be the wrapped `esds` form instead.
#[test]
fn an_apple_encoded_stream_decodes_back_to_pcm() {
    use mediaway_common::{AudioFrame, CodecKind, SampleFormat, StreamInfo};
    use mediaway_encoder::apple::AppleAudioEncoder;
    use mediaway_encoder::{AudioEncoder, AudioEncoderConfig};

    const FRAMES: usize = 40;
    const FRAME_SAMPLES: usize = 1024;
    let channels = u16::try_from(CHANNELS).unwrap();

    let config = AudioEncoderConfig {
        codec: CodecKind::Aac,
        sample_rate: RATE,
        channels,
        sample_format: SampleFormat::F32,
        time_base: Rational::new(1, RATE),
        bitrate_bps: 0,
    };
    let mut encoder = AppleAudioEncoder::open(&config).expect("the Apple AAC encoder opens");
    for i in 0..FRAMES {
        let mut pcm = Vec::with_capacity(FRAME_SAMPLES * CHANNELS as usize * 4);
        for s in 0..FRAME_SAMPLES {
            let t = (i * FRAME_SAMPLES + s) as f32 / RATE as f32;
            let v = (t * 440.0 * std::f32::consts::TAU).sin();
            for _ in 0..CHANNELS {
                pcm.extend_from_slice(&v.to_le_bytes());
            }
        }
        let frame = AudioFrame {
            pts: i64::try_from(i * FRAME_SAMPLES).unwrap(),
            duration: FRAME_SAMPLES as u64,
            sample_rate: RATE,
            channels,
            format: SampleFormat::F32,
            data: Bytes::from(pcm),
        };
        encoder.push_frame(&frame).expect("push_frame");
    }
    encoder.flush().expect("flush");

    let mut packets = Vec::new();
    while let Some(p) = encoder.poll_packet().expect("poll_packet") {
        packets.push(p);
    }
    let extra_data = match encoder.stream_info() {
        StreamInfo::Audio { extra_data, .. } => extra_data.clone(),
        other => panic!("not an audio stream: {other:?}"),
    };
    eprintln!(
        "ENCODER extra_data ({} bytes): {extra_data:02x?}",
        extra_data.len()
    );
    eprintln!(
        "ENCODER packets: {}, first sizes {:?}",
        packets.len(),
        packets
            .iter()
            .take(4)
            .map(|p| p.payload.len())
            .collect::<Vec<_>>()
    );
    assert!(!packets.is_empty(), "the encoder produced no packets");

    // What the decoder is given is the bare ASC. If the encoder handed back the wrapped form, unwrap
    // the DecoderSpecificInfo (tag 0x05) so the decode half is still exercised, and report it.
    let asc = bare_asc(&extra_data);
    eprintln!("ASC used for decode: {asc:02x?}");

    let decoder_config =
        AacDecoderConfig::new(RATE, channels, Rational::new(1, RATE), Bytes::from(asc));
    let mut decoder = AacDecoder::open(&decoder_config).expect("the decoder opens");
    for p in &packets {
        decoder.push_packet(p).expect("push_packet");
    }
    decoder.flush().expect("flush");
    let mut samples = 0usize;
    let mut energy = 0.0f64;
    while let Some(frame) = decoder.poll_frame().expect("poll_frame") {
        for chunk in frame.data.as_chunks::<4>().0 {
            let v = f64::from(f32::from_le_bytes(*chunk));
            energy += v * v;
        }
        samples += frame.data.len() / 4 / CHANNELS as usize;
    }
    let mean_square = energy / (samples.max(1) * CHANNELS as usize) as f64;
    eprintln!(
        "DECODED {} packets -> {samples} samples/ch, mean square {mean_square:.3}",
        packets.len()
    );
    assert!(
        samples >= packets.len() * FRAME_SAMPLES - FRAME_SAMPLES,
        "decoded {samples} samples for {} packets",
        packets.len()
    );
    assert!(
        mean_square > 0.05,
        "decoded audio is near-silent (mean square {mean_square})"
    );
}

/// The bare `AudioSpecificConfig` inside `extra_data`, whichever form the encoder exposes: already
/// bare, or an `esds` descriptor whose `DecoderSpecificInfo` (tag 0x05) carries it.
fn bare_asc(extra_data: &[u8]) -> Vec<u8> {
    if extra_data.first() != Some(&0x03) {
        return extra_data.to_vec();
    }
    // Walk to the first 0x05 tag and read its (single- or multi-byte) length.
    let mut i = 0;
    while i + 1 < extra_data.len() {
        if extra_data[i] == 0x05 {
            let mut len = 0usize;
            let mut j = i + 1;
            while j < extra_data.len() {
                len = (len << 7) | usize::from(extra_data[j] & 0x7f);
                let more = extra_data[j] & 0x80 != 0;
                j += 1;
                if !more {
                    break;
                }
            }
            return extra_data[j..j + len].to_vec();
        }
        i += 1;
    }
    extra_data.to_vec()
}
