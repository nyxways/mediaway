using System;
using System.Runtime.InteropServices;
using Mediaway.Common;
using Mediaway.Device.Audio;
using UnityEngine;

namespace Mediaway.Unity
{
    /// <summary>
    /// Polls an <see cref="IAudioCapture"/> (e.g. <c>Mediaway.Device.Audio.Microphone</c> — spelled
    /// out in a &lt;c&gt; tag, not a &lt;see cref&gt;, since it collides with
    /// <see cref="UnityEngine.Microphone"/> on the bare name) via
    /// <see cref="IAudioCapture.TryPollFrame"/> once per <see cref="Update"/> and feeds the
    /// interleaved float PCM into a streaming <see cref="AudioClip"/> played by this
    /// <see cref="UnityEngine.AudioSource"/>. UNVERIFIED — see this package's README.
    /// </summary>
    /// <remarks>
    /// Assumes <see cref="SampleFormat.F32"/> — the format <c>Mediaway.Device.Audio.Microphone</c>
    /// opens with today. A capture source using a different <see cref="SampleFormat"/> throws
    /// from <see cref="Update"/> rather than silently reinterpreting bytes.
    /// </remarks>
    [RequireComponent(typeof(AudioSource))]
    public sealed class MediawayStreamingAudioSource : MonoBehaviour
    {
        /// <summary>
        /// Ring buffer capacity in samples (not bytes/frames) — sized generously above one
        /// Unity audio buffer's worth so <see cref="OnAudioRead"/> underruns only under a real
        /// stall, not routine jitter between <see cref="Update"/> polls.
        /// </summary>
        private const int RingCapacitySamples = 48_000 * 2; // ~0.5s of 48kHz stereo.

        private readonly object _ringLock = new object();
        private float[] _ring = new float[RingCapacitySamples];
        private int _ringReadPos;
        private int _ringWritePos;
        private int _ringCount;

        private IAudioCapture? _capture;
        private int _channels;
        private AudioSource? _audioSource;

        /// <summary>
        /// Starts streaming from <paramref name="capture"/>. Call once; this component takes
        /// ownership of <paramref name="capture"/> and disposes it in <see cref="OnDestroy"/>.
        /// </summary>
        public void Begin(IAudioCapture capture, int channels, int sampleRate)
        {
            _capture = capture;
            _channels = channels;
            _audioSource = GetComponent<AudioSource>();

            // `stream: true` -> Unity calls OnAudioRead on its own audio thread as playback
            // consumes samples, instead of requiring the whole clip up front.
            var clip = AudioClip.Create(
                "MediawayStream", lengthSamples: sampleRate, channels: channels,
                frequency: sampleRate, stream: true, pcmreadercallback: OnAudioRead);

            _audioSource.clip = clip;
            _audioSource.loop = true;
            _audioSource.Play();
        }

        private void Update()
        {
            if (_capture is null)
            {
                return;
            }

            // Drain everything currently ready — Update() runs once per video frame, which is
            // much slower than the audio device's real sample rate, so more than one captured
            // AudioFrame is normally waiting per Update().
            while (_capture.TryPollFrame(out var frame))
            {
                using (frame) // AudioFrame owns a native buffer (Zero-Copy poll output).
                {
                    if (frame!.SampleFormat != SampleFormat.F32)
                    {
                        throw new NotSupportedException(
                            $"MediawayStreamingAudioSource only supports {nameof(SampleFormat.F32)} " +
                            $"today, got {frame.SampleFormat}.");
                    }

                    WriteToRing(MemoryMarshal.Cast<byte, float>(frame.Data.Span));
                }
            }
        }

        private void WriteToRing(ReadOnlySpan<float> samples)
        {
            lock (_ringLock)
            {
                foreach (float sample in samples)
                {
                    if (_ringCount == _ring.Length)
                    {
                        // Ring is full — a real stall (audio thread not consuming fast enough).
                        // Drop the oldest sample rather than growing unbounded or blocking
                        // Update(); an occasional glitch under real overload beats an
                        // ever-growing buffer or a stuck main thread.
                        _ringReadPos = (_ringReadPos + 1) % _ring.Length;
                        _ringCount--;
                    }

                    _ring[_ringWritePos] = sample;
                    _ringWritePos = (_ringWritePos + 1) % _ring.Length;
                    _ringCount++;
                }
            }
        }

        private void OnAudioRead(float[] data)
        {
            lock (_ringLock)
            {
                int n = Math.Min(data.Length, _ringCount);
                for (int i = 0; i < n; i++)
                {
                    data[i] = _ring[_ringReadPos];
                    _ringReadPos = (_ringReadPos + 1) % _ring.Length;
                }

                _ringCount -= n;

                // Underrun: not enough captured audio ready yet — fill the rest with silence
                // rather than replaying stale samples.
                for (int i = n; i < data.Length; i++)
                {
                    data[i] = 0f;
                }
            }
        }

        private void OnDestroy() => _capture?.Dispose();
    }
}
