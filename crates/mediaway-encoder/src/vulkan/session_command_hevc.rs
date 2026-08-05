//! HEVC sibling of [`super::session_command`]'s H.264 command recording —
//! `record_upload_and_barriers`/`submit_and_readback` are codec-agnostic and
//! stay shared there; only `vkCmdBeginVideoCodingKHR`/`vkCmdEncodeVideoKHR`/
//! `vkCmdEndVideoCodingKHR`'s picture-info `pNext` payload differs per codec
//! (same C-union reasoning as `mediaway-encoder-windows`'s D3D12 HEVC
//! backend's `ops_hevc.rs`).
//!
//! ADR-0002 adds real multi-slot DPB + P-frame reference cycling here too
//! (`DpbRecordParamsHevc`, mirroring
//! [`super::session_command::DpbRecordParams`]) — CBR rate control stays
//! H.264-only this pass (ADR-0002's scope), so `record_video_coding_hevc`
//! keeps the `RATE_CONTROL_MODE_DISABLED` fixed-QP shape unconditionally,
//! unlike its H.264 sibling.

#![allow(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "Vulkan FFI: every count/size here is driver-reported and small \
              (single digits to low thousands — queue families, DPB slots, \
              memory requirement counts, one coded picture's byte size); casts \
              mirror ash's own generated builder code (e.g. `.len() as _`)."
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "workspace `unreachable_pub` policy (Cargo.toml) wants `pub(crate)` here; \
              clippy::pedantic's redundant_pub_crate disagrees for private modules — the \
              two lints are mutually exclusive for this shape, workspace policy wins"
)]

use vulkanalia::vk;
use vulkanalia::vk::video as native;
use vulkanalia::vk::{
    DeviceV1_0, HasBuilder, KhrVideoEncodeQueueExtensionDeviceCommands,
    KhrVideoQueueExtensionDeviceCommands,
};

use crate::vulkan::hevc_params;
use crate::vulkan::session::{EncodeDevice, SessionResources, VulkanEncodeSessionError};
use crate::vulkan::session_command::{record_upload_and_barriers, submit_and_readback};

/// DPB slot wiring for one `vkCmdEncodeVideoKHR` HEVC call (ADR-0002) —
/// mirrors [`super::session_command::DpbRecordParams`]; see that type's doc
/// for each field's meaning (identical shape, `StdVideoEncodeH265*` in place
/// of `StdVideoEncodeH264*`).
/// Unlike [`super::session_command::DpbRecordParams`] (whose
/// `idr_only()` constructor still has a live caller in
/// `session_encode::encode_synthetic_intra_frame`, H.264's Stage 1 one-shot
/// diagnostic), this crate has no HEVC equivalent of that diagnostic —
/// `VulkanVideoEncoder::push_frame` is the only HEVC caller and it always
/// builds every field from `self`'s own GOP/DPB state (`gop_size == 1`
/// naturally reproduces Stage 1's IDR-only values through that same path —
/// see `encoder.rs`'s HEVC branch), so this type has no `idr_only()`
/// constructor to avoid an unused (`dead_code`) one.
pub(crate) struct DpbRecordParamsHevc {
    pub(crate) layer_count: u32,
    pub(crate) transition: bool,
    pub(crate) setup_slot: i32,
    pub(crate) setup_reference_info: Option<native::StdVideoEncodeH265ReferenceInfo>,
    pub(crate) reference: Option<(i32, native::StdVideoEncodeH265ReferenceInfo)>,
}

/// Grouped params for [`record_and_submit_hevc`] — HEVC sibling of
/// [`super::session_command::RecordParams`].
pub(crate) struct RecordParamsHevc<'a> {
    pub(crate) command_buffer: vk::CommandBuffer,
    pub(crate) coded_extent: vk::Extent2D,
    pub(crate) dst_size: vk::DeviceSize,
    pub(crate) picture_info_pnext: &'a mut vk::VideoEncodeH265PictureInfoKHR,
    pub(crate) dpb: DpbRecordParamsHevc,
}

