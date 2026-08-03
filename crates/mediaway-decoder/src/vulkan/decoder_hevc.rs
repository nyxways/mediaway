//! HEVC-specific session construction and per-picture decode — split from
//! `decoder.rs` purely to stay under this workspace's 1000-line-per-file
//! rule (Rust allows multiple `impl VulkanVideoDecoder` blocks across
//! modules in the same crate; this is a file-size split, not a different
//! architecture).
//!
//! **Scope this round**: only IDR pictures reach a real `vkCmdDecodeVideoKHR`
//! call via [`VulkanVideoDecoder::decode_slice_hevc`] — a P/B-slice HEVC NAL
//! is rejected with [`DecodeError::Unsupported`], honestly scoped rather than
//! silently mis-decoded. `hevc_slice.rs`'s own P-slice/short-term-RPS parsing
//! is real and independently sans-io tested (see that module) — this file
//! just does not yet feed a parsed P-slice through to a real decode call.
//! General-GOP HEVC hardware verification is an explicit follow-up; see
//! `adr/0001`'s 2026-07-30 addendum.
//!
//! Mirrors `decoder.rs`'s H.264 session/decode shape closely, including the
//! same three real Vulkan Video protocol fixes that H.264 needed (baked into
//! `session_command_hevc.rs` from the start, not rediscovered here): the
//! Annex-B start-code prefix on the uploaded bitstream (this file's own
//! responsibility, same as H.264's), the reference-slot `slotIndex = -1`
//! activation protocol, and the decode-target layout transition (both inside
//! `session_command_hevc::record_and_submit_hevc`).

#![allow(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "Vulkan FFI + HEVC syntax-derived counts are driver/bitstream-bounded and small — \
              casts mirror decoder.rs's identical allow."
)]

use crate::{DecodeError, VideoOutputPreference};
use mediaway_common::{Bytes, Packet, PixelFormat, VideoFrame, VideoFrameStorage};
use mediaway_sw::h264::{BitReader, split_annex_b};
use vulkanalia::vk;

use crate::vulkan::cpu_readback;
use crate::vulkan::decoder::{DecodedSession, VulkanVideoDecoder, map_err};
use crate::vulkan::dpb::{Dpb, DpbSlot};
use crate::vulkan::hevc_params::{HevcNalUnit, HevcNalUnitType, HevcPps, HevcSps, HevcVps};
use crate::vulkan::hevc_slice::HevcSliceSegmentHeader;
use crate::vulkan::session::{
    DecodeDevice, DecodeProfile, VulkanDecodeError, create_session_parameters_hevc,
    create_video_session, query_capabilities, query_video_format,
};
use crate::vulkan::session_command::{
    SessionResources, allocate_command_buffer, create_command_pool, create_dpb_image, create_fence,
    create_host_buffer, transition_dpb_image_once, upload_to_host_memory,
};
use crate::vulkan::session_command_hevc::{
    RecordParamsHevc, build_picture_info, record_and_submit_hevc,
};
use crate::vulkan::zero_copy;

/// Fixed bitstream-upload buffer capacity per picture — mirrors
/// `decoder.rs`'s identical H.264 constant.
const BITSTREAM_CAPACITY: vk::DeviceSize = 1 << 20;

/// Everything that depends on having parsed an HEVC SPS+PPS — created lazily
/// the first time both are known, mirroring `decoder::H264Session`'s role.
/// `pub(crate)` (and its `resources` field) so `decoder.rs`'s `Drop` impl can
/// tear it down (a sibling module, not a descendant of `decoder`, so needs
/// crate visibility rather than relying on Rust's parent/child privacy rule).
#[allow(
    clippy::redundant_pub_crate,
    reason = "conflicts with the workspace's own unreachable_pub lint (also enabled) — pub(crate) \
              here is deliberate: `mod decoder_hevc` is private, but decoder.rs (a sibling module) \
              still needs this type"
)]
pub(crate) struct HevcSession {
    pub(crate) resources: SessionResources,
    command_buffer: vk::CommandBuffer,
    coded_extent: vk::Extent2D,
    /// Required alignment for `VkVideoDecodeInfoKHR::srcBufferRange` — see
    /// `decoder::H264Session`'s identical field doc.
    bitstream_alignment: vk::DeviceSize,
    dpb: Dpb,
    #[allow(
        dead_code,
        reason = "kept for symmetry with H264Session's sps/pps/vps triplet; \
                                  this crate's own decode path never re-reads a VPS field \
                                  after session-parameters creation"
    )]
    vps: HevcVps,
    sps: HevcSps,
    pps: HevcPps,
}

