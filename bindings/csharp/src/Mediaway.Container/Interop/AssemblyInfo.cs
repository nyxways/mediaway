#if NET8_0_OR_GREATER
using System.Runtime.CompilerServices;

// Every native struct in this assembly (NativeStructs.cs) is deliberately kept fully
// blittable (native `bool` is a `byte` field, not `bool`) precisely so this attribute is
// safe: it disables the CLR's general-purpose struct marshalling subsystem, requiring
// LibraryImport to marshal every struct via direct memory layout instead. Only the net8.0
// build uses LibraryImport — the netstandard2.0 build (see NativeMethods.DllImport.cs) uses
// classic DllImport runtime marshalling instead, and this attribute type does not exist in
// the netstandard2.0 BCL. See docs/adr/0018-csharp-netstandard20-unity.md.
[assembly: DisableRuntimeMarshalling]
#endif
