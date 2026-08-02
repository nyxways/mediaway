namespace Mediaway.Device.Desktop.Interop;

/// <summary>
/// Raw P/Invoke declarations, 1:1 with <c>mediaway_desktop_capture_*</c> (Screen video,
/// <c>crates/mediaway-device-ffi/src/desktop_video.rs</c>) and
/// <c>mediaway_desktop_audio_capture_*</c> (Loopback/ProcessLoopback,
/// <c>crates/mediaway-device-ffi/src/desktop_audio.rs</c>) — both under the native
/// crate's <c>"desktop"</c> feature (<c>adr/0004-domain-feature-split.md</c>). Never
/// public — every call is wrapped by <see cref="Device.DesktopScreenCapture"/>/
/// <see cref="Device.DesktopAudioCapture"/> into a safe, idiomatic surface.
///
/// Declarations split across <c>NativeMethods.LibraryImport.cs</c> (net8.0,
/// source-generated) and <c>NativeMethods.DllImport.cs</c> (netstandard2.0, classic runtime
/// marshalling) — see docs/adr/0018-csharp-netstandard20-unity.md. Exactly one half compiles
/// per target framework.
/// </summary>
internal static unsafe partial class NativeMethods
{
    private const string LibraryName = "mediaway_device_ffi";
}
