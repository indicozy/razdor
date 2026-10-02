//! The original editor's new-map generator (docs/reference/editor/newmap.md): its dialog's
//! options, the share bars and their `[MakeMap]` widths, and the generator itself, which
//! builds the same map as DTMapEdit for the same seed, options and installed sprites.
//!
//! The generator keeps what the original's dialog keeps between two "create" clicks: its
//! work grid ([`Generator`]), so "keep" without "rebuild relief" cuts the new map from what
//! the last run left there (§10.1). A run writes the map cells as the original writes its
//! own in place: a run that stops early leaves them half made ([`Outcome`]).
//!
//! Where the original hangs (a coast-band loop that cannot finish, forest sprites that can
//! never be placed) or stops with an error it swallows (a flat field, the small forest pool),
//! the run stops cleanly at the same point and says why ([`Stop`]).

pub(crate) mod ext;
mod land;
mod relief;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::dt::dtm::{Header, MapObject, Scenario};
use crate::dt::ini::{Ini, Section};
use crate::rules::rng::Rng;

use super::defaults::DEFAULT_RELATIONS;
use super::grid::ObjectGrid;
use super::palette::{ForestFacts, Palette};

pub use relief::Grid;

/// The sizes the dialog offers (§1.2); maps are square.
pub const SIZES: [u32; 5] = [50, 100, 200, 400, 800];

/// Map types (§5), in the dialog's order.
pub const KINDS: [&str; 8] = [
    crate::i18n::n_("Land"),
    crate::i18n::n_("Lake"),
    crate::i18n::n_("River valley"),
    crate::i18n::n_("Estuary"),
    crate::i18n::n_("Coast"),
    crate::i18n::n_("Skerries"),
    crate::i18n::n_("Island"),
    crate::i18n::n_("Archipelago"),
];
pub const LAND: u8 = 0;
pub const LAKE: u8 = 1;
pub const VALLEY: u8 = 2;
pub const ESTUARY: u8 = 3;
pub const COAST: u8 = 4;
pub const SKERRIES: u8 = 5;
pub const ISLAND: u8 = 6;
pub const ARCHIPELAGO: u8 = 7;

/// The side the sea or the river mouth faces.
pub const ORIENTATIONS: [&str; 4] = [crate::i18n::n_("North"), crate::i18n::n_("South"), crate::i18n::n_("West"), crate::i18n::n_("East")];

/// The blur slider's range.
pub const MAX_BLUR: u8 = 5;

/// The dialog's options (§1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub size: u32,
    /// [`KINDS`] index.
    pub kind: u8,
    /// [`ORIENTATIONS`] index.
    pub orient: u8,
    pub shares: Shares,
    pub blur: u8,
    /// The "keep" box: use the typed seed (unless it is 0) and allow reusing the relief.
    pub keep: bool,
    /// With "keep": still compute the relief.
    pub rebuild: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { size: 50, kind: LAND, orient: 0, shares: Shares::default(), blur: 2, keep: false, rebuild: false }
    }
}

// ------------------------------------------------------------------------------------------
// The share bars (§1.1)
// ------------------------------------------------------------------------------------------

/// The widths of the panels the user sizes, in the original's pixels: every share of the
/// generator is a panel width. The stretched panels' widths follow ([`Shares::l0`] …). W3
/// and SL3 start at 25 in every dialog and are never saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shares {
    pub w0: i32,
    pub w1: i32,
    pub w3: i32,
    pub sl: i32,
    pub ll: i32,
    pub lh: i32,
    pub lr: i32,
    pub sl1: i32,
    pub sl3: i32,
    pub sw1: i32,
    pub f0: i32,
    pub f2: i32,
}

/// A bar's inside is 106 pixels and a splitter 3: a stretched panel takes the rest (0 when
/// nothing is left).
const BAR: i32 = 106;
const SPLIT: i32 = 3;
/// The two right-hand panels the form fixes at 25.
pub const FIXED: i32 = 25;

impl Default for Shares {
    /// The install's `[MakeMap]` values.
    fn default() -> Self {
        Shares { w0: 27, w1: 35, w3: FIXED, sl: 20, ll: 53, lh: 32, lr: 1, sl1: 54, sl3: FIXED, sw1: 44, f0: 39, f2: 22 }
    }
}

/// Which panel a splitter sizes, from which side, and the splitter's minimum (the form's
/// MinSize).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Splitter {
    pub field: Field,
    pub right: bool,
    pub min: i32,
}

