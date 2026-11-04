//! Razdor Studio's map generators (phase 2 milestone 5), ported from DTMapEdit's new-map
//! generator and world generator (docs/reference/editor/newmap.md, worldgen.md). They are
//! pure (no macroquad, no egui): [`newmap`] makes the terrain and the plants of a new map,
//! [`worldgen`] its buildings, roads, economy and armies, and [`to_package`] writes the result
//! as a package map (`terrain.png` and `map.toml`).

pub mod apply;
pub mod model;
pub mod newmap;
#[cfg(test)]
mod tests;
pub mod to_package;
pub mod worldgen;

use std::sync::atomic::AtomicBool;

use crate::dt::dtm::Scenario;
use model::Layer;

/// One run of the generators: the new-map dialog's options, the seed, and the world
/// generator's steps when they are on.
#[derive(Clone, Debug)]
pub struct Request {
    /// The terrain's options (size, map type, orientation, the share bars, blur). The "keep"
    /// and "rebuild" boxes are always on: the typed seed is the one used.
    pub map: newmap::Options,
    /// The seed field; 0 is refused (the original never replays it, §2).
    pub seed: i32,
    /// The world's Randomize clock (the first world step's seed); [`clock_of`] gives the
    /// default, derived from the seed, so the same seed gives the same world.
    pub clock: u32,
    pub world: Option<World>,
}

/// The default Randomize clock of a seed: the original reads the processor's time-stamp
/// counter there, which Razdor cannot replay, so the seed stands in for it.
pub fn clock_of(seed: i32) -> u32 {
    (seed as u32).wrapping_mul(0x9e37_79b9) | 1
}

/// The world generator's three steps (each on or off) and its settings.
#[derive(Clone, Debug)]
pub struct World {
    pub buildings: bool,
    pub economy: bool,
    pub armies: bool,
    pub options: worldgen::Options,
}

/// The install's data the generators read: the object sprites, the building pictures, the
/// units and the spell prices.
pub struct Sources<'a> {
    pub sprites: &'a newmap::Sprites,
    pub pictures: &'a worldgen::Pictures,
    pub units: &'a worldgen::Units,
    pub spell_prices: &'a [i32],
}