/// HEVC sibling of [`super::session_command::record_and_submit`] — shares
/// the upload/barrier and submit/readback halves, only the video-coding
/// scope's picture-info payload differs.
pub(crate) fn record_and_submit_hevc(
    encode_device: &EncodeDevice<'_>,
    resources: &mut SessionResources,
    params: &mut RecordParamsHevc<'_>,
) -> Result<Vec<u8>, VulkanEncodeSessionError> {
    let device = encode_device.device;
    let command_buffer = params.command_buffer;

    // SAFETY: same reasoning as `session_command::record_and_submit`'s reset
    // — `command_buffer` was allocated from a `RESET_COMMAND_BUFFER` pool.
    unsafe { device.reset_command_buffer(command_buffer, vk::CommandBufferResetFlags::empty()) }
        .map_err(|result| VulkanEncodeSessionError::VkCall {
            call: "vkResetCommandBuffer",
            result,
        })?;
    let begin_info =
        vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    // SAFETY: `command_buffer` was just reset above.
    unsafe { device.begin_command_buffer(command_buffer, &begin_info) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkBeginCommandBuffer",
            result,
        }
    })?;

    record_upload_and_barriers(
        device,
        resources,
        command_buffer,
        params.coded_extent,
        params.dst_size,
        params.dpb.layer_count,
        params.dpb.transition,
    );
    record_video_coding_hevc(encode_device, resources, params, command_buffer);

    // SAFETY: `command_buffer` was put into the recording state by
    // `vkBeginCommandBuffer` above and every command since has been valid.
    unsafe { device.end_command_buffer(command_buffer) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkEndCommandBuffer",
            result,
        }
    })?;

    submit_and_readback(encode_device, resources, command_buffer, params.dst_size)
}

