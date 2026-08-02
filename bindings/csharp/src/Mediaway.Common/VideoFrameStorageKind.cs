namespace Mediaway.Common;

/// <summary>
/// Which of a GPU-capable video frame's two storage representations is valid — mirrors
/// <c>mediaway_video_frame_storage_kind_t</c>. Shared shape between
/// <c>Mediaway.Device.Desktop</c>'s <c>DesktopVideoFrame</c> (polled output) and
/// <c>Mediaway.Pipeline</c>'s native frame interop (encode input); each consumer's own
/// type documents which fields <see cref="Cpu"/>/<see cref="Gpu"/> make valid.
/// </summary>
public enum VideoFrameStorageKind
{
    /// <summary>The CPU byte buffer is valid; the GPU handle is unused/zeroed.</summary>
    Cpu = 0,

    /// <summary>The GPU handle is valid; the CPU byte buffer is empty.</summary>
    Gpu = 1,
}
