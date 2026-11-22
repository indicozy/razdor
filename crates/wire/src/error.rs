//! The one error type of the crate: what failed validation or decoding, and which field.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireError {
    /// A required text is empty.
    Empty { field: &'static str },
    /// A text, list or map has more than its limit (lengths in characters or items).
    TooLong { field: &'static str, max: usize, len: usize },
    /// A number is above its limit.
    OutOfRange { field: &'static str, max: u64, value: u64 },
    /// A text has the wrong shape (charset, length, a code's format, an unsupported schema).
    Invalid { field: &'static str, reason: &'static str },
    /// A body is larger than its endpoint allows.
    BodyTooLarge { max: usize, len: usize },
    /// A framed body is cut or its header is out of range.
    Frame(&'static str),
    /// JSON did not decode into the type (a missing field, a wrong type, an unknown enum value).
    Json(String),
}

pub type WireResult<T = ()> = Result<T, WireError>;

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireError::Empty { field } => write!(f, "{field}: empty"),
            WireError::TooLong { field, max, len } => write!(f, "{field}: {len} is over the limit {max}"),
            WireError::OutOfRange { field, max, value } => write!(f, "{field}: {value} is over the limit {max}"),
            WireError::Invalid { field, reason } => write!(f, "{field}: {reason}"),
            WireError::BodyTooLarge { max, len } => write!(f, "body of {len} bytes is over the limit {max}"),
            WireError::Frame(what) => write!(f, "framed body: {what}"),
            WireError::Json(what) => write!(f, "json: {what}"),
        }
    }
}

impl std::error::Error for WireError {}
