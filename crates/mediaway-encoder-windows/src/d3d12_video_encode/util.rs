//! Small per-call D3D12 recording helpers shared by [`super`]'s `open` and
//! [`super::ops`]'s per-frame encode path.

use std::mem::ManuallyDrop;

use mediaway_encoder::EncodeError;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT, D3D12_RESOURCE_BARRIER, D3D12_RESOURCE_BARRIER_0,
    D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, D3D12_RESOURCE_BARRIER_FLAG_NONE,
    D3D12_RESOURCE_BARRIER_TYPE_TRANSITION, D3D12_RESOURCE_STATES,
    D3D12_RESOURCE_TRANSITION_BARRIER, D3D12_SUBRESOURCE_FOOTPRINT, D3D12_TEXTURE_COPY_LOCATION,
    D3D12_TEXTURE_COPY_LOCATION_0, D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
    D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX, ID3D12CommandQueue, ID3D12Fence, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};

/// `size_of::<T>()` as `u32` for a `DataSize` field — every `T` here is a small fixed-layout
/// D3D12 struct, always far below `u32::MAX`; `unwrap_or` never actually triggers.
pub(super) fn data_size<T>() -> u32 {
    u32::try_from(std::mem::size_of::<T>()).unwrap_or(u32::MAX)
}

pub(super) fn nv12_size(width: u32, height: u32) -> Result<usize, EncodeError> {
    let w = width as usize;
    let h = height as usize;
    w.checked_mul(h)
        .and_then(|y| y.checked_add(y / 2))
        .ok_or(EncodeError::InvalidInput)
}

pub(super) fn frame_rate(time_base_num: u64, time_base_den: u32) -> (u32, u32) {
    let fps_num = time_base_den;
    let fps_den = u32::try_from(time_base_num.max(1)).unwrap_or(1);
    (fps_num, fps_den)
}

pub(super) const fn align_up_u32(value: u32, align: u32) -> u32 {
    if align == 0 {
        return value;
    }
    let rem = value % align;
    if rem == 0 {
        value
    } else {
        value + (align - rem)
    }
}

pub(super) const fn align_up_u64(value: u64, align: u64) -> u64 {
    if align == 0 {
        return value;
    }
    let rem = value % align;
    if rem == 0 {
        value
    } else {
        value + (align - rem)
    }
}

/// Write `header` into `bitstream_buffer[0..header.len()]` once. The driver only ever
/// writes at `[FrameStartOffset..]`, so a header written here survives every later frame.
pub(super) fn write_header_once(
    bitstream_buffer: &ID3D12Resource,
    header: &[u8],
) -> Result<(), EncodeError> {
    let mut ptr: *mut u8 = std::ptr::null_mut();
    // SAFETY: READBACK-heap resource, CPU-writable via `Map` regardless of GPU-side state.
    unsafe {
        bitstream_buffer
            .Map(0, None, Some(std::ptr::from_mut(&mut ptr).cast()))
            .map_err(|_| EncodeError::Backend)?;
    }
    if ptr.is_null() {
        return Err(EncodeError::Backend);
    }
    // SAFETY: buffer was committed with capacity >= `header.len()` by the caller.
    unsafe {
        std::ptr::copy_nonoverlapping(header.as_ptr(), ptr, header.len());
    }
    // SAFETY: matches the `Map` above.
    unsafe { bitstream_buffer.Unmap(0, None) };
    Ok(())
}

