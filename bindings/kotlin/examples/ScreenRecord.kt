/*
 * Screen recording pipeline: capture + mic -> encode -> fragmented MP4 —
 * aspirational quick-start example.
 *
 * ASPIRATIONAL EXAMPLE: no `mediaway` Kotlin/Java package exists yet. This file
 * shows the target ergonomics for a future Kotlin binding over Mediaway's C ABI
 * (JNI under the hood, wrapped idiomatically: Closeable resources used with
 * `.use { }`, Kotlin exceptions instead of raw error codes). The API is also
 * meant to read naturally from plain Java, so no Kotlin-only surface (inline
 * value classes, etc.) is used in the public shape. See ../README.md and
 * docs/spec/c-ffi.md.
 *
 * Mirrors examples/screen_record.rs. Built from the same building blocks as
 * EncodeToMp4.kt (config -> open auto encoder -> open encode session ->
 * writeFrame -> finish), plus a device-capture layer (ScreenCapture,
 * Microphone), glued together by one small platform-agnostic `record`
 * function typed purely against the VideoCapture/AudioCapture interfaces —
 * it has no idea which concrete OS backend is underneath.
 *
 * Run (once the real package exists):
 *     kotlinc ScreenRecord.kt -include-runtime -d screen-record.jar
 *     java -jar screen-record.jar
 */

import io.mediaway.device.AudioCapture
import io.mediaway.device.AudioCaptureConfig
import io.mediaway.device.CaptureException
import io.mediaway.device.CaptureUnavailableException
import io.mediaway.device.CapturedAudioFrame
import io.mediaway.device.Microphone
import io.mediaway.device.Rational
import io.mediaway.device.ScreenCapture
import io.mediaway.device.VideoCapture
import io.mediaway.device.VideoCaptureConfig
import io.mediaway.encoder.AutoVideoEncodeConfig
import io.mediaway.encoder.AutoVideoEncoder
import io.mediaway.encoder.EncodeSession
import io.mediaway.encoder.NoBackendException
import io.mediaway.encoder.VideoFrame
import java.io.File
import java.time.Duration
import java.time.Instant

private const val DISPLAY_INDEX = 0
private const val FRAME_RATE_NUM = 30
private const val FRAME_RATE_DEN = 1
private const val SAMPLE_RATE_HZ = 48_000
private const val BITRATE_BPS = 8_000_000L
private val RECORD_DURATION: Duration = Duration.ofSeconds(3)

/**
 * No-op [AudioCapture] used when the microphone is unavailable, so [record]
 * never has to special-case a missing mic.
 */
private class NoAudioCapture : AudioCapture {
    override fun pollFrame(): CapturedAudioFrame? = null
    override fun close() {}
}

/**
 * Record from `video` and `audio` into `session` until `duration` elapses.
 *
 * Every parameter is typed against an **interface** — this function compiles
 * and behaves identically regardless of which concrete OS backend produced
 * `video`/`audio` (DXGI, WASAPI, a portal, WebCodecs, ...).
 */
fun record(
    video: VideoCapture,
    audio: AudioCapture,
    session: EncodeSession,
    width: Int,
    height: Int,
    duration: Duration,
) {
    val deadline = Instant.now().plus(duration)
    val ySize = width * height
    val uvSize = width * height / 2
    // Synthetic NV12 placeholder (Y=128, UV=128 -> grey) standing in for the
    // real captured pixels, which this example does not yet convert.
    val greyNv12 = ByteArray(ySize + uvSize).also { it.fill(128.toByte()) }

    var pts = 0L
    while (Instant.now().isBefore(deadline)) {
        // -- Video ------------------------------------------------------
        val videoFrame = try {
            video.pollFrame()
        } catch (e: CaptureException) {
            println("screen_record: capture error (${e.message})")
            break
        }
        if (videoFrame != null) {
            // The real frame may reference GPU-resident memory (e.g. a DXGI
            // surface); it must be released back to the OS once consumed.
            video.releaseFrame()

            session.writeFrame(
                VideoFrame(
                    pts = pts,
                    duration = 1L,
                    width = width,
                    height = height,
                    pixelFormat = VideoFrame.PixelFormat.NV12,
                    data = greyNv12,
                )
            )
            pts++
        }

        // -- Audio: drained but not yet wired into an audio track --------
        try {
            while (audio.pollFrame() != null) {
                // no-op: not wired into an audio track yet
            }
        } catch (e: CaptureException) {
            // Best-effort drain; a hard audio error should not stop video capture.
        }
    }
}

fun main() {
    // -- 1. Open platform capture backends. Screen capture is required —
    //       bail out if unavailable. Mic is optional — continue without it. --
    val screen = try {
        ScreenCapture.open(VideoCaptureConfig.screen(DISPLAY_INDEX, Rational(1, FRAME_RATE_NUM)))
    } catch (e: CaptureUnavailableException) {
        println("screen_record: capture unavailable (${e.message}) — platform not supported yet")
        return
    }

    val mic: AudioCapture = try {
        Microphone.open(AudioCaptureConfig.microphone(Rational(1, SAMPLE_RATE_HZ)))
    } catch (e: CaptureUnavailableException) {
        println("screen_record: mic unavailable (${e.message}) — continuing without audio")
        NoAudioCapture()
    }

    val width = screen.width
    val height = screen.height
    println("screen_record: ${width}x$height display ready")

    // -- 2. Build the encoder config from the capture's real geometry, then
    //       open the auto encoder + an encode session — same building blocks
    //       as EncodeToMp4.kt. ------------------------------------------------
    val config = AutoVideoEncodeConfig
        .h264Defaults(width = width, height = height, frameRateNum = FRAME_RATE_NUM, frameRateDen = FRAME_RATE_DEN)
        .withBitrateBps(BITRATE_BPS)

    val encoder = try {
        AutoVideoEncoder.open(config)
    } catch (e: NoBackendException) {
        println("screen_record: no H.264 encoder backend available on this platform: ${e.message}")
        screen.close()
        mic.close()
        return
    }

    // -- 3. Record for a fixed duration, close both capture objects, then
    //       flush the encode session to fragmented MP4 bytes. ----------------
    val mp4Bytes = encoder.use { enc ->
        EncodeSession(enc).use { session ->
            record(screen, mic, session, width, height, RECORD_DURATION)
            screen.close()
            mic.close()
            session.finish()
        }
    }

    val outFile = File("out_screen.mp4")
    outFile.writeBytes(mp4Bytes)

    println("screen_record: recorded ${RECORD_DURATION.seconds}s (${width}x$height) -> ${mp4Bytes.size} bytes of fMP4")
    println("screen_record: wrote ${outFile.absolutePath}")
}
