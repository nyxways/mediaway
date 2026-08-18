//! AV1-specific session construction and per-picture decode — split from
//! `decoder.rs` purely to stay under this workspace's 1000-line-per-file
//! rule, mirroring `decoder_hevc.rs`'s identical split (not a different
//! architecture).
//!
//! **Scope this round** (`adr/vulkan/0002`): only `frame_type == KEY_FRAME`
//! pictures with `show_frame == 1` and a single tile reach a real
//! `vkCmdDecodeVideoKHR` call — anything else is rejected with
//! [`DecodeError::Unsupported`]/[`DecodeError::InvalidInput`] by
//! `av1_params.rs`'s own parser, the same "reject rather than silently
//! mis-decode" convention `decoder_hevc.rs` already established for HEVC's
//! own P/B-slice cut. No mid-stream sequence-header change support — one
//! sequence header per session, matching H.264/HEVC's identical cut (this is
//! a genuine consequence of AV1's own session-parameters shape, not just a
//! scope choice — see `adr/vulkan/0002`'s "Session-parameters lifecycle"
//! section: unlike HEVC's SPS-then-PPS two-part wait, AV1 needs only one
//! parameter set, so this crate's own `pending_*` deferred-session-open state
//! (`decoder.rs`'s `pending_sps`/`pending_pps` pair) has no AV1 equivalent —
//! a session either already has its one sequence header, or does not).

#![allow(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "Vulkan FFI + AV1 syntax-derived counts are driver/bitstream-bounded and small — \
              casts mirror decoder_hevc.rs's identical allow."
)]

use crate::{DecodeError, VideoOutputPreference};
use mediaway_common::{Bytes, Packet, PixelFormat, VideoFrame, VideoFrameStorage};

use crate::vulkan::av1_params::{Av1PictureInfoOptionals, Av1SequenceHeader, ObuType, scan_obus};
use crate::vulkan::av1_refs::Av1RefSlots;
use crate::vulkan::cpu_readback;
use crate::vulkan::decoder::{DecodedSession, VulkanVideoDecoder, map_err};
use crate::vulkan::session::{
    DecodeDevice, DecodeProfile, create_session_parameters_av1, create_video_session,
    query_capabilities, query_video_format,
};
use crate::vulkan::session_command::{
    SessionResources, allocate_command_buffer, create_command_pool, create_dpb_image, create_fence,
    create_host_buffer, transition_dpb_image_once, upload_to_host_memory,
};
use crate::vulkan::session_command_av1::{RecordParamsAv1, record_and_submit_av1};
use crate::vulkan::zero_copy;
use vulkanalia::vk;

/// Fixed bitstream-upload buffer capacity per picture — mirrors
/// `decoder.rs`'s/`decoder_hevc.rs`'s identical constant.
const BITSTREAM_CAPACITY: vk::DeviceSize = 1 << 20;

/// Physical Vulkan DPB array layers this round's `KEY_FRAME`-only session
/// allocates — `adr/vulkan/0002`'s own § Reference-model design finding: a
/// key-frame-only stream needs only the current picture (+ one layer of
/// headroom), not AV1's full 8 logical reference-name slots, since a
/// `KEY_FRAME` never reads a reference and [`Av1RefSlots::clear_all`] runs
/// before every decode anyway.
const DPB_SLOT_COUNT: u32 = 2;

/// Everything that depends on having parsed an AV1 sequence header — created
/// lazily the first time one is known, mirroring `HevcSession`'s role.
/// `pub(crate)` (and its `resources` field) so `decoder.rs`'s `Drop` impl can
/// tear it down (a sibling module, not a descendant of `decoder`).
#[allow(
    clippy::redundant_pub_crate,
    reason = "conflicts with the workspace's own unreachable_pub lint (also enabled) — pub(crate) \
              here is deliberate: `mod decoder_av1` is private, but decoder.rs (a sibling module) \
              still needs this type"
)]
pub(crate) struct Av1Session {
    pub(crate) resources: SessionResources,
    command_buffer: vk::CommandBuffer,
    coded_extent: vk::Extent2D,
    bitstream_alignment: vk::DeviceSize,
    ref_slots: Av1RefSlots,
    seq: Av1SequenceHeader,
}

