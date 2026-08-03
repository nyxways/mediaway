//! [`VulkanVideoDecoder`]: a real, reusable, multi-frame H.264/HEVC decode
//! session implementing [`crate::VideoDecoder`].
//!
//! Mirrors `mediaway-encoder-vulkan::encoder::VulkanVideoEncoder`'s
//! persistent-session shape: instance/device/video session/session
//! parameters/DPB images/command pool/fence all persist across
//! `push_packet`/`poll_frame` calls — only per-picture upload/record/submit/
//! readback repeats. HEVC-specific session construction/decode live in
//! [`decoder_hevc`] (a private submodule of this one — split out to stay
//! under this workspace's 1000-line-per-source-file rule, not for any
//! architectural reason).
//!
//! **Scope this round** (see crate root docs / `adr/0001`):
//! - **H.264**: general P-slice GOP, hardware-verified (I/P-slices, B
//!   rejected, sliding-window DPB only, adaptive MMCO rejected).
//! - **HEVC**: VPS/SPS/PPS + slice-segment-header + short-term RPS parsing
//!   are real and sans-io-tested (`hevc_params.rs`/`hevc_slice.rs`), but this
//!   round's real GPU decode path only accepts **IDR pictures** — a P/B-slice
//!   HEVC NAL reaching [`decoder_hevc`]'s decode path is rejected with
//!   [`DecodeError::Unsupported`], honestly scoped rather than silently
//!   mis-decoded (general-GOP HEVC hardware verification is an explicit
//!   follow-up; see `adr/0001`'s 2026-07-30 addendum).
//! - Both codecs: one SPS/PPS(/VPS) per session (a later parameter-set NAL
//!   after the session is already open is ignored — no mid-stream
//!   parameter-set change support), single slice per picture, no cropping
//!   applied to reported frame geometry (coded == display extent). Bitstream
//!   buffer capacity is a fixed 1 MiB per picture (no dynamic resize) —
//!   large pictures/high bitrates that exceed this are rejected with
//!   [`DecodeError::InvalidInput`] rather than silently truncated.

#![allow(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "Vulkan FFI + H.264 syntax-derived counts are driver/bitstream-bounded and small — \
              casts mirror mediaway-encoder-vulkan::encoder's identical allow."
)]

use std::collections::VecDeque;

use crate::{DecodeError, VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};
use mediaway_sw::h264::{BitReader, NalUnit, NalUnitType, split_annex_b};
use vulkanalia::vk;
use vulkanalia::vk::{DeviceV1_0, InstanceV1_0};

use crate::vulkan::cpu_readback;
use crate::vulkan::decoder_hevc::HevcSession;
use crate::vulkan::dpb::{Dpb, DpbSlot};
use crate::vulkan::h264_params::{H264Pps, H264Sps, derive_pic_order_cnt_msb};
use crate::vulkan::h264_slice::{
    H264SliceHeader, H264SliceType, apply_ref_pic_list_modifications, default_ref_pic_list0,
};
use crate::vulkan::hevc_params::{HevcPps, HevcSps};
use crate::vulkan::session::{
    DecodeDevice, DecodeProfile, DeviceGuard, InstanceGuard, VulkanDecodeError, create_instance,
    create_logical_device, create_session_parameters_h264, create_video_session,
    find_h264_decode_device, find_hevc_decode_device, query_capabilities, query_video_format,
};
use crate::vulkan::session_command::{
    SessionResources, allocate_command_buffer, create_command_pool, create_dpb_image, create_fence,
    create_host_buffer, transition_dpb_image_once, upload_to_host_memory,
};
use crate::vulkan::session_command_h264::{
    RecordParamsH264, build_picture_info, record_and_submit_h264,
};
use crate::vulkan::zero_copy;

/// Fixed bitstream-upload buffer capacity per picture (see module doc's
/// scope cut).
const BITSTREAM_CAPACITY: vk::DeviceSize = 1 << 20;

