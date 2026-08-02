/*
 * Camera + mic capture -> encode -> fragmented MP4 — aspirational quick-start example.
 *
 * ASPIRATIONAL EXAMPLE: no `mediaway` Kotlin/Java package exists yet. This file
 * shows the target ergonomics for a future Kotlin binding over Mediaway's C ABI
 * (JNI under the hood, wrapped idiomatically: Closeable resources used with
 * `.use { }`, Kotlin exceptions instead of raw error codes). The API is also
 * meant to read naturally from plain Java, so no Kotlin-only surface (inline
 * value classes, etc.) is used in the public shape. See ../README.md and
 * docs/spec/c-ffi.md.
 *
 * Same shape as screen capture would be (see e.g. ../csharp/ScreenRecord.cs),
 * with a camera source instead of a screen source: open a camera capture + a
 * microphone (both fallible — the specific device may not be available), build
 * an "auto video encode" config at the capture's real geometry, and reuse the
 * exact same building blocks as EncodeToMp4.kt (AutoVideoEncoder ->
 * EncodeSession -> writeFrame -> finish), plus one small platform-agnostic
 * `record()` function that glues capture to encode. `record()` is typed purely
 * against the `VideoCapture` / `AudioCapture` interfaces, so it has no idea a
 * camera (rather than a screen) is underneath — the exact same function would
 * work unchanged for screen capture.
 *
 * Run (once the real package exists):
 *     kotlinc CameraRecord.kt -include-runtime -d camera-record.jar
 *     java -jar camera-record.jar
 */

import io.mediaway.container.Rational
import io.mediaway.device.AudioCapture
import io.mediaway.device.CameraCapture
import io.mediaway.device.CaptureUnavailableException
import io.mediaway.device.Microphone
import io.mediaway.device.VideoCapture
import io.mediaway.encoder.AutoVideoEncodeConfig
import io.mediaway.encoder.AutoVideoEncoder
import io.mediaway.encoder.EncodeSession
import io.mediaway.encoder.NoBackendException
import io.mediaway.encoder.VideoFrame
import java.io.File
import java.time.Duration
import java.time.Instant

private const val FPS = 30
private const val SECONDS = 3L
private const val AUDIO_SAMPLE_RATE = 48_000
private const val BITRATE_BPS = 4_000_000L

/**
 * Poll [video] (and drain [audio], if present) until [duration] elapses.
 *
 * Writes a synthetic grey NV12 placeholder frame into [session] for every
 * polled video frame — this example never touches the real captured pixels.
 * Typed purely against the [VideoCapture] / [AudioCapture] interfaces: this
 * function has no idea which concrete source (camera, screen, ...) it was
 * handed, so the exact same function works unchanged for screen capture.
 */
private fun record(
    video: VideoCapture,
    audio: AudioCapture?,
    session: EncodeSession,
    duration: Duration,
) {
    val deadline = Instant.now().plus(duration)

    // Synthetic NV12 placeholder (stand-in for real captured pixels): grey
    // Y=128, UV=128. Layout is width*height Y bytes followed by
    // width*height/2 interleaved UV bytes.
    val width = video.width
    val height = video.height
    val greyNv12 = ByteArray(width * height + width * height / 2) { 128.toByte() }

    var pts = 0L
    while (Instant.now().isBefore(deadline)) {
        val frame = video.pollFrame()
        if (frame != null) {
            // The polled frame may reference GPU-resident memory that this
            // example never touches — it must still be released back to the
            // OS once we're done with it, even though we encode a synthetic
            // buffer instead.
            video.releaseFrame()

            session.writeFrame(
                VideoFrame(
                    pts = pts++,
                    duration = 1,
                    width = width,
                    height = height,
                    pixelFormat = VideoFrame.PixelFormat.NV12,
                    data = greyNv12,
                )
            )
        }

        // Drain polled audio frames — not wired into an audio track yet.
        while (audio?.pollFrame() != null) {
            // no-op: draining only
        }
    }
}

fun main() {
    // -- 1. Open capture sources. Both are fallible: the specific camera or
    //       microphone may not be available on this machine. ---------------
    val videoCapture: VideoCapture = try {
        CameraCapture.open(deviceIndex = 0, frameRate = Rational(1, FPS))
    } catch (e: CaptureUnavailableException) {
        println("camera_record: camera unavailable (${e.message}) -- exiting")
        return
    }

    val audioCapture: AudioCapture? = try {
        Microphone.open(sampleRate = Rational(1, AUDIO_SAMPLE_RATE))
    } catch (e: CaptureUnavailableException) {
        // Recording without audio is a valid degraded mode -- log and continue.
        println("camera_record: microphone unavailable (${e.message}) -- continuing without audio")
        null
    }

    // The capture settles on its own stream geometry (whatever the camera
    // actually negotiated) -- read it back here rather than assuming it.
    val width = videoCapture.width
    val height = videoCapture.height
    println("camera_record: ${width}x$height camera" + if (audioCapture != null) ", mic ready" else "")

    // -- 2. Open the encoder + encode session (same building blocks as
    //       EncodeToMp4.kt), sized to the capture's real geometry. ----------
    val config = AutoVideoEncodeConfig
        .h264Defaults(width = width, height = height, frameRateNum = FPS, frameRateDen = 1)
        .withBitrateBps(BITRATE_BPS)

    val encoder = try {
        AutoVideoEncoder.open(config)
    } catch (e: NoBackendException) {
        println("camera_record: no H.264 encoder backend available on this platform (${e.message})")
        videoCapture.close()
        audioCapture?.close()
        return
    }

    // -- 3. Run the platform-agnostic record loop, then finish encoding and
    //       write the result. ------------------------------------------------
    val mp4Bytes = encoder.use { enc ->
        EncodeSession(enc).use { session ->
            record(videoCapture, audioCapture, session, Duration.ofSeconds(SECONDS))

            videoCapture.close()
            audioCapture?.close()

            session.finish()
        }
    }

    val outFile = File("out_camera.mp4")
    outFile.writeBytes(mp4Bytes)

    println("camera_record: ${width}x$height -> ${outFile.name} (${mp4Bytes.size} bytes)")
}
