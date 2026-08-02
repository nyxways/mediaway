/*
 * Mux + demux roundtrip — aspirational quick-start example.
 *
 * ASPIRATIONAL EXAMPLE: no `mediaway` Kotlin/Java package exists yet. This file
 * shows the target ergonomics for a future Kotlin binding over Mediaway's C ABI
 * (JNI under the hood, wrapped idiomatically: Closeable resources used with
 * `.use { }`, Kotlin exceptions instead of raw error codes). The API is also
 * meant to read naturally from plain Java, so no Kotlin-only surface (inline
 * value classes, etc.) is used in the public shape. See ../README.md and
 * docs/spec/c-ffi.md.
 *
 * Mirrors examples/mux_roundtrip.rs: register one H.264 video track and one
 * AAC audio track, push fake packets for a simulated 3-second clip, flush,
 * and read the fragmented MP4 bytes back with a streaming demuxer.
 *
 * Run (once the real package exists):
 *     kotlinc MuxRoundtrip.kt -include-runtime -d mux-roundtrip.jar
 *     java -jar mux-roundtrip.jar
 */

import io.mediaway.container.AudioStreamInfo
import io.mediaway.container.Codec
import io.mediaway.container.Demuxer
import io.mediaway.container.Muxer
import io.mediaway.container.Packet
import io.mediaway.container.Rational
import io.mediaway.container.VideoStreamInfo

private const val FRAME_COUNT = 90 // 3 s at 30 fps
private const val KEYFRAME_INTERVAL = 30
private val VIDEO_TIME_BASE = Rational(1, 30)
private val AUDIO_TIME_BASE = Rational(1, 48_000)

/** Mux one video + one audio track into fragmented MP4 bytes. */
fun buildFmp4(): ByteArray {
    val muxer = Muxer()

    // -- 1. Register tracks (open state) --------------------------------
    val videoId = muxer.addTrack(
        VideoStreamInfo(
            codec = Codec.H264,
            timeBase = VIDEO_TIME_BASE,
            width = 1920,
            height = 1080,
            extraData = ByteArray(0),
        )
    )
    val audioId = muxer.addTrack(
        AudioStreamInfo(
            codec = Codec.AAC,
            timeBase = AUDIO_TIME_BASE,
            extraData = ByteArray(0),
            sampleRate = 48_000,
            channels = 2,
        )
    )

    // -- 2. Transition to a live session — track registration closes here --
    muxer.begin().use { session ->
        for (i in 0 until FRAME_COUNT) {
            session.pushPacket(
                Packet(
                    streamId = videoId,
                    pts = i.toLong(),
                    dts = i.toLong(),
                    duration = 1,
                    isKeyframe = i % KEYFRAME_INTERVAL == 0,
                    isDiscard = false,
                    payload = byteArrayOf(0x00, 0x00, 0x00, 0x01), // placeholder NAL unit
                )
            )
            session.pushPacket(
                Packet(
                    streamId = audioId,
                    pts = i.toLong() * 1_600,
                    dts = i.toLong() * 1_600,
                    duration = 1_600,
                    isKeyframe = true,
                    isDiscard = false,
                    payload = byteArrayOf(0xFF.toByte(), 0xF1.toByte()),
                )
            )
        }
        session.flush()

        // -- 3. Pull bytes — caller owns I/O, the muxer never touches disk --
        return session.pollBytes()
    }
}

/** Feed muxed bytes into a demuxer and count video vs. audio packets. */
fun demuxAndCount(data: ByteArray): Pair<Int, Int> {
    Demuxer().use { demuxer ->
        demuxer.pushBytes(data)

        val streams = demuxer.streams
        println("mux_roundtrip: demuxer sees ${streams.size} stream(s)")
        for (stream in streams) {
            val geometry = stream.geometry
            if (geometry != null) {
                println("  stream ${stream.id} — ${stream.codec} ${geometry.width}x${geometry.height}")
            } else {
                println("  stream ${stream.id} — ${stream.codec} (no geometry)")
            }
        }

        var nVideo = 0
        var nAudio = 0
        while (true) {
            val packet = demuxer.pollPacket() ?: break
            val stream = streams.first { it.id == packet.streamId }
            if (stream.codec == Codec.H264) nVideo++ else nAudio++
        }
        return nVideo to nAudio
    }
}

fun main() {
    val fmp4Bytes = buildFmp4()
    println("mux_roundtrip: $FRAME_COUNT frames -> ${fmp4Bytes.size} bytes of fMP4")

    val (nVideo, nAudio) = demuxAndCount(fmp4Bytes)
    println("mux_roundtrip: recovered $nVideo video + $nAudio audio packets")
    check(nVideo > 0)
    println("mux_roundtrip: OK")
}