/// Everything that depends on having parsed an H.264 SPS+PPS — created
/// lazily the first time both are known (from `config.extra_data` at
/// [`VulkanVideoDecoder::open`] time, or from the first pushed packet).
#[allow(
    clippy::redundant_pub_crate,
    reason = "conflicts with the workspace's own unreachable_pub lint (also enabled) — pub(crate) \
              here is deliberate: `mod decoder` is private, but decoder_hevc.rs (a sibling module) \
              still needs this type"
)]
pub(crate) struct H264Session {
    resources: SessionResources,
    command_buffer: vk::CommandBuffer,
    coded_extent: vk::Extent2D,
    /// Required alignment for `VkVideoDecodeInfoKHR::srcBufferRange` (see
    /// `session.rs::Capabilities`'s doc) — the uploaded bitstream range is
    /// rounded up to this before every decode call.
    bitstream_alignment: vk::DeviceSize,
    dpb: Dpb,
    sps: H264Sps,
    pps: H264Pps,
    /// `PicOrderCntMsb`/`pic_order_cnt_lsb` of the last **reference**
    /// picture decoded — carried forward for [`derive_pic_order_cnt_msb`]
    /// (ITU-T H.264 § 8.2.1.1). Reset to `(0, 0)` on every IDR.
    prev_poc_msb: i32,
    prev_poc_lsb: u32,
}

/// Either codec's open session state — [`VulkanVideoDecoder::open`] picks one
/// variant based on `config.codec` and never switches. `pub(crate)`: built and
/// matched from `decoder_hevc.rs` (a sibling module, not a descendant of this
/// one, so needs at least crate visibility to construct/match `Hevc(..)`).
#[allow(
    clippy::redundant_pub_crate,
    reason = "conflicts with the workspace's own unreachable_pub lint (also enabled) — pub(crate) \
              here is deliberate: `mod decoder` is private, but decoder_hevc.rs (a sibling module) \
              still needs this type"
)]
pub(crate) enum DecodedSession {
    H264(H264Session),
    Hevc(HevcSession),
}

/// A real, reusable, hardware-backed `VK_KHR_video_decode_queue` H.264/HEVC
/// decoder.
///
/// Owns its own Vulkan instance and logical device — this backend never
/// imports a caller-supplied device (Zero-Copy output still hands out real
/// `VkImage`/`VkImageView` handles from that device via
/// [`GpuBufferHandle::Vulkan`] — see `zero_copy.rs`).
pub struct VulkanVideoDecoder {
    // Field order is load-bearing: Rust drops struct fields top-to-bottom,
    // and Vulkan requires every `VkDevice` to be destroyed before the
    // `VkInstance` it was created from — `device_guard` must drop (and
    // therefore be declared) before `instance_guard`. `session` is torn
    // down explicitly in this type's own `Drop::drop` before any of these
    // fields' auto-drop glue runs, so its position doesn't matter for
    // ordering — only that `device_guard` is still valid when that explicit
    // teardown runs (it is: custom `Drop::drop` always completes before the
    // compiler-generated field drops start). Mirrors
    // `mediaway-encoder-vulkan::encoder::VulkanVideoEncoder`'s identical note.
    pub(crate) device_guard: DeviceGuard,
    pub(crate) instance_guard: InstanceGuard,
    _entry: vulkanalia::Entry,
    pub(crate) physical_device: vk::PhysicalDevice,
    pub(crate) queue: vk::Queue,
    pub(crate) queue_family_index: u32,
    pub(crate) memory_properties: vk::PhysicalDeviceMemoryProperties,

    /// The codec this decoder was opened for — fixed at [`VulkanVideoDecoder::open`]
    /// time, never changes; [`VideoDecoder::push_packet`] dispatches on it.
    pub(crate) codec: CodecKind,
    pub(crate) session: Option<DecodedSession>,
    pending_sps: Option<H264Sps>,
    pending_pps: Option<H264Pps>,
    pub(crate) pending_hevc_sps: Option<HevcSps>,
    pub(crate) pending_hevc_pps: Option<HevcPps>,

    pub(crate) output: VideoOutputPreference,
    info: StreamInfo,
    pub(crate) pending: VecDeque<VideoFrame>,
    flushed: bool,
}

// SAFETY: every field is an owned Vulkan handle/`vulkanalia` wrapper
// (thread-safe per the Vulkan spec's external-synchronization rules — this
// type provides that synchronization itself via `&mut self`) or plain owned
// data.
unsafe impl Send for VulkanVideoDecoder {}

