namespace Mediaway.Device.Interop;

/// <summary>
/// Raw P/Invoke declarations, 1:1 with <c>mediaway_gpu_*</c> in
/// <c>crates/mediaway-ffi/src/device/gpu.rs</c>. Never public — every call is wrapped by
/// <see cref="Device.GpuDevice"/> into a safe, idiomatic surface.
///
/// Declarations split across <c>NativeMethods.LibraryImport.cs</c> (net8.0,
/// source-generated) and <c>NativeMethods.DllImport.cs</c> (netstandard2.0, classic runtime
/// marshalling) — see docs/adr/0018-csharp-netstandard20-unity.md. Exactly one half compiles
/// per target framework.
/// </summary>
internal static unsafe partial class NativeMethods
{
    private const string LibraryName = "mediaway_ffi";
}
