//! Per-frame D3D12 recording for HEVC — **parallel to [`super::ops`]**, not a generified
//! shared implementation (ADR-0004 § Why a parallel `hevc_ops.rs`/`hevc_decoder.rs`: the
//! existing H.264-typed `Session`/`ops.rs` are the last known-consistent baseline for the
//! still-unresolved H.264 D3D12 decode hang, so this module is a deliberate, acknowledged
//! near-line-for-line copy of `ops.rs::decode_frame`/`readback_dpb_slot_to_cpu`, retyped
//! for `DxvaPicParamsHevc`/`DxvaQmatrixHevc`/`DxvaSliceHevcShort`/`HevcRefMeta` — rather
//! than editing the H.264 files at all). Real, acknowledged duplication; a future
//! `Session<M>` generify-and-de-duplicate refactor is deferred until at least one codec's
//! GPU decode path is hardware-verified (ADR-0004 Open Question #7).

use std::mem::ManuallyDrop;

use crate::DecodeError;
use mediaway_common::Bytes;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT, D3D12_RESOURCE_STATE_COMMON,
    D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_VIDEO_DECODE_READ,
    D3D12_RESOURCE_STATE_VIDEO_DECODE_WRITE, D3D12_SUBRESOURCE_FOOTPRINT,
    D3D12_TEXTURE_COPY_LOCATION, D3D12_TEXTURE_COPY_LOCATION_0,
    D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
    ID3D12CommandList, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8_UNORM};
use windows::Win32::Media::MediaFoundation::{
    D3D12_VIDEO_DECODE_ARGUMENT_TYPE_INVERSE_QUANTIZATION_MATRIX,
    D3D12_VIDEO_DECODE_ARGUMENT_TYPE_PICTURE_PARAMETERS,
    D3D12_VIDEO_DECODE_ARGUMENT_TYPE_SLICE_CONTROL, D3D12_VIDEO_DECODE_COMPRESSED_BITSTREAM,
    D3D12_VIDEO_DECODE_FRAME_ARGUMENT, D3D12_VIDEO_DECODE_INPUT_STREAM_ARGUMENTS,
    D3D12_VIDEO_DECODE_OUTPUT_STREAM_ARGUMENTS1, D3D12_VIDEO_DECODE_REFERENCE_FRAMES,
};
use windows::core::Interface;

use super::hevc_decoder::SessionHevc;
use super::hevc_pic_params::{DxvaPicParamsHevc, DxvaQmatrixHevc, DxvaSliceHevcShort};
use super::hevc_refs::HevcRefMeta;
use super::util::{
    borrow_resource, data_size, nv12_size, transition_barrier, transition_barrier_all,
};

/// Non-owning `ManuallyDrop` borrow of a COM interface — see `ops::borrow_interface`'s
/// doc for the full safety reasoning this duplicates (no `AddRef`, read-only for the
/// synchronous call that follows, the real owner is untouched).
const fn borrow_interface<T>(iface: &T) -> ManuallyDrop<Option<T>> {
    // SAFETY: `T` here is always a `windows`-crate `repr(transparent)` COM wrapper
    // (`ID3D12VideoDecoderHeap` at this module's one call site); duplicating its pointer
    // bits without `AddRef` is safe because the duplicate is immediately wrapped in
    // `ManuallyDrop` and only read by the synchronous D3D12 call that follows, so its
    // `Drop`/`Release` never runs.
    unsafe { ManuallyDrop::new(Some(std::mem::transmute_copy(iface))) }
}

