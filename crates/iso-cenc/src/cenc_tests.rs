//! Unit tests for `ClearKey` `cenc` (sibling of `cenc.rs`).

#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::{Pattern, Subsample, decrypt_cenc, encrypt_cenc, iv_from_8};

/// NIST SP 800-38A F.5.1 — AES-128 encrypt of the initial counter block.
#[test]
fn aes128_ctr_nist_counter_block() {
    let key = hex16("2b7e151628aed2a6abf7158809cf4f3c");
    let counter = hex16("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff");
    let mut data = [0u8; 16];
    decrypt_cenc(&key, &counter, Pattern::NONE, &mut data, &[]).unwrap();
    // Zero plaintext ⇒ keystream = AES_K(counter).
    assert_eq!(data, hex16("ec8cdf7398607cb0f2d21675ea9ea1e4"));
    // CTR is an involution.
    let mut again = data;
    decrypt_cenc(&key, &counter, Pattern::NONE, &mut again, &[]).unwrap();
    assert_eq!(again, [0u8; 16]);
}

#[test]
fn aes128_ctr_nist_known_plaintext() {
    let key = hex16("2b7e151628aed2a6abf7158809cf4f3c");
    let counter = hex16("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff");
    let mut data = hex16("6bc1bee22e409f96e93d7e117393172a");
    encrypt_cenc(&key, &counter, Pattern::NONE, &mut data, &[]).unwrap();
    assert_eq!(data, hex16("874d6191b620e3261bef6864990db6ce"));
}

#[test]
fn subsample_clear_does_not_advance_counter() {
    let key = hex16("00000000000000000000000000000000");
    let iv = hex16("00000000000000000000000000000000");
    let mut sample = vec![0xAAu8; 4];
    sample.extend_from_slice(&[0u8; 16]);
    let subs = [Subsample {
        clear_bytes: 4,
        protected_bytes: 16,
    }];
    decrypt_cenc(&key, &iv, Pattern::NONE, &mut sample, &subs).unwrap();
    assert_eq!(&sample[..4], &[0xAA; 4]);

    let mut only_prot = [0u8; 16];
    decrypt_cenc(&key, &iv, Pattern::NONE, &mut only_prot, &[]).unwrap();
    assert_eq!(&sample[4..], &only_prot);
}

#[test]
fn iv_from_8_packs_high_bytes() {
    let iv = iv_from_8(&[1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(&iv[..8], &[1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(&iv[8..], &[0; 8]);
}

#[test]
fn subsample_overflow_errors() {
    let key = [0u8; 16];
    let iv = [0u8; 16];
    let mut data = [0u8; 8];
    let err = decrypt_cenc(
        &key,
        &iv,
        Pattern::NONE,
        &mut data,
        &[Subsample {
            clear_bytes: 4,
            protected_bytes: 8,
        }],
    );
    assert!(err.is_err());
}

fn hex16(s: &str) -> [u8; 16] {
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
    }
    out
}
