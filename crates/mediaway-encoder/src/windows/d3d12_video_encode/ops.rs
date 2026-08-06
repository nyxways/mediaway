//! Per-frame D3D12 recording: CPU→GPU NV12 upload+copy, `EncodeFrame` submission, and
//! compressed-bitstream readback. Split out of [`super`] to keep files under the
//! 1000-line source limit — see [`super::D3d12VideoEncoder`] for the owning struct.

use crate::EncodeError;
use mediaway_common::{Bytes, Packet};
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_DEST,
    D3D12_RESOURCE_STATE_VIDEO_ENCODE_READ, D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
    ID3D12CommandList, ID3D12PipelineState, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_NV12, DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8_UNORM, DXGI_RATIONAL,
};
use windows::Win32::Media::MediaFoundation::{
    D3D12_VIDEO_ENCODE_REFERENCE_FRAMES, D3D12_VIDEO_ENCODER_CODEC_H264,
    D3D12_VIDEO_ENCODER_COMPRESSED_BITSTREAM, D3D12_VIDEO_ENCODER_ENCODE_OPERATION_METADATA_BUFFER,
    D3D12_VIDEO_ENCODER_ENCODEFRAME_INPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_ENCODEFRAME_OUTPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_FRAME_SUBREGION_LAYOUT_MODE_FULL_FRAME,
    D3D12_VIDEO_ENCODER_FRAME_TYPE_H264_IDR_FRAME, D3D12_VIDEO_ENCODER_FRAME_TYPE_H264_P_FRAME,
    D3D12_VIDEO_ENCODER_INTRA_REFRESH, D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE,
    D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_ROW_BASED, D3D12_VIDEO_ENCODER_OUTPUT_METADATA,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_0,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_H264,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_H264_FLAG_NONE,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_DESC, D3D12_VIDEO_ENCODER_PICTURE_CONTROL_FLAG_NONE,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_FLAG_USED_AS_REFERENCE_PICTURE,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_SUBREGIONS_LAYOUT_DATA,
    D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC, D3D12_VIDEO_ENCODER_PROFILE_DESC,
    D3D12_VIDEO_ENCODER_PROFILE_DESC_0, D3D12_VIDEO_ENCODER_PROFILE_H264,
    D3D12_VIDEO_ENCODER_PROFILE_H264_MAIN, D3D12_VIDEO_ENCODER_RATE_CONTROL,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_FLAG_NONE, D3D12_VIDEO_ENCODER_RECONSTRUCTED_PICTURE,
    D3D12_VIDEO_ENCODER_REFERENCE_PICTURE_DESCRIPTOR_H264,
    D3D12_VIDEO_ENCODER_RESOLVE_METADATA_INPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_RESOLVE_METADATA_OUTPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_DESC, D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_FLAG_NONE,
    D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_FLAG_REQUEST_INTRA_REFRESH,
    D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE, D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_0,
    D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264,
};

use windows::core::{BOOL, Interface};

use super::D3d12VideoEncoder;
use super::util::{
    borrow_resource, data_size, placed_footprint_copy_location, signal_and_wait,
    subresource_copy_location, transition_barrier, transition_barrier_subresource,
    write_nv12_upload,
};

