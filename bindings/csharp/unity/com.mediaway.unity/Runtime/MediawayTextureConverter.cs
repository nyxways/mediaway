using System;
using Mediaway.Common;
using Mediaway.Device.Camera;
using UnityEngine;

namespace Mediaway.Unity
{
    /// <summary>
    /// Converts a captured <see cref="VideoFrame"/> into a Unity <see cref="Texture2D"/>.
    /// UNVERIFIED — see this package's README: written with no Unity Editor available to
    /// compile or run it against.
    /// </summary>
    public static class MediawayTextureConverter
    {
        /// <summary>
        /// Creates (or, if <paramref name="reuse"/> matches the frame's geometry, updates in
        /// place) a <see cref="Texture2D"/> from <paramref name="frame"/>.
        /// </summary>
        /// <remarks>
        /// <see cref="PixelFormat.Bgra8"/>/<see cref="PixelFormat.Rgba8"/> upload directly —
        /// they match <see cref="TextureFormat.BGRA32"/>/<see cref="TextureFormat.RGBA32"/>
        /// byte-for-byte, no conversion. <see cref="PixelFormat.Nv12"/>/
        /// <see cref="PixelFormat.I420"/> go through <see cref="ConvertYuvToRgba32"/> — a real,
        /// documented CPU/software cost (no GPU shader path ships yet), not a Zero-Copy
        /// conversion. See this package's README.
        /// </remarks>
        public static unsafe Texture2D CreateOrUpdate(VideoFrame frame, Texture2D? reuse = null)
        {
            Texture2D tex = (reuse != null && reuse.width == (int)frame.Width && reuse.height == (int)frame.Height)
                ? reuse
                : new Texture2D((int)frame.Width, (int)frame.Height, TextureFormat.RGBA32, mipChain: false);

            switch (frame.PixelFormat)
            {
                case PixelFormat.Bgra8:
                    UploadRaw(tex, frame.Data.Span, TextureFormat.BGRA32);
                    break;
                case PixelFormat.Rgba8:
                    UploadRaw(tex, frame.Data.Span, TextureFormat.RGBA32);
                    break;
                case PixelFormat.Nv12:
                case PixelFormat.I420:
                    var rgba = new byte[frame.Width * frame.Height * 4];
                    ConvertYuvToRgba32(frame.Data.Span, frame.PixelFormat, (int)frame.Width, (int)frame.Height, rgba);
                    tex.LoadRawTextureData(rgba);
                    tex.Apply(updateMipmaps: false);
                    break;
                default:
                    throw new NotSupportedException(
                        $"MediawayTextureConverter does not support {frame.PixelFormat} yet.");
            }

            return tex;
        }

        private static unsafe void UploadRaw(Texture2D tex, ReadOnlySpan<byte> data, TextureFormat expectedFormat)
        {
            if (tex.format != expectedFormat)
            {
                // Reusing a texture across a pixel-format change is a caller bug (frame source
                // format should not change mid-stream) — fail loudly rather than silently
                // reinterpreting bytes.
                throw new InvalidOperationException(
                    $"Texture format {tex.format} does not match frame pixel format " +
                    $"(expected {expectedFormat}). Do not reuse a texture across a format change.");
            }

            fixed (byte* ptr = data)
            {
                tex.LoadRawTextureData((IntPtr)ptr, data.Length);
            }

            tex.Apply(updateMipmaps: false);
        }

        /// <summary>
        /// CPU/software NV12 or I420 -&gt; RGBA32 (BT.601 full-range coefficients). A real,
        /// non-trivial per-pixel cost — see this file's/README's honesty note. Written for
        /// correctness, not throughput; a GPU shader-based conversion is the natural follow-up
        /// once this ships and gets measured in a real Unity project.
        /// </summary>
        public static void ConvertYuvToRgba32(
            ReadOnlySpan<byte> yuv, PixelFormat format, int width, int height, Span<byte> rgbaOut)
        {
            if (rgbaOut.Length < width * height * 4)
            {
                throw new ArgumentException("rgbaOut is too small for width*height*4 bytes.", nameof(rgbaOut));
            }

            int ySize = width * height;
            ReadOnlySpan<byte> yPlane = yuv.Slice(0, ySize);

            for (int row = 0; row < height; row++)
            {
                int uvRow = row / 2;
                for (int col = 0; col < width; col++)
                {
                    int uvCol = col / 2;
                    byte y = yPlane[row * width + col];
                    byte u, v;

                    if (format == PixelFormat.Nv12)
                    {
                        int uvIndex = ySize + uvRow * width + uvCol * 2;
                        u = yuv[uvIndex];
                        v = yuv[uvIndex + 1];
                    }
                    else // I420: separate U then V planes, each width/2 * height/2
                    {
                        int chromaWidth = width / 2;
                        int chromaPlaneSize = chromaWidth * (height / 2);
                        int uIndex = ySize + uvRow * chromaWidth + uvCol;
                        int vIndex = ySize + chromaPlaneSize + uvRow * chromaWidth + uvCol;
                        u = yuv[uIndex];
                        v = yuv[vIndex];
                    }

                    int c = y - 16;
                    int d = u - 128;
                    int e = v - 128;

                    byte r = ClampByte((298 * c + 409 * e + 128) >> 8);
                    byte g = ClampByte((298 * c - 100 * d - 208 * e + 128) >> 8);
                    byte b = ClampByte((298 * c + 516 * d + 128) >> 8);

                    int outIndex = (row * width + col) * 4;
                    rgbaOut[outIndex] = r;
                    rgbaOut[outIndex + 1] = g;
                    rgbaOut[outIndex + 2] = b;
                    rgbaOut[outIndex + 3] = 255;
                }
            }
        }

        private static byte ClampByte(int value)
        {
            // Hand-rolled instead of Math.Clamp: Unity's "API Compatibility Level" can still be
            // set to .NET Standard 2.0 (Math.Clamp needs 2.1+), and this package cannot be
            // compiler-verified in this environment (see README) — avoid the risk.
            if (value < 0)
            {
                return 0;
            }

            return value > 255 ? (byte)255 : (byte)value;
        }
    }
}
