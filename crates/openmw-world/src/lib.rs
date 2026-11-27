//! `razdor-openmw`: converts a Discord Times map (the `razdor-dtm-json` export) into an OpenMW
//! plugin. Milestone 2: the land (heights, normals, the margin ring) and the ground textures. See
//! `README.md` and `docs/specs/2026-11-27-openmw-world.md`.

pub mod convert;
pub mod grid;
pub mod heightmap;
pub mod input;
pub mod passable;
pub mod tes3;
pub mod textures;