/// A sized panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    W0,
    W1,
    W3,
    Sl,
    Ll,
    Lh,
    Lr,
    Sl1,
    Sl3,
    Sw1,
    F0,
    F2,
}

/// The seven bars' splitters, left to right; a bar's panels are its splitters' panels with
/// the stretched one where the left-hand splitters end.
pub const BARS: [&[Splitter]; 7] = [
    &[Splitter { field: Field::W0, right: false, min: 5 }],
    &[Splitter { field: Field::W1, right: false, min: 5 }, Splitter { field: Field::W3, right: true, min: 5 }],
    &[Splitter { field: Field::Sl, right: false, min: 5 }],
    &[
        Splitter { field: Field::Ll, right: false, min: 5 },
        Splitter { field: Field::Lh, right: false, min: 1 },
        Splitter { field: Field::Lr, right: true, min: 1 },
    ],
    &[Splitter { field: Field::Sl1, right: false, min: 5 }, Splitter { field: Field::Sl3, right: true, min: 5 }],
    &[Splitter { field: Field::Sw1, right: false, min: 5 }],
    &[Splitter { field: Field::F0, right: false, min: 5 }, Splitter { field: Field::F2, right: true, min: 5 }],
];

impl Shares {
    pub fn get(&self, f: Field) -> i32 {
        match f {
            Field::W0 => self.w0,
            Field::W1 => self.w1,
            Field::W3 => self.w3,
            Field::Sl => self.sl,
            Field::Ll => self.ll,
            Field::Lh => self.lh,
            Field::Lr => self.lr,
            Field::Sl1 => self.sl1,
            Field::Sl3 => self.sl3,
            Field::Sw1 => self.sw1,
            Field::F0 => self.f0,
            Field::F2 => self.f2,
        }
    }

    fn set(&mut self, f: Field, v: i32) {
        let slot = match f {
            Field::W0 => &mut self.w0,
            Field::W1 => &mut self.w1,
            Field::W3 => &mut self.w3,
            Field::Sl => &mut self.sl,
            Field::Ll => &mut self.ll,
            Field::Lh => &mut self.lh,
            Field::Lr => &mut self.lr,
            Field::Sl1 => &mut self.sl1,
            Field::Sl3 => &mut self.sl3,
            Field::Sw1 => &mut self.sw1,
            Field::F0 => &mut self.f0,
            Field::F2 => &mut self.f2,
        };
        *slot = v.max(0);
    }

    /// The stretched panel of bar `bar`: what the sized panels and the splitters leave.
    pub fn stretched(&self, bar: usize) -> i32 {
        let used: i32 = BARS[bar].iter().map(|s| self.get(s.field) + SPLIT).sum();
        (BAR - used).max(0)
    }

    pub fn l0(&self) -> i32 {
        self.stretched(0)
    }
    pub fn w2(&self) -> i32 {
        self.stretched(1)
    }
    pub fn l1(&self) -> i32 {
        self.stretched(2)
    }
    pub fn lm(&self) -> i32 {
        self.stretched(3)
    }
    pub fn sl2(&self) -> i32 {
        self.stretched(4)
    }
    pub fn sw2(&self) -> i32 {
        self.stretched(5)
    }
    pub fn f1(&self) -> i32 {
        self.stretched(6)
    }

    /// The panel widths of bar `bar`, left to right.
    pub fn panels(&self, bar: usize) -> Vec<i32> {
        let s = BARS[bar];
        let mut out: Vec<i32> = s.iter().filter(|s| !s.right).map(|s| self.get(s.field)).collect();
        out.push(self.stretched(bar));
        out.extend(s.iter().filter(|s| s.right).map(|s| self.get(s.field)));
        out
    }

    /// A drag of splitter `k` of bar `bar` asking its panel for `size` pixels, as the
    /// toolkit's splitter answers it: at least the splitter's minimum and at most what keeps
    /// the stretched panel that minimum, and a size at or below the minimum snaps to 0.
    pub fn drag(&mut self, bar: usize, k: usize, size: i32) {
        let s = BARS[bar][k];
        let max = self.get(s.field) + self.stretched(bar) - s.min;
        let mut v = if size < s.min { s.min } else if size > max { max } else { size };
        if v <= s.min {
            v = 0;
        }
        self.set(s.field, v);
    }

    // --- [MakeMap] ------------------------------------------------------------------------

