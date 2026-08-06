//! Per-frame D3D12 recording for AV1 — mirrors [`super::ops_hevc`]'s HEVC `encode_frame`,
//! but building the AV1-flavored `D3D12_VIDEO_ENCODER_AV1_SEQUENCE_STRUCTURE`/
//! `_PICTURE_CONTROL_CODEC_DATA`/`_PROFILE_DESC` union payloads. `upload_and_copy` (NV12
//! CPU→GPU staging, in [`super::ops`]) is codec-agnostic and shared; **unlike** H.264/HEVC,
//! AV1's packet readback ([`D3d12VideoEncoder::read_packet_av1`]) is NOT the shared
//! `ops::read_packet` — AV1's `OBU_FRAME` needs a per-frame `leb128` size field (the
//! driver's compressed byte count varies frame to frame), unlike H.264/HEVC's
//! start-code-delimited NALs which need no length prefix at all.

use crate::EncodeError;
use mediaway_common::{Bytes, Packet};
use std::mem::{ManuallyDrop, size_of};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_VIDEO_ENCODE_READ,
    D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE, ID3D12CommandList,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_RATIONAL};
use windows::Win32::Media::MediaFoundation::{
    D3D12_VIDEO_ENCODE_REFERENCE_FRAMES, D3D12_VIDEO_ENCODER_AV1_CDEF_CONFIG,
    D3D12_VIDEO_ENCODER_AV1_COMP_PREDICTION_TYPE_SINGLE_REFERENCE,
    D3D12_VIDEO_ENCODER_AV1_FRAME_TYPE_KEY_FRAME,
    D3D12_VIDEO_ENCODER_AV1_INTERPOLATION_FILTERS_EIGHTTAP,
    D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_CODEC_DATA,
    D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_FLAG_DISABLE_CDF_UPDATE,
    D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_FLAG_DISABLE_FRAME_END_UPDATE_CDF,
    D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_FLAG_ENABLE_FRAME_SEGMENTATION_AUTO,
    D3D12_VIDEO_ENCODER_AV1_PROFILE, D3D12_VIDEO_ENCODER_AV1_PROFILE_MAIN,
    D3D12_VIDEO_ENCODER_AV1_REFERENCE_PICTURE_DESCRIPTOR,
    D3D12_VIDEO_ENCODER_AV1_RESTORATION_CONFIG,
    D3D12_VIDEO_ENCODER_AV1_RESTORATION_TILESIZE_DISABLED,
    D3D12_VIDEO_ENCODER_AV1_RESTORATION_TYPE_DISABLED, D3D12_VIDEO_ENCODER_AV1_SEGMENTATION_CONFIG,
    D3D12_VIDEO_ENCODER_AV1_SEGMENTATION_MAP, D3D12_VIDEO_ENCODER_AV1_SEQUENCE_STRUCTURE,
    D3D12_VIDEO_ENCODER_AV1_TX_MODE_LARGEST, D3D12_VIDEO_ENCODER_CODEC_AV1,
    D3D12_VIDEO_ENCODER_CODEC_AV1_LOOP_FILTER_CONFIG,
    D3D12_VIDEO_ENCODER_CODEC_AV1_LOOP_FILTER_DELTA_CONFIG,
    D3D12_VIDEO_ENCODER_CODEC_AV1_QUANTIZATION_CONFIG,
    D3D12_VIDEO_ENCODER_CODEC_AV1_QUANTIZATION_DELTA_CONFIG,
    D3D12_VIDEO_ENCODER_COMPRESSED_BITSTREAM, D3D12_VIDEO_ENCODER_ENCODE_OPERATION_METADATA_BUFFER,
    D3D12_VIDEO_ENCODER_ENCODEFRAME_INPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_ENCODEFRAME_OUTPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_FRAME_SUBREGION_LAYOUT_MODE_FULL_FRAME,
    D3D12_VIDEO_ENCODER_FRAME_SUBREGION_METADATA, D3D12_VIDEO_ENCODER_INTRA_REFRESH,
    D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE, D3D12_VIDEO_ENCODER_OUTPUT_METADATA,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_0, D3D12_VIDEO_ENCODER_PICTURE_CONTROL_DESC,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_FLAG_NONE,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_SUBREGIONS_LAYOUT_DATA,
    D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC, D3D12_VIDEO_ENCODER_PROFILE_DESC,
    D3D12_VIDEO_ENCODER_PROFILE_DESC_0, D3D12_VIDEO_ENCODER_RATE_CONTROL,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_FLAG_NONE, D3D12_VIDEO_ENCODER_RECONSTRUCTED_PICTURE,
    D3D12_VIDEO_ENCODER_RESOLVE_METADATA_INPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_RESOLVE_METADATA_OUTPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_DESC, D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_FLAG_NONE,
    D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE, D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_0,
};
use windows::core::Interface;

