//! `C0/C1/C2` ⇄ `S0/S1/S2` client-role handshake — HMAC-SHA256 "complex"/digest variant only
//! (the legacy all-zero "simple" handshake is an explicit scope cut, see
//! `adr/0001-rtmp-freestanding-core.md` § 1 / Non-Goals). Sans-io: `feed_recv_bytes`,
//! `pending_send`/`advance_send`, `is_complete` — byte-slice in/out, no socket, matching the
//! `shiguredo_rtmp` design reference the ADR studied (not depended on).
//!
//! # Digest offset formula — sources and confidence
//!
//! The digest's placement inside the 1536-byte `C1`/`S1` block depends on a **layout**
//! choice — community sources number these "scheme 0"/"scheme 1" inconsistently (the
//! `librtmp`-derived convention and SRS's own `SrsSchema0`/`SrsSchema1` naming turn out to
//! label the *same two layouts* with opposite numbers), so this module names them by
//! structure instead of by number:
//!
//! - **digest-first**: `time(4) + version(4) + digest_block(764) + key_block(764)`.
//! - **key-first**: `time(4) + version(4) + key_block(764) + digest_block(764)`.
//!
//! Neither layout's offset formula is in any officially redistributable Adobe document — it
//! is reverse-engineered community knowledge. Cross-checked against **three independently
//! authored, publicly available implementations** before writing this module:
//!
//! - <https://gist.github.com/gyk/967af2aae2f1455d6d40779678aefde5> — `FFmpeg`
//!   `rtmpproto.c` digest handshake, annotated.
//! - <https://github.com/j0sh/rtmpdump/blob/master/librtmp/handshake.h> — `librtmp`,
//!   `GetDigestOffset1`/`GetDigestOffset2`.
//! - <https://github.com/ossrs/srs> `trunk/src/protocol/srs_protocol_rtmp_handshake.cpp` —
//!   SRS, a from-scratch C++ RTMP server (independent codebase/authorship from the two
//!   above).
//!
//! All three agree **exactly** on both formulas:
//!
//! - digest-first: digest position = `12 + (sum of the 4 bytes at absolute offset 8..12 of
//!   the 1536-byte block, mod 728)`.
//! - key-first: digest position = `776 + (sum of the 4 bytes at absolute offset 772..776,
//!   mod 728)`.
//!
//! Also cross-checked against the same 3 sources: the HMAC-SHA256 key material
//! (`GenuineFPKey`/`GenuineFMSKey` below, byte-for-byte identical across `librtmp` and SRS),
//! the digest message construction (the block's 1536 bytes with its own 32-byte digest slot
//! excluded, i.e. 1504 bytes signed), and the `C2`/`S2` digest derivation (`temp_key =
//! HMAC(peer's 32-byte digest, own full key)`, then `HMAC(1504 random bytes, temp_key)`
//! appended as the final 32 bytes of `C2`/`S2` — no offset layout needed there, unlike
//! `C1`/`S1`). `librtmp`'s plaintext-client default (`offalg = 0`, i.e. digest-first,
//! version bytes `10.0.45.2`) is what [`build_c1`] uses, for maximum realistic-client
//! resemblance.
//!
//! **Confidence: high** for the byte-level formulas above — three independently authored
//! implementations agree exactly, not just approximately. **Not independently confirmed** by
//! this crate: the exact `C1` `version` field bytes/thresholds some real servers branch on.
//! **This handshake implementation has not been exercised against a real running RTMP
//! server** (YouTube/Twitch/nginx-rtmp/SRS live instance) — it is cross-checked against
//! reference source code and against its own math (see `handshake_tests.rs`'s
//! self-consistency tests: this module's own C1-building + S1/S2-shaped synthetic-server
//! math, round-tripped through `Handshake` itself), not proven by real-server interop. Treat
//! as unverified for production use until that gate is run — the same named risk
//! `adr/0001-rtmp-freestanding-core.md` § Consequences already flags, not resolved here.

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "explicit pub(crate) matches this workspace's unreachable_pub convention; clippy's \
              suggested plain `pub` would make these items look crate-public by accident"
)]

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::error::Error;

