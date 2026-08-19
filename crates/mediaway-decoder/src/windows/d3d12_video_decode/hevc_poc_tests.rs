//! Pure unit tests for [`super::PocState::compute`] (ITU-T H.265 § 8.3.1) — no bitstream
//! parsing needed, [`super::PocState`] operates directly on already-parsed values.

use super::PocState;
use crate::windows::d3d12_video_decode::hevc_vps_sps_pps::Sps;

fn test_sps(log2_max_pic_order_cnt_lsb: u32) -> Sps {
    Sps {
        pic_width_in_luma_samples: 352,
        pic_height_in_luma_samples: 288,
        log2_max_pic_order_cnt_lsb,
        max_dec_pic_buffering: 4,
        log2_min_cb_size: 6,
        log2_diff_max_min_cb_size: 2,
        log2_min_tb_size: 2,
        log2_diff_max_min_tb_size: 3,
        max_transform_hierarchy_depth_inter: 2,
        max_transform_hierarchy_depth_intra: 1,
        amp_enabled_flag: true,
        sample_adaptive_offset_enabled_flag: false,
        sps_temporal_mvp_enabled_flag: false,
        strong_intra_smoothing_enabled_flag: true,
    }
}

#[test]
fn idr_resets_poc_to_zero_and_updates_state_when_reference() {
    let sps = test_sps(8);
    let state = PocState::default();
    let (poc, next) = state.compute(&sps, None, true, true);
    assert_eq!(poc, 0);
    // A following non-IDR picture with `pic_order_cnt_lsb == 0` must compute POC == 0
    // relative to this reset state (msb == 0, lsb == 0).
    let (poc2, _) = next.compute(&sps, Some(0), false, true);
    assert_eq!(poc2, 0);
}

#[test]
fn idr_as_non_reference_does_not_update_state() {
    let sps = test_sps(8);
    let mut state = PocState::default();
    // Seed state as if a prior reference picture had POC 100 (msb=96, lsb=4 for
    // log2_max_pic_order_cnt_lsb == 8, MaxPicOrderCntLsb == 256 -- doesn't wrap here,
    // simplest to just craft prev state directly via a real reference IDR first).
    let (_, seeded) = state.compute(&sps, None, true, true);
    let (_, seeded) = seeded.compute(&sps, Some(4), false, true);
    state = seeded;

    // A non-reference IDR (rare in practice, but legal) must not clobber `state`.
    let (poc, unchanged) = state.compute(&sps, None, true, false);
    assert_eq!(poc, 0); // this picture's own POC is still 0 (IDR rule)
    assert_eq!(unchanged, state); // but the persisted state for the *next* picture is untouched
}

#[test]
fn non_idr_poc_matches_lsb_directly_with_no_wrap() {
    let sps = test_sps(8); // MaxPicOrderCntLsb == 256
    let state = PocState::default(); // prev_lsb == 0, prev_msb == 0
    let (poc, _) = state.compute(&sps, Some(5), false, true);
    assert_eq!(poc, 5);
}

#[test]
fn poc_msb_wraps_forward_when_lsb_decreases_past_half_range() {
    let sps = test_sps(8); // MaxPicOrderCntLsb == 256, half == 128
    let state = PocState::default();
    // Reach prev_lsb == 200 / prev_msb == 0 via two gentle +100 steps (each single-step
    // delta stays <= half, so neither step itself triggers a wrap) rather than one big
    // jump straight from lsb == 0, which would itself wrap backward first.
    let (_, state) = state.compute(&sps, Some(100), false, true);
    let (_, state) = state.compute(&sps, Some(200), false, true);
    // Now lsb drops to 10: (200 - 10) == 190 >= 128 -> MSB should advance by +256.
    let (poc, _) = state.compute(&sps, Some(10), false, true);
    assert_eq!(poc, 256 + 10);
}

#[test]
fn poc_msb_wraps_backward_when_lsb_increases_past_half_range() {
    let sps = test_sps(8); // MaxPicOrderCntLsb == 256, half == 128
    let state = PocState::default();
    let (_, state) = state.compute(&sps, Some(10), false, true);
    // lsb jumps to 250: (250 - 10) == 240 > 128 -> MSB should regress by -256.
    let (poc, _) = state.compute(&sps, Some(250), false, true);
    assert_eq!(poc, -256 + 250);
}

#[test]
fn non_reference_picture_does_not_update_persisted_state() {
    let sps = test_sps(8);
    let state = PocState::default();
    let (_, after_ref) = state.compute(&sps, Some(5), false, true);
    // A non-reference picture computes its own POC normally...
    let (poc_non_ref, unchanged) = after_ref.compute(&sps, Some(50), false, false);
    assert_eq!(poc_non_ref, 50);
    // ...but must not become the new `prevTid0Pic` state for the picture after it.
    assert_eq!(unchanged, after_ref);
    let (poc_next, _) = unchanged.compute(&sps, Some(6), false, true);
    // Computed relative to `after_ref` (lsb == 5), not the non-reference picture
    // (lsb == 50) that was skipped over.
    assert_eq!(poc_next, 6);
}
