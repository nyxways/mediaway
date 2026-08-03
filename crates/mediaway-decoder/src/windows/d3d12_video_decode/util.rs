//! Small shared D3D12 recording/plumbing helpers — fence wait, resource-state
//! transitions, alignment — used by [`super::setup`], [`super::dpb`], and [`super::ops`].
//! Codec-agnostic (mirrors `mediaway-encoder-windows`'s own `d3d12_video_encode/util.rs`).

use std::mem::ManuallyDrop;

use crate::DecodeError;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_RESOURCE_BARRIER, D3D12_RESOURCE_BARRIER_0, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
    D3D12_RESOURCE_BARRIER_FLAG_NONE, D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
    D3D12_RESOURCE_STATES, D3D12_RESOURCE_TRANSITION_BARRIER, ID3D12CommandQueue, ID3D12Fence,
    ID3D12Resource,
};
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};

/// `size_of::<T>()` as `u32` for a `DataSize`/`Size` field — every `T` here is a small
/// fixed-layout struct, always far below `u32::MAX`; `unwrap_or` never actually triggers.
pub(super) fn data_size<T>() -> u32 {
    u32::try_from(std::mem::size_of::<T>()).unwrap_or(u32::MAX)
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

/// NV12 tightly-packed frame size (luma + half-size interleaved chroma) for readback
/// buffer sizing.
pub(super) fn nv12_size(width: u32, height: u32) -> Result<usize, DecodeError> {
    let w = width as usize;
    let h = height as usize;
    w.checked_mul(h)
        .and_then(|y| y.checked_add(y / 2))
        .ok_or(DecodeError::InvalidInput)
}

pub(super) const fn transition_barrier(
    resource: &ID3D12Resource,
    subresource: u32,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: borrow_resource(resource),
                Subresource: subresource,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

/// Barrier covering every subresource of `resource` (the whole DPB texture array, or a
/// single-subresource linear buffer where "all subresources" is just subresource 0).
pub(super) const fn transition_barrier_all(
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    transition_barrier(
        resource,
        D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
        before,
        after,
    )
}

/// Wrap a persistent resource as a non-owning ABI reference for a
/// `ManuallyDrop<Option<T>>` "lend a pointer for this call" struct field.
///
/// See `mediaway-encoder-windows`'s `d3d12_video_encode/util.rs::borrow_resource` for
/// the full safety reasoning this mirrors: the duplicated pointer bits are never
/// `Release`d (wrapped in `ManuallyDrop`, read only by the synchronous D3D12 call that
/// follows), so the real owner is untouched.
pub(super) const fn borrow_resource(
    resource: &ID3D12Resource,
) -> ManuallyDrop<Option<ID3D12Resource>> {
    // SAFETY: `ID3D12Resource` is `repr(transparent)` over one COM pointer. This
    // duplicates the pointer bits without an `AddRef`; the duplicate is immediately
    // wrapped in `ManuallyDrop` and only read by the synchronous D3D12 call that
    // follows, so its `Drop`/`Release` never runs. The real owner (alive on `self`/a
    // caller local) is untouched.
    unsafe { ManuallyDrop::new(Some(std::mem::transmute_copy(resource))) }
}

/// Signal `fence` on `queue` and block the calling thread until the GPU reaches that
/// value. Fully synchronous per submission — matches this backend's single-picture-
/// in-flight scope (no multi-frame pipelining yet).
pub(super) fn signal_and_wait(
    queue: &ID3D12CommandQueue,
    fence: &ID3D12Fence,
    fence_event: HANDLE,
    fence_value: &mut u64,
) -> Result<(), DecodeError> {
    *fence_value += 1;
    // SAFETY: `fence` and `queue` are live for the session; `Signal` is a POD-arg call.
    unsafe { queue.Signal(fence, *fence_value) }.map_err(|_err| DecodeError::Backend)?;
    // SAFETY: `GetCompletedValue` is a plain getter.
    if unsafe { fence.GetCompletedValue() } < *fence_value {
        // SAFETY: `fence_event` is a live event handle owned by the session.
        unsafe { fence.SetEventOnCompletion(*fence_value, fence_event) }
            .map_err(|_err| DecodeError::Backend)?;
        // SAFETY: blocking wait on our own event handle.
        unsafe { WaitForSingleObject(fence_event, INFINITE) };
    }
    Ok(())
}
