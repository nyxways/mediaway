// MuxRoundtrip.swift
//
// ASPIRATIONAL EXAMPLE: no `Mediaway` Swift package exists yet — no
// `mediaway-container-ffi` C ABI crate has shipped either. This file shows
// the target ergonomics for a future Swift binding over Mediaway's C ABI: a
// C bridging header wrapped in idiomatic Swift (classes with `deinit`/
// explicit `close()`, `throws`/`try` instead of raw C status codes). See
// ../README.md and docs/spec/c-ffi.md.
//
// Mirrors examples/mux_roundtrip.rs: register one H.264 video track and one
// AAC audio track, push fake packets for a simulated 3-second clip, flush,
// and read the fragmented MP4 bytes back with a streaming demuxer.
//
// Run (once the real package exists):
//     swift run MuxRoundtrip

import Mediaway
import Foundation

private let frameCount = 90 // 3 s at 30 fps
private let keyframeInterval = 30
private let videoTimeBase = Rational(1, 30)
private let audioTimeBase = Rational(1, 48_000)

/// Mux one video + one audio track into fragmented MP4 bytes.
func buildFmp4() throws -> Data {
    let muxer = Muxer()

    // 1. Register tracks (open state).
    let videoId = try muxer.addTrack(.video(
        codec: .h264,
        timeBase: videoTimeBase,
        width: 1920,
        height: 1080,
        extraData: Data()
    ))
    let audioId = try muxer.addTrack(.audio(
        codec: .aac,
        timeBase: audioTimeBase,
        extraData: Data(),
        sampleRate: 48_000,
        channels: 2
    ))

    // 2. Transition to a live session — track registration closes here.
    let session = try muxer.begin()
    defer { session.close() }

    for i in 0..<frameCount {
        try session.pushPacket(Packet(
            streamId: videoId,
            pts: Int64(i),
            dts: Int64(i),
            duration: 1,
            isKeyframe: i % keyframeInterval == 0,
            isDiscard: false,
            payload: Data([0x00, 0x00, 0x00, 0x01]) // placeholder NAL unit
        ))
        try session.pushPacket(Packet(
            streamId: audioId,
            pts: Int64(i) * 1_600,
            dts: Int64(i) * 1_600,
            duration: 1_600,
            isKeyframe: true,
            isDiscard: false,
            payload: Data([0xFF, 0xF1])
        ))
    }
    try session.flush()

    // 3. Pull bytes — caller owns I/O, the muxer never touches disk.
    return session.pollBytes()
}

/// Feed muxed bytes into a demuxer and count video vs. audio packets.
func demuxAndCount(_ data: Data) throws -> (video: Int, audio: Int) {
    let demuxer = Demuxer()
    defer { demuxer.close() }

    try demuxer.pushBytes(data)

    let streams = demuxer.streams
    print("mux_roundtrip: demuxer sees \(streams.count) stream(s)")
    for stream in streams {
        if let geometry = stream.geometry {
            print("  stream \(stream.id) — \(stream.codec) \(geometry.width)x\(geometry.height)")
        } else {
            print("  stream \(stream.id) — \(stream.codec) (no geometry)")
        }
    }

    var videoCount = 0
    var audioCount = 0
    while let packet = try demuxer.pollPacket() {
        guard let stream = streams.first(where: { $0.id == packet.streamId }) else { continue }
        if stream.codec == .h264 {
            videoCount += 1
        } else {
            audioCount += 1
        }
    }
    return (videoCount, audioCount)
}

// MARK: - Entry point

do {
    let fmp4Bytes = try buildFmp4()
    print("mux_roundtrip: \(frameCount) frames -> \(fmp4Bytes.count) bytes of fMP4")

    let (videoCount, audioCount) = try demuxAndCount(fmp4Bytes)
    print("mux_roundtrip: recovered \(videoCount) video + \(audioCount) audio packets")
    assert(videoCount > 0)
    print("mux_roundtrip: OK")
} catch {
    print("mux_roundtrip: failed — \(error)")
    exit(EXIT_FAILURE)
}
