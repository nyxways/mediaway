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
fn ensure_stages_generation_outside_the_returned_path() -> Result<(), TestMediaError> {
    let bytes = b"staged";
    let digest = hash_bytes(bytes);
    let mut generated_into = PathBuf::new();
    let path = ensure("staging/probe.bin", &digest, |p| {
        generated_into = p.to_path_buf();
        fs::write(p, bytes)?;
        Ok(())
    })?;

    // The generator must never write the final path directly: an in-place write
    // is what let concurrent `nextest` processes clobber each other.
    assert_ne!(generated_into, path);
    assert!(
        !generated_into.exists(),
        "staging file was not renamed away"
    );
    verify_hash(&path, &digest)?;
    Ok(())
}

#[test]
fn ensure_is_safe_for_concurrent_callers_of_one_fixture() -> Result<(), TestMediaError> {
    let bytes = b"contended";
    let digest = hash_bytes(bytes);
    let cold = cache_dir()?.join("contended").join("probe.bin");
    drop(fs::remove_file(&cold)); // force a cold cache; may already be absent

    let results = std::thread::scope(|scope| {
        // The collect is the point: every worker must be spawned before the
        // first join, or they run one at a time and never contend.
        #[allow(
            clippy::needless_collect,
            reason = "collecting is what makes the workers overlap"
        )]
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    ensure("contended/probe.bin", &digest, |p| {
                        fs::write(p, bytes)?;
                        Ok(())
                    })
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("worker thread panicked"))
            .collect::<Vec<_>>()
    });

    for result in results {
        assert_eq!(result?, cold);
    }
    verify_hash(&cold, &digest)?;

    // No staging files survive a contended run.
    let leftovers = fs::read_dir(cold.parent().expect("fixture has a parent"))?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
        .count();
    assert_eq!(leftovers, 0, "staging files left behind");
    Ok(())
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
