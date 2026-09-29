#if NET8_0_OR_GREATER
using System.Runtime.InteropServices;

namespace Mediaway.Container.Interop;

// Replay ring + MP4 placements (adr/container/0009-replay-ring-c-abi.md).
// netstandard2.0 twin: NativeMethods.Replay.DllImport.cs.
internal static unsafe partial class NativeMethods
{
    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_replay_ring_create(
        in NativeReplayRingConfig config, out nint outRing);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_replay_ring_add_stream(
        ReplayRingHandle ring, uint streamId, NativeRational timeBase);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_replay_ring_push(
        ReplayRingHandle ring, in NativePacketView packet);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_replay_ring_push_stored(
        ReplayRingHandle ring, in NativePacketMeta meta, in NativeStoredPayload stored);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_replay_ring_span_ms(
        ReplayRingHandle ring, out ulong outSpanMs);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_replay_ring_clip_last(
        ReplayRingHandle ring, ulong spanMs, out nint outClip, out byte outHas);

    [LibraryImport(LibraryName)]
    internal static partial nuint mediaway_replay_clip_packet_count(ReplayClipHandle clip);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_replay_clip_duration_ms(
        ReplayClipHandle clip, out ulong outMs);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_replay_clip_packet_at(
        ReplayClipHandle clip, nuint index, out NativeReplayClipEntry outEntry);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_replay_clip_free(nint clip);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_replay_ring_close(nint ring);

    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_muxer_create_with_placements();

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_muxer_poll_placements(
        MuxerHandle muxer, out nint outRows, out nuint outCount);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_placements_free(nint rows, nuint count);
}
#endif