use super::D3d12VideoEncoder;
use super::FIXED_QP_AV1;
use super::bitstream_av1;
use super::util::{borrow_resource, data_size, signal_and_wait, transition_barrier};

/// `D3D12_VIDEO_ENCODER_AV1_INVALID_DPB_RESOURCE_INDEX` (spec § 4.1.14) — a plain `#define`,
/// not part of an enum, so `windows` has no binding for it. Every unused
/// `ReferenceFramesReconPictureDescriptors` slot must carry this value, not `0`: `0` is a
/// *valid* index into `D3D12_VIDEO_ENCODER_PICTURE_CONTROL_DESC.ReferenceFrames`, so the
/// zeroed `Default` this backend used before real-hardware `EncodeFrame` was ever reached
/// (blocked upstream at `CheckFeatureSupport`, see `av1.rs`'s module doc) told the driver
/// every key frame referenced a slot 0 that never existed — confirmed via the debug layer:
/// "AV1 Picture control structure - Key Frames must not use any references."
const AV1_INVALID_DPB_RESOURCE_INDEX: u32 = 0xFF;

impl D3d12VideoEncoder {
    #[allow(
        clippy::too_many_lines,
        reason = "one EncodeFrame call needs many populated D3D12 structs; mirrors ops_hevc::encode_frame_hevc's sibling"
    )]
    pub(super) fn encode_frame_av1(
        &mut self,
        pts: i64,
        duration: u64,
        mut gop: D3D12_VIDEO_ENCODER_AV1_SEQUENCE_STRUCTURE,
    ) -> Result<Packet, EncodeError> {
        // SAFETY: freshly created/idle allocator+list (fully synchronous per frame).
        unsafe {
            self.encode_allocator
                .Reset()
                .map_err(|_| EncodeError::Backend)?;
            self.encode_list
                .Reset(&self.encode_allocator)
                .map_err(|_| EncodeError::Backend)?;
        }

        let barriers_before = [
            transition_barrier(
                &self.input_texture,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_READ,
            ),
            transition_barrier(
                &self.bitstream_buffer,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
            ),
            transition_barrier(
                &self.metadata_buffer,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
            ),
            transition_barrier(
                &self.resolved_metadata_buffer,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
            ),
        ];
        // SAFETY: `VIDEO_ENCODE_READ`/`WRITE` transitions are only valid recorded on a
        // video-encode command list — this is one.
        unsafe { self.encode_list.ResourceBarrier(&barriers_before) };

        let resolution = D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC {
            Width: self.width,
            Height: self.height,
        };

        let gop_desc = D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_AV1_SEQUENCE_STRUCTURE>(),
            Anonymous: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_0 {
                pAV1SequenceStructure: &raw mut gop,
            },
        };

        let rate_control_state = self.rate_control;
        let (rate_control_mode, rate_control_params) =
            super::setup::rate_control_mode_and_params(&rate_control_state);
        let rc = D3D12_VIDEO_ENCODER_RATE_CONTROL {
            Mode: rate_control_mode,
            Flags: D3D12_VIDEO_ENCODER_RATE_CONTROL_FLAG_NONE,
            ConfigParams: rate_control_params,
            TargetFrameRate: DXGI_RATIONAL {
                Numerator: self.fps_num,
                Denominator: self.fps_den,
            },
        };

        // Every field here must match what `super::bitstream_av1::write_frame_header`
        // writes into the bitstream for this same frame — see that function's doc comment.
        let mut pic_data = D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_CODEC_DATA {
            // ENABLE_FRAME_SEGMENTATION_AUTO is mandatory on this driver — set on every
            // frame, not just declared at the session level, per a real-hardware debug-
            // layer message (see `av1.rs::default_codec_config_av1`'s doc). The driver
            // decides `segmentation_params()` itself; `CustomSegmentation`/`SegmentsMap`
            // below stay zeroed (`CUSTOM`-only fields, unused in `AUTO` mode).
            Flags: D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_FLAG_DISABLE_CDF_UPDATE
                | D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_FLAG_DISABLE_FRAME_END_UPDATE_CDF
                | D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_FLAG_ENABLE_FRAME_SEGMENTATION_AUTO,
            FrameType: D3D12_VIDEO_ENCODER_AV1_FRAME_TYPE_KEY_FRAME,
            CompoundPredictionType: D3D12_VIDEO_ENCODER_AV1_COMP_PREDICTION_TYPE_SINGLE_REFERENCE,
            InterpolationFilter: D3D12_VIDEO_ENCODER_AV1_INTERPOLATION_FILTERS_EIGHTTAP,
            FrameRestorationConfig: D3D12_VIDEO_ENCODER_AV1_RESTORATION_CONFIG {
                FrameRestorationType: [D3D12_VIDEO_ENCODER_AV1_RESTORATION_TYPE_DISABLED; 3],
                LoopRestorationPixelSize: [D3D12_VIDEO_ENCODER_AV1_RESTORATION_TILESIZE_DISABLED;
                    3],
            },
            TxMode: D3D12_VIDEO_ENCODER_AV1_TX_MODE_LARGEST,
            SuperResDenominator: 8, // SUPERRES_NUM — matches bitstream_av1's inferred SuperresDenom
            OrderHint: 0,
            PictureIndex: self.frame_counter,
            TemporalLayerIndexPlus1: 1,
            SpatialLayerIndexPlus1: 1,
            ReferenceFramesReconPictureDescriptors:
                [D3D12_VIDEO_ENCODER_AV1_REFERENCE_PICTURE_DESCRIPTOR {
                    ReconstructedPictureResourceIndex: AV1_INVALID_DPB_RESOURCE_INDEX,
                    ..Default::default()
                }; 8],
            ReferenceIndices: [0; 7],
            PrimaryRefFrame: 7,      // PRIMARY_REF_NONE
            RefreshFrameFlags: 0xFF, // allFrames — every frame is a shown key frame
            LoopFilter: D3D12_VIDEO_ENCODER_CODEC_AV1_LOOP_FILTER_CONFIG::default(), // all-zero: deblocking disabled
            LoopFilterDelta: D3D12_VIDEO_ENCODER_CODEC_AV1_LOOP_FILTER_DELTA_CONFIG::default(),
            Quantization: D3D12_VIDEO_ENCODER_CODEC_AV1_QUANTIZATION_CONFIG {
                BaseQIndex: u64::from(FIXED_QP_AV1),
                ..Default::default()
            },
            QuantizationDelta: D3D12_VIDEO_ENCODER_CODEC_AV1_QUANTIZATION_DELTA_CONFIG::default(),
            CDEF: D3D12_VIDEO_ENCODER_AV1_CDEF_CONFIG::default(), // enable_cdef == 0 in the sequence header — unused
            QPMapValuesCount: 0,
            pRateControlQPMap: std::ptr::null_mut(),
            CustomSegmentation: D3D12_VIDEO_ENCODER_AV1_SEGMENTATION_CONFIG::default(),
            CustomSegmentsMap: D3D12_VIDEO_ENCODER_AV1_SEGMENTATION_MAP::default(),
        };
        self.frame_counter = self.frame_counter.wrapping_add(1);

        let pic_control_data = D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_CODEC_DATA>(),
            Anonymous: D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_0 {
                pAV1PicData: &raw mut pic_data,
            },
        };

        let input_args = D3D12_VIDEO_ENCODER_ENCODEFRAME_INPUT_ARGUMENTS {
            SequenceControlDesc: D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_DESC {
                Flags: D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_FLAG_NONE,
                IntraRefreshConfig: D3D12_VIDEO_ENCODER_INTRA_REFRESH {
                    Mode: D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE,
                    IntraRefreshDuration: 0,
                },
                RateControl: rc,
                PictureTargetResolution: resolution,
                SelectedLayoutMode: D3D12_VIDEO_ENCODER_FRAME_SUBREGION_LAYOUT_MODE_FULL_FRAME,
                FrameSubregionsLayoutData:
                    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_SUBREGIONS_LAYOUT_DATA::default(),
                CodecGopSequence: gop_desc,
            },
            PictureControlDesc: D3D12_VIDEO_ENCODER_PICTURE_CONTROL_DESC {
                IntraRefreshFrameIndex: 0,
                Flags: D3D12_VIDEO_ENCODER_PICTURE_CONTROL_FLAG_NONE,
                PictureControlCodecData: pic_control_data,
                ReferenceFrames: D3D12_VIDEO_ENCODE_REFERENCE_FRAMES {
                    NumTexture2Ds: 0,
                    ppTexture2Ds: std::ptr::null_mut(),
                    pSubresources: std::ptr::null_mut(),
                },
            },
            pInputFrame: borrow_resource(&self.input_texture),
            InputFrameSubresource: 0,
            CurrentFrameBitstreamMetadataSize: u32::try_from(self.header_len_aligned)
                .unwrap_or(u32::MAX),
        };

        let output_args = D3D12_VIDEO_ENCODER_ENCODEFRAME_OUTPUT_ARGUMENTS {
            Bitstream: D3D12_VIDEO_ENCODER_COMPRESSED_BITSTREAM {
                pBuffer: borrow_resource(&self.bitstream_buffer),
                FrameStartOffset: self.header_len_aligned,
            },
            ReconstructedPicture: D3D12_VIDEO_ENCODER_RECONSTRUCTED_PICTURE {
                pReconstructedPicture: ManuallyDrop::new(None),
                ReconstructedPictureSubresource: 0,
            },
            EncoderOutputMetadata: D3D12_VIDEO_ENCODER_ENCODE_OPERATION_METADATA_BUFFER {
                pBuffer: borrow_resource(&self.metadata_buffer),
                Offset: 0,
            },
        };

        // SAFETY: encoder/heap were created for this exact codec/profile/resolution;
        // buffers were sized from `D3D12_FEATURE_VIDEO_ENCODER_RESOURCE_REQUIREMENTS` at
        // `open`. No reference frames — `ReconstructedPicture` is `None` per the API's
        // documented "only needed if used as a reference" contract.
        unsafe {
            self.encode_list.EncodeFrame(
                &self.encoder,
                &self.encoder_heap,
                &raw const input_args,
                &raw const output_args,
            );
        }

        let metadata_write_to_read = transition_barrier(
            &self.metadata_buffer,
            D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
            D3D12_RESOURCE_STATE_VIDEO_ENCODE_READ,
        );
        // SAFETY: same video-encode command list as the `EncodeFrame` call above.
        unsafe { self.encode_list.ResourceBarrier(&[metadata_write_to_read]) };

        let mut profile_av1: D3D12_VIDEO_ENCODER_AV1_PROFILE = D3D12_VIDEO_ENCODER_AV1_PROFILE_MAIN;
        let profile_desc = D3D12_VIDEO_ENCODER_PROFILE_DESC {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_AV1_PROFILE>(),
            Anonymous: D3D12_VIDEO_ENCODER_PROFILE_DESC_0 {
                pAV1Profile: &raw mut profile_av1,
            },
        };
        let input_metadata = D3D12_VIDEO_ENCODER_RESOLVE_METADATA_INPUT_ARGUMENTS {
            EncoderCodec: D3D12_VIDEO_ENCODER_CODEC_AV1,
            EncoderProfile: profile_desc,
            EncoderInputFormat: DXGI_FORMAT_NV12,
            EncodedPictureEffectiveResolution: resolution,
            HWLayoutMetadata: D3D12_VIDEO_ENCODER_ENCODE_OPERATION_METADATA_BUFFER {
                pBuffer: borrow_resource(&self.metadata_buffer),
                Offset: 0,
            },
        };
        let output_metadata = D3D12_VIDEO_ENCODER_RESOLVE_METADATA_OUTPUT_ARGUMENTS {
            ResolvedLayoutMetadata: D3D12_VIDEO_ENCODER_ENCODE_OPERATION_METADATA_BUFFER {
                pBuffer: borrow_resource(&self.resolved_metadata_buffer),
                Offset: 0,
            },
        };
        // SAFETY: same video-encode command list; `HWLayoutMetadata` was just transitioned
        // to `VIDEO_ENCODE_READ` above.
        unsafe {
            self.encode_list.ResolveEncoderOutputMetadata(
                &raw const input_metadata,
                &raw const output_metadata,
            );
        }

        let barriers_after = [
            transition_barrier(
                &self.input_texture,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_READ,
                D3D12_RESOURCE_STATE_COMMON,
            ),
            transition_barrier(
                &self.bitstream_buffer,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
                D3D12_RESOURCE_STATE_COMMON,
            ),
            transition_barrier(
                &self.metadata_buffer,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_READ,
                D3D12_RESOURCE_STATE_COMMON,
            ),
            transition_barrier(
                &self.resolved_metadata_buffer,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
                D3D12_RESOURCE_STATE_COMMON,
            ),
        ];
        // SAFETY: same video-encode command list as every barrier/call above.
        unsafe { self.encode_list.ResourceBarrier(&barriers_after) };

        // SAFETY: matched Reset/Close pair on this list.
        unsafe { self.encode_list.Close() }.map_err(|_| EncodeError::Backend)?;
        let generic: ID3D12CommandList =
            self.encode_list.cast().map_err(|_| EncodeError::Backend)?;
        // SAFETY: `generic` is the just-closed `encode_list`, valid for this VIDEO_ENCODE queue.
        unsafe { self.encode_queue.ExecuteCommandLists(&[Some(generic)]) };
        signal_and_wait(
            &self.encode_queue,
            &self.fence,
            self.fence_event,
            &mut self.fence_value,
        )?;

        self.read_packet_av1(pts, duration)
    }

    /// AV1-specific packet readback — **not** the shared [`super::ops::D3d12VideoEncoder::read_packet`]:
    /// wraps the driver's compressed tile bytes in an `OBU_FRAME` (frame header + tile
    /// group, AV1 spec §5.10.1) with a per-frame `leb128` `obu_size`, appended after the
    /// fixed per-session temporal-delimiter + sequence-header OBU prefix.
    pub(super) fn read_packet_av1(
        &mut self,
        pts: i64,
        duration: u64,
    ) -> Result<Packet, EncodeError> {
        let mut resolved_ptr: *mut u8 = std::ptr::null_mut();
        // SAFETY: READBACK-heap resource; CPU `Map` is valid regardless of the GPU-side
        // `D3D12_RESOURCE_STATES` the last-recorded command list left it in.
        unsafe {
            self.resolved_metadata_buffer
                .Map(0, None, Some(std::ptr::from_mut(&mut resolved_ptr).cast()))
                .map_err(|_| EncodeError::Backend)?;
        }
        if resolved_ptr.is_null() {
            return Err(EncodeError::Backend);
        }
        // SAFETY: buffer was sized >= `size_of::<D3D12_VIDEO_ENCODER_OUTPUT_METADATA>()
        // + size_of::<D3D12_VIDEO_ENCODER_FRAME_SUBREGION_METADATA>()` at `open`; the
        // driver writes the base struct at offset 0 and (this backend's `FULL_FRAME`
        // layout always yields exactly one subregion) one subregion entry immediately
        // after it during `ResolveEncoderOutputMetadata`.
        let meta = unsafe {
            resolved_ptr
                .cast::<D3D12_VIDEO_ENCODER_OUTPUT_METADATA>()
                .read_unaligned()
        };
        // SAFETY: same buffer, `size_of::<D3D12_VIDEO_ENCODER_OUTPUT_METADATA>()` bytes in
        // — within bounds per the `open`-time sizing referenced above.
        let subregion = unsafe {
            resolved_ptr
                .add(size_of::<D3D12_VIDEO_ENCODER_OUTPUT_METADATA>())
                .cast::<D3D12_VIDEO_ENCODER_FRAME_SUBREGION_METADATA>()
                .read_unaligned()
        };
        // SAFETY: matches the `Map` above.
        unsafe { self.resolved_metadata_buffer.Unmap(0, None) };

        // `D3D12_VIDEO_ENCODER_ENCODE_ERROR_FLAG_NO_ERROR` is `0` — any nonzero flag is an error.
        if meta.EncodeErrorFlags != 0 {
            return Err(EncodeError::Backend);
        }
        // This backend always requests `FULL_FRAME` subregion layout — exactly one entry.
        if meta.WrittenSubregionsCount != 1 {
            return Err(EncodeError::Backend);
        }
        // Per the official D3D12 spec ("Resolved buffer layouts for
        // ResolveEncoderOutputMetadata", `D3D12_VIDEO_ENCODER_FRAME_SUBREGION_METADATA`):
        // `bSize` includes `bStartOffset` bytes of *leading padding* before the real
        // coded tile data begins — the actual tile payload is `[bStartOffset, bSize)`
        // within this subregion, not `[0, EncodedBitstreamWrittenBytesCount)` as this
        // backend previously (wrongly) assumed by reading the whole written range
        // verbatim. A real, previously-undiscovered gap, kept as a correctness fix even
        // though real-hardware verification (ADR-0007's AV1 addenda) found
        // `bStartOffset == 0` on this crate's reference RTX 4090 — behaviorally
        // equivalent to the old code *on this driver*, but not guaranteed on others,
        // and this is the spec-correct extraction regardless. Ruled out as this
        // backend's still-open decodability bug on this hardware.
        let tile_size = subregion
            .bSize
            .checked_sub(subregion.bStartOffset)
            .ok_or(EncodeError::Backend)?;
        if tile_size == 0 {
            return Err(EncodeError::Backend);
        }
        let tile_len_usize = usize::try_from(tile_size).map_err(|_| EncodeError::Backend)?;
        let base_offset_usize =
            usize::try_from(self.header_len_aligned).map_err(|_| EncodeError::Backend)?;
        let start_offset_usize =
            usize::try_from(subregion.bStartOffset).map_err(|_| EncodeError::Backend)?;
        let tile_start_usize = base_offset_usize
            .checked_add(start_offset_usize)
            .ok_or(EncodeError::Backend)?;
        let end = tile_start_usize
            .checked_add(tile_len_usize)
            .ok_or(EncodeError::Backend)?;
        if end as u64 > self.bitstream_capacity {
            return Err(EncodeError::Backend);
        }

        let mut payload = self.header_bytes.clone(); // clone: own Packet payload built from the persistent per-session TD+SeqHdr OBU bytes
        let obu_payload_len = self.av1_frame_header_bytes.len() + tile_len_usize;
        payload.push(bitstream_av1::obu_header_byte(bitstream_av1::OBU_FRAME));
        bitstream_av1::write_leb128(&mut payload, obu_payload_len as u64);
        payload.extend_from_slice(&self.av1_frame_header_bytes);

        let mut slice_ptr: *mut u8 = std::ptr::null_mut();
        // SAFETY: READBACK-heap resource, same reasoning as the metadata `Map` above.
        unsafe {
            self.bitstream_buffer
                .Map(0, None, Some(std::ptr::from_mut(&mut slice_ptr).cast()))
                .map_err(|_| EncodeError::Backend)?;
        }
        if slice_ptr.is_null() {
            // SAFETY: matches the `Map` immediately above.
            unsafe { self.bitstream_buffer.Unmap(0, None) };
            return Err(EncodeError::Backend);
        }
        // SAFETY: `tile_start_usize + tile_len_usize <= bitstream_capacity`, checked
        // above; the buffer was committed with that capacity at `open`.
        unsafe {
            let slice = std::slice::from_raw_parts(slice_ptr.add(tile_start_usize), tile_len_usize);
            payload.extend_from_slice(slice);
        }
        // SAFETY: matches the `Map` above.
        unsafe { self.bitstream_buffer.Unmap(0, None) };

        Ok(Packet {
            stream_id: self.info.id(),
            pts,
            dts: pts,
            duration,
            is_keyframe: true,
            is_discard: false,
            payload: Bytes::from(payload),
        })
    }
}
