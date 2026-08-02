//! High-level RTMP publish-client mux surface: `connect`/`createStream`/`publish` AMF0
//! commands, `onMetaData`, and audio/video message push. Composes [`ChunkEncoder`] (chunk
//! stream) + [`crate::amf0`] (command/data encode). Push-append-to-`&mut Vec<u8>`, no
//! `finish()` — same shape as `flv::Muxer`.

#![forbid(unsafe_code)]

use crate::amf0;
use crate::chunk_common::{
    MSG_AMF0_COMMAND, MSG_AMF0_DATA, MSG_AUDIO, MSG_SET_CHUNK_SIZE, MSG_VIDEO,
};
use crate::chunk_encoder::ChunkEncoder;
use crate::types::OnMetaData;

/// Chunk stream reserved for low-level protocol-control messages (`Set Chunk Size`, …).
const CSID_PROTOCOL_CONTROL: u32 = 2;
/// Chunk stream carrying `connect`/`createStream`/`publish` AMF0 commands.
const CSID_COMMAND: u32 = 3;
/// Chunk stream carrying the `onMetaData` AMF0 data message.
const CSID_DATA: u32 = 4;
/// Chunk stream carrying audio messages.
const CSID_AUDIO: u32 = 5;
/// Chunk stream carrying video messages.
const CSID_VIDEO: u32 = 6;

/// `NetStream` ID assumed for `publish`/audio/video/`onMetaData` messages. This crate does
/// not decode AMF0 (§ 3 of `adr/0001-rtmp-freestanding-core.md`), so it cannot read the
/// server's real `createStream` `_result` payload to learn the assigned stream ID — `1` is
/// what virtually every real RTMP server assigns the first stream on a connection, but this
/// is a documented assumption, not a protocol guarantee, and is not verified against a real
/// server by this implementation.
const ASSUMED_STREAM_ID: u32 = 1;

/// Builds RTMP `connect`/`createStream`/`publish` command messages, `onMetaData`, and
/// audio/video message chunks.
#[derive(Debug)]
pub struct Muxer {
    encoder: ChunkEncoder,
    transaction_id: f64,
    chunk_size: u32,
    chunk_size_sent: bool,
}

impl Muxer {
    /// New mux session using `chunk_size`-bounded fragments for everything this `Muxer`
    /// writes (RTMP's default is 128; `0` is clamped to `1`). The first call to any
    /// `out`-appending method also emits a `Set Chunk Size` protocol-control message so the
    /// peer's own decoder matches.
    #[must_use]
    pub fn new(chunk_size: u32) -> Self {
        Self {
            encoder: ChunkEncoder::new(chunk_size),
            transaction_id: 1.0,
            chunk_size,
            chunk_size_sent: false,
        }
    }

    fn maybe_emit_set_chunk_size(&mut self, out: &mut Vec<u8>) {
        if self.chunk_size_sent {
            return;
        }
        self.chunk_size_sent = true;
        let payload = self.chunk_size.to_be_bytes();
        self.encoder.encode_message(
            CSID_PROTOCOL_CONTROL,
            MSG_SET_CHUNK_SIZE,
            0,
            0,
            &payload,
            out,
        );
    }

    fn next_transaction_id(&mut self) -> f64 {
        let id = self.transaction_id;
        self.transaction_id += 1.0;
        id
    }

    /// Append a `connect` AMF0 command (`NetConnection`, message stream `0`), prefixed by
    /// `Set Chunk Size` on the first call.
    pub fn write_connect(&mut self, app: &str, tc_url: &str, out: &mut Vec<u8>) {
        self.maybe_emit_set_chunk_size(out);
        let mut payload = Vec::new();
        let _ = amf0::write_string(&mut payload, "connect");
        amf0::write_number(&mut payload, self.next_transaction_id());
        amf0::write_object_start(&mut payload);
        write_property_str(&mut payload, "app", app);
        write_property_str(&mut payload, "flashVer", "MEDIAWAY/1,0,0,0");
        write_property_str(&mut payload, "tcUrl", tc_url);
        amf0::write_object_end(&mut payload);
        self.encoder
            .encode_message(CSID_COMMAND, MSG_AMF0_COMMAND, 0, 0, &payload, out);
    }

