// EncodeToMp4.swift — Mediaway Swift quick start (ASPIRATIONAL EXAMPLE).
//
// No Mediaway Swift binding exists yet — this is design-only, per ../README.md
// and docs/spec/c-ffi.md (Tier B: a C bridging header over the C ABI). It shows
// the target ergonomics for a future `Mediaway` Swift package: classes with
// `deinit`/explicit `close()` over native handles, Swift `throws`/`try`
// instead of raw error codes, and `Data` for byte buffers. Mirrors
// examples/encode_to_mp4.rs.
//
// Scenario: auto-select the best available OS/GPU H.264 encoder (Zero-Copy
// GPU path preferred, CPU-upload fallback), encode 90 synthetic grey NV12
// frames (3 s at 30 fps) at 640x480 / 2 Mbps, and write the resulting
// fragmented MP4 bytes to `out.mp4` in the current directory.

import Foundation
import Mediaway

let width = 640
let height = 480
let fps = 30
let seconds = 3
let frameCount = fps * seconds // 90 frames

// ── 1. Build the encode config — defaults for H264 at this resolution/fps,
//    then override bitrate. ─────────────────────────────────────────────────
var config = VideoEncodeConfig.defaults(
    codec: .h264,
    width: width,
    height: height,
    frameRate: Rational(1, fps)
)
config.bitrateBps = 2_000_000

// ── 2. Open the auto encoder — tries the best backend for this
//    platform/GPU; throws if none is available yet. This is the one call
//    expected to fail gracefully on unsupported platforms, so we catch it
//    specifically and exit cleanly instead of propagating. ────────────────
let encoder: AutoVideoEncoder
do {
    encoder = try AutoVideoEncoder(config: config)
} catch {
    print("encode_to_mp4: no auto encoder available on this platform (\(error))")
    exit(0)
}
print("encode_to_mp4: running on this platform")

do {
    // ── 3. Wrap the opened encoder in an encode session ──────────────────────
    let session = try EncodeSession(encoder: encoder)
    defer { session.close() }

    // ── 4. Synthetic NV12 source (replace with real frames in your app) ─────
    // NV12 layout: width*height Y bytes, followed by width*height/2
    // interleaved UV bytes.
    let ySize = width * height
    let uvSize = width * height / 2
    let nv12Frame = Data(repeating: 128, count: ySize + uvSize) // grey Y=128, UV=128

    for pts in 0..<Int64(frameCount) {
        let frame = VideoFrame(
            pts: pts,
            duration: 1,
            width: width,
            height: height,
            pixelFormat: .nv12,
            data: nv12Frame
        )
        try session.writeFrame(frame)
    }

    // ── 5. Flush the encoder, finalize the muxer, and get the MP4 bytes ──────
    let mp4Bytes = try session.finish()
    try mp4Bytes.write(to: URL(fileURLWithPath: "out.mp4"))

    print("encode_to_mp4: \(frameCount) frames -> out.mp4 (\(mp4Bytes.count) bytes)")
} catch {
    print("encode_to_mp4: encode failed (\(error))")
    exit(1)
}
