//! The checks the types' `validate` fns share. Crate-private: the limits are in `limits`.

use std::collections::BTreeMap;

use crate::error::{WireError, WireResult};
use crate::limits;

/// The schema version is 1 up to the crate's [`crate::SCHEMA`].
pub(crate) fn schema(v: u32) -> WireResult {
    if v == 0 || v > crate::SCHEMA {
        return Err(WireError::Invalid { field: "schema", reason: "unsupported schema version" });
    }
    Ok(())
}

fn max_len(field: &'static str, len: usize, max: usize) -> WireResult {
    if len > max {
        return Err(WireError::TooLong { field, max, len });
    }
    Ok(())
}

/// A required line of text: not empty, not over `max` characters, no control characters.
pub(crate) fn text(field: &'static str, s: &str, max: usize) -> WireResult {
    if s.is_empty() {
        return Err(WireError::Empty { field });
    }
    max_len(field, s.chars().count(), max)?;
    if s.chars().any(char::is_control) {
        return Err(WireError::Invalid { field, reason: "control character" });
    }
    Ok(())
}

/// Text that may be empty (the same rules otherwise).
pub(crate) fn opt_text(field: &'static str, s: &str, max: usize) -> WireResult {
    if s.is_empty() {
        return Ok(());
    }
    text(field, s, max)
}

/// Free text (a log line, a backtrace): only the length is checked.
pub(crate) fn free(field: &'static str, s: &str, max: usize) -> WireResult {
    max_len(field, s.chars().count(), max)
}

/// A name that stands for a file or a key (a map's file name, a mod's id): a text with no path parts.
pub(crate) fn key(field: &'static str, s: &str, max: usize) -> WireResult {
    text(field, s, max)?;
    if s.contains(['/', '\\']) || s == "." || s == ".." {
        return Err(WireError::Invalid { field, reason: "path separator in a key" });
    }
    Ok(())
}

/// Lowercase hex of a length in `min..=max`.
pub(crate) fn hex(field: &'static str, s: &str, min: usize, max: usize) -> WireResult {
    max_len(field, s.len(), max)?;
    if s.len() < min {
        return Err(WireError::Invalid { field, reason: "too short" });
    }
    if !s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(WireError::Invalid { field, reason: "not lowercase hex" });
    }
    Ok(())
}

pub(crate) fn sha256(field: &'static str, s: &str) -> WireResult {
    hex(field, s, 64, 64)
}

pub(crate) fn count<T>(field: &'static str, items: &[T], max: usize) -> WireResult {
    max_len(field, items.len(), max)
}

pub(crate) fn number(field: &'static str, value: u64, max: u64) -> WireResult {
    if value > max {
        return Err(WireError::OutOfRange { field, max, value });
    }
    Ok(())
}

/// A measured time or share: finite, not negative, not over `max`.
pub(crate) fn float(field: &'static str, value: f32, max: f32) -> WireResult {
    if !value.is_finite() || value < 0.0 || value > max {
        return Err(WireError::Invalid { field, reason: "not a finite number in range" });
    }
    Ok(())
}

/// A map of settings: its size, its keys and its values.
pub(crate) fn settings(field: &'static str, m: &BTreeMap<String, String>) -> WireResult {
    max_len(field, m.len(), limits::MAX_SETTINGS)?;
    for (k, v) in m {
        key(field, k, limits::MAX_KEY)?;
        free(field, v, limits::MAX_SETTING_VALUE)?;
    }
    Ok(())
}
