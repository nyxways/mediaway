//! AV1 sibling of [`super::session_command`]'s H.264 command recording (and
//! [`super::session_command_hevc`]'s HEVC one) —
//! `record_upload_and_barriers`/`submit_and_readback` are codec-agnostic and
//! stay shared there; only `vkCmdEncodeVideoKHR`'s picture-info `pNext`
//! payload differs per codec (same C-union reasoning both siblings' doc
//! comments explain).
//!
//! `vkCmdEncodeVideoKHR`'s destination-buffer bytes are this frame's own OBU(s)
//! (temporal delimiter + frame OBU) — the separate `OBU_SEQUENCE_HEADER` OBU
//! is fetched once via [`crate::vulkan::session_encode::get_encoded_headers_av1`] and
//! prepended by the caller (`encoder.rs`), the same "fetch headers once,
//! concatenate per frame" shape H.264/HEVC use (see `av1_params.rs`'s module
//! doc for why AV1's header fetch needs no codec-specific `pNext` struct).

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
use vulkanalia::vk::{
    DeviceV1_0, HasBuilder, KhrVideoEncodeQueueExtensionDeviceCommands,
    KhrVideoQueueExtensionDeviceCommands,
};

use crate::vulkan::av1_params;
use crate::vulkan::session::{EncodeDevice, SessionResources, VulkanEncodeSessionError};
use crate::vulkan::session_command::{record_upload_and_barriers, submit_and_readback};

/// Grouped params for [`record_and_submit_av1`] — AV1 sibling of
/// [`super::session_command::RecordParams`]/
/// [`super::session_command_hevc::RecordParamsHevc`].
pub(crate) struct RecordParamsAv1<'a> {
    pub(crate) command_buffer: vk::CommandBuffer,
    pub(crate) coded_extent: vk::Extent2D,
    pub(crate) dst_size: vk::DeviceSize,
    pub(crate) picture_info_pnext: &'a mut vk::VideoEncodeAV1PictureInfoKHR,
}

/// AV1 sibling of [`super::session_command::record_and_submit`] — shares the
/// upload/barrier and submit/readback halves, only the video-coding scope's
/// picture-info payload differs.
pub(crate) fn record_and_submit_av1(
    encode_device: &EncodeDevice<'_>,
    resources: &mut SessionResources,
    params: &mut RecordParamsAv1<'_>,
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
    );
    record_video_coding_av1(encode_device, resources, params, command_buffer);

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
/// for one AV1 `KEY_FRAME` — mirrors
/// `session_command::record_video_coding`'s H.264 shape exactly, only the
/// picture-info `pNext` payload type differs.
fn record_video_coding_av1(
    encode_device: &EncodeDevice<'_>,
    resources: &SessionResources,
    params: &mut RecordParamsAv1<'_>,
    command_buffer: vk::CommandBuffer,
) {
    let device = encode_device.device;
    let dpb_resource = vk::VideoPictureResourceInfoKHR::builder()
        .coded_offset(vk::Offset2D { x: 0, y: 0 })
        .coded_extent(params.coded_extent)
        .base_array_layer(0)
        .image_view_binding(resources.dpb_image_view);
    let reference_extension_header = av1_params::build_extension_header();
    let reference_info = av1_params::build_reference_info(&reference_extension_header);
    let mut dpb_slot_info =
        vk::VideoEncodeAV1DpbSlotInfoKHR::builder().std_reference_info(&reference_info);
    let setup_slot = vk::VideoReferenceSlotInfoKHR::builder()
        .slot_index(0)
        .picture_resource(&dpb_resource)
        .push_next(&mut dpb_slot_info);
    let begin_slots = [vk::VideoReferenceSlotInfoKHR::builder().slot_index(0)];

    // SAFETY: `resources.encode_feedback_query_pool` has exactly 1 query
    // slot; resetting before use is required — a query pool's slots start in
    // an undefined state.
    unsafe {
        device.cmd_reset_query_pool(command_buffer, resources.encode_feedback_query_pool, 0, 1);
    }

    let mut rate_control = vk::VideoEncodeRateControlInfoKHR::builder()
        .rate_control_mode(vk::VideoEncodeRateControlModeFlagsKHR::DISABLED);
    let begin_info = vk::VideoBeginCodingInfoKHR::builder()
        .video_session(resources.session)
        .video_session_parameters(resources.session_parameters)
        .reference_slots(&begin_slots)
        .push_next(&mut rate_control);
    // SAFETY: `begin_info` and its chained `rate_control`/`begin_slots` stay
    // alive for this call; `command_buffer` is currently recording.
    unsafe {
        device.cmd_begin_video_coding_khr(command_buffer, &begin_info);
    }

    let src_resource = vk::VideoPictureResourceInfoKHR::builder()
        .coded_offset(vk::Offset2D { x: 0, y: 0 })
        .coded_extent(params.coded_extent)
        .base_array_layer(0)
        .image_view_binding(resources.input_image_view);
    let encode_info = vk::VideoEncodeInfoKHR::builder()
        .dst_buffer(resources.dst_buffer)
        .dst_buffer_offset(0)
        .dst_buffer_range(params.dst_size)
        .src_picture_resource(src_resource)
        .setup_reference_slot(&setup_slot)
        .push_next(params.picture_info_pnext);
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
    // (`src_resource`/`setup_slot`/`dpb_resource`/the AV1 picture-info chain
    // built by the caller) stay alive for this call; `command_buffer` is
    // inside an active video-coding scope from the `cmd_begin_video_coding_khr`
    // call above.
    unsafe {
        device.cmd_encode_video_khr(command_buffer, &encode_info);
    }
    // SAFETY: matches the `cmd_begin_query` above, same `command_buffer`,
    // still inside the video-coding scope.
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
