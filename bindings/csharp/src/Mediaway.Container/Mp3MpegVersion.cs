namespace Mediaway.Container;

/// <summary>MPEG audio version — mirrors <c>mediaway_mpeg_version_t</c>.</summary>
public enum Mp3MpegVersion
{
    /// <summary>MPEG-1 — 44100/48000/32000 Hz family.</summary>
    Mpeg1 = 0,

    /// <summary>MPEG-2 — 22050/24000/16000 Hz family.</summary>
    Mpeg2 = 1,

    /// <summary>MPEG-2.5 — 11025/12000/8000 Hz family (unofficial low-rate extension).</summary>
    Mpeg25 = 2,
}
