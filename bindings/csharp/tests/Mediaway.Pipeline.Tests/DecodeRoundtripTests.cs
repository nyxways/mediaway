using System.Runtime.InteropServices;
using Mediaway.Common;
using Mediaway.Container;
using Xunit;

namespace Mediaway.Pipeline.Tests;

/// <summary>
/// Exercises the real native <c>mediaway_ffi</c> decode surface end-to-end — mirrors
/// <c>crates/mediaway-ffi/tests/{decode,audio_decode}_smoke.rs</c> and the C++ binding's
/// <c>examples/pipeline/decode_roundtrip.cpp</c> to verify the P/Invoke layer against the
/// actual ABI, not just that it compiles. This machine has a hardware-verified WMF H.264
/// backend, so <see cref="DecoderUnavailableException"/> is treated as a real test failure
/// for the video case; the Opus decode path is cross-platform (<c>mediaway-sw</c>).
/// </summary>
public sealed class DecodeRoundtripTests
{
    private const uint Width = 64;
    private const uint Height = 64;
    private const int FrameCount = 10;

    [Fact]
    public void VideoDecode_EncodeMuxDemuxDecode_RoundTrips()
    {
        // ── encode + mux (mid-gray NV12, no real capture needed) ──────────────
        using var encoder = AutoVideoEncoder.Open(
            VideoEncodeConfig.CreateDefault(VideoCodec.H264, Width, Height, new Rational(1, 30)));
        using var session = EncodeSession.Open(encoder);

        var nv12Len = (int)(Width * Height + (Width * Height / 2));
        var plane = new byte[nv12Len];
        Array.Fill(plane, (byte)128);
        for (long pts = 0; pts < FrameCount; pts++)
        {
            session.WriteFrame(new VideoFrame
            {
                Pts = pts,
                Duration = 1,
                Width = Width,
                Height = Height,
                PixelFormat = PixelFormat.Nv12,
                Data = plane,
            });
        }

        using var fmp4 = session.Finish();

        // ── demux: recover real H.264 packets + AVCC extra_data ────────────────
        using var demuxer = new Demuxer();
        demuxer.PushBytes(fmp4.Memory.Span);
        var streams = demuxer.Streams;
        Assert.Single(streams);
        using var videoStream = streams[0];
        Assert.True(videoStream.Geometry.HasValue);
        Assert.True(videoStream.ExtraData.Length > 0, "expected AVCC extra_data");
        var extraData = videoStream.ExtraData.ToArray();

        var packets = new List<Packet>();
        while (demuxer.PollPacket() is { } packet)
        {
            packets.Add(packet);
        }
        Assert.Equal(FrameCount, packets.Count);

        // ── decode: extra_data supplied at open time (adr/0004 §1) ────────────
        using var decodeSession = DecodeSession.Open(new VideoDecodeConfig
        {
            Codec = VideoCodec.H264,
            Width = Width,
            Height = Height,
            TimeBase = new Rational(1, 30),
            ExtraData = extraData,
        });

        foreach (var packet in packets)
        {
            decodeSession.PushPacket(new DecodePacket
            {
                Pts = packet.Pts,
                Dts = packet.Dts,
                Duration = packet.Duration,
                IsKeyframe = packet.IsKeyframe,
                Payload = packet.Payload,
            });
            packet.Dispose();
        }
        decodeSession.Flush();

        var decoded = 0;
        while (decodeSession.PollFrame() is { } frame)
        {
            Assert.Equal(Width, frame.Width);
            Assert.Equal(Height, frame.Height);
            Assert.True(frame.Data.Length >= nv12Len, "decoded frame implausibly small");
            decoded++;
        }
        Assert.True(decoded > 0, "expected at least one decoded frame");
    }

    private const uint SampleRate = 48_000;
    private const ushort Channels = 1;
    private const int FrameSamples = 960; // 20ms @ 48kHz mono
    private const int AudioFrameCount = 50;