impl SessionHevc {
    /// Write `nal_bytes` (the current slice's RBSP-framed NAL payload, header byte
    /// included) into the compressed-bitstream input buffer at offset 0.
    fn write_bitstream(&mut self, nal_bytes: &[u8]) -> Result<(), DecodeError> {
        if u64::try_from(nal_bytes.len()).unwrap_or(u64::MAX) > self.bitstream_capacity {
            return Err(DecodeError::InvalidInput);
        }
        let mut ptr: *mut u8 = std::ptr::null_mut();
        // SAFETY: `bitstream_buffer` is a CUSTOM-heap (UPLOAD-equivalent) resource,
        // always CPU-`Map`-able regardless of its fixed `VIDEO_DECODE_READ` GPU state.
        unsafe {
            self.bitstream_buffer
                .Map(0, None, Some(std::ptr::from_mut(&mut ptr).cast()))
                .map_err(|_err| DecodeError::Backend)?;
        }
        if ptr.is_null() {
            return Err(DecodeError::Backend);
        }
        // SAFETY: `nal_bytes.len() <= bitstream_capacity`, checked above.
        unsafe {
            std::ptr::copy_nonoverlapping(nal_bytes.as_ptr(), ptr, nal_bytes.len());
        }
        // SAFETY: matches the `Map` above.
        unsafe { self.bitstream_buffer.Unmap(0, None) };
        Ok(())
    }

    /// Build the `(textures, subresources)` backing arrays for
    /// `D3D12_VIDEO_DECODE_REFERENCE_FRAMES` — see `ops::Session::build_reference_frame_arrays`'s
    /// identical doc (same "one texture-array DPB, differing only by subresource" shape).
    fn build_reference_frame_arrays(
        &self,
        refs: &[(u32, HevcRefMeta)],
    ) -> (Vec<Option<ID3D12Resource>>, Vec<u32>) {
        let mut textures = Vec::with_capacity(refs.len());
        let mut subresources = Vec::with_capacity(refs.len());
        for &(slot, _) in refs {
            // clone: COM AddRef share — one array entry per active reference, all the
            // same DPB texture-array resource; dropped (Released) normally when this
            // function's caller's local `Vec`s go out of scope after the synchronous
            // `DecodeFrame1` call. Mirrors `ops::Session::build_reference_frame_arrays`.
            textures.push(Some(self.dpb.texture().clone()));
            subresources.push(slot);
        }
        (textures, subresources)
    }