impl D3d12VideoEncoder {
    pub(super) fn upload_and_copy(&mut self, nv12: &[u8]) -> Result<(), EncodeError> {
        write_nv12_upload(
            &self.upload_buffer,
            nv12,
            self.width,
            self.height,
            self.row_pitch,
            self.luma_size,
        )?;

        // SAFETY: freshly created/idle allocator+list (fully synchronous per frame).
        unsafe {
            self.copy_allocator
                .Reset()
                .map_err(|_| EncodeError::Backend)?;
            self.copy_list
                .Reset(&self.copy_allocator, None::<&ID3D12PipelineState>)
                .map_err(|_| EncodeError::Backend)?;
        }

        let to_copy_dest = transition_barrier(
            &self.input_texture,
            D3D12_RESOURCE_STATE_COMMON,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        // SAFETY: recording on a freshly reset COPY-type command list.
        unsafe { self.copy_list.ResourceBarrier(&[to_copy_dest]) };

        let luma_dst = subresource_copy_location(&self.input_texture, 0);
        let luma_src = placed_footprint_copy_location(
            &self.upload_buffer,
            0,
            DXGI_FORMAT_R8_UNORM,
            self.width,
            self.height,
            self.row_pitch,
        );
        // SAFETY: `input_texture` is COPY_DEST, `upload_buffer` is GENERIC_READ.
        unsafe {
            self.copy_list.CopyTextureRegion(
                &raw const luma_dst,
                0,
                0,
                0,
                &raw const luma_src,
                None,
            );
        }

        let uv_dst = subresource_copy_location(&self.input_texture, 1);
        let uv_src = placed_footprint_copy_location(
            &self.upload_buffer,
            self.luma_size,
            DXGI_FORMAT_R8G8_UNORM,
            self.width / 2,
            self.height / 2,
            self.row_pitch,
        );
        // SAFETY: same resources/states as the luma copy above.
        unsafe {
            self.copy_list
                .CopyTextureRegion(&raw const uv_dst, 0, 0, 0, &raw const uv_src, None);
        }

        let to_common = transition_barrier(
            &self.input_texture,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATE_COMMON,
        );
        // SAFETY: recording on the same COPY-type command list.
        unsafe { self.copy_list.ResourceBarrier(&[to_common]) };

        // SAFETY: matched Reset/Close pair on this list.
        unsafe { self.copy_list.Close() }.map_err(|_| EncodeError::Backend)?;
        let generic: ID3D12CommandList = self.copy_list.cast().map_err(|_| EncodeError::Backend)?;
        // SAFETY: `generic` is the just-closed `copy_list`, valid for this COPY queue.
        unsafe { self.copy_queue.ExecuteCommandLists(&[Some(generic)]) };
        signal_and_wait(
            &self.copy_queue,
            &self.fence,
            self.fence_event,
            &mut self.fence_value,
        )
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one EncodeFrame call needs many populated D3D12 structs; splitting further fragments one atomic recording sequence"
    )]
    pub(super) fn encode_frame_h264(
        &mut self,
        pts: i64,
        duration: u64,
        mut gop: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264,
        decision: Option<super::gop::FrameDecision>,
    ) -> Result<Packet, EncodeError> {
        // `decision` is `None` outside GOP mode (`self.recon_pool`/`self.h264_gop_state`
        // both `None`) — defaults reproduce this backend's pre-GOP-support IDR-only
        // behavior exactly (`is_idr: true, frame_num: 0, poc: 0`).
        let is_idr = decision.is_none_or(|d| d.is_idr);
        let poc = decision.map_or(0, |d| d.poc);
        // `Some(i)` on every frame of an intra-refresh session (never on that
        // session's own startup IDR) — see `gop.rs`'s `FrameDecision` doc.
        let intra_refresh_frame_index = decision.and_then(|d| d.intra_refresh_frame_index);
        // A P frame's single reference is always the immediately preceding frame's
        // reconstructed picture — see `gop.rs`'s module doc for why no separate
        // capability check is needed beyond `!is_idr`.
        let has_reference = !is_idr && self.recon_pool.is_some();
        let write_slot = self.recon_pool.as_ref().map(|pool| pool.write_slot);
        let read_slot = write_slot.map(|w| 1 - w);
        // Every GOP-mode frame becomes the *next* frame's single reference, so every
        // one (IDR and P alike) must be marked `USED_AS_REFERENCE_PICTURE` — per the
        // D3D12 spec (`local/standards/d3d12-video-encoding-h264-hevc/`), this flag is
        // what actually tells the driver to populate `ReconstructedPicture`; providing
        // that output resource without the flag is an undefined/illegal combination.
        let picture_control_flags = if self.recon_pool.is_some() {
            D3D12_VIDEO_ENCODER_PICTURE_CONTROL_FLAG_USED_AS_REFERENCE_PICTURE
        } else {
            D3D12_VIDEO_ENCODER_PICTURE_CONTROL_FLAG_NONE
        };

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

        // Recon-pool slots (array slices of one texture-array resource, see
        // `setup::ReconPool`) always rest at `COMMON` between calls — the write slot
        // moves to `VIDEO_ENCODE_WRITE` (the driver writes this frame's reconstructed
        // picture there), and, for a P frame, the read slot moves to
        // `VIDEO_ENCODE_READ` (it holds the previous frame's reconstructed picture,
        // this frame's only reference). Per-subresource barriers, not
        // `transition_barrier`'s all-subresources shape — the two slices are
        // independently in different states at once.
        if let (Some(pool), Some(write)) = (&self.recon_pool, write_slot) {
            let mut recon_barriers_before = vec![transition_barrier_subresource(
                &pool.texture,
                write,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
            )];
            if has_reference {
                // `read_slot` is `Some` iff `write_slot` is (`has_reference` implies
                // `write_slot.is_some()`, see its computation above) — `write` is an
                // unreachable-in-practice fallback, never actually used.
                let read = read_slot.unwrap_or(write);
                recon_barriers_before.push(transition_barrier_subresource(
                    &pool.texture,
                    read,
                    D3D12_RESOURCE_STATE_COMMON,
                    D3D12_RESOURCE_STATE_VIDEO_ENCODE_READ,
                ));
            }
            // SAFETY: same video-encode command list as the barrier call above.
            unsafe { self.encode_list.ResourceBarrier(&recon_barriers_before) };
        }

        let resolution = D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC {
            Width: self.width,
            Height: self.height,
        };

        let gop_desc = D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264>(),
            Anonymous: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_0 {
                pH264GroupOfPictures: &raw mut gop,
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

        // Reference-frame wiring (P frame only). `ref_texture` holds a real,
        // refcount-bumped clone of the previous frame's reconstructed-picture
        // texture: `ppTexture2Ds` below is an "array of owned interface pointers"
        // ABI shape (plain `Option<ID3D12Resource>`, not `ManuallyDrop`-wrapped
        // like `borrow_resource`'s single-argument non-owning-duplicate lend), so
        // a real owned handle — released when `ref_texture` drops at the end of
        // this function — is the correct way to satisfy it.
        let mut ref_texture: [Option<ID3D12Resource>; 1] = [None];
        let mut ref_subresource = [0u32; 1];
        let mut ref_descriptor =
            [D3D12_VIDEO_ENCODER_REFERENCE_PICTURE_DESCRIPTOR_H264::default(); 1];
        let list0 = [0u32; 1];
        if let (true, Some(pool), Some(read), Some((prev_poc, prev_order))) = (
            has_reference,
            &self.recon_pool,
            read_slot,
            self.last_h264_reference,
        ) {
            // clone: COM AddRef — see the comment above `ref_texture`'s declaration;
            // this is an owned-array ABI shape, not a same-scope borrowed lend. Both
            // recon-pool slots are array slices of the *same* underlying resource
            // (see `setup::ReconPool`), so this clones that one shared texture;
            // `ref_subresource[0] = read` is what actually selects the reference slice.
            ref_texture[0] = Some(pool.texture.clone());
            ref_subresource[0] = read;
            ref_descriptor[0] = D3D12_VIDEO_ENCODER_REFERENCE_PICTURE_DESCRIPTOR_H264 {
                ReconstructedPictureResourceIndex: 0,
                IsLongTermReference: BOOL(0),
                LongTermPictureIdx: 0,
                PictureOrderCountNumber: prev_poc,
                FrameDecodingOrderNumber: prev_order,
                TemporalLayerIndex: 0,
            };
        }

        let mut pic_data = D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_H264 {
            Flags: D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_H264_FLAG_NONE,
            FrameType: if is_idr {
                D3D12_VIDEO_ENCODER_FRAME_TYPE_H264_IDR_FRAME
            } else {
                D3D12_VIDEO_ENCODER_FRAME_TYPE_H264_P_FRAME
            },
            pic_parameter_set_id: 0,
            idr_pic_id: self.frame_counter,
            PictureOrderCountNumber: poc,
            FrameDecodingOrderNumber: self.frame_decoding_order,
            TemporalLayerIndex: 0,
            List0ReferenceFramesCount: u32::from(has_reference),
            pList0ReferenceFrames: if has_reference {
                list0.as_ptr().cast_mut()
            } else {
                std::ptr::null_mut()
            },
            List1ReferenceFramesCount: 0,
            pList1ReferenceFrames: std::ptr::null_mut(),
            ReferenceFramesReconPictureDescriptorsCount: u32::from(has_reference),
            pReferenceFramesReconPictureDescriptors: if has_reference {
                ref_descriptor.as_mut_ptr()
            } else {
                std::ptr::null_mut()
            },
            adaptive_ref_pic_marking_mode_flag: 0,
            RefPicMarkingOperationsCommandsCount: 0,
            pRefPicMarkingOperationsCommands: std::ptr::null_mut(),
            List0RefPicModificationsCount: 0,
            pList0RefPicModifications: std::ptr::null_mut(),
            List1RefPicModificationsCount: 0,
            pList1RefPicModifications: std::ptr::null_mut(),
            QPMapValuesCount: 0,
            pRateControlQPMap: std::ptr::null_mut(),
        };
        self.frame_counter = self.frame_counter.wrapping_add(1);
        self.frame_decoding_order = self.frame_decoding_order.wrapping_add(1);

        let pic_control_data = D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_H264>(),
            Anonymous: D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_0 {
                pH264PicData: &raw mut pic_data,
            },
        };

        let reference_frames = if has_reference {
            D3D12_VIDEO_ENCODE_REFERENCE_FRAMES {
                NumTexture2Ds: 1,
                ppTexture2Ds: ref_texture.as_mut_ptr(),
                pSubresources: ref_subresource.as_mut_ptr(),
            }
        } else {
            D3D12_VIDEO_ENCODE_REFERENCE_FRAMES {
                NumTexture2Ds: 0,
                ppTexture2Ds: std::ptr::null_mut(),
                pSubresources: std::ptr::null_mut(),
            }
        };

        // The startup IDR of an intra-refresh session is a "non-IR frame" (spec
        // wording) — flag/config stay off exactly when `intra_refresh_frame_index`
        // is `None`, which `gop.rs` already guarantees for that frame.
        let sequence_control_flags = if intra_refresh_frame_index.is_some() {
            D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_FLAG_REQUEST_INTRA_REFRESH
        } else {
            D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_FLAG_NONE
        };
        let intra_refresh_config = D3D12_VIDEO_ENCODER_INTRA_REFRESH {
            Mode: if intra_refresh_frame_index.is_some() {
                D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_ROW_BASED
            } else {
                D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE
            },
            IntraRefreshDuration: self.intra_refresh_period.unwrap_or(0),
        };

        let input_args = D3D12_VIDEO_ENCODER_ENCODEFRAME_INPUT_ARGUMENTS {
            SequenceControlDesc: D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_DESC {
                Flags: sequence_control_flags,
                IntraRefreshConfig: intra_refresh_config,
                RateControl: rc,
                PictureTargetResolution: resolution,
                SelectedLayoutMode: D3D12_VIDEO_ENCODER_FRAME_SUBREGION_LAYOUT_MODE_FULL_FRAME,
                FrameSubregionsLayoutData:
                    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_SUBREGIONS_LAYOUT_DATA::default(),
                CodecGopSequence: gop_desc,
            },
            PictureControlDesc: D3D12_VIDEO_ENCODER_PICTURE_CONTROL_DESC {
                IntraRefreshFrameIndex: intra_refresh_frame_index.unwrap_or(0),
                Flags: picture_control_flags,
                PictureControlCodecData: pic_control_data,
                ReferenceFrames: reference_frames,
            },
            pInputFrame: borrow_resource(&self.input_texture),
            InputFrameSubresource: 0,
            CurrentFrameBitstreamMetadataSize: u32::try_from(self.header_len_aligned)
                .unwrap_or(u32::MAX),
        };

        // GOP mode always requests a reconstructed picture (even for IDR frames — a
        // future P frame may need to reference it); IDR-only mode (`self.recon_pool`
        // `None`) keeps today's `None`, matching the API's documented "only needed if
        // used as a reference" contract.
        let reconstructed_picture =
            if let (Some(pool), Some(write)) = (&self.recon_pool, write_slot) {
                D3D12_VIDEO_ENCODER_RECONSTRUCTED_PICTURE {
                    pReconstructedPicture: borrow_resource(&pool.texture),
                    ReconstructedPictureSubresource: write,
                }
            } else {
                D3D12_VIDEO_ENCODER_RECONSTRUCTED_PICTURE {
                    pReconstructedPicture: ManuallyDrop::new(None),
                    ReconstructedPictureSubresource: 0,
                }
            };
        let output_args = D3D12_VIDEO_ENCODER_ENCODEFRAME_OUTPUT_ARGUMENTS {
            Bitstream: D3D12_VIDEO_ENCODER_COMPRESSED_BITSTREAM {
                pBuffer: borrow_resource(&self.bitstream_buffer),
                FrameStartOffset: self.header_len_aligned,
            },
            ReconstructedPicture: reconstructed_picture,
            EncoderOutputMetadata: D3D12_VIDEO_ENCODER_ENCODE_OPERATION_METADATA_BUFFER {
                pBuffer: borrow_resource(&self.metadata_buffer),
                Offset: 0,
            },
        };

        // SAFETY: encoder/heap were created for this exact codec/profile/resolution;
        // buffers were sized from `D3D12_FEATURE_VIDEO_ENCODER_RESOURCE_REQUIREMENTS` at
        // `open`. `ReconstructedPicture`/`ReferenceFrames` are `None`/empty outside GOP
        // mode, matching the API's documented "only needed if used as a reference"
        // contract; in GOP mode both point at `self.recon_pool` textures transitioned
        // to the correct video-encode state by the barriers above.
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

        let mut profile_h264: D3D12_VIDEO_ENCODER_PROFILE_H264 =
            D3D12_VIDEO_ENCODER_PROFILE_H264_MAIN;
        let profile_desc = D3D12_VIDEO_ENCODER_PROFILE_DESC {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_PROFILE_H264>(),
            Anonymous: D3D12_VIDEO_ENCODER_PROFILE_DESC_0 {
                pH264Profile: &raw mut profile_h264,
            },
        };
        let input_metadata = D3D12_VIDEO_ENCODER_RESOLVE_METADATA_INPUT_ARGUMENTS {
            EncoderCodec: D3D12_VIDEO_ENCODER_CODEC_H264,
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

        if let (Some(pool), Some(write)) = (&self.recon_pool, write_slot) {
            let mut recon_barriers_after = vec![transition_barrier_subresource(
                &pool.texture,
                write,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
                D3D12_RESOURCE_STATE_COMMON,
            )];
            if has_reference {
                let read = read_slot.unwrap_or(write);
                recon_barriers_after.push(transition_barrier_subresource(
                    &pool.texture,
                    read,
                    D3D12_RESOURCE_STATE_VIDEO_ENCODE_READ,
                    D3D12_RESOURCE_STATE_COMMON,
                ));
            }
            // SAFETY: same video-encode command list as every barrier/call above.
            unsafe { self.encode_list.ResourceBarrier(&recon_barriers_after) };
        }

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

        // The GPU work is done and waited-on — the write slot now genuinely holds this
        // frame's reconstructed picture, so it's safe to record as the next frame's
        // reference and flip the ping-pong pool. Only touched in GOP mode.
        if let Some(pool) = &mut self.recon_pool {
            self.last_h264_reference = Some((poc, self.frame_decoding_order.wrapping_sub(1)));
            pool.write_slot = 1 - pool.write_slot;
        }

        self.read_packet(pts, duration, is_idr)
    }

    pub(super) fn read_packet(
        &mut self,
        pts: i64,
        duration: u64,
        is_keyframe: bool,
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
        // SAFETY: buffer was sized >= `size_of::<D3D12_VIDEO_ENCODER_OUTPUT_METADATA>()` at
        // `open`; the driver writes a full struct at offset 0 during `ResolveEncoderOutputMetadata`.
        let meta = unsafe {
            resolved_ptr
                .cast::<D3D12_VIDEO_ENCODER_OUTPUT_METADATA>()
                .read_unaligned()
        };
        // SAFETY: matches the `Map` above.
        unsafe { self.resolved_metadata_buffer.Unmap(0, None) };

        // `D3D12_VIDEO_ENCODER_ENCODE_ERROR_FLAG_NO_ERROR` is `0` — any nonzero flag is an error.
        if meta.EncodeErrorFlags != 0 {
            return Err(EncodeError::Backend);
        }
        let written = meta.EncodedBitstreamWrittenBytesCount;
        if written == 0 {
            return Err(EncodeError::Backend);
        }
        let written_usize = usize::try_from(written).map_err(|_| EncodeError::Backend)?;
        let offset_usize =
            usize::try_from(self.header_len_aligned).map_err(|_| EncodeError::Backend)?;
        let end = offset_usize
            .checked_add(written_usize)
            .ok_or(EncodeError::Backend)?;
        if end as u64 > self.bitstream_capacity {
            return Err(EncodeError::Backend);
        }

        // clone: own Packet payload built from the persistent per-session SPS/PPS
        // bytes; only prepended on keyframes — a P frame's decoder already has the
        // parameter sets from the GOP's IDR, so repeating them would just be waste.
        let mut payload = if is_keyframe {
            self.header_bytes.clone()
        } else {
            Vec::new()
        };
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
        // SAFETY: `offset_usize + written_usize <= bitstream_capacity`, checked above; the
        // buffer was committed with that capacity at `open`.
        unsafe {
            let slice = std::slice::from_raw_parts(slice_ptr.add(offset_usize), written_usize);
            payload.extend_from_slice(slice);
        }
        // SAFETY: matches the `Map` above.
        unsafe { self.bitstream_buffer.Unmap(0, None) };

        Ok(Packet {
            stream_id: self.info.id(),
            pts,
            dts: pts,
            duration,
            is_keyframe,
            is_discard: false,
            payload: Bytes::from(payload),
        })
    }
}
