//! Handshake unit tests, including **self-consistency** tests: this crate's own C1-building
//! and digest math used to construct a synthetic-but-compliant S1/S2 "server" response, then
//! fed back into `Handshake` to verify it accepts its own math. This is not a real 3-way
//! live-server test (out of scope, no test infra for that — see task framing), but it does
//! exercise the full digest/HMAC pipeline end to end against itself.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::{
    C1_S1_LEN, DIGEST_LEN, GENUINE_FMS_KEY, GENUINE_FP_KEY, Handshake, build_c1, compute_digest,
    digest_offset_digest_first, digest_offset_key_first, fill_padding, hmac_sha256,
};
use crate::error::Error;

#[test]
fn digest_offset_formulas_stay_in_bounds() {
    let mut block = [0u8; C1_S1_LEN];
    fill_padding(&mut block);
    let digest_first = digest_offset_digest_first(&block);
    let key_first = digest_offset_key_first(&block);
    assert!(digest_first + DIGEST_LEN <= C1_S1_LEN);
    assert!(key_first + DIGEST_LEN <= C1_S1_LEN);
    // digest-first digest sits within the first 764-byte block; key-first within the second.
    assert!(digest_first < 764);
    assert!(key_first >= 764);
}

#[test]
fn build_c1_embeds_a_digest_that_validates_against_its_own_formula() {
    let c1 = build_c1();
    let pos = digest_offset_digest_first(&c1);
    let expected = compute_digest(&c1, pos, &GENUINE_FP_KEY[..30]).unwrap();
    assert_eq!(&c1[pos..pos + DIGEST_LEN], &expected[..]);
}

/// Build a synthetic, compliant S1 in the given layout (digest-first or key-first),
/// mirroring `build_c1`'s own approach but signed with `GenuineFMSKey[..36]` as a real
/// server would.
fn build_fake_s1(digest_first_layout: bool) -> [u8; C1_S1_LEN] {
    let mut s1 = [0u8; C1_S1_LEN];
    fill_padding(&mut s1);
    let pos = if digest_first_layout {
        digest_offset_digest_first(&s1)
    } else {
        digest_offset_key_first(&s1)
    };
    let digest = compute_digest(&s1, pos, &GENUINE_FMS_KEY[..36]).unwrap();
    s1[pos..pos + DIGEST_LEN].copy_from_slice(&digest);
    s1
}

#[test]
fn locate_s1_digest_finds_digest_first_layout() {
    let s1 = build_fake_s1(true);
    let pos = digest_offset_digest_first(&s1);
    let found = super::locate_s1_digest(&s1).unwrap();
    assert_eq!(found, s1[pos..pos + DIGEST_LEN]);
}

#[test]
fn locate_s1_digest_finds_key_first_layout() {
    let s1 = build_fake_s1(false);
    let pos = digest_offset_key_first(&s1);
    let found = super::locate_s1_digest(&s1).unwrap();
    assert_eq!(found, s1[pos..pos + DIGEST_LEN]);
}

#[test]
fn locate_s1_digest_fails_on_garbage() {
    let s1 = [0u8; C1_S1_LEN]; // all-zero: won't validate under either layout's HMAC
    let err = super::locate_s1_digest(&s1).unwrap_err();
    assert!(matches!(err, Error::S1DigestNotFound));
}

/// Build a synthetic, compliant S2 for the given C1's digest, mirroring the real
/// server-side derivation: `temp_key = HMAC(c1_digest, GenuineFMSKey_full)`, then
/// `digest = HMAC(random_1504, temp_key)`.
fn build_fake_s2(c1_digest: &[u8; DIGEST_LEN]) -> [u8; C1_S1_LEN] {
    let temp_key = hmac_sha256(&GENUINE_FMS_KEY, c1_digest).unwrap();
    let mut random = [0u8; C1_S1_LEN - DIGEST_LEN];
    fill_padding(&mut random);
    let digest = hmac_sha256(&temp_key, &random).unwrap();
    let mut s2 = [0u8; C1_S1_LEN];
    s2[..C1_S1_LEN - DIGEST_LEN].copy_from_slice(&random);
    s2[C1_S1_LEN - DIGEST_LEN..].copy_from_slice(&digest);
    s2
}

#[test]
fn new_handshake_queues_c0_c1_immediately() {
    let hs = Handshake::new();
    let pending = hs.pending_send();
    assert_eq!(pending.len(), 1 + C1_S1_LEN);
    assert_eq!(pending[0], 0x03);
    assert!(!hs.is_complete());
}

#[test]
fn full_handshake_self_consistency_round_trip() {
    let mut hs = Handshake::new();

    // "Send" C0+C1: extract what the client produced, then advance past it.
    let c0c1 = hs.pending_send().to_vec();
    assert_eq!(c0c1.len(), 1 + C1_S1_LEN);
    let c1 = &c0c1[1..];
    hs.advance_send(c0c1.len());
    assert!(hs.pending_send().is_empty());

    // A compliant synthetic server locates C1's digest using this crate's own (verified)
    // digest-first formula, since `build_c1` always uses that layout.
    let c1_digest_pos = digest_offset_digest_first(c1);
    let mut c1_digest = [0u8; DIGEST_LEN];
    c1_digest.copy_from_slice(&c1[c1_digest_pos..c1_digest_pos + DIGEST_LEN]);

    let s1 = build_fake_s1(true);
    let s2 = build_fake_s2(&c1_digest);

    let mut server_response = vec![0x03]; // S0
    server_response.extend_from_slice(&s1);
    server_response.extend_from_slice(&s2);

    hs.feed_recv_bytes(&server_response).unwrap();

    // C2 should now be queued.
    let c2 = hs.pending_send().to_vec();
    assert_eq!(c2.len(), C1_S1_LEN);
    assert!(!hs.is_complete()); // C2 not yet "sent" (advance_send not called)

    // Verify C2's digest independently: temp_key = HMAC(s1_digest, GenuineFPKey_full), then
    // digest = HMAC(c2_random, temp_key).
    let s1_digest_pos = digest_offset_digest_first(&s1);
    let mut s1_digest = [0u8; DIGEST_LEN];
    s1_digest.copy_from_slice(&s1[s1_digest_pos..s1_digest_pos + DIGEST_LEN]);
    let temp_key = hmac_sha256(&GENUINE_FP_KEY, &s1_digest).unwrap();
    let expected_digest = hmac_sha256(&temp_key, &c2[..C1_S1_LEN - DIGEST_LEN]).unwrap();
    assert_eq!(&c2[C1_S1_LEN - DIGEST_LEN..], &expected_digest[..]);

    hs.advance_send(c2.len());
    assert!(hs.is_complete());
}

#[test]
fn unexpected_s0_version_errors() {
    let mut hs = Handshake::new();
    hs.advance_send(hs.pending_send().len());
    let mut bogus = vec![0x06]; // RTMPE version byte, out of scope
    bogus.extend_from_slice(&[0u8; C1_S1_LEN * 2]);
    let err = hs.feed_recv_bytes(&bogus).unwrap_err();
    assert!(matches!(err, Error::UnexpectedS0Version(0x06)));
}

#[test]
fn incomplete_response_does_not_complete() {
    let mut hs = Handshake::new();
    hs.advance_send(hs.pending_send().len());
    hs.feed_recv_bytes(&[0x03]).unwrap(); // only S0, far short of S0+S1+S2
    assert!(!hs.is_complete());
    assert!(hs.pending_send().is_empty());
}