    /// Submit one `DecodeFrame1` call for the current picture's sole slice, wait for the
    /// fence. See `ops::Session::decode_frame`'s doc for the full barrier-sequence
    /// reasoning (this function is a retyped near-copy of it, per this file's module doc).
    ///
    /// # Errors
    ///
    /// [`DecodeError::Backend`] on any D3D12 recording/submission/fence failure.
    /// [`DecodeError::InvalidInput`] when `slice_nal` exceeds the bitstream buffer's
    /// fixed capacity.
    #[allow(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        reason = "one linear DecodeFrame1 submission; splitting fragments the record/execute/wait sequence"
    )]
    pub(super) fn decode_frame(
        &mut self,
        slice_nal: &[u8],
        pic_params: &mut DxvaPicParamsHevc,
        qmatrix: &mut DxvaQmatrixHevc,
        slice_short: &mut DxvaSliceHevcShort,
        output_slot: u32,
        refs: &[(u32, HevcRefMeta)],
    ) -> Result<(), DecodeError> {
        self.write_bitstream(slice_nal)?;
        let (mut ref_textures, mut ref_subresources) = self.build_reference_frame_arrays(refs);

        let reference_frames = if ref_textures.is_empty() {
            D3D12_VIDEO_DECODE_REFERENCE_FRAMES {
                NumTexture2Ds: 0,
                ppTexture2Ds: std::ptr::null_mut(),
                pSubresources: std::ptr::null_mut(),
                ppHeaps: std::ptr::null_mut(),
            }
        } else {
            D3D12_VIDEO_DECODE_REFERENCE_FRAMES {
                NumTexture2Ds: u32::try_from(ref_textures.len()).unwrap_or(0),
                ppTexture2Ds: ref_textures.as_mut_ptr(),
                pSubresources: ref_subresources.as_mut_ptr(),
                ppHeaps: std::ptr::null_mut(),
            }
        };

        let mut frame_arguments = [D3D12_VIDEO_DECODE_FRAME_ARGUMENT::default(); 10];
        frame_arguments[0] = D3D12_VIDEO_DECODE_FRAME_ARGUMENT {
            Type: D3D12_VIDEO_DECODE_ARGUMENT_TYPE_PICTURE_PARAMETERS,
            Size: data_size::<DxvaPicParamsHevc>(),
            pData: std::ptr::from_mut(pic_params).cast(),
        };
        frame_arguments[1] = D3D12_VIDEO_DECODE_FRAME_ARGUMENT {
            Type: D3D12_VIDEO_DECODE_ARGUMENT_TYPE_INVERSE_QUANTIZATION_MATRIX,
            Size: data_size::<DxvaQmatrixHevc>(),
            pData: std::ptr::from_mut(qmatrix).cast(),
        };
        frame_arguments[2] = D3D12_VIDEO_DECODE_FRAME_ARGUMENT {
            Type: D3D12_VIDEO_DECODE_ARGUMENT_TYPE_SLICE_CONTROL,
            Size: data_size::<DxvaSliceHevcShort>(),
            pData: std::ptr::from_mut(slice_short).cast(),
        };

        let input_args = D3D12_VIDEO_DECODE_INPUT_STREAM_ARGUMENTS {
            NumFrameArguments: 3,
            FrameArguments: frame_arguments,
            ReferenceFrames: reference_frames,
            CompressedBitstream: D3D12_VIDEO_DECODE_COMPRESSED_BITSTREAM {
                pBuffer: borrow_resource(&self.bitstream_buffer),
                Offset: 0,
                Size: u64::try_from(slice_nal.len()).unwrap_or(0),
            },
            pHeap: borrow_interface(&self.decoder_heap),
        };

        let output_args = D3D12_VIDEO_DECODE_OUTPUT_STREAM_ARGUMENTS1 {
            pOutputTexture2D: borrow_resource(self.dpb.texture()),
            OutputSubresource: output_slot,
            ..Default::default()
        };

        // SAFETY: freshly created/idle allocator+list (fully synchronous per picture).
        unsafe {
            self.decode_allocator
                .Reset()
                .map_err(|_err| DecodeError::Backend)?;
            self.decode_list
                .Reset(&self.decode_allocator)
                .map_err(|_err| DecodeError::Backend)?;
        }

        // NV12 two-plane subresource handling — same real hardware finding `ops.rs`
        // documents for H.264 (luma at subresource `slot`, chroma at `slot + num_slots`,
        // both must be independently barriered).
        let num_slots = self.dpb.table().num_slots();
        let output_write = [
            transition_barrier(
                self.dpb.texture(),
                output_slot,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_VIDEO_DECODE_WRITE,
            ),
            transition_barrier(
                self.dpb.texture(),
                output_slot + num_slots,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_VIDEO_DECODE_WRITE,
            ),
        ];
        // SAFETY: `VIDEO_DECODE_WRITE` transitions are only valid recorded on a
        // video-decode command list — this is one.
        unsafe { self.decode_list.ResourceBarrier(&output_write) };

        let ref_reads: Vec<_> = refs
            .iter()
            .flat_map(|&(slot, _)| {
                [
                    transition_barrier(
                        self.dpb.texture(),
                        slot,
                        D3D12_RESOURCE_STATE_COMMON,
                        D3D12_RESOURCE_STATE_VIDEO_DECODE_READ,
                    ),
                    transition_barrier(
                        self.dpb.texture(),
                        slot + num_slots,
                        D3D12_RESOURCE_STATE_COMMON,
                        D3D12_RESOURCE_STATE_VIDEO_DECODE_READ,
                    ),
                ]
            })
            .collect();
        if !ref_reads.is_empty() {
            // SAFETY: same video-decode command list as the output barrier above.
            unsafe { self.decode_list.ResourceBarrier(&ref_reads) };
        }

        // SAFETY: `decoder`/`decoder_heap` were created for this exact profile/
        // resolution at `open`; `input_args`/`output_args` embed pointers to same-scope
        // locals (`ref_textures`/`ref_subresources`/`pic_params`/`qmatrix`/`slice_short`)
        // all still alive for the duration of this call.
        unsafe {
            self.decode_list.DecodeFrame1(
                &self.decoder,
                &raw const output_args,
                &raw const input_args,
            );
        }

        let output_to_common = [
            transition_barrier(
                self.dpb.texture(),
                output_slot,
                D3D12_RESOURCE_STATE_VIDEO_DECODE_WRITE,
                D3D12_RESOURCE_STATE_COMMON,
            ),
            transition_barrier(
                self.dpb.texture(),
                output_slot + num_slots,
                D3D12_RESOURCE_STATE_VIDEO_DECODE_WRITE,
                D3D12_RESOURCE_STATE_COMMON,
            ),
        ];
        // SAFETY: same video-decode command list as the `DecodeFrame1` call above.
        unsafe { self.decode_list.ResourceBarrier(&output_to_common) };
        let ref_to_common: Vec<_> = refs
            .iter()
            .flat_map(|&(slot, _)| {
                [
                    transition_barrier(
                        self.dpb.texture(),
                        slot,
                        D3D12_RESOURCE_STATE_VIDEO_DECODE_READ,
                        D3D12_RESOURCE_STATE_COMMON,
                    ),
                    transition_barrier(
                        self.dpb.texture(),
                        slot + num_slots,
                        D3D12_RESOURCE_STATE_VIDEO_DECODE_READ,
                        D3D12_RESOURCE_STATE_COMMON,
                    ),
                ]
            })
            .collect();
        if !ref_to_common.is_empty() {
            // SAFETY: same video-decode command list as every barrier/call above.
            unsafe { self.decode_list.ResourceBarrier(&ref_to_common) };
        }

        // SAFETY: matched Reset/Close pair on this list.
        unsafe { self.decode_list.Close() }.map_err(|_err| DecodeError::Backend)?;
        let generic: ID3D12CommandList = self
            .decode_list
            .cast()
            .map_err(|_err| DecodeError::Backend)?;
        // SAFETY: `generic` is the just-closed `decode_list`, valid for this
        // VIDEO_DECODE queue.
        unsafe { self.decode_queue.ExecuteCommandLists(&[Some(generic)]) };
        super::util::signal_and_wait(
            &self.decode_queue,
            &self.fence,
            self.fence_event,
            &mut self.fence_value,
        )
    }

    /// Copy DPB slot `slot`'s NV12 pixels into a tightly-packed CPU buffer — byte-for-byte
    /// the same costly-path readback `ops::Session::readback_dpb_slot_to_cpu` documents
    /// (this function is a retyped near-copy of it).
    ///
    /// # Errors
    ///
    /// [`DecodeError::Backend`] on any D3D12 recording/submission/fence/map failure.
    #[allow(
        clippy::too_many_lines,
        reason = "one linear record/execute/wait/map readback sequence; splitting fragments it"
    )]
    pub(super) fn readback_dpb_slot_to_cpu(&mut self, slot: u32) -> Result<Bytes, DecodeError> {
        let width = self.width;
        let height = self.height;
        let row_pitch = super::util::align_up_u32(
            width,
            windows::Win32::Graphics::Direct3D12::D3D12_TEXTURE_DATA_PITCH_ALIGNMENT,
        );
        let luma_size = u64::from(row_pitch) * u64::from(height);

        // SAFETY: freshly created/idle allocator+list (fully synchronous per readback).
        unsafe {
            self.copy_allocator
                .Reset()
                .map_err(|_err| DecodeError::Backend)?;
            self.copy_list
                .Reset(
                    &self.copy_allocator,
                    None::<&windows::Win32::Graphics::Direct3D12::ID3D12PipelineState>,
                )
                .map_err(|_err| DecodeError::Backend)?;
        }

        let readback_to_copy_dest = transition_barrier_all(
            &self.readback_buffer,
            D3D12_RESOURCE_STATE_COMMON,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        // SAFETY: recording on a freshly reset copy command list.
        unsafe { self.copy_list.ResourceBarrier(&[readback_to_copy_dest]) };

        let num_slots = self.dpb.table().num_slots();
        let luma_src = D3D12_TEXTURE_COPY_LOCATION {
            pResource: borrow_resource(self.dpb.texture()),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: slot, // plane 0 (luma)
            },
        };
        let luma_dst = D3D12_TEXTURE_COPY_LOCATION {
            pResource: borrow_resource(&self.readback_buffer),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                    Offset: 0,
                    Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                        Format: DXGI_FORMAT_R8_UNORM,
                        Width: width,
                        Height: height,
                        Depth: 1,
                        RowPitch: row_pitch,
                    },
                },
            },
        };
        // SAFETY: `readback_buffer` is `COPY_DEST`; the DPB slot source stays `COMMON`
        // (implicit read promotion for copy sources).
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

        let uv_src = D3D12_TEXTURE_COPY_LOCATION {
            pResource: borrow_resource(self.dpb.texture()),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: slot + num_slots, // plane 1 (chroma)
            },
        };
        let uv_dst = D3D12_TEXTURE_COPY_LOCATION {
            pResource: borrow_resource(&self.readback_buffer),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                    Offset: luma_size,
                    Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                        Format: DXGI_FORMAT_R8G8_UNORM,
                        Width: width / 2,
                        Height: height / 2,
                        Depth: 1,
                        RowPitch: row_pitch,
                    },
                },
            },
        };
        // SAFETY: same resources/states as the luma copy above.
        unsafe {
            self.copy_list
                .CopyTextureRegion(&raw const uv_dst, 0, 0, 0, &raw const uv_src, None);
        }

        let readback_to_common = transition_barrier_all(
            &self.readback_buffer,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATE_COMMON,
        );
        // SAFETY: same copy command list as every call above.
        unsafe { self.copy_list.ResourceBarrier(&[readback_to_common]) };

        // SAFETY: matched Reset/Close pair on this list.
        unsafe { self.copy_list.Close() }.map_err(|_err| DecodeError::Backend)?;
        let generic: ID3D12CommandList =
            self.copy_list.cast().map_err(|_err| DecodeError::Backend)?;
        // SAFETY: `generic` is the just-closed `copy_list`.
        unsafe { self.copy_queue.ExecuteCommandLists(&[Some(generic)]) };
        super::util::signal_and_wait(
            &self.copy_queue,
            &self.fence,
            self.fence_event,
            &mut self.fence_value,
        )?;

        let nv12_len = nv12_size(width, height)?;
        let mut out = vec![0u8; nv12_len];
        let mut ptr: *mut u8 = std::ptr::null_mut();
        // SAFETY: `readback_buffer` is a READBACK-heap-equivalent resource, CPU-`Map`-able.
        unsafe {
            self.readback_buffer
                .Map(0, None, Some(std::ptr::from_mut(&mut ptr).cast()))
                .map_err(|_err| DecodeError::Backend)?;
        }
        if ptr.is_null() {
            return Err(DecodeError::Backend);
        }
        let row_pitch_usize = row_pitch as usize;
        let width_usize = width as usize;
        let height_usize = height as usize;
        // SAFETY: `ptr` is valid for `readback_buffer`'s committed size (>= the placed
        // footprints written above) for the duration of this Map.
        unsafe {
            for row in 0..height_usize {
                let dst_off = row * width_usize;
                std::ptr::copy_nonoverlapping(
                    ptr.add(row * row_pitch_usize),
                    out[dst_off..dst_off + width_usize].as_mut_ptr(),
                    width_usize,
                );
            }
            let chroma_base = width_usize * height_usize;
            let luma_size_usize = usize::try_from(luma_size).unwrap_or(0);
            for row in 0..(height_usize / 2) {
                let dst_off = chroma_base + row * width_usize;
                std::ptr::copy_nonoverlapping(
                    ptr.add(luma_size_usize + row * row_pitch_usize),
                    out[dst_off..dst_off + width_usize].as_mut_ptr(),
                    width_usize,
                );
            }
        }
        // SAFETY: matches the `Map` above.
        unsafe { self.readback_buffer.Unmap(0, None) };

        Ok(Bytes::from(out))
    }
}
