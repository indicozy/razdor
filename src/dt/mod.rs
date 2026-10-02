//! Readers for the files of an installed *Discord Times* (Community Update).
//!
//! Pure data decoding: no macroquad and no game rules. Nothing here ships original content;
//! everything is read at runtime from the player's own install (see [`install`]).
//! Formats are documented in `docs/reference/dtm-format.md` and `docs/reference/mechanics.md`,
//! images in `docs/reference/graphics-formats.md`.
pub mod container;
pub mod data;
pub mod dtm;
pub mod gfx;
pub mod ini;
pub mod install;
pub mod sound;
pub mod text;

use std::fmt;
use std::path::PathBuf;

/// Everything that can go wrong while reading the original's files.
#[derive(Debug)]
pub enum DtError {
    /// A file could not be read.
    Io { path: PathBuf, source: std::io::Error },
    /// `RAZDOR_DT_DIR` is not set.
    NoInstallDir,
    /// The file does not start with the expected magic bytes.
    BadMagic { what: &'static str },
    /// The data ends before a field or section that must be there.
    Truncated { what: &'static str, offset: usize },
    /// The bzip2 stream is corrupt.
    Bzip2(String),
    /// The zlib stream of a Community editor demo map is corrupt.
    Zlib(String),
    /// The map is too big to expand its terrain.
    Terrain(String),
    /// Bytes remain after the last known part of the payload.
    TrailingBytes { offset: usize, count: usize },
    /// An ini value could not be interpreted.
    BadValue { section: String, key: String, value: String },
    /// A required ini key or section is missing.
    Missing { section: String, key: String },
    /// An image record is malformed (bad size, or it runs past the end of the data).
    Image { what: &'static str, offset: usize },
    /// A sound file is malformed or in a format Razdor does not play.
    Sound(String),
}

impl fmt::Display for DtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DtError::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            DtError::NoInstallDir => write!(f, "no Discord Times install found (set RAZDOR_DT_DIR once, or put the game in ~/Games)"),
            DtError::BadMagic { what } => write!(f, "not a {what} (bad magic bytes)"),
            DtError::Truncated { what, offset } => write!(f, "{what} truncated at offset {offset:#x}"),
            DtError::Bzip2(e) => write!(f, "bzip2: {e}"),
            DtError::Zlib(e) => write!(f, "zlib: {e}"),
            DtError::Terrain(e) => write!(f, "terrain: {e}"),
            DtError::TrailingBytes { offset, count } => write!(f, "{count} trailing bytes at {offset:#x}"),
            DtError::BadValue { section, key, value } => write!(f, "[{section}] {key}={value}: bad value"),
            DtError::Missing { section, key } => write!(f, "[{section}] {key} is missing"),
            DtError::Image { what, offset } => write!(f, "bad {what} at offset {offset:#x}"),
            DtError::Sound(e) => write!(f, "sound: {e}"),
        }
    }
}

impl std::error::Error for DtError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DtError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
