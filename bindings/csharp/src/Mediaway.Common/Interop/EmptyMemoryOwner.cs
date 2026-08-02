using System.Buffers;

namespace Mediaway.Common.Interop;

/// <summary>
/// Shared no-op <see cref="IMemoryOwner{T}"/> for the "nothing ready yet" case of a native
/// poll that returns a null/zero-length owned buffer — avoids allocating a
/// <see cref="NativeOwnedMemoryManager"/> (and a native release call) when there is nothing
/// to release.
/// </summary>
public sealed class EmptyMemoryOwner<T> : IMemoryOwner<T>
{
    public static readonly EmptyMemoryOwner<T> Instance = new();

    private EmptyMemoryOwner()
    {
    }

    public Memory<T> Memory => Memory<T>.Empty;

    public void Dispose()
    {
        // Nothing to release.
    }
}