    /// The section as the dialog reads it (0x51cc28): twelve integers, a missing key 0. W2
    /// and SL2 are read but the stretched panels ignore them; W3 and SL3 are the form's 25.
    pub fn from_section(sec: &Section) -> Shares {
        let int = |k: &str| sec.get(k).and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(0).max(0);
        Shares {
            w0: int("W0"),
            w1: int("W1"),
            w3: FIXED,
            sl: int("SL"),
            ll: int("LL"),
            lh: int("LH"),
            lr: int("LR"),
            sl1: int("SL1"),
            sl3: FIXED,
            sw1: int("Sw1"),
            f0: int("F0"),
            f2: int("F2"),
        }
    }

    /// The twelve keys the exit button writes (0x528438), the stretched W2 and SL2 as they
    /// stand.
    pub fn keys(&self) -> Vec<(&'static str, String)> {
        let v = [
            ("W0", self.w0),
            ("W1", self.w1),
            ("W2", self.w2()),
            ("SL", self.sl),
            ("LL", self.ll),
            ("LH", self.lh),
            ("LR", self.lr),
            ("SL1", self.sl1),
            ("SL2", self.sl2()),
            ("Sw1", self.sw1),
            ("F0", self.f0),
            ("F2", self.f2),
        ];
        v.into_iter().map(|(k, n)| (k, n.to_string())).collect()
    }

    /// `[MakeMap]` of Razdor's editor ini in `dir`, else the install's shipped values. The
    /// install's own ini is never read or written for it.
    pub fn load(dir: Option<&Path>) -> Shares {
        let read = |p: PathBuf| std::fs::read(p).ok().and_then(|b| Ini::from_cp1251(&b).section(SECTION).map(Shares::from_section));
        dir.and_then(|d| read(d.join(super::options::FILE))).unwrap_or_default()
    }

    /// Writes the section to Razdor's editor ini in `dir`, keeping its other keys.
    pub fn save(&self, dir: &Path) -> std::io::Result<PathBuf> {
        super::options::update_section(dir, SECTION, &self.keys())
    }
}

/// The settings section of the generator.
pub const SECTION: &str = "MakeMap";

// ------------------------------------------------------------------------------------------
// Sprites
// ------------------------------------------------------------------------------------------

/// What the generator reads about the installed object sprites (§9.1): the editor's
/// per-group counts and which sprites are loaded.
#[derive(Clone, Debug)]
pub struct Sprites {
    /// The massif counts as the loader keeps them (0x59a718): one byte per `7·class +
    /// sprite div 10` for classes 1–7, so a sprite id of 70 or more counts towards the next
    /// class's groups (the original's layout, kept).
    massif: [u8; 7 * 8 + 26],
    forest: ForestFacts,
    loaded: Vec<[bool; 256]>,
}

impl Sprites {
    pub fn from_palette(p: &Palette) -> Sprites {
        let mut s = Sprites { massif: [0; 82], forest: p.forest_facts(), loaded: vec![[false; 256]; 13] };
        for o in &p.objects {
            if (o.class as usize) < s.loaded.len() {
                s.loaded[o.class as usize][o.sprite as usize] = true;
            }
            if (1..=7).contains(&o.class) {
                let i = 7 * o.class as usize + o.sprite as usize / 10;
                s.massif[i] = s.massif[i].wrapping_add(1);
            }
        }
        s
    }

    /// Hill (class 1) variants of size `k`.
    pub fn hills(&self, k: i32) -> i32 {
        self.massif[7 + k as usize] as i32
    }

    /// Mountain (class 5) variants of size `k`.
    pub fn mountains(&self, k: i32) -> i32 {
        self.massif[35 + k as usize] as i32
    }

    /// Plant sprites of family `f` of class `class` (9–11).
    pub fn plants(&self, class: u8, f: u8) -> i32 {
        self.forest.count(class, f) as i32
    }

    /// Whether the object word `class·256 + sprite` has a loaded sprite.
    pub fn loaded(&self, word: u16) -> bool {
        let (class, sprite) = ((word >> 8) as usize, (word & 0xff) as usize);
        class >= 1 && class < self.loaded.len() && self.loaded[class][sprite]
    }

    /// The forest families (§9.4 step 3): 0–9 with a sprite in class 9, 10 or 11.
    pub fn families(&self) -> Vec<u8> {
        (0..10u8).filter(|&f| (9..=11).any(|c| self.forest.count(c, f) != 0)).collect()
    }
}

// ------------------------------------------------------------------------------------------
// Map cells
// ------------------------------------------------------------------------------------------

/// The map cells as the generator writes them: the mark (terrain code, a negative object
/// class or a marker), the terrain ("ground"), the object word of classes 1–8, the tree
/// word, and the generator's scratch flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cells {
    pub w: i32,
    pub h: i32,
    pub mark: Vec<i8>,
    pub ground: Vec<u8>,
    pub obj: Vec<u16>,
    pub tree: Vec<u16>,
    pub flag: Vec<u8>,
}