/// `vkCmdBeginVideoCodingKHR` → `vkCmdEncodeVideoKHR` → `vkCmdEndVideoCodingKHR`
/// for one HEVC picture — mirrors
/// `session_command::record_video_coding`'s H.264 shape (ADR-0002's real
/// setup/reference slot pair, `VkVideoEncodeH265DpbSlotInfoKHR` chained so a
/// *future* frame can read this one back) with one difference: rate control
/// stays `DISABLED` fixed-QP unconditionally — CBR is H.264-only this pass
/// (see this file's module doc).
#[allow(
    clippy::too_many_lines,
    reason = "linear per-frame Vulkan Video struct construction — mirrors \
              session_command::record_video_coding's own too_many_lines allow \
              and reasoning (setup/reference slot resources, DPB-slot-info chains \
              must all stay on this function's own stack frame for the raw pointers \
              built from them to remain valid through the unsafe calls at the bottom)"
)]
fn record_video_coding_hevc(
    encode_device: &EncodeDevice<'_>,
    resources: &SessionResources,
    params: &mut RecordParamsHevc<'_>,
    command_buffer: vk::CommandBuffer,
) {
    let device = encode_device.device;

    // --- setup slot: the DPB slot this frame's picture is written into ---
    let setup_resource = vk::VideoPictureResourceInfoKHR::builder()
        .coded_offset(vk::Offset2D { x: 0, y: 0 })
        .coded_extent(params.coded_extent)
        .base_array_layer(params.dpb.setup_slot as u32)
        .image_view_binding(resources.dpb_image_view);
    let setup_reference_info = params
        .dpb
        .setup_reference_info
        .unwrap_or_else(|| hevc_params::build_reference_info(0, true));
    let mut setup_dpb_slot_info = vk::VideoEncodeH265DpbSlotInfoKHR::builder()
        .std_reference_info(&setup_reference_info)
        .build();
    let setup_slot = if params.dpb.setup_reference_info.is_some() {
        vk::VideoReferenceSlotInfoKHR::builder()
            .slot_index(params.dpb.setup_slot)
            .picture_resource(&setup_resource)
            .push_next(&mut setup_dpb_slot_info)
    } else {
        vk::VideoReferenceSlotInfoKHR::builder()
            .slot_index(params.dpb.setup_slot)
            .picture_resource(&setup_resource)
    };
    // `begin_slots`' own setup-slot entry stays exactly Stage 1's shape (no
    // `picture_resource`, no DPB-slot-info chain) — this slot's content is
    // being (re)initialized by this very operation, matching the
    // already-hardware-verified H.264 convention.
    let begin_setup_slot =
        vk::VideoReferenceSlotInfoKHR::builder().slot_index(params.dpb.setup_slot);

    // --- reference slot: the sole L0 reference a P-frame reads (GOP only) ---
    let has_reference = params.dpb.reference.is_some();
    let (reference_slot_index, reference_std_info) = params
        .dpb
        .reference
        .unwrap_or_else(|| (0, hevc_params::build_reference_info(0, true)));
    let reference_resource = vk::VideoPictureResourceInfoKHR::builder()
        .coded_offset(vk::Offset2D { x: 0, y: 0 })
        .coded_extent(params.coded_extent)
        .base_array_layer(reference_slot_index as u32)
        .image_view_binding(resources.dpb_image_view);
    // Two separate `StdVideoEncodeH265DpbSlotInfoKHR` locals (not one reused
    // for both arrays below): each `push_next` call takes an exclusive
    // borrow, and both the `begin_slots` and `encode_info.reference_slots`
    // arrays must stay alive simultaneously through the unsafe calls below —
    // same reasoning as `session_command::record_video_coding`'s H.264 twin.
    let mut begin_reference_dpb_slot_info = vk::VideoEncodeH265DpbSlotInfoKHR::builder()
        .std_reference_info(&reference_std_info)
        .build();
    let mut encode_reference_dpb_slot_info = vk::VideoEncodeH265DpbSlotInfoKHR::builder()
        .std_reference_info(&reference_std_info)
        .build();
    let begin_reference_slot = vk::VideoReferenceSlotInfoKHR::builder()
        .slot_index(reference_slot_index)
        .picture_resource(&reference_resource)
        .push_next(&mut begin_reference_dpb_slot_info);
    let encode_reference_slot = vk::VideoReferenceSlotInfoKHR::builder()
        .slot_index(reference_slot_index)
        .picture_resource(&reference_resource)
        .push_next(&mut encode_reference_dpb_slot_info);
    let begin_slots_array = [begin_setup_slot, begin_reference_slot];
    let begin_slots = if has_reference {
        &begin_slots_array[..]
    } else {
        &begin_slots_array[..1]
    };
    let encode_reference_slots_array = [encode_reference_slot];

    // SAFETY: `resources.encode_feedback_query_pool` has exactly 1 query
    // slot; resetting before use is required — a query pool's slots start in
    // an undefined state.
    unsafe {
        device.cmd_reset_query_pool(command_buffer, resources.encode_feedback_query_pool, 0, 1);
    }

    // Rate control stays fixed-QP `DISABLED` unconditionally — CBR is
    // H.264-only this pass (ADR-0002's scope; see this file's module doc).
    let mut rate_control = vk::VideoEncodeRateControlInfoKHR::builder()
        .rate_control_mode(vk::VideoEncodeRateControlModeFlagsKHR::DISABLED);
    let begin_info = vk::VideoBeginCodingInfoKHR::builder()
        .video_session(resources.session)
        .video_session_parameters(resources.session_parameters)
        .reference_slots(begin_slots)
        .push_next(&mut rate_control);
    // SAFETY: `begin_info` and its chained `rate_control`/`begin_slots` (and
    // everything `begin_slots`' entries themselves chain) stay alive for
    // this call; `command_buffer` is currently recording.
    unsafe {
        device.cmd_begin_video_coding_khr(command_buffer, &begin_info);
    }

    let src_resource = vk::VideoPictureResourceInfoKHR::builder()
        .coded_offset(vk::Offset2D { x: 0, y: 0 })
        .coded_extent(params.coded_extent)
        .base_array_layer(0)
        .image_view_binding(resources.input_image_view);
    let encode_info = if has_reference {
        vk::VideoEncodeInfoKHR::builder()
            .dst_buffer(resources.dst_buffer)
            .dst_buffer_offset(0)
            .dst_buffer_range(params.dst_size)
            .src_picture_resource(src_resource)
            .setup_reference_slot(&setup_slot)
            .reference_slots(&encode_reference_slots_array)
            .push_next(params.picture_info_pnext)
    } else {
        vk::VideoEncodeInfoKHR::builder()
            .dst_buffer(resources.dst_buffer)
            .dst_buffer_offset(0)
            .dst_buffer_range(params.dst_size)
            .src_picture_resource(src_resource)
            .setup_reference_slot(&setup_slot)
            .push_next(params.picture_info_pnext)
    };
    // SAFETY: `command_buffer` is inside the active video-coding scope opened
    // above; query 0 was just reset and is not currently active.
    unsafe {
        device.cmd_begin_query(
            command_buffer,
            resources.encode_feedback_query_pool,
            0,
            vk::QueryControlFlags::empty(),
        );
    }
    // SAFETY: `encode_info` and everything it chains/borrows
    // (`src_resource`/`setup_slot`/`encode_reference_slots_array`/the HEVC
    // picture-info chain built by the caller) stay alive for this call;
    // `command_buffer` is inside an active video-coding scope from the
    // `cmd_begin_video_coding_khr` call immediately above.
    unsafe {
        device.cmd_encode_video_khr(command_buffer, &encode_info);
    }
    // SAFETY: matches the `cmd_begin_query` immediately above, same
    // `command_buffer`, still inside the video-coding scope.
    unsafe {
        device.cmd_end_query(command_buffer, resources.encode_feedback_query_pool, 0);
    }

    let end_info = vk::VideoEndCodingInfoKHR::builder();
    // SAFETY: ends the video-coding scope opened above on this same
    // `command_buffer`.
    unsafe {
        device.cmd_end_video_coding_khr(command_buffer, &end_info);
    }
}