impl VulkanVideoDecoder {
    /// Creates the Vulkan video session, session parameters, combined DPB
    /// image, bitstream/readback buffers, command pool/buffer, and fence for
    /// one AV1 sequence header. Mirrors `build_session_hevc` closely; real
    /// AV1-specific differences: [`create_session_parameters_av1`]'s
    /// single-pointer shape (no add-info/max-count list — AV1 "lacks
    /// sequence identifiers," see `adr/vulkan/0002`'s "Session-parameters
    /// lifecycle" section) and `max_active_reference_pictures == 0` (a
    /// `KEY_FRAME` never reads a reference this round).
    pub(crate) fn build_session_av1(
        &self,
        seq: Av1SequenceHeader,
    ) -> Result<Av1Session, crate::vulkan::session::VulkanDecodeError> {
        let instance = &self.instance_guard.instance;
        let device = &self.device_guard.device;
        let decode_device = DecodeDevice {
            device,
            queue: self.queue,
            queue_family_index: self.queue_family_index,
        };

        let mut profile = DecodeProfile::new_av1();
        let capabilities = query_capabilities(instance, self.physical_device, &mut profile)?;
        let coded_extent = vk::Extent2D {
            width: seq.width(),
            height: seq.height(),
        };
        capabilities.validate_requested_extent(coded_extent.width, coded_extent.height)?;

        let picture_format = query_video_format(
            instance,
            self.physical_device,
            &mut profile,
            vk::ImageUsageFlags::VIDEO_DECODE_DPB_KHR | vk::ImageUsageFlags::VIDEO_DECODE_DST_KHR,
        )?;

        let dpb_slot_count = DPB_SLOT_COUNT.min(capabilities.max_dpb_slots.max(1));

        let mut resources = SessionResources::default();
        let (session, session_memories) = create_video_session(
            &decode_device,
            &self.memory_properties,
            &mut profile,
            &capabilities,
            coded_extent,
            picture_format,
            dpb_slot_count,
            0, // max_active_reference_pictures: KEY_FRAME never reads a reference
        )?;
        resources.session = session;
        resources.session_memories = session_memories;

        let color_config = seq.to_std_color_config();
        let timing_info = Av1SequenceHeader::build_timing_info();
        let std_seq_header = seq.to_std(&color_config, &timing_info);
        resources.session_parameters =
            create_session_parameters_av1(&decode_device, session, &std_seq_header)?;

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

        Ok(Av1Session {
            resources,
            command_buffer,
            coded_extent,
            bitstream_alignment: capabilities.min_bitstream_buffer_size_alignment.max(1),
            ref_slots: Av1RefSlots::new(dpb_slot_count as usize),
            seq,
        })
    }

