using Mediaway.Common;
using Mediaway.Pipeline.Interop;

namespace Mediaway.Pipeline;

/// <summary>
/// Live encoder capability probe (<c>adr/pipeline/0007-stream-bytes-aac-decode-support-probe.md</c>
/// §3): which backends could encode a codec at a given size on this machine right now.
/// <para>
/// <b>Costly.</b> Each row opens a throwaway session (a real Media Foundation / VA-API /
/// VideoToolbox session). Call it when a settings screen opens, never per frame or in a loop,
/// and treat the answer as a snapshot: it can change with the driver's state.
/// </para>
/// </summary>
public static class EncoderSupport
{
    /// <summary>
    /// Probe every encode backend for <paramref name="codec"/> <b>at</b> <paramref name="width"/> ×
    /// <paramref name="height"/>. Encoder support is resolution-dependent — a hardware encoder
    /// has minimum and maximum dimensions, so a <see cref="SupportState.Supported"/> answer at one
    /// size implies nothing at another. There is deliberately no resolution-free form: pass the
    /// size you will encode. Audio codecs ignore the size, but it must still be non-zero.
    /// </summary>
    /// <returns>
    /// One row per backend, in the library's order. Empty on a platform with no per-backend
    /// selection.
    /// </returns>
    /// <exception cref="MediawayPipelineException">
    /// <see cref="MediawayPipelineStatus.InvalidInput"/> for a zero width or height.
    /// </exception>
    public static unsafe IReadOnlyList<EncoderCapability> Query(CodecKind codec, uint width, uint height)
    {
        MediawayPipelineException.ThrowIfError(
            NativeMethods.mediaway_encoder_support_at(codec, width, height, out nint rows, out nuint count));

        if (rows == 0 || count == 0)
        {
            return Array.Empty<EncoderCapability>();
        }

        try
        {
            var native = new ReadOnlySpan<NativeEncoderCapability>((void*)rows, (int)count);
            var result = new EncoderCapability[native.Length];
            for (int i = 0; i < native.Length; i++)
            {
                result[i] = new EncoderCapability
                {
                    Backend = native[i].Backend,
                    State = native[i].State,
                    PathClass = native[i].PathClass,
                };
            }

            return result;
        }
        finally
        {
            NativeMethods.mediaway_encoder_support_free(rows, count);
        }
    }

    /// <summary><see cref="Query(CodecKind, uint, uint)"/> for a video codec.</summary>
    public static IReadOnlyList<EncoderCapability> Query(VideoCodec codec, uint width, uint height) =>
        Query((CodecKind)(int)codec, width, height);
}

/// <summary>
/// Live decoder capability probe. Decode has one implementation per platform, so the answer is
/// one <see cref="SupportState"/>, not a list. It is how a caller learns whether AAC decode
/// exists here (<see cref="AudioDecodeSession.OpenAac"/>) without opening a session.
/// <para><b>Costly:</b> opens a throwaway session — call it once, not per frame.</para>
/// </summary>
public static class DecoderSupport
{
    /// <summary>Whether decoding <paramref name="codec"/> is usable on this machine right now.</summary>
    public static SupportState Query(CodecKind codec)
    {
        MediawayPipelineException.ThrowIfError(
            NativeMethods.mediaway_decoder_support(codec, out SupportState state));
        return state;
    }
}