/// The map a run made: the scenario (the terrain, the objects, the buildings and the armies)
/// and what each world step reported.
#[derive(Clone, Debug)]
pub struct Generated {
    pub scenario: Scenario,
    pub seed: u32,
    /// Each step that ran, with its counters (the world generator's window shows them).
    pub steps: Vec<(&'static str, worldgen::Report)>,
}

/// Why a run did not make a map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Seed 0: the original never replays it.
    ZeroSeed,
    /// A map size the generator does not make (one of 50, 100, 200, 400, 800).
    Size(u32),
    /// The terrain stopped (the break button, a flat field, a loop that cannot end, …).
    Terrain(newmap::Stop),
    /// A world step stopped; the name of the step is `buildings`, `economy` or `armies`.
    Step(&'static str, worldgen::Stop),
}

/// Makes a map for `req` from the install's data, and the world steps on it. `brk` is the
/// break button (checked by the terrain's long loops).
pub fn generate(req: &Request, src: &Sources, brk: &AtomicBool) -> Result<Generated, Failure> {
    if req.seed == 0 {
        return Err(Failure::ZeroSeed);
    }
    let size = req.map.size;
    if !newmap::SIZES.contains(&size) {
        return Err(Failure::Size(size));
    }
    let clock = req.clock;
    let options = newmap::Options { keep: true, rebuild: true, ..req.map };
    let job = newmap::Job {
        options,
        seed: req.seed,
        sprites: src.sprites.clone(),
        start: newmap::Cells::zero(size),
        clock,
    };
    let out = newmap::Generator::new().run(job, brk);
    if let Some(stop) = out.stop {
        return Err(Failure::Terrain(stop));
    }
    let mut s = model::new_scenario(size, size, 6);
    s.terrain = out.cells.terrain();
    s.objects = out.cells.objects();
    s.header.generator_seed = out.seed as u32;
    let mut rng = out.rng;
    let mut steps = Vec::new();
    if let Some(w) = &req.world {
        let inp = worldgen::Inputs { pictures: src.pictures, units: src.units, spell_prices: src.spell_prices, brush: 1, clock };
        if w.buildings {
            // The new-map generator leaves its flags in the cells' scratch bytes, which the
            // first step reads at its edges (worldgen.md §3.1).
            let mut cells = Layer::load(&s);
            cells.set_scratch(&out.cells.flag);
            let r = worldgen::buildings_and_roads(&mut s, &mut cells, &mut rng, &inp, &w.options);
            if let Some(stop) = r.stop {
                return Err(Failure::Step("buildings", stop));
            }
            steps.push(("buildings", r));
        }
        if w.economy {
            let r = worldgen::economy(&mut s, &mut rng, &inp, &w.options);
            if let Some(stop) = r.stop {
                return Err(Failure::Step("economy", stop));
            }
            steps.push(("economy", r));
        }
        if w.armies {
            let r = worldgen::armies(&mut s, &mut rng, &inp, &w.options);
            if let Some(stop) = r.stop {
                return Err(Failure::Step("armies", stop));
            }
            steps.push(("armies", r));
        }
    }
    Ok(Generated { scenario: s, seed: out.seed as u32, steps })
}

/// What the generators read from the game's install: the object sprites, the building
/// pictures, the units and the spell prices.
pub struct InstallData {
    pub sprites: newmap::Sprites,
    pub pictures: worldgen::Pictures,
    pub units: worldgen::Units,
    pub spell_prices: Vec<i32>,
}

impl InstallData {
    /// The data of `dt` (its `Objects.ugs`, its game data and its `_Global.ini` tables).
    pub fn from_install(dt: &crate::dt::install::DtInstall) -> Result<InstallData, String> {
        let objects = dt.map_objects().map_err(|e| e.to_string())?;
        let content = crate::rules::content::Content::from_dt(dt);
        let mut spell_prices: Vec<i32> = vec![0; content.spells.iter().map(|sp| sp.id).max().unwrap_or(0) as usize];
        for sp in &content.spells {
            if let Some(slot) = spell_prices.get_mut(sp.id as usize - 1) {
                *slot = sp.cost_gold;
            }
        }
        Ok(InstallData {
            sprites: newmap::Sprites::from_install(&objects),
            pictures: worldgen::Pictures::from_install(&objects),
            units: worldgen::Units::from_content(&content),
            spell_prices,
        })
    }

    pub fn sources(&self) -> Sources<'_> {
        Sources { sprites: &self.sprites, pictures: &self.pictures, units: &self.units, spell_prices: &self.spell_prices }
    }
}

/// The message the studio shows for a run that made no map.
pub fn describe(f: &Failure) -> String {
    use crate::i18n::tr;
    match f {
        Failure::ZeroSeed => tr("Seed 0 cannot be replayed: type another seed, or press Random.").to_string(),
        Failure::Size(_) => tr("The package holds maps of 50, 100 and 200 cells only.").to_string(),
        Failure::Terrain(_) => tr("The terrain generator stopped before the end. Try another seed.").to_string(),
        Failure::Step(step, _) => match *step {
            "buildings" => tr("The buildings and roads step stopped before the end. Try other chances.").to_string(),
            "economy" => tr("The economy step stopped before the end. Check the income and trade values.").to_string(),
            _ => tr("The armies step stopped before the end. Check the budgets.").to_string(),
        },
    }
}

/// The message the studio shows for a map the package cannot hold.
pub fn describe_refusal(r: &to_package::Refusal) -> String {
    use crate::i18n::tr;
    match r {
        to_package::Refusal::Size(_) => tr("The package holds maps of 50, 100 and 200 cells only.").to_string(),
        to_package::Refusal::Terrain { .. } => tr("The map has a terrain the package does not have.").to_string(),
        to_package::Refusal::Unit { .. } => tr("A unit of the map has no name in the package (no dt: alias).").to_string(),
        to_package::Refusal::NoReachCell => tr("No cell is reachable from the hero's start, so the map has no place for its victory.").to_string(),
        to_package::Refusal::Troops { .. } => tr("An army has more troops than the package holds (six).").to_string(),
    }
}
