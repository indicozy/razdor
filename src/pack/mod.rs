//! Content packages (`.rzpack`): reading, checking, compiling and the `dt:` alias table. No
//! macroquad here; the format is documented in `docs/reference/rzpack-format.md`.

pub mod aliases;
pub mod archive;
pub mod campaign;
pub mod check;
pub mod compile;
pub mod content;
pub mod diag;
pub mod files;
pub mod load;
pub mod manifest;
pub mod map;
pub mod reach;
pub mod map_check;
pub mod open;
#[cfg(test)]
pub mod playthrough;
#[cfg(test)]
pub mod playthrough_ch23;
#[cfg(test)]
pub mod playthrough_camp;
#[cfg(test)]
pub mod playthrough_ch45;
#[cfg(test)]
pub mod playthrough_abbey;
#[cfg(test)]
pub mod playthrough_salt;
pub mod refs;
pub mod sim;
pub mod terrain;
pub mod texts;
pub mod trust;
pub mod triggers;

pub use check::check;
pub use diag::{Diagnostic, Report, Severity};
pub use files::Files;
pub use load::Package;
pub use open::Opened;