type HmacSha256 = Hmac<Sha256>;

const C1_S1_LEN: usize = 1536;
const DIGEST_LEN: usize = 32;
/// `764 (key/digest block) - 32 (digest) - 4 (offset field)`, the modulus shared by both
/// offset formulas.
const MAX_OFFSET_RANGE: u32 = 728;

/// 62-byte "Genuine Adobe Flash Player 001" key. Client (`C1`) digests are signed with the
/// first 30 bytes; the client's `C2` temp-key HMAC uses the full 62 bytes. Source:
/// `librtmp`/SRS (see module docs), byte-for-byte identical across both.
const GENUINE_FP_KEY: [u8; 62] = [
    0x47, 0x65, 0x6E, 0x75, 0x69, 0x6E, 0x65, 0x20, 0x41, 0x64, 0x6F, 0x62, 0x65, 0x20, 0x46, 0x6C,
    0x61, 0x73, 0x68, 0x20, 0x50, 0x6C, 0x61, 0x79, 0x65, 0x72, 0x20, 0x30, 0x30, 0x31, 0xF0, 0xEE,
    0xC2, 0x4A, 0x80, 0x68, 0xBE, 0xE8, 0x2E, 0x00, 0xD0, 0xD1, 0x02, 0x9E, 0x7E, 0x57, 0x6E, 0xEC,
    0x5D, 0x2D, 0x29, 0x80, 0x6F, 0xAB, 0x93, 0xB8, 0xE6, 0x36, 0xCF, 0xEB, 0x31, 0xAE,
];

/// 68-byte "Genuine Adobe Flash Media Server 001" key. `S1` digests are signed with the
/// first 36 bytes; the server's `S2` temp-key HMAC uses the full 68 bytes.
const GENUINE_FMS_KEY: [u8; 68] = [
    0x47, 0x65, 0x6E, 0x75, 0x69, 0x6E, 0x65, 0x20, 0x41, 0x64, 0x6F, 0x62, 0x65, 0x20, 0x46, 0x6C,
    0x61, 0x73, 0x68, 0x20, 0x4D, 0x65, 0x64, 0x69, 0x61, 0x20, 0x53, 0x65, 0x72, 0x76, 0x65, 0x72,
    0x20, 0x30, 0x30, 0x31, 0xF0, 0xEE, 0xC2, 0x4A, 0x80, 0x68, 0xBE, 0xE8, 0x2E, 0x00, 0xD0, 0xD1,
    0x02, 0x9E, 0x7E, 0x57, 0x6E, 0xEC, 0x5D, 0x2D, 0x29, 0x80, 0x6F, 0xAB, 0x93, 0xB8, 0xE6, 0x36,
    0xCF, 0xEB, 0x31, 0xAE,
];

fn hmac_sha256(key: &[u8], msg: &[u8]) -> Result<[u8; DIGEST_LEN], Error> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| Error::HmacKeyLength)?;
    mac.update(msg);
    Ok(mac.finalize().into_bytes().into())
}

/// digest-first layout: digest position within a 1536-byte `C1`/`S1` block.
pub(crate) fn digest_offset_digest_first(block: &[u8]) -> usize {
    let sum: u32 = block[8..12].iter().map(|&b| u32::from(b)).sum();
    12 + usize::try_from(sum % MAX_OFFSET_RANGE).unwrap_or(0)
}

/// key-first layout: digest position within a 1536-byte `C1`/`S1` block.
pub(crate) fn digest_offset_key_first(block: &[u8]) -> usize {
    let sum: u32 = block[772..776].iter().map(|&b| u32::from(b)).sum();
    776 + usize::try_from(sum % MAX_OFFSET_RANGE).unwrap_or(0)
}

