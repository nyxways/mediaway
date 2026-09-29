namespace Mediaway.Device.Desktop;

/// <summary>Whether the mouse pointer is drawn into captured frames.</summary>
public enum CursorCapture
{
    /// <summary>Not drawn — the default. Every backend supports this.</summary>
    Excluded = 0,

    /// <summary>Drawn where it overlaps the source. DXGI Screen capture answers <see cref="MediawayDeviceStatus.Unsupported"/> rather than record without it.</summary>
    Included = 1,
}

/// <summary>Whether Windows draws its capture border around a captured window.</summary>
public enum WindowBorder
{
    /// <summary>Leave the OS default: the border is shown.</summary>
    Shown = 0,

    /// <summary>
    /// Ask the OS not to draw it (Windows 11 build 22000+). A refusal is not an error at open —
    /// the border is drawn on screen, never into frames — so read
    /// <see cref="IDesktopWindowCapture.BorderHidden"/> to learn what actually happened.
    /// </summary>
    Hidden = 1,
}

/// <summary>How the frame dimensions of a window capture relate to the window's.</summary>
public enum FrameDimensions
{
    /// <summary>Exactly the window's size, odd or not.</summary>
    Native = 0,

    /// <summary>
    /// Each odd axis loses its last column or row, so any hardware encoder accepts the frames.
    /// Costs nothing on WGC (the frame pool is simply sized down).
    /// </summary>
    EvenCropped = 1,
}

/// <summary>A rectangle of the captured surface, in surface pixels. Zero width or height is never valid.</summary>
/// <remarks>
/// A region at <c>(0, 0)</c> is free (WGC sizes its frame pool to it). A region anywhere else
/// costs one GPU copy per frame — not Zero-Copy.
/// </remarks>
public readonly record struct CaptureRegion(uint X, uint Y, uint Width, uint Height);

/// <summary>
/// Options for <see cref="DesktopWindowCapture.Open"/>. Every default is the previous behaviour:
/// whole window, no pointer, OS default border, native dimensions.
/// </summary>
public sealed class WindowCaptureOptions
{
    /// <summary>Whether the pointer is drawn into frames. Default <see cref="CursorCapture.Excluded"/>.</summary>
    public CursorCapture Cursor { get; init; } = CursorCapture.Excluded;

    /// <summary>Default <see cref="WindowBorder.Shown"/>.</summary>
    public WindowBorder Border { get; init; } = WindowBorder.Shown;

    /// <summary>Default <see cref="FrameDimensions.Native"/>.</summary>
    public FrameDimensions Dimensions { get; init; } = FrameDimensions.Native;

    /// <summary>
    /// Record only this rectangle of the window; <see langword="null"/> records all of it. A
    /// window that later shrinks below the region fails with
    /// <see cref="MediawayDeviceStatus.RegionOutOfBounds"/>, never a padded frame.
    /// </summary>
    public CaptureRegion? Region { get; init; }
}
