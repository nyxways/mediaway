# Ending an encode

**Call `finish(self)`** (both `VideoEncoder` and `AudioEncoder`). It flushes and returns
every remaining packet ([ADR-0006](../../../../crates/mediaway-encoder/adr/0006-finish-ends-a-stream.md)).

- **Dropping an unflushed encoder discards its in-flight frames.** These are the *last* frames,
  because hardware encoders are pipelined. A recorder that did this lost its final frame on
  every recording (measured).
- `Drop` cannot flush for you: it has nowhere to return the packets.
- Keep using `flush()` + `poll_packet()` when the encoder must stay usable after end-of-input.
- On the Windows async (NVIDIA) path, an unflushed drop was also the sharpest trigger for the
  access violation fixed in #108 — see [async-mft](async-mft.md) rule 4.
- `mediaway::EncodeSession::finish` / `finish_into` already flush; this is for callers
  driving encoders directly.
