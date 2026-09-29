#if !NET8_0_OR_GREATER
using System.Runtime.InteropServices;

namespace Mediaway.Container.Interop;

// Replay ring + MP4 placements (adr/container/0009-replay-ring-c-abi.md).
// net8.0 twin: NativeMethods.Replay.LibraryImport.cs.
internal static unsafe partial class NativeMethods
{
    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_replay_ring_create(
        in NativeReplayRingConfig config, out nint outRing);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_replay_ring_add_stream(
        ReplayRingHandle ring, uint streamId, NativeRational timeBase);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_replay_ring_push(
        ReplayRingHandle ring, in NativePacketView packet);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_replay_ring_push_stored(
        ReplayRingHandle ring, in NativePacketMeta meta, in NativeStoredPayload stored);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_replay_ring_span_ms(
        ReplayRingHandle ring, out ulong outSpanMs);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_replay_ring_clip_last(
        ReplayRingHandle ring, ulong spanMs, out nint outClip, out byte outHas);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nuint mediaway_replay_clip_packet_count(ReplayClipHandle clip);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_replay_clip_duration_ms(
        ReplayClipHandle clip, out ulong outMs);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_replay_clip_packet_at(
        ReplayClipHandle clip, nuint index, out NativeReplayClipEntry outEntry);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_replay_clip_free(nint clip);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_replay_ring_close(nint ring);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_muxer_create_with_placements();

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_muxer_poll_placements(
        MuxerHandle muxer, out nint outRows, out nuint outCount);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_placements_free(nint rows, nuint count);
}
#endif
