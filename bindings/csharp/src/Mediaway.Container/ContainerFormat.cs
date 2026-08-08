namespace Mediaway.Container;

/// <summary>
/// Which format <see cref="Muxer"/>/<see cref="Demuxer"/> open — mirrors
/// <c>mediaway_container_format_t</c>. Only formats sharing MP4's multi-track, typestated
/// (Open → Live) track-registration shape are reachable here; Ogg/ADTS/FLV/MPEG-TS/MP3/WAV
/// have their own dedicated classes (<see cref="OggMuxer"/>, <see cref="AdtsMuxer"/>,
/// <see cref="FlvMuxer"/>, <see cref="TsMuxer"/>, <see cref="Mp3Muxer"/>, <see cref="WavMuxer"/>).
/// </summary>
public enum ContainerFormat
{
    /// <summary>Fragmented MP4 (ISOBMFF) — the default for <see cref="Muxer"/>/<see cref="Demuxer"/>.</summary>
    Mp4 = 0,

    /// <summary>WebM (Matroska/EBML).</summary>
    WebM = 1,
}