    /// Decodes one `OBU_FRAME`'s payload. Only `KEY_FRAME`/`show_frame == 1`/
    /// single-tile pictures reach a real decode call — see the module doc.
    pub(crate) fn decode_frame_av1(&mut self, obu_payload: &[u8]) -> Result<(), DecodeError> {
        let Some(DecodedSession::Av1(session)) = self.session.as_mut() else {
            return Err(DecodeError::InvalidInput);
        };
        let (frame_header, tile_layout) =
            crate::vulkan::av1_params::parse_frame_header(obu_payload, &session.seq)
                .map_err(|e| map_err(e.into()))?;
        // Real sanity check, not a formality: this crate's `KEY_FRAME`-only
        // scope already rejects `frame_size_override_flag == 1` combined
        // with a size different from the sequence header's max dimensions
        // (`av1_frame_header.rs`'s own parse), so this should always hold —
        // catches a future bug in that check (or in this session's own
        // `coded_extent` sizing) before it reaches the GPU with a
        // mismatched picture size.
        if frame_header.frame_width != session.coded_extent.width
            || frame_header.frame_height != session.coded_extent.height
        {
            return Err(DecodeError::InvalidInput);
        }

        session
            .ref_slots
            .clear_all()
            .map_err(|e| map_err(e.into()))?;
        let dst_slot_index = session
            .ref_slots
            .allocate_slot()
            .map_err(|e| map_err(e.into()))? as u32;

        // No Annex-B start code for AV1 — the uploaded range is the
        // OBU_FRAME's own payload bytes as-is (see `av1_frame_header.rs`'s
        // module doc for the frameHeaderOffset/tile-offset design this
        // implies).
        let aligned_len = vk::DeviceSize::try_from(obu_payload.len())
            .unwrap_or(vk::DeviceSize::MAX)
            .div_ceil(session.bitstream_alignment)
            * session.bitstream_alignment;
        if aligned_len > session.resources.bitstream_capacity {
            return Err(DecodeError::InvalidInput);
        }
        let mut padded = obu_payload.to_vec();
        padded.resize(aligned_len as usize, 0);
        upload_to_host_memory(
            &self.device_guard.device,
            session.resources.bitstream_memory,
            &padded,
        )
        .map_err(map_err)?;

        let mut optionals = Av1PictureInfoOptionals::new(&frame_header);
        optionals.finish();

        let decode_device = DecodeDevice {
            device: &self.device_guard.device,
            queue: self.queue,
            queue_family_index: self.queue_family_index,
        };
        let params = RecordParamsAv1 {
            command_buffer: session.command_buffer,
            coded_extent: session.coded_extent,
            bitstream_len: aligned_len,
            dst_slot_index,
            tile_offset: tile_layout.tile_offset,
            tile_size: tile_layout.tile_size,
            frame_header: &frame_header,
            optionals: &optionals,
        };
        record_and_submit_av1(&decode_device, &mut session.resources, &params).map_err(map_err)?;

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
                    .ref_slots
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

/// Scans `data` (a temporal unit's worth of low-overhead-bitstream-format
/// OBUs) for the first `OBU_SEQUENCE_HEADER`, mirrors
/// `decoder_hevc::scan_parameter_sets` — AV1 has only one parameter-set type
/// this crate needs (see the module doc), so there is no HEVC-shaped "wait
/// for a second parameter set" state to track.
#[allow(
    clippy::redundant_pub_crate,
    reason = "conflicts with the workspace's own unreachable_pub lint (also enabled) — pub(crate) \
              here is deliberate: `mod decoder_av1` is private, but decoder.rs (a sibling module) \
              still needs this function"
)]
pub(crate) fn scan_parameter_sets(data: &[u8], seq: &mut Option<Av1SequenceHeader>) {
    if data.is_empty() {
        return;
    }
    let Ok(obus) = scan_obus(data) else {
        return;
    };
    for obu in obus {
        if matches!(obu.obu_type, ObuType::SequenceHeader)
            && let Ok(parsed) = Av1SequenceHeader::parse(obu.payload)
        {
            *seq = Some(parsed);
        }
    }
}

/// Handles one packet's worth of AV1 OBUs — mirrors
/// `VideoDecoder::push_packet`'s H.264 loop in `decoder.rs`/
/// `push_packet_hevc`'s identical free-function-not-trait-method shape.
#[allow(
    clippy::redundant_pub_crate,
    reason = "conflicts with the workspace's own unreachable_pub lint (also enabled) — pub(crate) \
              here is deliberate: `mod decoder_av1` is private, but decoder.rs (a sibling module) \
              still needs this function"
)]
pub(crate) fn push_packet_av1(
    decoder: &mut VulkanVideoDecoder,
    packet: &Packet,
) -> Result<(), DecodeError> {
    let obus = scan_obus(&packet.payload).map_err(|e| map_err(e.into()))?;
    for obu in obus {
        match obu.obu_type {
            ObuType::SequenceHeader => {
                let seq = Av1SequenceHeader::parse(obu.payload).map_err(|e| map_err(e.into()))?;
                if decoder.session.is_none() {
                    decoder.session = Some(DecodedSession::Av1(
                        decoder.build_session_av1(seq).map_err(map_err)?,
                    ));
                }
                // A later sequence header on an already-open session is
                // ignored — no mid-stream parameter-set change support, see
                // the module doc.
            }
            ObuType::Frame => {
                decoder.decode_frame_av1(obu.payload)?;
            }
            ObuType::TemporalDelimiter | ObuType::Metadata | ObuType::Padding => {}
            ObuType::FrameHeader
            | ObuType::TileGroup
            | ObuType::RedundantFrameHeader
            | ObuType::TileList
            | ObuType::Other(_) => {
                return Err(DecodeError::Unsupported);
            }
        }
    }
    Ok(())
}
