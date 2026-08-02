#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

#[test]
fn solid_red_digest_constant_matches_bytes() {
    let bytes = solid_rgba8_bytes(64, 64, [255, 0, 0, 255]);
    assert_eq!(hash_bytes(&bytes), SOLID_RED_64X64_BLAKE3);
}

#[test]
fn solid_red_is_cached_and_hash_checked() -> Result<(), TestMediaError> {
    let path = ensure_solid_red_64x64()?;
    assert!(path.is_file());
    verify_hash(&path, SOLID_RED_64X64_BLAKE3)?;

    fs::write(&path, b"not-a-fixture")?;
    let again = ensure_solid_red_64x64()?;
    assert_eq!(path, again);
    verify_hash(&path, SOLID_RED_64X64_BLAKE3)?;
    Ok(())
}

#[test]
fn ensure_fails_when_generator_drifts() {
    let err = ensure("drift/probe.bin", SOLID_RED_64X64_BLAKE3, |path| {
        fs::write(path, b"wrong")?;
        Ok(())
    });
    assert!(matches!(err, Err(TestMediaError::HashMismatch { .. })));
}

#[test]
fn solid_nv12_digest_constant_matches_bytes() {
    let bytes = solid_nv12_bytes(64, 64, 128, 128, 128);
    assert_eq!(bytes.len(), 64 * 64 + 64 * 64 / 2);
    assert_eq!(hash_bytes(&bytes), SOLID_GRAY_NV12_64X64_BLAKE3);
}

#[test]
fn solid_nv12_is_cached_and_hash_checked() -> Result<(), TestMediaError> {
    let path = ensure_solid_gray_nv12_64x64()?;
    assert!(path.is_file());
    verify_hash(&path, SOLID_GRAY_NV12_64X64_BLAKE3)?;

    fs::write(&path, b"not-a-fixture")?;
    let again = ensure_solid_gray_nv12_64x64()?;
    assert_eq!(path, again);
    verify_hash(&path, SOLID_GRAY_NV12_64X64_BLAKE3)?;
    Ok(())
}

#[test]
fn pcm_silence_digest_constant_matches_bytes() {
    let bytes = pcm_silence_bytes(960, 2);
    assert_eq!(bytes.len(), 960 * 2 * 2);
    assert!(bytes.iter().all(|&b| b == 0));
    assert_eq!(hash_bytes(&bytes), PCM_SILENCE_48K_STEREO_20MS_BLAKE3);
}

#[test]
fn pcm_silence_is_cached_and_hash_checked() -> Result<(), TestMediaError> {
    let path = ensure_pcm_silence_48k_stereo_20ms()?;
    assert!(path.is_file());
    verify_hash(&path, PCM_SILENCE_48K_STEREO_20MS_BLAKE3)?;

    fs::write(&path, b"not-a-fixture")?;
    let again = ensure_pcm_silence_48k_stereo_20ms()?;
    assert_eq!(path, again);
    verify_hash(&path, PCM_SILENCE_48K_STEREO_20MS_BLAKE3)?;
    Ok(())
}