/// Copy tightly-packed NV12 (`nv12_size` layout) into the row-pitch-aligned upload buffer.
pub(super) fn write_nv12_upload(
    upload_buffer: &ID3D12Resource,
    nv12: &[u8],
    width: u32,
    height: u32,
    row_pitch: u32,
    luma_size: u64,
) -> Result<(), EncodeError> {
    let mut ptr: *mut u8 = std::ptr::null_mut();
    // SAFETY: UPLOAD-heap resource, CPU-writable.
    unsafe {
        upload_buffer
            .Map(0, None, Some(std::ptr::from_mut(&mut ptr).cast()))
            .map_err(|_| EncodeError::Backend)?;
    }
    if ptr.is_null() {
        return Err(EncodeError::Backend);
    }
    let width = width as usize;
    let height = height as usize;
    let row_pitch = row_pitch as usize;
    let luma_size = usize::try_from(luma_size).unwrap_or(usize::MAX);
    // SAFETY: `ptr` is valid for the buffer's committed size for the duration of this Map;
    // `nv12` was checked by the caller to hold at least `nv12_size(width, height)` bytes.
    unsafe {
        for row in 0..height {
            let src_off = row * width;
            std::ptr::copy_nonoverlapping(
                nv12[src_off..src_off + width].as_ptr(),
                ptr.add(row * row_pitch),
                width,
            );
        }
        let chroma_base = width * height;
        for row in 0..(height / 2) {
            let src_off = chroma_base + row * width;
            std::ptr::copy_nonoverlapping(
                nv12[src_off..src_off + width].as_ptr(),
                ptr.add(luma_size + row * row_pitch),
                width,
            );
        }
    }
    // SAFETY: matches the `Map` above.
    unsafe { upload_buffer.Unmap(0, None) };
    Ok(())
}

pub(super) const fn subresource_copy_location(
    resource: &ID3D12Resource,
    subresource_index: u32,
) -> D3D12_TEXTURE_COPY_LOCATION {
    D3D12_TEXTURE_COPY_LOCATION {
        pResource: borrow_resource(resource),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: subresource_index,
        },
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "mirrors D3D12_PLACED_SUBRESOURCE_FOOTPRINT fields 1:1"
)]
pub(super) const fn placed_footprint_copy_location(
    resource: &ID3D12Resource,
    offset: u64,
    format: DXGI_FORMAT,
    width: u32,
    height: u32,
    row_pitch: u32,
) -> D3D12_TEXTURE_COPY_LOCATION {
    D3D12_TEXTURE_COPY_LOCATION {
        pResource: borrow_resource(resource),
        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                Offset: offset,
                Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                    Format: format,
                    Width: width,
                    Height: height,
                    Depth: 1,
                    RowPitch: row_pitch,
                },
            },
        },
    }
}

pub(super) const fn transition_barrier(
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: borrow_resource(resource),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

/// Wrap a persistent resource as a non-owning ABI reference for a `ManuallyDrop<Option<T>>`
/// "lend a pointer for this call" struct field (`D3D12_RESOURCE_TRANSITION_BARRIER::pResource`,
/// `D3D12_VIDEO_ENCODER_COMPRESSED_BITSTREAM::pBuffer`, …).
pub(super) const fn borrow_resource(
    resource: &ID3D12Resource,
) -> ManuallyDrop<Option<ID3D12Resource>> {
    // SAFETY: `ID3D12Resource` is `repr(transparent)` over one COM pointer. This duplicates
    // the pointer bits without an `AddRef`; the duplicate is immediately wrapped in
    // `ManuallyDrop` and only read by the synchronous D3D12 call that follows, so its
    // `Drop`/`Release` never runs. The real owner (`resource`, alive on `self`/a caller
    // local) is untouched — this matches the ABI contract these `windows`-crate fields are
    // designed for (lend a pointer, do not transfer ownership).
    unsafe { ManuallyDrop::new(Some(std::mem::transmute_copy(resource))) }
}

/// Signal `fence` on `queue` and block the calling thread until the GPU reaches that value.
/// Fully synchronous per submission — matches this backend's CPU-upload staging scope (no
/// multi-frame pipelining yet).
pub(super) fn signal_and_wait(
    queue: &ID3D12CommandQueue,
    fence: &ID3D12Fence,
    fence_event: HANDLE,
    fence_value: &mut u64,
) -> Result<(), EncodeError> {
    *fence_value += 1;
    // SAFETY: `fence` and `queue` are live for the session; `Signal` is a POD-arg call.
    unsafe { queue.Signal(fence, *fence_value) }.map_err(|_| EncodeError::Backend)?;
    // SAFETY: `GetCompletedValue` is a plain getter.
    if unsafe { fence.GetCompletedValue() } < *fence_value {
        // SAFETY: `fence_event` is a live event handle owned by the session.
        unsafe { fence.SetEventOnCompletion(*fence_value, fence_event) }
            .map_err(|_| EncodeError::Backend)?;
        // SAFETY: blocking wait on our own event handle.
        unsafe { WaitForSingleObject(fence_event, INFINITE) };
    }
    Ok(())
}
