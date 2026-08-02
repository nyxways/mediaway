namespace Mediaway.Device.Desktop;

/// <summary>
/// A Screen (DXGI Desktop Duplication) Zero-Copy video capture session.
/// </summary>
/// <remarks>
/// Deliberately narrower than <c>Mediaway.Device.Camera.IVideoCapture</c>: no
/// <c>TryPollFrame</c>-drives-<c>ReadFramesAsync</c> convenience, and
/// <see cref="ReleaseFrame"/> is a real, caller-driven step rather than something this
/// binding calls automatically after every poll. A GPU-backed frame's
/// <see cref="DesktopVideoFrame.GpuBuffer"/> stays valid for the whole time the caller is
/// using it (e.g. feeding it into a Zero-Copy video encoder) — auto-releasing immediately
/// after <see cref="TryPollFrame"/> (as Camera's CPU-only session safely does) would
/// invalidate the texture handle before the caller ever gets to read it. Buffering
/// multiple in-flight frames in a channel (the mechanism <c>ReadFramesAsync</c> is built
/// on) is similarly unsafe here — a queued GPU handle can be overwritten by the shared
/// duplication session before it is drained. Call <see cref="TryPollFrame"/>, use the
/// frame, then call <see cref="ReleaseFrame"/> before polling again.
/// </remarks>
public interface IDesktopVideoCapture : IDisposable
{
    /// <summary>The negotiated frame width — only known after the backend has negotiated with the OS.</summary>
    uint Width { get; }

    /// <summary>The negotiated frame height.</summary>
    uint Height { get; }

    /// <summary>
    /// Non-blocking poll for one frame. Returns <see langword="false"/> if nothing was
    /// ready yet this call (delta-based: DXGI only delivers a new frame when the desktop
    /// image or cursor changes) — not necessarily an error. <paramref name="frame"/> is
    /// <see langword="null"/> in that case.
    /// </summary>
    bool TryPollFrame(out DesktopVideoFrame? frame);

    /// <summary>
    /// Releases backend resources held by the last polled frame (e.g. DXGI
    /// <c>ReleaseFrame</c>). Must be called before the next <see cref="TryPollFrame"/> that
    /// acquires a new frame — this is the point at which a previously returned
    /// <see cref="DesktopVideoFrame.GpuBuffer"/> becomes invalid, not
    /// <see cref="DesktopVideoFrame.Dispose"/> (which is a no-op for the GPU case; see
    /// that type's docs).
    /// </summary>
    void ReleaseFrame();
}
