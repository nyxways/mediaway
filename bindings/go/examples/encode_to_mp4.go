// Package main is an ASPIRATIONAL example.
//
// No mediaway Go binding exists yet — this file shows the target ergonomics
// for a future package wrapping mediaway-pipeline (and the underlying
// mediaway-encoder auto backend selection) via cgo over the C ABI (see
// docs/spec/c-ffi.md, Tier B). It mirrors examples/encode_to_mp4.rs: build a
// 640x480 @30fps H.264 config, open the best available OS/GPU encoder on
// this platform (Zero-Copy GPU preferred, CPU-upload fallback), push
// synthetic NV12 frames through an encode session, and write the resulting
// fragmented MP4 bytes to disk.
//
// Import path and package name below are illustrative
// ("mediaway.dev/mediaway-go/pipeline").
package main

import (
	"fmt"
	"os"

	"mediaway.dev/mediaway-go/pipeline"
)

func main() {
	const (
		width   = 640
		height  = 480
		fps     = 30
		seconds = 3
		frames  = fps * seconds // 90 frames = 3 s at 30 fps
	)

	// ── 1. Build the encode config — defaults for H.264 at this resolution/ ──
	// framerate, then override bitrate.
	config := pipeline.NewAutoVideoEncodeConfig(
		pipeline.CodecH264,
		width,
		height,
		pipeline.Rational{Num: 1, Den: fps},
	)
	config.BitrateBps = 2_000_000

	// ── 2. Open the auto encoder — tries the best available backend on this ──
	// platform and returns a non-nil error if none exists here. This is a
	// normal, expected outcome on unsupported platforms/GPUs, not a crash.
	encoder, err := pipeline.OpenAutoEncoder(config)
	if err != nil {
		fmt.Printf("encode_to_mp4: open failed (%v) — platform not supported yet\n", err)
		return
	}
	defer encoder.Close()

	fmt.Println("encode_to_mp4: running on this platform")

	// ── 3. Wrap the encoder in an encode session — wires encoder output ──────
	// packets into the internal fragmented-MP4 muxer.
	session, err := pipeline.NewEncodeSession(encoder)
	if err != nil {
		fmt.Printf("encode_to_mp4: open encode session failed: %v\n", err)
		return
	}
	defer session.Close()

	// ── Synthetic NV12 source (replace with real frames in your app) ─────────
	// Grey Y=128 everywhere, U/V=128 everywhere: width*height Y bytes
	// followed by width*height/2 interleaved UV bytes.
	nv12Len := width*height + width*height/2
	source := make([]byte, nv12Len)
	for i := range source {
		source[i] = 128
	}

	for pts := int64(0); pts < frames; pts++ {
		frame := pipeline.VideoFrame{
			PTS:         pts,
			Duration:    1,
			Width:       width,
			Height:      height,
			PixelFormat: pipeline.PixelFormatNV12,
			Data:        source,
		}
		if err := session.WriteFrame(frame); err != nil {
			fmt.Printf("encode_to_mp4: write frame %d failed: %v\n", pts, err)
			return
		}
	}

	// ── 4. Flush the encoder, finalize the muxer, and get the MP4 bytes ───────
	mp4Bytes, err := session.Finish()
	if err != nil {
		fmt.Printf("encode_to_mp4: finish failed: %v\n", err)
		return
	}

	if err := os.WriteFile("out.mp4", mp4Bytes, 0o644); err != nil {
		fmt.Printf("encode_to_mp4: write out.mp4 failed: %v\n", err)
		return
	}

	fmt.Printf("encode_to_mp4: %d frames -> out.mp4 (%d bytes)\n", frames, len(mp4Bytes))
}
