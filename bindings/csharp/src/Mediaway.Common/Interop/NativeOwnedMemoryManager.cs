using System.Buffers;

namespace Mediaway.Common.Interop;

/// <summary>
/// Wraps a native-owned byte buffer as <see cref="ReadOnlyMemory{Byte}"/> with zero
/// copying. The wrapped native buffer is released through <c>release</c> only when this
/// manager is disposed — never eagerly copied into a managed array. Every
/// <c>mediaway-*-ffi</c> capability package uses this for its owned output buffers (e.g.
/// muxed container bytes, demuxed packet payloads); only the native <c>release</c>
/// delegate (that capability's own <c>*_free</c> function) differs.
/// </summary>
/// <remarks>
/// The wrapped pointer is native (not GC-managed) memory, so <see cref="Pin"/> /
/// <see cref="Unpin"/> are no-ops — there is nothing for the GC to move.
/// </remarks>
public sealed unsafe class NativeOwnedMemoryManager : MemoryManager<byte>
{
    private readonly nint _pointer;
    private readonly nuint _length;
    private readonly Action<nint, nuint> _release;
    private bool _released;

    public NativeOwnedMemoryManager(nint pointer, nuint length, Action<nint, nuint> release)
    {
        _pointer = pointer;
        _length = length;
        _release = release;
    }

    // No finalizer, deliberately: unlike a SafeHandle (a pointer only, no aliasing concern),
    // a Span<byte> obtained from GetSpan() may still be in use when a finalizer would run —
    // freeing the buffer under it would be a use-after-free, not just late cleanup (CA2015).
    // The caller must Dispose this deterministically (or the ReadOnlyMemory<byte> it wraps
    // via `using`-equivalent ownership of the underlying IMemoryOwner<byte>).

    public override Span<byte> GetSpan() =>
        _pointer == 0 ? Span<byte>.Empty : new Span<byte>((void*)_pointer, checked((int)_length));

    public override MemoryHandle Pin(int elementIndex = 0)
    {
        if ((nuint)elementIndex > _length)
        {
            throw new ArgumentOutOfRangeException(nameof(elementIndex));
        }

        return new MemoryHandle((void*)(_pointer + elementIndex));
    }

    public override void Unpin()
    {
        // No-op: the wrapped pointer is native memory, never subject to GC relocation.
    }

    protected override void Dispose(bool disposing)
    {
        if (_released)
        {
            return;
        }

        _released = true;
        _release(_pointer, _length);
    }
}
