//! The API's shared types: what the game sends and the server answers, for the analytics and the
//! cloud saves (`docs/superpowers/specs/2026-11-22-analytics-design.md`, `…-cloud-saves-design.md`).
//! Both sides use these types and their [`Validate`] checks, so the formats cannot drift. The crate
//! does no I/O and needs no async runtime. See `README.md` beside it.
//!
//! Forward compatibility: no type denies unknown fields, so a newer peer's extra fields are ignored.
//! A new enum variant is refused by the older side (serde); a newer `schema` is refused by [`Validate`].

#![forbid(unsafe_code)]

pub mod analytics;
pub mod cloud;
mod check;
pub mod error;
pub mod frame;
pub mod ids;
pub mod limits;
pub mod paths;
#[cfg(test)]
mod tests;

pub use error::{WireError, WireResult};
pub use ids::{AccountKey, InstallId, LinkCode};

use serde::{Deserialize, Serialize};

/// The schema version of every body this crate defines. A body with `schema` 0 or above this is refused.
pub const SCHEMA: u32 = 1;

/// A body that checks itself against the limits. Both the game (before it sends) and the server
/// (before it stores) call it; a refusal names the field and the rule.
pub trait Validate {
    fn validate(&self) -> WireResult;
}

/// The server's plain answer to a write: the id it stored under, when the write made one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Set by the cloud's writes that really happened (`{"ok":true}`). A silent answer is `{}`, so
    /// a client never takes an unconfirmed answer for success (it would forget its key).
    #[serde(default, skip_serializing_if = "is_false")]
    pub ok: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Ack {
    /// The confirmed answer of a cloud write that happened.
    pub fn confirmed() -> Ack {
        Ack { id: None, ok: true }
    }
}
