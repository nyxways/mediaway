using System.Runtime.CompilerServices;

// Mediaway.Pipeline's capture-to-encode bridge (EncodeSession.WriteFrameFromCameraCapture)
// downcasts a caller's IVideoCapture to the concrete CameraCaptureSession to reach its
// internal CameraCaptureHandle — the raw handle the native bridge functions take
// (adr/pipeline/0005-capture-encode-bridge-c-abi.md). Not gated by NET8_0_OR_GREATER: this
// has nothing to do with struct marshalling, and Pipeline needs it under both TFMs.
[assembly: InternalsVisibleTo("Mediaway.Pipeline")]

#if NET8_0_OR_GREATER
// Every native struct in this assembly (NativeStructs.cs) is deliberately kept fully
// blittable (native `bool` is a `byte` field, not `bool`) so this attribute is safe: it
// disables the CLR's general-purpose struct marshalling subsystem, requiring LibraryImport
// to marshal every struct via direct memory layout instead. Only the net8.0 build uses
// LibraryImport — see docs/adr/0018-csharp-netstandard20-unity.md.
[assembly: DisableRuntimeMarshalling]
#endif
