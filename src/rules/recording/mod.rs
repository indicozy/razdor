//! Replays (rules only): the full-state snapshot a replay continues from, the engine's hooks that
//! take real time's decisions out of the rules, and the input stream. The design is
//! `docs/superpowers/specs/2026-11-11-replays-design.md`; the flow is `docs/flows/replays.md`.
//!
//! - [`snapshot`]: the exact state of a live game (`Snapshot`) and its hash (`state_hash`). A
//!   save is not one: a load reseeds and resets (kept for saves).
//! - [`stream`]: the input stream (`Entry`s with a `Stamp`), its compact encoding.
//! - [`hook`]: the engine's hooks: a recorder receives what the rules report; a playback gives
//!   back what real time decided (the event seed, the «Максимум» moves, the patch globals) and
//!   the music's places. Nothing is attached by default: the rules then run as they always did.

pub mod hook;
pub mod snapshot;
pub mod stream;

#[cfg(test)]
mod tests;
