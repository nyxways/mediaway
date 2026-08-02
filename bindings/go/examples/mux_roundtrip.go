// Package main is an ASPIRATIONAL example.
//
// No mediaway Go binding exists yet — this file shows the target ergonomics
// for a future package wrapping mediaway-container-ffi via cgo (see
// docs/spec/c-ffi.md, Tier B). It mirrors examples/mux_roundtrip.rs: build a
// fragmented-MP4 muxer with one H.264 video track and one AAC audio track,
// push synthetic packets, pull out the muxed bytes, then demux them back and
// count the recovered packets.
//
// Import path and package name below are illustrative
// ("mediaway.dev/mediaway-go/container").
package main

import (
	"fmt"
	"log"

	"mediaway.dev/mediaway-go/container"
)

func main() {
	const (
		frameCount = 90 // 3 s at 30 fps
		fps        = 30
		sampleRate = 48_000
	)

	// ── 1. Build the muxer and register tracks (open state) ────────────────
	muxer, err := container.NewFragmentedMP4Muxer()
	if err != nil {
		log.Fatalf("new muxer: %v", err)
	}
	defer muxer.Close()

	videoTrack, err := muxer.AddTrack(container.VideoStreamInfo{
		ID:        0,
		Codec:     container.CodecH264,
		TimeBase:  container.Rational{Num: 1, Den: fps},
		Width:     1920,
		Height:    1080,
		ExtraData: nil,
	})
	if err != nil {
		log.Fatalf("add video track: %v", err)
	}

	audioTrack, err := muxer.AddTrack(container.AudioStreamInfo{
		ID:         1,
		Codec:      container.CodecAAC,
		TimeBase:   container.Rational{Num: 1, Den: sampleRate},
		ExtraData:  nil,
		SampleRate: sampleRate,
		Channels:   2,
	})
	if err != nil {
		log.Fatalf("add audio track: %v", err)
	}

	// ── 2. Transition to the live state — track registration closes here ──
	if err := muxer.Begin(); err != nil {
		log.Fatalf("begin: %v", err)
	}

	videoPayload := []byte{0x00, 0x00, 0x00, 0x01} // placeholder NAL start code
	audioPayload := []byte{0xff, 0xf1}

	for i := int64(0); i < frameCount; i++ {
		err := muxer.PushPacket(container.Packet{
			StreamID:   videoTrack,
			PTS:        i,
			DTS:        i,
			Duration:   1,
			IsKeyframe: i%30 == 0,
			IsDiscard:  false,
			Payload:    videoPayload,
		})
		if err != nil {
			log.Fatalf("push video packet %d: %v", i, err)
		}

		err = muxer.PushPacket(container.Packet{
			StreamID:   audioTrack,
			PTS:        i * 1_600,
			DTS:        i * 1_600,
			Duration:   1_600,
			IsKeyframe: true,
			IsDiscard:  false,
			Payload:    audioPayload,
		})
		if err != nil {
			log.Fatalf("push audio packet %d: %v", i, err)
		}
	}

	if err := muxer.Flush(); err != nil {
		log.Fatalf("flush: %v", err)
	}

	// ── 3. Pull the muxed bytes — caller owns I/O, the muxer never touches ──
	// files or sockets itself.
	mp4Bytes, err := muxer.PollBytes()
	if err != nil {
		log.Fatalf("poll bytes: %v", err)
	}
	fmt.Printf("mux_roundtrip: %d frames -> %d bytes of fMP4\n", frameCount, len(mp4Bytes))

	// ── 4. Demux the same bytes back ────────────────────────────────────────
	demuxer, err := container.NewFragmentedMP4Demuxer()
	if err != nil {
		log.Fatalf("new demuxer: %v", err)
	}
	defer demuxer.Close()

	if err := demuxer.PushBytes(mp4Bytes); err != nil {
		log.Fatalf("push bytes: %v", err)
	}

	streams, err := demuxer.Streams()
	if err != nil {
		log.Fatalf("streams: %v", err)
	}
	fmt.Printf("mux_roundtrip: demuxer sees %d stream(s)\n", len(streams))
	for _, s := range streams {
		if s.Geometry != nil {
			fmt.Printf("  stream %d - %s %dx%d\n", s.ID, s.Codec, s.Geometry.Width, s.Geometry.Height)
		} else {
			fmt.Printf("  stream %d - %s (no geometry)\n", s.ID, s.Codec)
		}
	}

	var nVideo, nAudio uint32
	for {
		pkt, err := demuxer.PollPacket()
		if err != nil {
			log.Fatalf("poll packet: %v", err)
		}
		if pkt == nil {
			break // no more packets right now
		}
		if pkt.StreamID == videoTrack {
			nVideo++
		} else {
			nAudio++
		}
	}
	fmt.Printf("mux_roundtrip: recovered %d video + %d audio packets\n", nVideo, nAudio)
}