impl VulkanVideoDecoder {
    /// Opens a real H.264 or HEVC Vulkan Video decode session.
    ///
    /// If `config.extra_data` already contains the Annex-B parameter sets,
    /// the Vulkan video session/images are created immediately; otherwise
    /// creation is deferred until the first pushed packet supplies them (see
    /// the module doc).
    ///
    /// # Errors
    ///
    /// - [`DecodeError::Unsupported`] — `config.codec` is neither
    ///   [`CodecKind::H264`] nor [`CodecKind::Hevc`], `config.pixel_format`
    ///   is not [`PixelFormat::Nv12`], or no Vulkan loader/device on this
    ///   host advertises a decode queue family for the requested codec.
    /// - [`DecodeError::Backend`] — a Vulkan object-creation call failed.
    #[allow(
        clippy::similar_names,
        reason = "pending_sps/pending_pps (and their HEVC counterparts) name the two halves of \
                  one deferred-session-open state pair — matching, not confusable, names"
    )]
    pub fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        if !matches!(config.codec, CodecKind::H264 | CodecKind::Hevc) {
            return Err(DecodeError::Unsupported);
        }
        if config.pixel_format != PixelFormat::Nv12 {
            return Err(DecodeError::Unsupported);
        }
        let is_hevc = config.codec == CodecKind::Hevc;

        let (entry, instance_guard) = create_instance().map_err(map_err)?;
        let (physical_device, queue_family_index) = {
            let InstanceGuard { instance } = &instance_guard;
            if is_hevc {
                find_hevc_decode_device(instance).map_err(map_err)?
            } else {
                find_h264_decode_device(instance).map_err(map_err)?
            }
        };
        let device_guard = {
            let InstanceGuard { instance } = &instance_guard;
            create_logical_device(instance, physical_device, queue_family_index).map_err(map_err)?
        };
        // SAFETY: `queue_family_index`/index `0` were the exact queue this
        // device was created with above.
        let queue = unsafe { device_guard.device.get_device_queue(queue_family_index, 0) };
        // SAFETY: `physical_device` came from `find_h264_decode_device`/
        // `find_hevc_decode_device` on this same instance.
        let memory_properties = unsafe {
            instance_guard
                .instance
                .get_physical_device_memory_properties(physical_device)
        };

        let mut decoder = Self {
            device_guard,
            instance_guard,
            _entry: entry,
            physical_device,
            queue,
            queue_family_index,
            memory_properties,
            codec: config.codec,
            session: None,
            pending_sps: None,
            pending_pps: None,
            pending_hevc_sps: None,
            pending_hevc_pps: None,
            output: config.output,
            info: stream_info_from(config),
            pending: VecDeque::new(),
            flushed: false,
        };

        if is_hevc {
            let mut pending_sps = None;
            let mut pending_pps = None;
            crate::vulkan::decoder_hevc::scan_parameter_sets(
                &config.extra_data,
                &mut pending_sps,
                &mut pending_pps,
            );
            if let (Some(sps), Some(pps)) = (pending_sps.take(), pending_pps.take()) {
                decoder.session = Some(DecodedSession::Hevc(
                    decoder.build_session_hevc(sps, pps).map_err(map_err)?,
                ));
            } else {
                decoder.pending_hevc_sps = pending_sps;
                decoder.pending_hevc_pps = pending_pps;
            }
        } else {
            let mut pending_sps = None;
            let mut pending_pps = None;
            scan_parameter_sets(&config.extra_data, &mut pending_sps, &mut pending_pps);
            if let (Some(sps), Some(pps)) = (pending_sps.take(), pending_pps.take()) {
                decoder.session = Some(DecodedSession::H264(
                    decoder.build_session_h264(sps, pps).map_err(map_err)?,
                ));
            } else {
                decoder.pending_sps = pending_sps;
                decoder.pending_pps = pending_pps;
            }
        }
        Ok(decoder)
    }

    /// Creates the Vulkan video session, session parameters, combined DPB
    /// image, bitstream/readback buffers, command pool/buffer, and fence for
    /// one H.264 SPS+PPS pair.
    #[allow(
        clippy::similar_names,
        reason = "sps/pps and std_sps/std_pps each name the two halves of one parameter-set \
                  pair — matching, not confusable, names"
    )]
    fn build_session_h264(
        &self,
        sps: H264Sps,
        pps: H264Pps,
    ) -> Result<H264Session, VulkanDecodeError> {
        let instance = &self.instance_guard.instance;
        let device = &self.device_guard.device;
        let decode_device = DecodeDevice {
            device,
            queue: self.queue,
            queue_family_index: self.queue_family_index,
        };

        let mut profile = DecodeProfile::new_h264();
        let capabilities = query_capabilities(instance, self.physical_device, &mut profile)?;
        let coded_extent = vk::Extent2D {
            width: sps.pic_width_in_mbs * 16,
            height: sps.pic_height_in_map_units * 16,
        };
        capabilities.validate_requested_extent(coded_extent.width, coded_extent.height)?;

        let picture_format = query_video_format(
            instance,
            self.physical_device,
            &mut profile,
            vk::ImageUsageFlags::VIDEO_DECODE_DPB_KHR | vk::ImageUsageFlags::VIDEO_DECODE_DST_KHR,
        )?;

        // +1: room for the picture currently being decoded alongside every
        // active short-term reference.
        let dpb_slot_count = (sps.max_num_ref_frames + 1)
            .max(1)
            .min(capabilities.max_dpb_slots.max(1));
        let max_active_reference_pictures = sps
            .max_num_ref_frames
            .min(capabilities.max_active_reference_pictures);

        let mut resources = SessionResources::default();
        let (session, session_memories) = create_video_session(
            &decode_device,
            &self.memory_properties,
            &mut profile,
            &capabilities,
            coded_extent,
            picture_format,
            dpb_slot_count,
            max_active_reference_pictures,
        )?;
        resources.session = session;
        resources.session_memories = session_memories;

        let std_sps = sps.to_std();
        let std_pps = pps.to_std();
        resources.session_parameters =
            create_session_parameters_h264(&decode_device, session, &std_sps, &std_pps)?;

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

        Ok(H264Session {
            resources,
            command_buffer,
            coded_extent,
            bitstream_alignment: capabilities.min_bitstream_buffer_size_alignment.max(1),
            dpb: Dpb::new(dpb_slot_count as usize),
            sps,
            pps,
            prev_poc_msb: 0,
            prev_poc_lsb: 0,
        })
    }

    #[allow(
        clippy::too_many_lines,
        reason = "linear per-picture decode sequence (slice-header parse -> DPB update -> \
                  ref-list build -> slot allocate -> upload -> GPU submit -> output) — \
                  splitting further would just move consecutive steps of the same picture's \
                  decode into a same-file helper"
    )]
    fn decode_slice_h264(&mut self, nal: &NalUnit, raw_nal: &[u8]) -> Result<(), DecodeError> {
        let Some(DecodedSession::H264(session)) = self.session.as_mut() else {
            return Err(DecodeError::InvalidInput);
        };
        let mut reader = BitReader::new(&nal.rbsp);
        let slice = H264SliceHeader::parse(
            &mut reader,
            &session.sps,
            &session.pps,
            nal.unit_type,
            nal.ref_idc,
        )
        .map_err(|e| map_err(e.into()))?;
        let is_idr = matches!(nal.unit_type, NalUnitType::IdrSlice);
        let is_reference = nal.ref_idc != 0;

        if is_idr {
            session.dpb.clear_all().map_err(|e| map_err(e.into()))?;
            session.prev_poc_msb = 0;
            session.prev_poc_lsb = 0;
        }
        session
            .dpb
            .refresh_frame_num_wraps(slice.frame_num, session.sps.max_frame_num);

        let max_pic_order_cnt_lsb = 1u32 << session.sps.log2_max_pic_order_cnt_lsb;
        let poc_msb = derive_pic_order_cnt_msb(
            slice.pic_order_cnt_lsb,
            session.prev_poc_msb,
            session.prev_poc_lsb,
            max_pic_order_cnt_lsb,
        );
        let pic_order_cnt = poc_msb + slice.pic_order_cnt_lsb as i32;
        if is_reference {
            session.prev_poc_msb = poc_msb;
            session.prev_poc_lsb = slice.pic_order_cnt_lsb;
        }

        let reference_slots: Vec<(u32, DpbSlot)> = if matches!(slice.slice_type, H264SliceType::P) {
            let default_list = default_ref_pic_list0(&session.dpb);
            let modified = apply_ref_pic_list_modifications(
                default_list,
                &session.dpb,
                &slice.ref_pic_list_modifications_l0,
                slice.frame_num as i32,
                session.sps.max_frame_num as i32,
            );
            let active_count = slice.num_ref_idx_l0_active.max(1) as usize;
            modified
                .into_iter()
                .take(active_count)
                .filter_map(|index| session.dpb.slot(index).map(|slot| (index as u32, *slot)))
                .collect()
        } else {
            Vec::new()
        };

        let dst_slot_index = session.dpb.allocate_slot().map_err(|e| map_err(e.into()))? as u32;

        // Prepend a real Annex-B start code (`00 00 01`) before the NAL
        // header + payload bytes in the uploaded buffer — the hardware's own
        // bitstream scanner locates the NAL by that start code (`slice_offsets`
        // below points at it, offset `0`), it is not implicit just because
        // this crate's own offset happens to equal the NAL header's position.
        // Confirmed against a real working implementation (FFmpeg's
        // `vulkan_decode.c::ff_vk_decode_add_slice`, which always prepends
        // `{ 0x0, 0x0, 0x1 }` and records the slice offset at *that* prefix,
        // not past it) — this crate initially uploaded `raw_nal` bare, with
        // no start code, which decoded with no `VkResult` error but wrote no
        // observable output.
        let mut with_start_code = vec![0x00u8, 0x00, 0x01];
        with_start_code.extend_from_slice(raw_nal);

        // `VkVideoDecodeInfoKHR::srcBufferRange` must be a multiple of the
        // driver-reported `minBitstreamBufferSizeAlignment` — round the
        // uploaded byte range up (zero-padded; H.264 Annex-B decoders treat
        // trailing zero bytes as harmless `cabac_zero_word`/padding) rather
        // than declaring an unaligned range (see `session.rs::Capabilities`'s
        // doc: an unaligned range was found empirically to make
        // `vkCmdDecodeVideoKHR` silently no-op with no `VkResult` failure).
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

        let picture_info = build_picture_info(
            session.sps.seq_parameter_set_id as u8,
            session.pps.pic_parameter_set_id as u8,
            slice.frame_num as u16,
            pic_order_cnt,
            is_idr,
            is_reference,
            matches!(slice.slice_type, H264SliceType::I),
        );
        let decode_device = DecodeDevice {
            device: &self.device_guard.device,
            queue: self.queue,
            queue_family_index: self.queue_family_index,
        };
        let params = RecordParamsH264 {
            command_buffer: session.command_buffer,
            coded_extent: session.coded_extent,
            bitstream_len: aligned_len,
            dst_slot_index,
            reference_slots: &reference_slots,
            picture_info: &picture_info,
        };
        record_and_submit_h264(&decode_device, &mut session.resources, &params).map_err(map_err)?;

        if is_reference {
            let frame_num_i32 = i32::try_from(slice.frame_num).unwrap_or(0);
            session
                .dpb
                .insert(
                    dst_slot_index as usize,
                    DpbSlot::new_reference(slice.frame_num, frame_num_i32, pic_order_cnt),
                )
                .map_err(|e| map_err(e.into()))?;
        }

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

/// Scans `data` (assumed Annex-B) for the first SPS/PPS, ignoring parse
/// failures (a caller may legitimately pass non-Annex-B or empty
/// `extra_data` — see [`VulkanVideoDecoder::open`]'s doc).
fn scan_parameter_sets(data: &[u8], sps: &mut Option<H264Sps>, pps: &mut Option<H264Pps>) {
    if data.is_empty() {
        return;
    }
    let Ok(units) = split_annex_b(data) else {
        return;
    };
    for unit in units {
        let Ok(nal) = NalUnit::parse(unit) else {
            continue;
        };
        match nal.unit_type {
            NalUnitType::Sps => {
                if let Ok(parsed) = H264Sps::parse(&nal.rbsp) {
                    *sps = Some(parsed);
                }
            }
            NalUnitType::Pps => {
                if let Ok(parsed) = H264Pps::parse(&nal.rbsp) {
                    *pps = Some(parsed);
                }
            }
            _ => {}
        }
    }
}

impl VideoDecoder for VulkanVideoDecoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        if self.flushed {
            return Err(DecodeError::Closed);
        }
        if self.codec == CodecKind::Hevc {
            return crate::vulkan::decoder_hevc::push_packet_hevc(self, packet);
        }
        let units = split_annex_b(&packet.payload).map_err(|_| DecodeError::InvalidInput)?;
        for unit in units {
            let nal = NalUnit::parse(unit).map_err(|_| DecodeError::InvalidInput)?;
            match nal.unit_type {
                NalUnitType::Sps => {
                    let sps = H264Sps::parse(&nal.rbsp).map_err(|e| map_err(e.into()))?;
                    if self.session.is_none() {
                        if let Some(pps) = self.pending_pps.take() {
                            self.session = Some(DecodedSession::H264(
                                self.build_session_h264(sps, pps).map_err(map_err)?,
                            ));
                        } else {
                            self.pending_sps = Some(sps);
                        }
                    }
                }
                NalUnitType::Pps => {
                    let pps = H264Pps::parse(&nal.rbsp).map_err(|e| map_err(e.into()))?;
                    if self.session.is_none() {
                        if let Some(sps) = self.pending_sps.take() {
                            self.session = Some(DecodedSession::H264(
                                self.build_session_h264(sps, pps).map_err(map_err)?,
                            ));
                        } else {
                            self.pending_pps = Some(pps);
                        }
                    }
                }
                NalUnitType::IdrSlice | NalUnitType::NonIdrSlice => {
                    self.decode_slice_h264(&nal, unit)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        // Every pushed picture is decoded and drained synchronously — no
        // pipeline depth to flush.
        self.flushed = true;
        Ok(())
    }
}

impl Drop for VulkanVideoDecoder {
    fn drop(&mut self) {
        // SAFETY: `decode_slice_h264`/`decode_slice_hevc`/`build_session_h264`/
        // `build_session_hevc` always wait on `resources.fence` synchronously
        // before returning, so no GPU work is outstanding here.
        match &self.session {
            Some(DecodedSession::H264(session)) => {
                session.resources.destroy(&self.device_guard.device);
            }
            Some(DecodedSession::Hevc(session)) => {
                session.resources.destroy(&self.device_guard.device);
            }
            None => {}
        }
    }
}

fn stream_info_from(config: &VideoDecoderConfig) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: config.codec,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: config.width,
            height: config.height,
        },
        extra_data: config.extra_data.clone(), // clone: own StreamInfo copy, independent of caller's VideoDecoderConfig lifetime
    }
}