impl Cells {
    /// Cleared cells, as the relief path leaves them.
    pub fn zero(size: u32) -> Cells {
        let n = size as usize * size as usize;
        Cells { w: size as i32, h: size as i32, mark: vec![0; n], ground: vec![0; n], obj: vec![0; n], tree: vec![0; n], flag: vec![0; n] }
    }

    /// The cells of an open map at a new size: each cell `(x, y)` keeps what that cell of the
    /// map holds (the original's cell array does not move with the size), cells beyond it
    /// are empty.
    pub fn of_map(s: &Scenario, marks: &[i8], size: u32) -> Cells {
        let mut c = Cells::zero(size);
        let (w, h) = (s.width() as i32, s.height() as i32);
        let grid = ObjectGrid::from_objects(s.width(), s.height(), &s.objects);
        for y in 0..c.h.min(h) {
            for x in 0..c.w.min(w) {
                let (i, j) = (c.idx(x, y), (y * w + x) as usize);
                c.ground[i] = s.terrain.get(j).copied().unwrap_or(0);
                c.mark[i] = marks.get(j).copied().unwrap_or(c.ground[i] as i8);
                let [a, b] = grid.at(x as i64, y as i64);
                c.obj[i] = a.map_or(0, word);
                c.tree[i] = b.map_or(0, word);
            }
        }
        c
    }

    pub fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.w + x) as usize
    }

    /// The terrain codes, row by row.
    pub fn terrain(&self) -> Vec<u8> {
        self.ground.clone()
    }

    /// The objects as a save writes them: per cell the object, then the tree.
    pub fn objects(&self) -> Vec<MapObject> {
        let mut out = Vec::new();
        for y in 0..self.h {
            for x in 0..self.w {
                let i = self.idx(x, y);
                for wd in [self.obj[i], self.tree[i]] {
                    if wd != 0 {
                        out.push(MapObject { x: x as u16, y: y as u16, sprite: wd as u8, class: (wd >> 8) as u8 });
                    }
                }
            }
        }
        out
    }
}

fn word(o: MapObject) -> u16 {
    o.sprite as u16 | (o.class as u16) << 8
}

/// The shared point list of a run (the original's 500,000 records): every list the run
/// builds overwrites it from the start, so a draw of `Random(0)` reads whatever entry 0 a
/// former list left (the original's behaviour, kept).
pub(crate) struct Points {
    pub x: Vec<i16>,
    pub y: Vec<i16>,
    pub r: Vec<i16>,
}

pub(crate) const POINTS: usize = 500_000;

impl Points {
    fn new() -> Points {
        Points { x: vec![0; POINTS], y: vec![0; POINTS], r: vec![0; POINTS] }
    }

    pub fn at(&self, i: usize) -> (i32, i32) {
        (self.x[i] as i32, self.y[i] as i32)
    }

    pub fn put(&mut self, i: usize, x: i32, y: i32) -> Result<(), Stop> {
        if i >= POINTS {
            return Err(Stop::RangeError);
        }
        self.x[i] = x as i16;
        self.y[i] = y as i16;
        Ok(())
    }
}

// ------------------------------------------------------------------------------------------
// Running
// ------------------------------------------------------------------------------------------

/// Why a run stopped before the end. The original swallows every error of a run (§10.3)
/// and hangs where noted; the map stays as far as it got and the header is not reset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// The break button.
    Break,
    /// Every height equal: the rescale divides 0 by 0 (§4 step 1).
    FlatField,
    /// All shares of a cut are 0: a division by zero.
    NoShares,
    /// A list outgrew the original's point list (a range error).
    RangeError,
    /// The original would loop for ever here (only its break button ends it).
    Hang(Stage),
    /// The forest's progress step is 0 (fewer pool cells than `(W div 50)² × 30`): the
    /// original divides by it after the first cluster (§9.4 step 5).
    SmallForestPool,
}

/// Where a run can hang.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// §8 step 2: lowland must grow and cannot.
    MoreLowland,
    /// §8 step 5: no sand cell left to take back.
    Sand,
    /// §8 step 6: no lowland cell left for marsh.
    Marsh,
    /// §8 step 7: no marsh cell left for swamp.
    Swamp,
    /// §9.4 step 6: no forest cell can get a loaded sprite.
    Forest,
}

pub(crate) type Step<T> = Result<T, Stop>;

