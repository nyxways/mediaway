//! Pure unit tests for [`super::PocState::compute`] — hand-built SPS/slice-header
//! fixtures, no D3D12/hardware involved (see ADR-0002's sibling Vulkan ADR: "the
//! highest-value real test coverage before any hardware is involved").

use super::PocState;
use crate::d3d12_video_decode::h264_slice::{SliceHeader, SliceType};
use crate::d3d12_video_decode::h264_sps_pps::Sps;

fn sps_type0(log2_max_pic_order_cnt_lsb: u32) -> Sps {
    Sps {
        pic_order_cnt_type: 0,
        log2_max_pic_order_cnt_lsb,
        ..Sps::default()
    }
}

fn sh_type0(pic_order_cnt_lsb: u32) -> SliceHeader {
    SliceHeader {
        slice_type: SliceType::P,
        pic_order_cnt_lsb,
        ..SliceHeader::default()
    }
}

#[test]
fn type0_idr_starts_at_zero() {
    let state = PocState::default();
    let sps = sps_type0(8);
    let sh = sh_type0(0);
    let (poc, _next) = state.compute(&sps, &sh, true, 1);
    assert_eq!(poc.pic_order_cnt, 0);
    assert_eq!(poc.top_field_order_cnt, 0);
}

#[test]
fn type0_advances_without_wrap() {
    let sps = sps_type0(8);
    let (poc0, state1) = PocState::default().compute(&sps, &sh_type0(0), true, 1);
    assert_eq!(poc0.pic_order_cnt, 0);

    let (poc1, state2) = state1.compute(&sps, &sh_type0(2), false, 1);
    assert_eq!(poc1.pic_order_cnt, 2);

    let (poc2, _state3) = state2.compute(&sps, &sh_type0(4), false, 1);
    assert_eq!(poc2.pic_order_cnt, 4);
}

#[test]
fn type0_wraps_msb_when_lsb_drops_by_more_than_half_max() {
    // MaxPicOrderCntLsb = 256 (log2 == 8). A prior reference picture already
    // established prevPicOrderCntLsb = 254 (built directly rather than via two
    // `compute` calls — jumping from lsb 0 straight to 254 in one call is itself a
    // large delta the § 8.2.1.1 formula would "correct" as a wrap, which is not what
    // this test is exercising). The *next* picture's lsb = 2 is the real wraparound
    // (a long-running stream cycling past `MaxPicOrderCntLsb`) and must bump
    // PicOrderCntMsb by +256.
    let sps = sps_type0(8);
    let state = PocState {
        prev_pic_order_cnt_msb: 0,
        prev_pic_order_cnt_lsb: 254,
        prev_frame_num: 0,
        prev_frame_num_offset: 0,
    };
    let (poc, _next) = state.compute(&sps, &sh_type0(2), false, 1);
    assert_eq!(poc.pic_order_cnt, 258);
}

#[test]
fn type0_non_reference_picture_does_not_update_state() {
    let sps = sps_type0(8);
    let state1 = PocState::default().compute(&sps, &sh_type0(0), true, 1).1;
    // A non-reference picture (nal_ref_idc == 0) must not move prev_pic_order_cnt_lsb.
    let state_after_nonref = state1.compute(&sps, &sh_type0(50), false, 0).1;
    assert_eq!(state_after_nonref, state1);
}

fn sps_type1(offsets: &[i32]) -> Sps {
    Sps {
        pic_order_cnt_type: 1,
        log2_max_frame_num: 4,
        offset_for_ref_frame: offsets.iter().copied().collect(),
        ..Sps::default()
    }
}

fn sh_type1(frame_num: u32, delta0: i32) -> SliceHeader {
    SliceHeader {
        slice_type: SliceType::P,
        frame_num,
        delta_pic_order_cnt: [delta0, 0],
        ..SliceHeader::default()
    }
}

#[test]
fn type1_zero_cycle_length_passes_through_delta() {
    let sps = sps_type1(&[]);
    let (poc, _next) = PocState::default().compute(&sps, &sh_type1(0, 0), true, 1);
    assert_eq!(poc.pic_order_cnt, 0);

    let (poc, _next) = PocState::default().compute(&sps, &sh_type1(1, 7), false, 1);
    assert_eq!(poc.pic_order_cnt, 7);
}

#[test]
fn type1_accumulates_offset_for_ref_frame_cycle() {
    let sps = sps_type1(&[4, 4, 4]);
    let state = PocState::default()
        .compute(&sps, &sh_type1(0, 0), true, 1)
        .1;
    // frame_num=1: FrameNumOffset=0, AbsFrameNum=1, cycle_cnt=0, in_cycle=0 ->
    // expectedPicOrderCnt = offset_for_ref_frame[0] = 4.
    let (poc, _next) = state.compute(&sps, &sh_type1(1, 0), false, 1);
    assert_eq!(poc.pic_order_cnt, 4);
}

fn sps_type2(log2_max_frame_num: u32) -> Sps {
    Sps {
        pic_order_cnt_type: 2,
        log2_max_frame_num,
        ..Sps::default()
    }
}

fn sh_type2(frame_num: u32) -> SliceHeader {
    SliceHeader {
        slice_type: SliceType::P,
        frame_num,
        ..SliceHeader::default()
    }
}

#[test]
fn type2_idr_is_zero_then_doubles_frame_num() {
    let sps = sps_type2(4);
    let (poc0, state1) = PocState::default().compute(&sps, &sh_type2(0), true, 1);
    assert_eq!(poc0.pic_order_cnt, 0);

    let (poc1, _state2) = state1.compute(&sps, &sh_type2(1), false, 1);
    assert_eq!(poc1.pic_order_cnt, 2);
}

#[test]
fn type2_wraps_frame_num_offset() {
    // log2_max_frame_num == 2 -> MaxFrameNum == 4.
    let sps = sps_type2(2);
    let state = PocState::default().compute(&sps, &sh_type2(2), true, 1).1;
    // prev_frame_num (2) > frame_num (0) -> FrameNumOffset = 0 + 4 = 4.
    let (poc, _next) = state.compute(&sps, &sh_type2(0), false, 1);
    assert_eq!(poc.pic_order_cnt, 8);
}

#[test]
fn type2_non_reference_picture_is_odd() {
    let sps = sps_type2(4);
    let state = PocState::default().compute(&sps, &sh_type2(0), true, 1).1;
    let (poc, _next) = state.compute(&sps, &sh_type2(1), false, 0);
    assert_eq!(poc.pic_order_cnt, 1);
}
