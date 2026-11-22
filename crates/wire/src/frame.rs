//! Framed bodies: a JSON header and raw bytes in one request. Used by the battle upload (the
//! record), the crash's save, and a save put (the save file). Layout, little-endian:
//!
//! `u32 header length | header (JSON, at most limits::MAX_FRAME_HEADER bytes) | body (the rest)`
//!
//! The receiver decodes the header, validates it (its declared `len` or `record_len` must equal the
//! body's length, and the body's size must be under the endpoint's limit), then checks the body's
//! sha256 when the header carries one. No I/O here: the caller reads and writes the bytes.

use serde::{de::DeserializeOwned, Serialize};

use crate::error::{WireError, WireResult};
use crate::limits;

/// Encodes `header` and `body` into one buffer.
pub fn encode<H: Serialize>(header: &H, body: &[u8]) -> WireResult<Vec<u8>> {
    let json = serde_json::to_vec(header).map_err(|e| WireError::Json(e.to_string()))?;
    if json.len() > limits::MAX_FRAME_HEADER {
        return Err(WireError::TooLong { field: "frame header", max: limits::MAX_FRAME_HEADER, len: json.len() });
    }
    let mut out = Vec::with_capacity(4 + json.len() + body.len());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(body);
    Ok(out)
}

/// Splits a framed buffer into its header and the body (borrowed from `bytes`).
pub fn decode<H: DeserializeOwned>(bytes: &[u8]) -> WireResult<(H, &[u8])> {
    if bytes.len() < 4 {
        return Err(WireError::Frame("shorter than its length prefix"));
    }
    let n = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if n > limits::MAX_FRAME_HEADER {
        return Err(WireError::Frame("header longer than the limit"));
    }
    let rest = &bytes[4..];
    if rest.len() < n {
        return Err(WireError::Frame("header cut short"));
    }
    let header = serde_json::from_slice(&rest[..n]).map_err(|e| WireError::Json(e.to_string()))?;
    Ok((header, &rest[n..]))
}
