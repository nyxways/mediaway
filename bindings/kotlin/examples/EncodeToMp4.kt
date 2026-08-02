/*
 * Auto video encode -> fragmented MP4 — aspirational quick-start example.
 *
 * ASPIRATIONAL EXAMPLE: no `mediaway` Kotlin/Java package exists yet. This file
 * shows the target ergonomics for a future Kotlin binding over Mediaway's C ABI
 * (JNI under the hood, wrapped idiomatically: Closeable resources used with
 * `.use { }`, Kotlin exceptions instead of raw error codes). The API is also
 * meant to read naturally from plain Java, so no Kotlin-only surface (inline
 * value classes, etc.) is used in the public shape. See ../README.md and
 * docs/spec/c-ffi.md.
 *
 * Mirrors mediaway_encoder::auto: build a config, open the best available
 * H.264 encoder backend for this platform (Zero-Copy GPU path preferred, CPU
 * upload as fallback), push raw NV12 frames into an encode session, and get
 * back complete fragmented MP4 bytes — the session wires the encoder's output
 * packets into a container muxer internally, so the caller never sees packets
 * or muxer tracks directly.
 *
 * Run (once the real package exists):
 *     kotlinc EncodeToMp4.kt -include-runtime -d encode-to-mp4.jar
 *     java -jar encode-to-mp4.jar
 */

import io.mediaway.encoder.AutoVideoEncodeConfig
import io.mediaway.encoder.AutoVideoEncoder
import io.mediaway.encoder.EncodeSession
import io.mediaway.encoder.NoBackendException
import io.mediaway.encoder.VideoFrame
import java.io.File

private const val WIDTH = 640
private const val HEIGHT = 480
private const val FRAME_RATE_NUM = 30
private const val FRAME_RATE_DEN = 1
private const val BITRATE_BPS = 2_000_000L
private const val FRAME_COUNT = 90 // 3 s at 30 fps
private const val FRAME_DURATION = 1L // one tick at a 1/30 s time base

/** Build one solid-grey NV12 frame: Y=128 everywhere, U/V=128 everywhere. */
private fun greyNv12Frame(pts: Long): VideoFrame {
    val ySize = WIDTH * HEIGHT
    val uvSize = WIDTH * HEIGHT / 2
    val data = ByteArray(ySize + uvSize)
    data.fill(128.toByte())

    return VideoFrame(
        pts = pts,
        duration = FRAME_DURATION,
        width = WIDTH,
        height = HEIGHT,
        pixelFormat = VideoFrame.PixelFormat.NV12,
        data = data,
    )
}

fun main() {
    // -- 1. Describe what to encode: H.264 defaults for this resolution and
    //       frame rate, then override the bitrate. ------------------------
    val config = AutoVideoEncodeConfig
        .h264Defaults(width = WIDTH, height = HEIGHT, frameRateNum = FRAME_RATE_NUM, frameRateDen = FRAME_RATE_DEN)
        .withBitrateBps(BITRATE_BPS)

    // -- 2. Open the best available backend on this machine. This is
    //       inherently fallible: on a platform/GPU with no suitable H.264
    //       encoder yet, bail out gracefully instead of crashing. ---------
    val encoder = try {
        AutoVideoEncoder.open(config)
    } catch (e: NoBackendException) {
        println("encode_to_mp4: no H.264 encoder backend available on this platform: ${e.message}")
        return
    }

    // -- 3. Push frames through an encode session, then flush to fMP4 bytes --
    val mp4Bytes = encoder.use { enc ->
        EncodeSession(enc).use { session ->
            for (i in 0 until FRAME_COUNT) {
                session.writeFrame(greyNv12Frame(pts = i.toLong()))
            }
            session.finish()
        }
    }

    val outFile = File("out.mp4")
    outFile.writeBytes(mp4Bytes)

    println("encode_to_mp4: $FRAME_COUNT frames (${WIDTH}x$HEIGHT @ $FRAME_RATE_NUM fps) -> ${mp4Bytes.size} bytes of fMP4")
    println("encode_to_mp4: wrote ${outFile.absolutePath}")
}
