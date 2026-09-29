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

use super::{AacDecoder, AacDecoderConfig};

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

/// The `esds` payload Core Audio's own MPEG-4 AAC magic cookies use: an `ES_Descriptor` wrapping a
/// `DecoderConfigDescriptor` that carries the `AudioSpecificConfig` as `DecoderSpecificInfo`.
fn esds_cookie(asc: &[u8]) -> Vec<u8> {
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
    let esds = esds_cookie(&ASC);
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