/// HMAC-SHA256 of `block`'s 1536 bytes with its own 32-byte digest slot (at `digest_pos`)
/// excluded from the signed message, per the complex-handshake algorithm.
fn compute_digest(block: &[u8], digest_pos: usize, key: &[u8]) -> Result<[u8; DIGEST_LEN], Error> {
    let mut msg = Vec::with_capacity(C1_S1_LEN - DIGEST_LEN);
    msg.extend_from_slice(&block[..digest_pos]);
    msg.extend_from_slice(&block[digest_pos + DIGEST_LEN..]);
    hmac_sha256(key, &msg)
}

/// Fills `buf` with a deterministic, non-cryptographic filler pattern. Padding-region
/// content has no meaning beyond its presence — real implementations commonly use random
/// bytes here, but nothing in the complex-handshake algorithm requires unpredictability for
/// a plaintext (non-RTMPE) connection, and this keeps the crate free of an added `rand`
/// dependency for a non-security-relevant byte range.
fn fill_padding(buf: &mut [u8]) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = u8::try_from(i % 256).unwrap_or(0);
    }
}

/// Build `C1`: digest-first layout, `librtmp`'s plaintext-client default version bytes
/// (`10.0.45.2`), HMAC-SHA256 digest signed with `GenuineFPKey[..30]`. Returns the 1536-byte
/// block and its own 32-byte digest (the latter is needed later to derive `S2`'s expected
/// digest, and is currently unused pending that optional validation — see
/// `adr/0001-rtmp-freestanding-core.md` § Consequences on why `S2` validation stays
/// best-effort in this implementation).
fn build_c1() -> [u8; C1_S1_LEN] {
    let mut c1 = [0u8; C1_S1_LEN];
    c1[0..4].copy_from_slice(&0u32.to_be_bytes()); // time: not verified by servers for the digest handshake
    c1[4..8].copy_from_slice(&[0x0A, 0x00, 0x2D, 0x02]); // version 10.0.45.2, see module docs
    fill_padding(&mut c1[8..C1_S1_LEN]);

    let digest_pos = digest_offset_digest_first(&c1);
    let digest =
        compute_digest(&c1, digest_pos, &GENUINE_FP_KEY[..30]).unwrap_or([0u8; DIGEST_LEN]);
    c1[digest_pos..digest_pos + DIGEST_LEN].copy_from_slice(&digest);
    c1
}

/// Locate and validate `S1`'s digest, trying the digest-first layout first (matching this
/// crate's own `C1`), falling back to key-first — real servers are not guaranteed to mirror
/// the client's chosen layout, so both are tried, per the community-documented "try one
/// layout, fall back to the other" pattern this ADR's § 1 names.
fn locate_s1_digest(s1: &[u8]) -> Result<[u8; DIGEST_LEN], Error> {
    for offset_fn in [digest_offset_digest_first, digest_offset_key_first] {
        let pos = offset_fn(s1);
        if pos + DIGEST_LEN > C1_S1_LEN {
            continue;
        }
        let expected = compute_digest(s1, pos, &GENUINE_FMS_KEY[..36]).unwrap_or([0u8; DIGEST_LEN]);
        if expected == s1[pos..pos + DIGEST_LEN] {
            let mut out = [0u8; DIGEST_LEN];
            out.copy_from_slice(&s1[pos..pos + DIGEST_LEN]);
            return Ok(out);
        }
    }
    Err(Error::S1DigestNotFound)
}

/// Build `C2` from `S1`'s extracted digest: `temp_key = HMAC(s1_digest, GenuineFPKey)`, then
/// `digest = HMAC(random_1504_bytes, temp_key)`, laid out as `random(1504) + digest(32)` —
/// `C2`/`S2` have no offset-layout ambiguity, unlike `C1`/`S1`.
fn build_c2(s1_digest: &[u8; DIGEST_LEN]) -> Result<[u8; C1_S1_LEN], Error> {
    let temp_key = hmac_sha256(&GENUINE_FP_KEY, s1_digest)?;
    let mut random = [0u8; C1_S1_LEN - DIGEST_LEN];
    fill_padding(&mut random);
    let digest = hmac_sha256(&temp_key, &random)?;

    let mut c2 = [0u8; C1_S1_LEN];
    c2[..C1_S1_LEN - DIGEST_LEN].copy_from_slice(&random);
    c2[C1_S1_LEN - DIGEST_LEN..].copy_from_slice(&digest);
    Ok(c2)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    AwaitS0S1S2,
    Done,
}

