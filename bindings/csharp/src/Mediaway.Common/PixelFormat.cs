namespace Mediaway.Common;

/// <summary>
/// Raw video/frame pixel layout — one C# type shared by every capability package that
/// deals with raw frames (<c>Mediaway.Pipeline</c>, <c>Mediaway.Device</c>), even though
/// the Rust C ABI still defines it separately per header
/// (<c>mediaway_pixel_format_t</c> in both <c>pipeline.h</c> and <c>device.h</c>, not yet
/// unified into <c>mediaway-common-ffi</c>). Values are numerically identical to both.
/// </summary>
public enum PixelFormat
{
    Nv12 = 0,
    I420 = 1,
    Bgra8 = 2,
    Rgba8 = 3,
    Yuyv = 4,
}
