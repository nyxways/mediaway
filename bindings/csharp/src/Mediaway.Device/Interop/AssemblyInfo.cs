using System.Runtime.CompilerServices;

// MediawayDeviceException.ThrowIfError (internal) is called from every leaf package's
// Interop layer — each leaf package gets its own P/Invoke declarations and native structs
// (adr/0004-domain-feature-split.md mirrored at the C# layer), but they all share this
// base package's status→exception mapping rather than duplicating it four times.
[assembly: InternalsVisibleTo("Mediaway.Device.Camera")]
[assembly: InternalsVisibleTo("Mediaway.Device.Desktop")]
[assembly: InternalsVisibleTo("Mediaway.Device.Audio")]
[assembly: InternalsVisibleTo("Mediaway.Device.Hotplug")]

#if NET8_0_OR_GREATER
// This package now has its own P/Invoke surface too (the GPU device factory,
// NativeStructs.cs) — every native struct there is deliberately kept fully blittable
// (native `bool` is a `byte` field, not `bool`), so this attribute is safe: it disables the
// CLR's general-purpose struct marshalling subsystem, requiring LibraryImport to marshal
// every struct via direct memory layout instead. Only the net8.0 build uses LibraryImport —
// see docs/adr/0018-csharp-netstandard20-unity.md.
[assembly: DisableRuntimeMarshalling]
#endif
