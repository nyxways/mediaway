namespace Mediaway.Common;

/// <summary>
/// Codec kind — mirrors the native <c>mediaway_codec_kind_t</c> shared across every
/// <c>mediaway-*-ffi</c> C ABI. Pre-1.0: values may be renumbered; do not persist these
/// across builds.
/// </summary>
public enum CodecKind
{
    /// <summary>H.264 / AVC video.</summary>
    H264 = 0,

    /// <summary>HEVC / H.265 video.</summary>
    Hevc = 1,

    /// <summary>AV1 video.</summary>
    Av1 = 2,

    /// <summary>VP9 video.</summary>
    Vp9 = 3,

    /// <summary>AAC audio.</summary>
    Aac = 4,

    /// <summary>Opus audio.</summary>
    Opus = 5,

    /// <summary>MP3 (MPEG-1/2/2.5 Layer III) audio.</summary>
    Mp3 = 6,

    /// <summary>Vorbis audio.</summary>
    Vorbis = 7,

    /// <summary>WebVTT subtitle.</summary>
    WebVtt = 8,

    /// <summary>Tx3g timed text subtitle.</summary>
    Tx3g = 9,

    /// <summary>Uncompressed / raw video.</summary>
    RawVideo = 10,

    /// <summary>Uncompressed / raw PCM audio.</summary>
    RawAudio = 11,
}
