namespace Mediaway.Common;

/// <summary>
/// Audio PCM sample layout — one C# type shared by every capability package that deals
/// with raw PCM (<c>Mediaway.Device.Audio</c>, <c>Mediaway.Device.Desktop</c>), matching
/// <see cref="PixelFormat"/>'s precedent: values are numerically identical across both
/// Rust headers' <c>mediaway_sample_format_t</c>. Only <see cref="F32"/> is accepted by
/// the real Windows WASAPI backend today.
/// </summary>
public enum SampleFormat
{
    S16 = 0,
    S32 = 1,
    F32 = 2,
}