impl VulkanVideoDecoder {
    /// Creates the Vulkan video session, session parameters, combined DPB
    /// image, bitstream/readback buffers, command pool/buffer, and fence for
    /// one HEVC SPS+PPS pair. Mirrors `build_session_h264` closely.
    ///
    /// The VPS is **synthesized** from the SPS's own `sps_video_parameter_set_id`
    /// rather than requiring a real parsed VPS NAL first — this crate's own
    /// decode path never needs any VPS content beyond that id (the session
    /// parameters object just needs consistent ids across VPS/SPS/PPS), so
    /// tracking "have we seen a real VPS yet" as a third session-open
    /// prerequisite alongside SPS+PPS would add state this crate never
    /// actually uses.
    #[allow(
        clippy::similar_names,
        reason = "sps/pps/vps and std_sps/std_pps/std_vps each name one parameter-set triplet — \
                  matching, not confusable, names"
    )]
    pub(crate) fn build_session_hevc(
        &self,
        sps: HevcSps,
        pps: HevcPps,
    ) -> Result<HevcSession, VulkanDecodeError> {
        let vps = HevcVps {
            vps_video_parameter_set_id: sps.sps_video_parameter_set_id,
        };
        let instance = &self.instance_guard.instance;
        let device = &self.device_guard.device;
        let decode_device = DecodeDevice {
            device,
            queue: self.queue,
            queue_family_index: self.queue_family_index,
        };

        let mut profile = DecodeProfile::new_hevc();
        let capabilities = query_capabilities(instance, self.physical_device, &mut profile)?;
        let coded_extent = vk::Extent2D {
            width: sps.pic_width_in_luma_samples,
            height: sps.pic_height_in_luma_samples,
        };
        capabilities.validate_requested_extent(coded_extent.width, coded_extent.height)?;

        let picture_format = query_video_format(
            instance,
            self.physical_device,
            &mut profile,
            vk::ImageUsageFlags::VIDEO_DECODE_DPB_KHR | vk::ImageUsageFlags::VIDEO_DECODE_DST_KHR,
        )?;

        // +1: room for the picture currently being decoded alongside every
        // active reference. This round's real GPU decode path only ever
        // handles IDR pictures (which clear the whole DPB), so 2 slots is
        // already generous — kept proportional to `max_dec_pic_buffering`
        // for when general-GOP HEVC decode lands.
        let dpb_slot_count = (sps.max_dec_pic_buffering + 1)
            .max(1)
            .min(capabilities.max_dpb_slots.max(1));

        let mut resources = SessionResources::default();
        let (session, session_memories) = create_video_session(
            &decode_device,
            &self.memory_properties,
            &mut profile,
            &capabilities,
            coded_extent,
            picture_format,
            dpb_slot_count,
            sps.max_dec_pic_buffering
                .min(capabilities.max_active_reference_pictures),
        )?;
        resources.session = session;
        resources.session_memories = session_memories;

        let profile_tier_level = sps.to_std_profile_tier_level();
        let dec_pic_buf_mgr = sps.to_std_dec_pic_buf_mgr();
        let std_vps = vps.to_std(&profile_tier_level, &dec_pic_buf_mgr);
        let std_sps = sps.to_std(&profile_tier_level, &dec_pic_buf_mgr);
        let std_pps = pps.to_std(vps.vps_video_parameter_set_id);
        resources.session_parameters =
            create_session_parameters_hevc(&decode_device, session, &std_vps, &std_sps, &std_pps)?;

        let (dpb_image, dpb_image_memory, dpb_image_view) = create_dpb_image(
            &decode_device,
            &self.memory_properties,
            &mut profile,
            picture_format,
            coded_extent,
            dpb_slot_count,
        )?;
        resources.dpb_image = dpb_image;
        resources.dpb_image_memory = dpb_image_memory;
        resources.dpb_image_view = dpb_image_view;

        let (bitstream_buffer, bitstream_memory) = create_host_buffer(
            device,
            &self.memory_properties,
            BITSTREAM_CAPACITY,
            vk::BufferUsageFlags::VIDEO_DECODE_SRC_KHR,
        )?;
        resources.bitstream_buffer = bitstream_buffer;
        resources.bitstream_memory = bitstream_memory;
        resources.bitstream_capacity = BITSTREAM_CAPACITY;

        let readback_size = cpu_readback::nv12_byte_size(coded_extent.width, coded_extent.height);
        let (readback_buffer, readback_memory) = create_host_buffer(
            device,
            &self.memory_properties,
            readback_size,
            vk::BufferUsageFlags::TRANSFER_DST,
        )?;
        resources.readback_buffer = readback_buffer;
        resources.readback_memory = readback_memory;
        resources.readback_size = readback_size;

        resources.command_pool = create_command_pool(device, self.queue_family_index)?;
        let command_buffer = allocate_command_buffer(device, resources.command_pool)?;
        resources.fence = create_fence(device)?;

        transition_dpb_image_once(&decode_device, &resources, dpb_slot_count, command_buffer)?;

        Ok(HevcSession {
            resources,
            command_buffer,
            coded_extent,
            bitstream_alignment: capabilities.min_bitstream_buffer_size_alignment.max(1),
            dpb: Dpb::new(dpb_slot_count as usize),
            vps,
            sps,
            pps,
        })
    }

    /// Decodes one HEVC slice-segment NAL. Only IDR pictures reach a real
    /// decode call this round — see the module doc's scope cut.
    pub(crate) fn decode_slice_hevc(
        &mut self,
        nal: &HevcNalUnit,
        raw_nal: &[u8],
    ) -> Result<(), DecodeError> {
        let Some(DecodedSession::Hevc(session)) = self.session.as_mut() else {
            return Err(DecodeError::InvalidInput);
        };
        let mut reader = BitReader::new(&nal.rbsp);
        let slice =
            HevcSliceSegmentHeader::parse(&mut reader, &session.sps, &session.pps, nal.unit_type)
                .map_err(|e| map_err(e.into()))?;
        let _ = &slice; // slice_type/pic_order_cnt_lsb/short_term_rps: unused this round (IDR-only path below)
        if !matches!(nal.unit_type, HevcNalUnitType::Idr) {
            return Err(DecodeError::Unsupported);
        }

        session.dpb.clear_all().map_err(|e| map_err(e.into()))?;
        let dst_slot_index = session.dpb.allocate_slot().map_err(|e| map_err(e.into()))? as u32;

        // Real Annex-B start code prefix — see `decoder.rs::decode_slice_h264`'s
        // identical comment for the real-hardware finding this mirrors.
        let mut with_start_code = vec![0x00u8, 0x00, 0x01];
        with_start_code.extend_from_slice(raw_nal);
        let aligned_len = vk::DeviceSize::try_from(with_start_code.len())
            .unwrap_or(vk::DeviceSize::MAX)
            .div_ceil(session.bitstream_alignment)
            * session.bitstream_alignment;
        if aligned_len > session.resources.bitstream_capacity {
            return Err(DecodeError::InvalidInput);
        }
        with_start_code.resize(aligned_len as usize, 0);
        upload_to_host_memory(
            &self.device_guard.device,
            session.resources.bitstream_memory,
            &with_start_code,
        )
        .map_err(map_err)?;

        // IDR: PicOrderCntVal is always 0, no references at all (the DPB was
        // just cleared above) — `RefPicSetStCurrBefore`/`After` stay
        // all-`0xFF` (unused).
        let picture_info = build_picture_info(
            session.vps.vps_video_parameter_set_id,
            session.pps.pps_seq_parameter_set_id,
            session.pps.pps_pic_parameter_set_id,
            0,
            true,
            true,
            [0xFF; 8],
            [0xFF; 8],
        );
        let decode_device = DecodeDevice {
            device: &self.device_guard.device,
            queue: self.queue,
            queue_family_index: self.queue_family_index,
        };
        let params = RecordParamsHevc {
            command_buffer: session.command_buffer,
            coded_extent: session.coded_extent,
            bitstream_len: aligned_len,
            dst_slot_index,
            reference_slots: &[],
            picture_info: &picture_info,
        };
        record_and_submit_hevc(&decode_device, &mut session.resources, &params).map_err(map_err)?;

        session
            .dpb
            .insert(dst_slot_index as usize, DpbSlot::new_reference(0, 0, 0))
            .map_err(|e| map_err(e.into()))?;

        let output = match self.output {
            VideoOutputPreference::CpuFramesOk => {
                let bytes = cpu_readback::read_nv12(
                    &decode_device,
                    &session.resources,
                    session.command_buffer,
                    session.coded_extent,
                    dst_slot_index,
                )
                .map_err(map_err)?;
                VideoFrameStorage::Cpu {
                    data: Bytes::from(bytes),
                }
            }
            VideoOutputPreference::ZeroCopyGpu => {
                session
                    .dpb
                    .mark_outstanding(dst_slot_index as usize)
                    .map_err(|e| map_err(e.into()))?;
                let handle =
                    zero_copy::build_handle(&session.resources, dst_slot_index).map_err(map_err)?;
                VideoFrameStorage::Gpu(handle)
            }
        };

        self.pending.push_back(VideoFrame {
            pts: 0,
            duration: 0,
            width: session.coded_extent.width,
            height: session.coded_extent.height,
            format: PixelFormat::Nv12,
            storage: output,
        });
        Ok(())
    }
}