/// What a run is given.
#[derive(Clone, Debug)]
pub struct Job {
    pub options: Options,
    /// The seed field.
    pub seed: i32,
    pub sprites: Sprites,
    /// The map the cells start from when the relief is reused (the original writes over its
    /// open map; the relief path clears it).
    pub start: Cells,
    /// The processor clock, for a seed of 0 or "keep" off.
    pub clock: u32,
}

/// What a run leaves.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub cells: Cells,
    /// The editor's generator after the run.
    pub rng: Rng,
    /// The seed field after the run.
    pub seed: i32,
    /// The seed the run wrote into the open map's header at its start (a clock seed).
    pub header_seed: Option<u32>,
    pub stop: Option<Stop>,
}

impl Outcome {
    /// The run reached its end, which resets the header (§11).
    pub fn complete(&self) -> bool {
        self.stop.is_none()
    }
}

/// The dialog's state between runs: the work grid and the forest's progress divisor (set
/// only by a relief run, §10.1).
pub struct Generator {
    grid: Grid,
    divisor: i32,
}

impl Default for Generator {
    fn default() -> Self {
        Generator::new()
    }
}

/// The run's working state.
pub(crate) struct Ctx<'a> {
    pub w: i32,
    pub h: i32,
    pub o: Options,
    pub rng: Rng,
    pub grid: &'a mut Grid,
    pub cells: Cells,
    pub pts: Points,
    pub sprites: &'a Sprites,
    pub brk: &'a AtomicBool,
    pub divisor: &'a mut i32,
}

impl Ctx<'_> {
    /// The original's break check.
    pub fn check(&self) -> Step<()> {
        if self.brk.load(Ordering::Relaxed) {
            Err(Stop::Break)
        } else {
            Ok(())
        }
    }
}

impl Generator {
    /// A new dialog's state: a zeroed grid (the original's fresh allocation).
    pub fn new() -> Generator {
        Generator { grid: Grid::new(), divisor: 0 }
    }

    /// One "create" click (0x51f2ac). `brk` is the break button.
    pub fn run(&mut self, job: Job, brk: &AtomicBool) -> Outcome {
        let Job { options: o, mut seed, sprites, start, clock } = job;
        let size = if SIZES.contains(&o.size) { o.size } else { SIZES[0] };
        // §2: a typed seed only with "keep" on, and never 0.
        // The editor's one generator is seeded here either way.
        let mut header_seed = None;
        let rng = if seed == 0 || !o.keep {
            seed = clock as i32;
            header_seed = Some(clock);
            Rng::new(clock)
        } else {
            Rng::new(seed as u32)
        };
        let relief = o.rebuild || !o.keep;
        let cells = if relief { Cells::zero(size) } else { start };
        let mut c = Ctx {
            w: size as i32,
            h: size as i32,
            o: Options { size, ..o },
            rng,
            grid: &mut self.grid,
            cells,
            pts: Points::new(),
            sprites: &sprites,
            brk,
            divisor: &mut self.divisor,
        };
        let stop = generate(&mut c, relief, seed).err();
        Outcome { cells: c.cells, rng: c.rng, seed, header_seed, stop }
    }
}

fn generate(c: &mut Ctx, relief: bool, seed: i32) -> Step<()> {
    let bounds = relief::relief(c, relief)?;
    // §2: everything after the relief replays from the seed field.
    c.rng = Rng::new(seed as u32);
    land::terrain(c, &bounds)?;
    land::objects(c)
}

// ------------------------------------------------------------------------------------------
// The new document (§11)
// ------------------------------------------------------------------------------------------

/// The map the exit button leaves after a run (§11), from the open map `old`: the run's
/// cells; after a complete run a cleared header with the size and the seed (0x14), the
/// relation matrix of a new map, no records or texts and a fixed title. After a run that
/// stopped the original skips the header reset: `old`'s header stays, with the seed the run
/// wrote at its start, the new size (Razdor keeps the size and the cells in step) and the
/// relation matrix.
pub fn new_scenario(out: &Outcome, old: &Scenario, title: &str) -> Scenario {
    let c = &out.cells;
    let mut header = if out.complete() {
        Header { generator_seed: out.seed as u32, ..Header::default() }
    } else {
        let mut h = old.header.clone();
        if let Some(s) = out.header_seed {
            h.generator_seed = s;
        }
        h
    };
    header.width = c.w as u32;
    header.height = c.h as u32;
    header.relations = DEFAULT_RELATIONS;
    Scenario { header, terrain: c.terrain(), objects: c.objects(), title: title.to_string(), ..Scenario::default() }
}

/// The file name the original gives a generated map.
pub const FILE_NAME: &str = "New";
