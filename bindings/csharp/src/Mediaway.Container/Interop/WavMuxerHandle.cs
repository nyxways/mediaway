using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>
/// Owns one native <c>mediaway_wav_muxer_t*</c>. Unlike <see cref="MuxerHandle"/>,
/// <c>mediaway_wav_muxer_finish</c> consumes only the Rust-side internal state, not this
/// handle — the handle itself is always released via <c>mediaway_wav_muxer_close</c>,
/// finished or not (safe per the header's own doc comment).
/// </summary>
internal sealed class WavMuxerHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private WavMuxerHandle() : base(ownsHandle: true)
    {
    }

    internal static WavMuxerHandle Create(uint sampleRate, ushort channels, ushort bitsPerSample)
    {
        var instance = new WavMuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_wav_muxer_create(sampleRate, channels, bitsPerSample));
        return instance;
    }

    internal static WavMuxerHandle CreateWithFormat(in NativeWaveFormat format)
    {
        var instance = new WavMuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_wav_muxer_create_with_format(in format));
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_wav_muxer_close(handle);
        return true;
    }
}
