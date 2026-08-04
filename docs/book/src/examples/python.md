# Python

Data-processing pipelines and ML input/output streams call Mediaway via `ctypes`/`cffi`
over the [`mediaway-ffi`](https://github.com/nyxways/mediaway/tree/main/crates/mediaway-ffi)
C ABI. Status: ✅ verified.

## Install

```bash
pip install mediaway
```

```python
from mediaway import Codec, Muxer, Packet, Rational, VideoStreamInfo

with Muxer() as muxer:
    video_id = muxer.add_video_track(VideoStreamInfo(
        codec=Codec.H264, width=1920, height=1080,
        frame_rate=Rational(1, 30),
    ))
    with muxer.begin() as live:  # Open -> Live (handle moves; registration impossible)
        live.push_packet(Packet(
            stream_index=video_id, pts=Rational(0, 30),
            payload=b"\x00\x00\x00\x01", key=True,
        ))
        live.flush()
        chunks = []
        while True:
            chunk = live.poll_bytes()  # caller owns byte I/O (sans-io)
            if chunk is None:
                break
            chunks.append(chunk)

mp4 = b"".join(chunks)
```

Examples live in [`bindings/python/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/python/examples):

| Capability | Example files |
|------------|---------------|
| Container | `container/mux_roundtrip.py` |
| Device | `device/camera_record.py` · `capture_microphone.py` · `capture_screen.py` |
| Pipeline | `pipeline/encode_audio.py` · `encode_to_mp4.py` · `screen_record.py` |

Build and run instructions: [`bindings/python/README.md`](https://github.com/nyxways/mediaway/blob/main/bindings/python/README.md).