/// Maps this crate's raw Vulkan-call error type to the facade's codec-agnostic
/// [`DecodeError`].
#[allow(
    clippy::needless_pass_by_value,
    reason = "by-value lets every `?`-using call site pass this directly as \
              `.map_err(map_err)` instead of a `.map_err(|e| map_err(&e))` closure, matching \
              mediaway-encoder-vulkan::encoder's identical map_err shape"
)]
#[allow(
    clippy::redundant_pub_crate,
    reason = "conflicts with the workspace's own unreachable_pub lint (also enabled) — pub(crate) \
              here is deliberate: `mod decoder` is private, but decoder_hevc.rs (a sibling module) \
              still needs this function"
)]
pub(crate) fn map_err(err: VulkanDecodeError) -> DecodeError {
    match err {
        VulkanDecodeError::Loader(_)
        | VulkanDecodeError::CreateInstance(_)
        | VulkanDecodeError::EnumeratePhysicalDevices(_)
        | VulkanDecodeError::NoDecodeCapableDevice
        | VulkanDecodeError::NoVideoFormat { .. }
        | VulkanDecodeError::SeparateReferenceImagesRequired => DecodeError::Unsupported,
        VulkanDecodeError::UnsupportedResolution { .. }
        | VulkanDecodeError::Bitstream(_)
        | VulkanDecodeError::HevcBitstream(_)
        | VulkanDecodeError::MissingParameterSet { .. } => DecodeError::InvalidInput,
        VulkanDecodeError::VkCall { .. } | VulkanDecodeError::NoMemoryType { .. } => {
            DecodeError::Backend
        }
        VulkanDecodeError::Dpb(_) => DecodeError::Backend,
    }
}