    /// Append a `createStream` AMF0 command (`NetConnection`, message stream `0`).
    pub fn write_create_stream(&mut self, out: &mut Vec<u8>) {
        self.maybe_emit_set_chunk_size(out);
        let mut payload = Vec::new();
        let _ = amf0::write_string(&mut payload, "createStream");
        amf0::write_number(&mut payload, self.next_transaction_id());
        amf0::write_null(&mut payload);
        self.encoder
            .encode_message(CSID_COMMAND, MSG_AMF0_COMMAND, 0, 0, &payload, out);
    }

    /// Append a `publish` AMF0 command as a `live` stream, on the assumed stream ID `1`
    /// (see `ASSUMED_STREAM_ID`'s doc comment in this module for the assumption this rests
    /// on).
    pub fn write_publish(&mut self, stream_key: &str, out: &mut Vec<u8>) {
        self.maybe_emit_set_chunk_size(out);
        let mut payload = Vec::new();
        let _ = amf0::write_string(&mut payload, "publish");
        amf0::write_number(&mut payload, 0.0); // NetStream commands conventionally use txn ID 0
        amf0::write_null(&mut payload);
        let _ = amf0::write_string(&mut payload, fit_to_u16_bytes(stream_key));
        let _ = amf0::write_string(&mut payload, "live");
        self.encoder.encode_message(
            CSID_COMMAND,
            MSG_AMF0_COMMAND,
            0,
            ASSUMED_STREAM_ID,
            &payload,
            out,
        );
    }

    /// Append an `onMetaData` AMF0 data message (ECMA Array of the fields set on `meta`).
    pub fn write_metadata(&mut self, meta: &OnMetaData, out: &mut Vec<u8>) {
        self.maybe_emit_set_chunk_size(out);
        let mut payload = Vec::new();
        let _ = amf0::write_string(&mut payload, "onMetaData");

        let fields = [
            ("width", meta.width),
            ("height", meta.height),
            ("framerate", meta.framerate),
            ("videocodecid", meta.videocodecid),
            ("audiocodecid", meta.audiocodecid),
        ];
        let count = u32::try_from(fields.iter().filter(|(_, v)| v.is_some()).count()).unwrap_or(0);
        amf0::write_ecma_array_start(&mut payload, count);
        for (key, value) in fields {
            if let Some(value) = value {
                let _ = amf0::write_property_name(&mut payload, key);
                amf0::write_number(&mut payload, value);
            }
        }
        amf0::write_object_end(&mut payload);
        self.encoder.encode_message(
            CSID_DATA,
            MSG_AMF0_DATA,
            0,
            ASSUMED_STREAM_ID,
            &payload,
            out,
        );
    }

    /// Append a video message (`data` is an already-built FLV-tag-body-shaped payload — see
    /// `adr/0001-rtmp-freestanding-core.md` § Payload boundary) on the assumed stream ID.
    pub fn push_video_data(&mut self, data: &[u8], timestamp_ms: u32, out: &mut Vec<u8>) {
        self.maybe_emit_set_chunk_size(out);
        self.encoder.encode_message(
            CSID_VIDEO,
            MSG_VIDEO,
            timestamp_ms,
            ASSUMED_STREAM_ID,
            data,
            out,
        );
    }

    /// Append an audio message (`data` is an already-built FLV-tag-body-shaped payload) on
    /// the assumed stream ID.
    pub fn push_audio_data(&mut self, data: &[u8], timestamp_ms: u32, out: &mut Vec<u8>) {
        self.maybe_emit_set_chunk_size(out);
        self.encoder.encode_message(
            CSID_AUDIO,
            MSG_AUDIO,
            timestamp_ms,
            ASSUMED_STREAM_ID,
            data,
            out,
        );
    }
}

/// Writes `key` + a String property value, silently truncating either to fit AMF0's 16-bit
/// length prefix (65,535 bytes) on the extraordinarily unlikely overflow. `Muxer`'s
/// command-writing methods are infallible by design
/// (`adr/0001-rtmp-freestanding-core.md` § 4); the low-level `amf0::write_string`/
/// `write_property_name` stay fallible for callers who need to detect this instead.
fn write_property_str(out: &mut Vec<u8>, key: &str, value: &str) {
    let _ = amf0::write_property_name(out, fit_to_u16_bytes(key));
    let _ = amf0::write_string(out, fit_to_u16_bytes(value));
}

/// Truncate `value` (at a UTF-8 char boundary) to fit AMF0's 16-bit length prefix.
fn fit_to_u16_bytes(value: &str) -> &str {
    if u16::try_from(value.len()).is_ok() {
        return value;
    }
    let mut end = usize::from(u16::MAX);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
#[path = "mux_tests.rs"]
mod tests;
