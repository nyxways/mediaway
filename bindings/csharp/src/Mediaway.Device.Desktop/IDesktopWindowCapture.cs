namespace Mediaway.Device.Desktop;

/// <summary>
/// A single-window (WGC) Zero-Copy video capture session. Polling and frame release follow
/// <see cref="IDesktopVideoCapture"/>'s rules exactly.
/// </summary>
public interface IDesktopWindowCapture : IDesktopVideoCapture
{
    /// <summary>
    /// Whether the OS actually hid the capture border, read once when the session opened. Only
    /// meaningful after <see cref="WindowBorder.Hidden"/> was requested; <see langword="false"/>
    /// when it was refused (or older than Windows 11 build 22000).
    /// </summary>
    bool BorderHidden { get; }
}
