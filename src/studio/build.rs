//! One rebuild of the package, as a pure function of its inputs so it can run on the studio's
//! worker thread: the folder read from disk with the open documents' text (and the painted
//! terrains) over it, parsed, checked, and the shown map compiled on the install and walked for its
//! unreached story cells (`pack::reach`). Nothing here touches the window or the workspace.

use std::path::PathBuf;
use std::sync::Arc;

use crate::dt::install::DtInstall;
use crate::pack::check;
use crate::pack::compile::{self, Compiled};
use crate::pack::diag::Report;
use crate::pack::files::Files;
use crate::pack::load::Package;
use crate::pack::reach::{self, Site};
use crate::pack::terrain::{self, Palette};

/// What one rebuild reads.
pub struct Job {
    /// The package folder.
    pub root: PathBuf,
    /// Files to put over the disk: the open text documents, and the painted terrains.
    pub overlays: Vec<(String, Vec<u8>)>,
    /// The map to compile and to walk (`None`: none is shown).
    pub map: Option<String>,
    /// The language of the texts the compile uses.
    pub lang: String,
    /// The game's install (`None` when there is none: no map is compiled).
    pub install: Option<Arc<DtInstall>>,
}

/// What one rebuild made.
pub struct Built {
    /// The package as read, the overlays over the disk.
    pub package: Package,
    /// The checks' diagnostics (the parse problems first).
    pub report: Report,
    /// The map the compile and the walk are of.
    pub map: Option<String>,
    /// The compile of the map: its scenario, or the report of why it did not compile.
    pub compiled: Option<Result<Compiled, Report>>,
    /// The story cells the hero's start cannot reach on foot, when the map's story reads.
    pub unreached: Option<Vec<Site>>,
}

/// Runs a rebuild. An error only when the folder cannot be read at all.
pub fn run(job: Job) -> Result<Built, String> {
    let mut files = Files::load(&job.root)?;
    for (path, bytes) in job.overlays {
        files.overlay(&path, bytes)?;
    }
    let (package, _) = Package::parse(files);
    let report = check::check(&package);
    let (compiled, unreached) = match (&job.map, &job.install) {
        (Some(map), install) => {
            let compiled = install.as_ref().map(|i| compile::compile_drawn(&package, i, &job.lang, map));
            (compiled, unreached_of(&package, map))
        }
        (None, _) => (None, None),
    };
    Ok(Built { package, report, map: job.map, compiled, unreached })
}

/// The story cells of `map` the hero's start cannot reach on foot (`pack::reach`), or `None` when
/// the map's story or its terrain does not read.
fn unreached_of(package: &Package, map: &str) -> Option<Vec<Site>> {
    let spec = package.maps.get(map)?.spec.as_ref()?;
    let palette = Palette::with_overrides(&spec.terrain_palette).ok()?;
    let png = package.files.get(&format!("maps/{map}/terrain.png"))?;
    let t = terrain::decode(png, &palette).ok()?;
    let story = reach::story(&package.files, map).ok()?;
    let foot = reach::foot_map(&package.files, map, t.width as i32, t.height as i32, &t.cells).ok()?;
    Some(reach::unreached(&foot, story.hero, &story).into_iter().cloned().collect())
}
