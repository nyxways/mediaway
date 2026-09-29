using System.Buffers;

namespace Mediaway.Container.Interop;

/// <summary>
/// A <see cref="ReadOnlyMemory{Byte}"/> view of one clip entry's payload, <b>borrowed</b> from the
/// native clip and valid until the <see cref="ReplayClip"/> is disposed. Zero-Copy: the native clip
/// shares the ring's <c>Bytes</c> by reference count, and this reads them in place.
/// </summary>
/// <remarks>
/// Disposing the clip does not leave this view dangling silently: <see cref="GetSpan"/> throws
/// <see cref="ObjectDisposedException"/> once the clip is closed, and <see cref="Pin"/> holds a
/// reference on the clip handle until the returned <see cref="MemoryHandle"/> is disposed, so a
/// pinned pointer keeps the native memory alive. The one thing this cannot prevent is a
/// <see cref="Span{T}"/> already obtained and then used after <c>Dispose</c> (the same contract
/// every <c>Span</c> over native memory has); do not keep one across the clip's lifetime.
/// </remarks>
internal sealed unsafe class ClipPayloadMemoryManager : MemoryManager<byte>
{
    private readonly ReplayClipHandle _clip;
    private readonly nint _pointer;
    private readonly int _length;

    internal ClipPayloadMemoryManager(ReplayClipHandle clip, nint pointer, nuint length)
    {
        _clip = clip;
        _pointer = pointer;
        _length = checked((int)length);
    }

    public override Span<byte> GetSpan()
    {
        if (_clip.IsClosed)
        {
            throw new ObjectDisposedException(
                nameof(ReplayClip), "The clip this payload belongs to has been disposed.");
        }

        return new Span<byte>((void*)_pointer, _length);
    }

    public override MemoryHandle Pin(int elementIndex = 0)
    {
        if ((uint)elementIndex > (uint)_length)
        {
            throw new ArgumentOutOfRangeException(nameof(elementIndex));
        }

        var added = false;
        _clip.DangerousAddRef(ref added);
        return new MemoryHandle((void*)(_pointer + elementIndex), pinnable: this);
    }

    public override void Unpin() => _clip.DangerousRelease();

    // The native memory is the clip's, not this view's; nothing to release here.
    protected override void Dispose(bool disposing)
    {
    }
}
