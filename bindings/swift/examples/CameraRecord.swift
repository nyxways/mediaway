// CameraRecord.swift
//
// ASPIRATIONAL EXAMPLE: no `Mediaway` Swift package exists yet — no
// `mediaway-device-ffi` / `mediaway-encoder-ffi` C ABI crates have shipped
// either. This shows the target ergonomics for a future Swift binding over
// Mediaway's C ABI: a C bridging header wrapped in idiomatic Swift (classes
// with `deinit`/explicit `close()`, `throws`/`try` instead of raw C status
// codes). See ../README.md and docs/spec/c-ffi.md.
//
// Same shape as ../ScreenRecord.swift, with a camera source in place of a
// screen source: open camera + microphone capture, build an H.264 auto
// encoder sized to the camera's negotiated geometry, run the same
// platform-agnostic `record` loop that polls both sources and writes
// synthetic grey NV12 placeholder frames into the encode session, then
// finish the session and write the fragmented MP4 bytes to `out_camera.mp4`.
//
// `CameraCapture`/`Microphone` (concrete backends) and the `VideoCapture`/
// `AudioCapture` protocols they conform to are all part of the imagined
// `Mediaway` package — same as `AutoVideoEncoder`/`EncodeSession`/`VideoFrame`
// in ../EncodeToMp4.swift. Only `record(...)` and the top-level script below
// are example/app code.
//
// Run (once the real package exists):
//     swift run CameraRecord

import Foundation
import Mediaway

// MARK: - Platform-agnostic record loop

/// Poll `video` and `audio` until `duration` elapses, writing one synthetic
/// grey NV12 placeholder frame into `session` per captured video frame, and
/// draining (but not yet doing anything with) audio frames.
///
/// Typed entirely against the `VideoCapture`/`AudioCapture` protocols — this
/// is the exact same function used in ../ScreenRecord.swift; it does not
/// know or care whether the concrete backend behind `video` is a camera or a
/// screen. `audio` is `nil` when the caller could not open a microphone;
/// recording continues video-only in that case.
func record(
    video: VideoCapture,
    audio: AudioCapture?,
    session: EncodeSession,
    width: Int,
    height: Int,
    duration: TimeInterval
) {
    let deadline = Date().addingTimeInterval(duration)
    // Synthetic NV12 placeholder (Y=128, UV=128 → grey), same shape as
    // ../EncodeToMp4.swift: width*height Y bytes + width*height/2
    // interleaved UV bytes.
    let ySize = width * height
    let uvSize = width * height / 2
    let greyNV12 = Data(repeating: 128, count: ySize + uvSize)

    var pts: Int64 = 0
    while Date() < deadline {
        do {
            if try video.pollFrame() != nil {
                // A real backend would convert the captured pixels (often
                // GPU-resident camera memory) to NV12 here; this example
                // writes the synthetic placeholder instead and releases the
                // frame back to the OS once it is no longer needed.
                video.releaseFrame()

                let frame = VideoFrame(
                    pts: pts,
                    duration: 1,
                    width: width,
                    height: height,
                    pixelFormat: .nv12,
                    data: greyNV12
                )
                try session.writeFrame(frame)
                pts += 1
            }
        } catch {
            print("camera_record: capture error (\(error))")
            break
        }

        // Audio track not wired in this example — frames are drained only.
        guard let audio else { continue }
        while (try? audio.pollFrame()) != nil {}
    }
}

// MARK: - Entry point

let fps = 30
let frameRate = Rational(1, fps)

// ── 1. Open camera capture (device 0 = default/first camera) — throws if
//    that camera is not available. Handle gracefully instead of crashing. ──
let camera: CameraCapture
do {
    camera = try CameraCapture(device: 0, frameRate: frameRate)
} catch {
    print("camera_record: camera unavailable (\(error))")
    exit(0)
}
print("camera_record: \(camera.width)x\(camera.height) camera stream")

// ── 2. Open microphone capture — also fallible; recording continues
//    video-only if no microphone is available. ─────────────────────────────
var mic: Microphone?
do {
    mic = try Microphone(sampleRate: Rational(1, 48_000))
    print("camera_record: microphone ready")
} catch {
    print("camera_record: microphone unavailable (\(error)) — continuing without audio")
    mic = nil
}

do {
    // ── 3. Build the encoder config at the camera's negotiated geometry,
    //    then open the auto encoder + encode session. ────────────────────────
    var config = VideoEncodeConfig.defaults(
        codec: .h264,
        width: camera.width,
        height: camera.height,
        frameRate: frameRate
    )
    config.bitrateBps = 4_000_000

    let encoder = try AutoVideoEncoder(config: config)
    let session = try EncodeSession(encoder: encoder)

    // ── 4. Run the shared record loop for 3 seconds. ─────────────────────────
    record(
        video: camera,
        audio: mic,
        session: session,
        width: camera.width,
        height: camera.height,
        duration: 3
    )

    // ── 5. Close both capture objects. ───────────────────────────────────────
    camera.close()
    mic?.close()

    // ── 6. Finish the encode session and write the fragmented MP4 bytes. ────
    let mp4Bytes = try session.finish()
    try mp4Bytes.write(to: URL(fileURLWithPath: "out_camera.mp4"))

    print("camera_record: -> out_camera.mp4 (\(mp4Bytes.count) bytes)")
} catch {
    camera.close()
    mic?.close()
    print("camera_record: recording failed (\(error))")
    exit(1)
}
