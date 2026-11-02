//! Content packages (`.rzpack`): reading, checking, compiling and the `dt:` alias table. No
//! macroquad here; the format is documented in `docs/reference/rzpack-format.md`.

pub mod aliases;
pub mod check;
pub mod compile;
pub mod content;
pub mod diag;
pub mod files;
pub mod load;
pub mod manifest;
pub mod map;
pub mod map_check;
pub mod open;
pub mod refs;
pub mod sim;
pub mod terrain;
pub mod texts;
pub mod triggers;

pub use check::check;
pub use diag::{Diagnostic, Report, Severity};
pub use files::Files;
pub use load::Package;
pub use open::Opened;
