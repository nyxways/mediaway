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
//!
//! ADR-0002's AV1 follow-up adds real multi-slot DPB + `LAST_FRAME`
//! single-forward-reference cycling here too (`DpbRecordParamsAv1`, mirroring
//! [`super::session_command_hevc::DpbRecordParamsHevc`]) — CBR rate control
//! stays out of scope for AV1 (same "H.264-only" reasoning ADR-0002 already
//! gives for HEVC), so `record_video_coding_av1` keeps the
//! `RATE_CONTROL_MODE_DISABLED` fixed-QP shape unconditionally, unlike its
//! H.264 sibling. **This wiring is implemented but genuinely unverifiable on
//! this crate's reference hardware** — the AV1 base (IDR-only) per-frame
//! encode output is already hardware-verified invalid (driver-maturity
//! limitation, `adr/0001`'s AV1 addendum), so GOP mode built on top of it
//! inherits the same unverifiable status; see
//! `encoder_tests.rs::push_seven_av1_frames_gop_or_skip`.

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

use crate::vulkan::av1_params;
use crate::vulkan::session::{EncodeDevice, SessionResources, VulkanEncodeSessionError};
use crate::vulkan::session_command::{record_upload_and_barriers, submit_and_readback};

/// DPB slot wiring for one `vkCmdEncodeVideoKHR` AV1 call (ADR-0002's AV1
/// follow-up) — mirrors [`super::session_command_hevc::DpbRecordParamsHevc`];
/// see that type's doc for each field's meaning (`StdVideoEncodeAV1*` in
/// place of `StdVideoEncodeH265*`). Like `DpbRecordParamsHevc`, this crate has
/// no AV1 equivalent of H.264's Stage 1 one-shot diagnostic —
/// `VulkanVideoEncoder::push_frame` is the only caller, always building every
/// field from `self`'s own GOP/DPB state (`gop_size == 1` naturally
/// reproduces the original `KEY_FRAME`-only values through that same path —
/// see `encoder.rs`'s AV1 branch).
pub(crate) struct DpbRecordParamsAv1 {
    pub(crate) layer_count: u32,
    pub(crate) transition: bool,
    pub(crate) setup_slot: i32,
    pub(crate) setup_reference_info: Option<native::StdVideoEncodeAV1ReferenceInfo>,
    pub(crate) reference: Option<(i32, native::StdVideoEncodeAV1ReferenceInfo)>,
}

/// Grouped params for [`record_and_submit_av1`] — AV1 sibling of
/// [`super::session_command::RecordParams`]/
/// [`super::session_command_hevc::RecordParamsHevc`].
pub(crate) struct RecordParamsAv1<'a> {
    pub(crate) command_buffer: vk::CommandBuffer,
    pub(crate) coded_extent: vk::Extent2D,
    pub(crate) dst_size: vk::DeviceSize,
    pub(crate) picture_info_pnext: &'a mut vk::VideoEncodeAV1PictureInfoKHR,
    pub(crate) dpb: DpbRecordParamsAv1,
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
        params.dpb.layer_count,
        params.dpb.transition,
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
/// for one AV1 picture — mirrors
/// `session_command_hevc::record_video_coding_hevc`'s shape (ADR-0002's real
/// setup/reference slot pair, `VkVideoEncodeAV1DpbSlotInfoKHR` chained so a
/// *future* frame can read this one back) with the same "rate control stays
/// `DISABLED` fixed-QP unconditionally" choice HEVC makes — CBR is out of
/// scope for AV1 too (see this file's module doc).
#[allow(
    clippy::too_many_lines,
    reason = "linear per-frame Vulkan Video struct construction — mirrors \
              session_command_hevc::record_video_coding_hevc's own too_many_lines allow \
              and reasoning (setup/reference slot resources, DPB-slot-info chains \
              must all stay on this function's own stack frame for the raw pointers \
              built from them to remain valid through the unsafe calls at the bottom)"
)]
fn record_video_coding_av1(
    encode_device: &EncodeDevice<'_>,
    resources: &SessionResources,
    params: &mut RecordParamsAv1<'_>,
    command_buffer: vk::CommandBuffer,
) {
    let device = encode_device.device;
    // Fallback-only extension header: pointed to by the `unwrap_or_else`
    // default `StdVideoEncodeAV1ReferenceInfo` below, which is only ever
    // constructed (never actually chained into a live pNext the driver
    // reads) when `params.dpb.setup_reference_info`/`.reference` is `None`
    // (the base `gop_size == 1` path) — kept alive for this whole function's
    // stack frame regardless, same "always build a valid struct even if
    // unused" pattern `session_command.rs`'s H.264 fallback uses.
    let default_extension_header = av1_params::build_extension_header();

    // --- setup slot: the DPB slot this frame's picture is written into ---
    let setup_resource = vk::VideoPictureResourceInfoKHR::builder()
        .coded_offset(vk::Offset2D { x: 0, y: 0 })
        .coded_extent(params.coded_extent)
        .base_array_layer(params.dpb.setup_slot as u32)
        .image_view_binding(resources.dpb_image_view);
    let setup_reference_info = params
        .dpb
        .setup_reference_info
        .unwrap_or_else(|| av1_params::build_reference_info(0, true, &default_extension_header));
    let mut setup_dpb_slot_info = vk::VideoEncodeAV1DpbSlotInfoKHR::builder()
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
    // `begin_slots`' own setup-slot entry stays exactly the base shape (no
    // `picture_resource`, no DPB-slot-info chain) — this slot's content is
    // being (re)initialized by this very operation, matching the
    // already-hardware-verified H.264/HEVC convention.
    let begin_setup_slot =
        vk::VideoReferenceSlotInfoKHR::builder().slot_index(params.dpb.setup_slot);

    // --- reference slot: the sole LAST_FRAME reference a P-frame reads (GOP only) ---
    let has_reference = params.dpb.reference.is_some();
    let (reference_slot_index, reference_std_info) = params.dpb.reference.unwrap_or_else(|| {
        (
            0,
            av1_params::build_reference_info(0, true, &default_extension_header),
        )
    });
    let reference_resource = vk::VideoPictureResourceInfoKHR::builder()
        .coded_offset(vk::Offset2D { x: 0, y: 0 })
        .coded_extent(params.coded_extent)
        .base_array_layer(reference_slot_index as u32)
        .image_view_binding(resources.dpb_image_view);
    // Two separate `StdVideoEncodeAV1DpbSlotInfoKHR` locals (not one reused
    // for both arrays below): each `push_next` call takes an exclusive
    // borrow, and both the `begin_slots` and `encode_info.reference_slots`
    // arrays must stay alive simultaneously through the unsafe calls below —
    // same reasoning as `session_command::record_video_coding`'s H.264 twin.
    let mut begin_reference_dpb_slot_info = vk::VideoEncodeAV1DpbSlotInfoKHR::builder()
        .std_reference_info(&reference_std_info)
        .build();
    let mut encode_reference_dpb_slot_info = vk::VideoEncodeAV1DpbSlotInfoKHR::builder()
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

    // Rate control stays fixed-QP `DISABLED` unconditionally — CBR is out of
    // scope for AV1 (see this file's module doc).
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
    // (`src_resource`/`setup_slot`/`encode_reference_slots_array`/the AV1
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