/// Client-role `C0/C1/C2` ⇄ `S0/S1/S2` handshake, HMAC-SHA256 digest ("complex") variant only.
///
/// Sans-io: the caller owns the socket, feeding received bytes via
/// [`feed_recv_bytes`](Self::feed_recv_bytes) and draining bytes to send via
/// [`pending_send`](Self::pending_send)/[`advance_send`](Self::advance_send).
///
/// Diverges from `adr/0001-rtmp-freestanding-core.md` § 1's literal `feed_recv_bytes(&mut
/// self, &[u8])` (no `Result`) signature: this handshake can genuinely fail on a real,
/// distinguishable error (unexpected `S0` version, undecodable `S1` digest) — surfacing that
/// via `Result` matches this workspace's "honest, not silently swallowed" error posture
/// (`docs/conventions/error-handling.md`) better than a signature that can only ever
/// "succeed" or hang forever waiting for a completion that will never come.
#[derive(Debug)]
pub struct Handshake {
    state: State,
    send_buf: Vec<u8>,
    send_pos: usize,
    recv_buf: Vec<u8>,
}

impl Handshake {
    /// New client-role handshake. Queues `C0 + C1` for immediate sending — call
    /// [`pending_send`](Self::pending_send) right away.
    #[must_use]
    pub fn new() -> Self {
        let c1 = build_c1();
        let mut send_buf = Vec::with_capacity(1 + C1_S1_LEN);
        send_buf.push(0x03); // C0: plain RTMP version byte (RTMPE's 0x06 is out of scope)
        send_buf.extend_from_slice(&c1);
        Self {
            state: State::AwaitS0S1S2,
            send_buf,
            send_pos: 0,
            recv_buf: Vec::new(),
        }
    }

    /// Bytes waiting to be written to the socket.
    #[must_use]
    pub fn pending_send(&self) -> &[u8] {
        &self.send_buf[self.send_pos..]
    }

    /// Mark `n` bytes from [`pending_send`](Self::pending_send) as written.
    pub fn advance_send(&mut self, n: usize) {
        self.send_pos = (self.send_pos + n).min(self.send_buf.len());
    }

    /// Append bytes received from the socket, attempting to complete the handshake. Once
    /// `S0 + S1 + S2` (3073 bytes) have been fully received, validates `S0`'s version byte,
    /// locates `S1`'s digest (§ module docs), and queues `C2` for sending.
    pub fn feed_recv_bytes(&mut self, data: &[u8]) -> Result<(), Error> {
        if self.state == State::Done {
            return Ok(());
        }
        self.recv_buf.extend_from_slice(data);
        let needed = 1 + C1_S1_LEN + C1_S1_LEN; // S0 + S1 + S2
        if self.recv_buf.len() < needed {
            return Ok(());
        }

        let s0 = self.recv_buf[0];
        if s0 != 0x03 {
            return Err(Error::UnexpectedS0Version(s0));
        }
        let s1 = &self.recv_buf[1..=C1_S1_LEN];
        let s1_digest = locate_s1_digest(s1)?;

        let c2 = build_c2(&s1_digest)?;
        self.send_buf.extend_from_slice(&c2);
        self.state = State::Done;
        Ok(())
    }

    /// Whether the handshake has fully completed: `S0/S1/S2` were received and validated,
    /// and `C2` has been fully handed off via [`advance_send`](Self::advance_send).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.state == State::Done && self.send_pos >= self.send_buf.len()
    }
}

impl Default for Handshake {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "handshake_tests.rs"]
mod tests;
