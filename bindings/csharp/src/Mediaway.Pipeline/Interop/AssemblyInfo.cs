#if NET8_0_OR_GREATER
using System.Runtime.CompilerServices;

// Every native struct in this assembly (NativeStructs.cs) is deliberately kept fully
// blittable (native `bool` would be a `byte` field, not `bool` — this ABI's structs happen
// to have none, but the convention is kept consistent with Mediaway.Container) so this
// attribute is safe: it disables the CLR's general-purpose struct marshalling subsystem,
// requiring LibraryImport to marshal every struct via direct memory layout instead. Only the
// net8.0 build uses LibraryImport — see docs/adr/0018-csharp-netstandard20-unity.md.
[assembly: DisableRuntimeMarshalling]
#endif