/// Scans `data` (assumed Annex-B) for the first VPS/SPS/PPS. VPS content is
/// ignored (this crate synthesizes its own — see [`VulkanVideoDecoder::build_session_hevc`]'s
/// doc); only SPS/PPS are collected, mirroring `decoder::scan_parameter_sets`.
#[allow(
    clippy::redundant_pub_crate,
    reason = "conflicts with the workspace's own unreachable_pub lint (also enabled) — pub(crate) \
              here is deliberate: `mod decoder_hevc` is private, but decoder.rs (a sibling module) \
              still needs this function"
)]
pub(crate) fn scan_parameter_sets(
    data: &[u8],
    sps: &mut Option<HevcSps>,
    pps: &mut Option<HevcPps>,
) {
    if data.is_empty() {
        return;
    }
    let Ok(units) = split_annex_b(data) else {
        return;
    };
    for unit in units {
        let Ok(nal) = HevcNalUnit::parse(unit) else {
            continue;
        };
        match nal.unit_type {
            HevcNalUnitType::Sps => {
                if let Ok(parsed) = HevcSps::parse(&nal.rbsp) {
                    *sps = Some(parsed);
                }
            }
            HevcNalUnitType::Pps => {
                if let Ok(parsed) = HevcPps::parse(&nal.rbsp) {
                    *pps = Some(parsed);
                }
            }
            _ => {}
        }
    }
}

