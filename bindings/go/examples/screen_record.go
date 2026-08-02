// Package main is an ASPIRATIONAL example.
//
// No mediaway Go binding exists yet — this file shows the target ergonomics
// for a future package wrapping mediaway-device (screen + microphone capture)
// and mediaway-pipeline (auto encoder + encode session) via cgo over the C
// ABI (see docs/spec/c-ffi.md, Tier B). It mirrors examples/screen_record.rs:
// open a screen capture and a microphone, build the same kind of auto H.264
// encode config/session as encode_to_mp4.go, run one small platform-agnostic
// record loop that polls both capture sources and writes synthetic NV12
// frames into the session, then finish the session and write the resulting
// fragmented MP4 bytes to disk.
//
// Import paths and package names below are illustrative
// ("mediaway.dev/mediaway-go/device", "mediaway.dev/mediaway-go/pipeline").
package main

import (
	"fmt"
	"os"
	"time"

	"mediaway.dev/mediaway-go/device"
	"mediaway.dev/mediaway-go/pipeline"
)

func main() {
	const (
		displayIndex  = 0 // 0 = primary display
		fps           = 30
		micSampleRate = 48_000
		bitrateBps    = 8_000_000
		recordSeconds = 3
	)

	videoTimeBase := pipeline.Rational{Num: 1, Den: fps}

	// ── 1. Open screen capture — fallible; the OS/platform backend may not ───
	// exist yet, so report it and bail out gracefully instead of crashing.
	videoCap, err := device.OpenScreenCapture(device.NewScreenCaptureConfig(displayIndex, videoTimeBase))
	if err != nil {
		fmt.Printf("screen_record: capture unavailable (%v) — platform not supported yet\n", err)
		return
	}
	defer videoCap.Close()

	// ── 2. Open the microphone — also fallible, but losing audio is not fatal: ─
	// keep recording video-only rather than aborting the whole session.
	var audioCap device.AudioCapture // stays nil if no microphone is available
	micTimeBase := pipeline.Rational{Num: 1, Den: micSampleRate}
	mic, err := device.OpenMicrophone(device.NewMicrophoneConfig(micTimeBase))
	if err != nil {
		fmt.Printf("screen_record: mic unavailable (%v) — continuing without audio\n", err)
	} else {
		defer mic.Close()
		audioCap = mic
	}

	// ── 3. Read back the geometry the capture actually settled on ────────────
	geometry := videoCap.Geometry()
	width, height := geometry.Width, geometry.Height
	fmt.Printf("screen_record: %dx%d display, audio=%v\n", width, height, audioCap != nil)

	// ── 4. Build the encode config from that geometry, open the auto encoder ──
	// and wrap it in an encode session — same building blocks as encode_to_mp4.go.
	config := pipeline.NewAutoVideoEncodeConfig(pipeline.CodecH264, width, height, videoTimeBase)
	config.BitrateBps = bitrateBps

	encoder, err := pipeline.OpenAutoEncoder(config)
	if err != nil {
		fmt.Printf("screen_record: encoder unavailable (%v) — platform not supported yet\n", err)
		return
	}
	defer encoder.Close()

	session, err := pipeline.NewEncodeSession(encoder)
	if err != nil {
		fmt.Printf("screen_record: open encode session failed: %v\n", err)
		return
	}
	defer session.Close()

	// ── 5. The one reusable record loop — knows nothing about which OS ───────
	// backend it's talking to.
	record(videoCap, audioCap, session, width, height, recordSeconds*time.Second)

	// ── 6. Flush the encoder, finalize the muxer, and write the MP4 bytes ─────
	mp4Bytes, err := session.Finish()
	if err != nil {
		fmt.Printf("screen_record: finish failed: %v\n", err)
		return
	}

	if err := os.WriteFile("out_screen.mp4", mp4Bytes, 0o644); err != nil {
		fmt.Printf("screen_record: write out_screen.mp4 failed: %v\n", err)
		return
	}

	fmt.Printf("screen_record: %dx%d -> out_screen.mp4 (%d bytes)\n", width, height, len(mp4Bytes))
}

// record polls videoCap for frames until duration elapses, writing a
// synthetic grey NV12 placeholder frame into session for each captured video
// frame, and draining (without yet doing anything with) any frames polled
// from audioCap. audioCap may be nil if no microphone was opened.
//
// videoCap and audioCap are taken purely as the device.VideoCapture /
// device.AudioCapture interfaces, so this function compiles and runs
// identically no matter which concrete OS backend sits behind them.
func record(
	videoCap device.VideoCapture,
	audioCap device.AudioCapture,
	session *pipeline.EncodeSession,
	width, height uint32,
	duration time.Duration,
) {
	deadline := time.Now().Add(duration)

	// Grey Y=128 everywhere, U/V=128 everywhere: width*height Y bytes followed
	// by width*height/2 interleaved UV bytes — same shape as encode_to_mp4.go.
	// Stands in for the real captured pixels until GPU-frame -> NV12
	// conversion is wired up.
	nv12Len := int(width)*int(height) + int(width)*int(height)/2
	greyNV12 := make([]byte, nv12Len)
	for i := range greyNV12 {
		greyNV12[i] = 128
	}

	var pts int64
	for time.Now().Before(deadline) {
		// ── Video ────────────────────────────────────────────────────────────
		frame, err := videoCap.PollFrame()
		if err != nil {
			fmt.Printf("record: capture error (%v)\n", err)
			return
		}
		if frame != nil {
			// The frame may reference GPU-resident memory (e.g. a D3D11
			// texture or IOSurface); release it back to the OS once we're
			// done with it, before writing our placeholder in its place.
			if err := videoCap.ReleaseFrame(); err != nil {
				fmt.Printf("record: release frame failed (%v)\n", err)
			}

			err := session.WriteFrame(pipeline.VideoFrame{
				PTS:         pts,
				Duration:    1,
				Width:       width,
				Height:      height,
				PixelFormat: pipeline.PixelFormatNV12,
				Data:        greyNV12,
			})
			if err != nil {
				fmt.Printf("record: write frame failed (%v)\n", err)
				return
			}
			pts++
		}

		// ── Audio ────────────────────────────────────────────────────────────
		if audioCap != nil {
			for {
				af, err := audioCap.PollFrame()
				if err != nil || af == nil {
					break
				}
				// Drained but not yet wired to an encoder/track; a second
				// (audio) track lands with the muxer's audio path.
			}
		}
	}
}