    [Fact]
    public unsafe void AudioDecode_OpusEncodeDecode_RoundTrips()
    {
        // ── encode via the raw C ABI (Mediaway.Pipeline.AudioEncoder is AAC-only today) ──
        var encConfig = new RawNativeAudioEncodeConfig
        {
            Codec = CodecKind.Opus,
            SampleRate = SampleRate,
            Channels = Channels,
            SampleFormat = SampleFormat.F32,
            TimeBaseNum = 1,
            TimeBaseDen = 50,
            BitrateBps = 0,
        };
        var openStatus = RawNativeMethods.mediaway_audio_encoder_open(in encConfig, out nint encSession);
        if (openStatus == MediawayPipelineStatus.NoBackend)
        {
            return; // Opus encode unavailable on this build — graceful skip, not a failure.
        }
        Assert.Equal(MediawayPipelineStatus.Ok, openStatus);

        var encoded = new List<(long Pts, byte[] Data)>();
        for (var i = 0; i < AudioFrameCount; i++)
        {
            var pcm = new float[FrameSamples];
            for (var s = 0; s < FrameSamples; s++)
            {
                var t = (double)((i * FrameSamples) + s) / SampleRate;
                pcm[s] = (float)Math.Sin(2 * Math.PI * 440 * t);
            }

            fixed (float* pcmPtr = pcm)
            {
                var view = new RawNativeAudioFrameView
                {
                    Pts = i,
                    Duration = 0,
                    SampleRate = SampleRate,
                    Channels = Channels,
                    SampleFormat = SampleFormat.F32,
                    Data = (byte*)pcmPtr,
                    DataLen = (nuint)(pcm.Length * sizeof(float)),
                };
                Assert.Equal(MediawayPipelineStatus.Ok,
                    RawNativeMethods.mediaway_audio_encode_session_push_pcm(encSession, in view));
            }

            while (true)
            {
                var pollStatus = RawNativeMethods.mediaway_audio_encode_session_poll_packet(
                    encSession, out RawNativeAudioPacket packet, out byte hasPacket);
                Assert.Equal(MediawayPipelineStatus.Ok, pollStatus);
                if (hasPacket == 0)
                {
                    break;
                }

                var data = new ReadOnlySpan<byte>(packet.Payload, (int)packet.PayloadLen).ToArray();
                encoded.Add((packet.Pts, data));
                RawNativeMethods.mediaway_pipeline_ffi_packet_free(ref packet);
            }
        }
        RawNativeMethods.mediaway_audio_encode_session_flush(encSession);
        RawNativeMethods.mediaway_audio_encode_session_close(encSession);
        Assert.True(encoded.Count > 0, "expected at least one encoded Opus packet");

        // ── decode via the public wrapper ───────────────────────────────────────
        using var decodeSession = AudioDecodeSession.Open(SampleRate, Channels, new Rational(1, 50));
        foreach (var (pts, data) in encoded)
        {
            decodeSession.PushPacket(new DecodePacket { Pts = pts, Payload = data });
        }
        decodeSession.Flush();

        var decoded = 0;
        while (decodeSession.PollFrame() is { } frame)
        {
            Assert.Equal(SampleRate, frame.SampleRate);
            Assert.Equal(Channels, frame.Channels);
            decoded++;
        }
        Assert.True(decoded > 0, "expected at least one decoded Opus frame");
    }

    // Minimal test-local P/Invoke for the Opus encode side only — Mediaway.Pipeline.Interop's
    // NativeMethods is internal to that assembly (no InternalsVisibleTo to this test project),
    // matching Mediaway.Device.Tests' precedent of a test-only raw P/Invoke declaration rather
    // than exposing internals for test convenience.
    [StructLayout(LayoutKind.Sequential)]
    private struct RawNativeAudioEncodeConfig
    {
        public CodecKind Codec;
        public uint SampleRate;
        public ushort Channels;
        public SampleFormat SampleFormat;
        public ulong TimeBaseNum;
        public uint TimeBaseDen;
        public uint BitrateBps;
    }

    [StructLayout(LayoutKind.Sequential)]
    private unsafe struct RawNativeAudioFrameView
    {
        public long Pts;
        public ulong Duration;
        public uint SampleRate;
        public ushort Channels;
        public SampleFormat SampleFormat;
        public byte* Data;
        public nuint DataLen;
    }

    [StructLayout(LayoutKind.Sequential)]
    private unsafe struct RawNativeAudioPacket
    {
        public long Pts;
        public long Dts;
        public ulong Duration;
        public byte IsKeyframe;
        public byte IsDiscard;
        public byte* Payload;
        public nuint PayloadLen;
    }

    private static unsafe class RawNativeMethods
    {
        private const string LibraryName = "mediaway_ffi";

        [DllImport(LibraryName, ExactSpelling = true)]
        internal static extern MediawayPipelineStatus mediaway_audio_encoder_open(
            in RawNativeAudioEncodeConfig config, out nint outSession);

        [DllImport(LibraryName, ExactSpelling = true)]
        internal static extern MediawayPipelineStatus mediaway_audio_encode_session_push_pcm(
            nint session, in RawNativeAudioFrameView frame);

        [DllImport(LibraryName, ExactSpelling = true)]
        internal static extern MediawayPipelineStatus mediaway_audio_encode_session_poll_packet(
            nint session, out RawNativeAudioPacket outPacket, out byte outHasPacket);

        [DllImport(LibraryName, ExactSpelling = true)]
        internal static extern MediawayPipelineStatus mediaway_audio_encode_session_flush(nint session);

        [DllImport(LibraryName, ExactSpelling = true)]
        internal static extern void mediaway_audio_encode_session_close(nint session);

        [DllImport(LibraryName, ExactSpelling = true)]
        internal static extern void mediaway_pipeline_ffi_packet_free(ref RawNativeAudioPacket packet);
    }
}
