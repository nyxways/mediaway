//! AMF0 (Action Message Format 0) **encode-only** subset — Number, Boolean, String, Object,
//! Null, ECMA Array.
//!
//! No decoder: a publish-only RTMP client does not need to parse `_result`/`onStatus`
//! command *contents* to make progress (see `adr/0001` § 3).
//!
//! Push-append-to-`&mut Vec<u8>` shape, same idiom as `flv-core`'s Muxer. Object/ECMA-Array values
//! are written as a sequence of calls — [`write_object_start`], then per property
//! [`write_property_name`] + one value writer, then [`write_object_end`] — rather than a
//! value-tree type, since this crate's actual need (command args, `onMetaData`) is a small,
//! fixed number of flat key/value pairs, not a general AMF value graph.

#![forbid(unsafe_code)]

use crate::error::Error;

const TYPE_NUMBER: u8 = 0x00;
const TYPE_BOOLEAN: u8 = 0x01;
const TYPE_STRING: u8 = 0x02;
const TYPE_OBJECT: u8 = 0x03;
const TYPE_NULL: u8 = 0x05;
const TYPE_ECMA_ARRAY: u8 = 0x08;
const OBJECT_END: [u8; 3] = [0x00, 0x00, 0x09];

/// Append an AMF0 Number: `0x00` + 8-byte big-endian IEEE-754 `f64`.
pub fn write_number(out: &mut Vec<u8>, value: f64) {
    out.push(TYPE_NUMBER);
    out.extend_from_slice(&value.to_be_bytes());
}

/// Append an AMF0 Boolean: `0x01` + 1 byte (`0x00` or `0x01`).
pub fn write_boolean(out: &mut Vec<u8>, value: bool) {
    out.push(TYPE_BOOLEAN);
    out.push(u8::from(value));
}

/// Append an AMF0 String: `0x02` + 16-bit big-endian byte length + UTF-8 bytes.
///
/// # Errors
/// [`Error::StringTooLong`] if `value` is longer than 65,535 bytes (AMF0's 16-bit length
/// prefix cannot represent more).
pub fn write_string(out: &mut Vec<u8>, value: &str) -> Result<(), Error> {
    let bytes = value.as_bytes();
    let len = u16::try_from(bytes.len()).map_err(|_| Error::StringTooLong(bytes.len()))?;
    out.push(TYPE_STRING);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

/// Append an AMF0 object/array property name: 16-bit big-endian byte length + UTF-8 bytes.
///
/// **No** type marker — property names are bare AMF0 UTF-8 strings, unlike a standalone
/// AMF0 String value written by [`write_string`].
///
/// # Errors
/// [`Error::StringTooLong`] if `key` is longer than 65,535 bytes.
pub fn write_property_name(out: &mut Vec<u8>, key: &str) -> Result<(), Error> {
    let bytes = key.as_bytes();
    let len = u16::try_from(bytes.len()).map_err(|_| Error::StringTooLong(bytes.len()))?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

/// Append an AMF0 Null: `0x05`, no payload.
pub fn write_null(out: &mut Vec<u8>) {
    out.push(TYPE_NULL);
}

/// Append the AMF0 Object marker (`0x03`). Follow with [`write_property_name`] + a value
/// writer per property, then [`write_object_end`].
pub fn write_object_start(out: &mut Vec<u8>) {
    out.push(TYPE_OBJECT);
}

/// Append the AMF0 ECMA Array marker (`0x08`) + its 32-bit big-endian associative-count field.
///
/// `count` is informational (readers commonly ignore it); follow with the same
/// [`write_property_name`] + value-writer + [`write_object_end`] shape as an Object.
pub fn write_ecma_array_start(out: &mut Vec<u8>, count: u32) {
    out.push(TYPE_ECMA_ARRAY);
    out.extend_from_slice(&count.to_be_bytes());
}

/// Append the Object/ECMA-Array terminator: empty property name (`0x00 0x00`) + end marker
/// (`0x09`).
pub fn write_object_end(out: &mut Vec<u8>) {
    out.extend_from_slice(&OBJECT_END);
}

#[cfg(test)]
#[path = "amf0_tests.rs"]
mod tests;
