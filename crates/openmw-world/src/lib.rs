//! `razdor-openmw`: converts a Discord Times map (the `razdor-dtm-json` export) into an OpenMW
//! plugin. Milestone 1: the land and the ground textures. See `README.md` and
//! `docs/specs/2026-11-27-openmw-world.md`.

pub mod convert;
pub mod grid;
pub mod input;
pub mod tes3;
pub mod textures;
