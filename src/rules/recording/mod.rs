//! Replays, phase 1 (rules only): the recording of what a game's player did and what real time
//! decided, and the full-state snapshot a replay continues from. Design:
//! `docs/superpowers/specs/2026-11-11-replays-design.md`; the flow: `docs/flows/replays.md`.
//!
//! - [`snapshot`]: the exact state of a live game (`Snapshot`) and its hash (`state_hash`). A
//!   save is not one: a load reseeds and resets (kept for saves).
//! - [`stream`]: the input stream (`Entry`s with a `Stamp`), its compact encoding.
//! - [`hook`]: the engine's hooks: a recorder receives what the rules report; a playback gives
//!   back what real time decided (the event seed, the «Максимум» moves, the patch globals) and
//!   the music's places. Nothing is attached by default: the rules then run as they always did.

pub mod battle_start;
pub mod header;
pub mod hook;
pub mod snapshot;
pub mod stream;

#[cfg(test)]
mod tests;
