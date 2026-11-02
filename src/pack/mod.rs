//! Content packages (`.rzpack`): reading, checking and the `dt:` alias table. No macroquad here;
//! the format is documented in `docs/reference/rzpack-format.md`.

pub mod aliases;
pub mod check;
pub mod content;
pub mod diag;
pub mod files;
pub mod load;
pub mod manifest;
pub mod refs;
pub mod texts;

pub use check::check;
pub use diag::{Diagnostic, Report, Severity};
pub use files::Files;
pub use load::Package;