/// Handles one packet's worth of HEVC NAL units — mirrors
/// `VideoDecoder::push_packet`'s H.264 loop in `decoder.rs`, as a free
/// function (not a trait method) since [`crate::vulkan::decoder::VulkanVideoDecoder::push_packet`]
/// dispatches to this directly rather than duplicating the trait impl.
#[allow(
    clippy::redundant_pub_crate,
    reason = "conflicts with the workspace's own unreachable_pub lint (also enabled) — pub(crate) \
              here is deliberate: `mod decoder_hevc` is private, but decoder.rs (a sibling module) \
              still needs this function"
)]
pub(crate) fn push_packet_hevc(
    decoder: &mut VulkanVideoDecoder,
    packet: &Packet,
) -> Result<(), DecodeError> {
    let units = split_annex_b(&packet.payload).map_err(|_| DecodeError::InvalidInput)?;
    for unit in units {
        let nal = HevcNalUnit::parse(unit).map_err(|e| map_err(e.into()))?;
        match nal.unit_type {
            HevcNalUnitType::Sps => {
                let sps = HevcSps::parse(&nal.rbsp).map_err(|e| map_err(e.into()))?;
                if decoder.session.is_none() {
                    if let Some(pps) = decoder.pending_hevc_pps.take() {
                        decoder.session = Some(DecodedSession::Hevc(
                            decoder.build_session_hevc(sps, pps).map_err(map_err)?,
                        ));
                    } else {
                        decoder.pending_hevc_sps = Some(sps);
                    }
                }
            }
            HevcNalUnitType::Pps => {
                let pps = HevcPps::parse(&nal.rbsp).map_err(|e| map_err(e.into()))?;
                if decoder.session.is_none() {
                    if let Some(sps) = decoder.pending_hevc_sps.take() {
                        decoder.session = Some(DecodedSession::Hevc(
                            decoder.build_session_hevc(sps, pps).map_err(map_err)?,
                        ));
                    } else {
                        decoder.pending_hevc_pps = Some(pps);
                    }
                }
            }
            HevcNalUnitType::Idr | HevcNalUnitType::Cra | HevcNalUnitType::Trail => {
                decoder.decode_slice_hevc(&nal, unit)?;
            }
            HevcNalUnitType::Vps | HevcNalUnitType::Other(_) => {}
        }
    }
    Ok(())
}
