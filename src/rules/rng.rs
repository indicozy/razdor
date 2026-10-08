//! The original's random numbers (engine.md §3).
//!
//! [`Rng`] is the game's one generator: the C runtime's `rand()` stream (`S × 214013 +
//! 2531011`, 15 bits out). It is not saved: a new map sets it to 1 (so the markets of a fresh
//! map are always the same), and a loaded game starts from what the load sequence leaves
//! ([`Rng::jitter_plants`], one draw per army, the world music's draw; `rules::save`).
//! [`EventRng`] is the Community event generator, seeded from the clock.

/// The game's generator: one 32-bit state, `0` when the program starts.
#[derive(Clone, Debug, Default)]
pub struct Rng(u32);

/// A record of the game generator's draws, for the diff test (`difftest`): off unless
/// [`trace::start`] was called on this thread.
pub mod trace {
    use std::cell::RefCell;
    use std::panic::Location;

    /// One `Random(n)`: the state before it and the source line that drew.
    #[derive(Clone, Debug)]
    pub struct Draw {
        pub before: u32,
        pub n: i32,
        pub site: &'static Location<'static>,
    }

    thread_local! {
        static DRAWS: RefCell<Option<Vec<Draw>>> = const { RefCell::new(None) };
    }

    /// Starts recording (and drops what was recorded).
    pub fn start() {
        DRAWS.with(|d| *d.borrow_mut() = Some(Vec::new()));
    }

    /// The draws since the last take; recording goes on.
    pub fn take() -> Vec<Draw> {
        DRAWS.with(|d| d.borrow_mut().as_mut().map(std::mem::take).unwrap_or_default())
    }

    /// Stops recording.
    pub fn stop() {
        DRAWS.with(|d| *d.borrow_mut() = None);
    }

    #[track_caller]
    pub(super) fn record(before: u32, n: i32) {
        let site = Location::caller();
        DRAWS.with(|d| {
            if let Some(v) = d.borrow_mut().as_mut() {
                v.push(Draw { before, n, site });
            }
        });
    }
}

/// The draw the world music makes when a map's world screen starts (engine.md §3.2, §9):
/// the time to the next track change, `Random(90000)`. The music itself is the interface's.
pub const WORLD_MUSIC_DRAW: i32 = 90_000;
/// The idle-animation offset each AI army draws at a map or save load (ms).
pub const ARMY_IDLE_DRAW: i32 = 3000;

impl Rng {
    /// A generator whose state is `state` (the original sets 1 at every map load).
    pub fn new(state: u32) -> Self {
        Rng(state)
    }

    /// The state as a map load leaves it, before the markets are stocked.
    pub fn map_load() -> Self {
        Rng(1)
    }

    pub fn state(&self) -> u32 {
        self.0
    }

    /// The original's `Random(n)`: the state always steps, even for `n = 0` (which gives 0);
    /// then the top 15 bits mod `n`. So `n` above 32768 never gives more than 32767, and a
    /// negative `n` acts as `|n|` (the original's behaviour, kept).
    #[track_caller]
    pub fn random(&mut self, n: i32) -> i32 {
        trace::record(self.0, n);
        self.untraced(n)
    }

    /// [`Rng::random`] left out of the draw trace, for drawing (which the original does not
    /// do with this generator).
    fn untraced(&mut self, n: i32) -> i32 {
        self.0 = self.0.wrapping_mul(214_013).wrapping_add(2_531_011);
        if n == 0 {
            return 0;
        }
        ((self.0 >> 16) & 0x7fff) as i32 % n
    }

    /// `lo..=hi` from one `Random(hi − lo + 1)`, for Razdor's own rolls (rules the original
    /// does not have); one draw even when the range is empty, which gives `lo`.
    #[track_caller]
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        let n = (hi as i64 - lo as i64 + 1).clamp(0, i32::MAX as i64) as i32;
        lo.saturating_add(self.random(n))
    }

    /// The plant jitter of a map or save load (engine.md §3.3): for every cell of the plant
    /// layer of a map `w` wide ([`plant_layer`]: `w + 8` cells per row, the original's cell
    /// array) in row-major order whose object word (class in the high byte) is a plant
    /// (classes 9–11), the state is set from the cell's hash, then the x offset, the y offset
    /// and the sway phase are drawn. The hash takes the column in that wider row, as the
    /// original does. Only the stream is kept here; the map draws the offsets by [`plant_offset`].
    /// Returns whether any plant re-seeded it.
    pub fn jitter_plants(&mut self, w: i32, cells: &[u16]) -> bool {
        let stride = (w + CELL_PAD_X).max(1);
        let mut any = false;
        for (l, &word) in cells.iter().enumerate() {
            if !(9..=11).contains(&(word >> 8)) {
                continue;
            }
            let (x, y) = (l as i32 % stride, l as i32 / stride);
            self.0 = plant_hash(x, y, word);
            // x offset: 4 + Random(16) on odd rows, 28 − Random(16) on even ones; then y
            // offset Random(11), sway phase Random(1000).
            self.random(16);
            self.random(11);
            self.random(1000);
            any = true;
        }
        any
    }

    /// The generator after a save load (engine.md §3.2, 0x4b771c): it is not saved, so the
    /// plant jitter re-seeds it from the map's last plant, every army of the map file draws
    /// its idle offset, and the world music draws. A map without plants keeps the state the
    /// program had *(guess: 0, the state at start; the original keeps the last session's)*.
    pub fn save_load(w: i32, plants: &[u16], armies: usize) -> Rng {
        Rng::save_load_with_music(w, plants, armies).0
    }

    /// [`Rng::save_load`], and the world music's draw: its first change comes that many ms
    /// after 90 s.
    pub fn save_load_with_music(w: i32, plants: &[u16], armies: usize) -> (Rng, i32) {
        let mut r = Rng::default();
        r.jitter_plants(w, plants);
        for _ in 0..armies {
            r.random(ARMY_IDLE_DRAW);
        }
        let music = r.random(WORLD_MUSIC_DRAW);
        (r, music)
    }
}

/// The seed of a plant cell: `Trunc(sin(800y + x) × 10⁶ + cos(600x + y) × 10⁴ + word)`, its
/// low 32 bits. The original evaluates it with the x87 `fsin`/`fcos`; this takes the f64
/// ones *(guess: the FPU precision the game runs with is unknown, engine.md §11)*.
pub fn plant_hash(x: i32, y: i32, word: u16) -> u32 {
    let (x, y) = (x as f64, y as f64);
    let v = (800.0 * y + x).sin() * 1e6 + (600.0 * x + y).cos() * 1e4 + word as f64;
    v.trunc() as i64 as u32
}

/// The offset in pixels, right and down, at which the original draws plant `word` (classes
/// 9–11) on cell `(x, y)` (0x4cfb24, 0x483344): from the cell's seed, `28 − Random(16)` on
/// even rows or `4 + Random(16)` on odd ones, then `Random(11)`. The same draws as
/// [`Rng::jitter_plants`], made again for the picture and kept out of the draw trace.
pub fn plant_offset(x: i32, y: i32, word: u16) -> (i32, i32) {
    let mut r = Rng::new(plant_hash(x, y, word));
    let dx = if y % 2 == 0 { 28 - r.untraced(16) } else { 4 + r.untraced(16) };
    (dx, r.untraced(11))
}

/// The original's cell array is 8 cells wider and 2 rows taller than the map (saves-data
/// notes, map load step 4); the plant jitter walks all of it.
pub const CELL_PAD_X: i32 = 8;
pub const CELL_PAD_Y: i32 = 2;

/// The cells' plant layer of a map `w × h`: the object word (`class << 8 | sprite`) of the
/// last object of class 0 or 9 and above on each cell, objects at `y × (w + 8) + x` in file
/// order, over the original's `(w + 8) × (h + 2)` cell array (so an object up to 7 columns
/// past the map's edge keeps its row, and only one further out lands on the next row).
pub fn plant_layer(w: i32, h: i32, objects: impl IntoIterator<Item = (i32, i32, u8, u8)>) -> Vec<u16> {
    let stride = (w + CELL_PAD_X).max(0);
    let mut cells = vec![0u16; (stride * (h + CELL_PAD_Y).max(0)) as usize];
    for (x, y, class, sprite) in objects {
        if (1..=8).contains(&class) || x < 0 || y < 0 {
            continue;
        }
        if let Some(c) = cells.get_mut((y * stride + x) as usize) {
            *c = (class as u16) << 8 | sprite as u16;
        }
    }
    cells
}

/// The Community event generator (engine.md §3.5): its own 32-bit state, seeded from the CPU
/// clock whenever a map or a save is loaded, never saved. Used by event opcode 18 only.
#[derive(Clone, Debug, Default)]
pub struct EventRng(u32);

impl EventRng {
    pub fn new(state: u32) -> Self {
        EventRng(state)
    }

    /// Seeded from the clock, as the original does from the time-stamp counter.
    pub fn from_clock() -> Self {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        EventRng(t as u32 ^ (t >> 32) as u32)
    }

    fn step(&mut self) -> u32 {
        // Numerical Recipes' constants, typed as hexadecimal in the original.
        self.0 = self.0.wrapping_mul(0x0166_4525).wrapping_add(0x1390_4223);
        self.0
    }

    /// `lo..=hi`: draws until the state falls below the largest multiple of `n = hi − lo + 1`
    /// (unsigned), then `lo + state mod n`. The original's retry loop jumps back one step too
    /// far, so after a rejection the limit becomes the rejected state × n (kept). A range of
    /// 2³² values (n = 0) divides by zero there; this returns `lo` *(guess)*.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        let n = (hi.wrapping_sub(lo) as u32).wrapping_add(1);
        if n == 0 {
            return lo;
        }
        let mut q = u32::MAX / n;
        loop {
            let limit = q.wrapping_mul(n);
            let s = self.step();
            if s < limit {
                return lo.wrapping_add((s % n) as i32);
            }
            q = s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(r: &mut Rng, k: usize) -> Vec<i32> {
        (0..k).map(|_| r.random(32768)).collect()
    }

    #[test]
    fn the_stream_is_the_c_runtimes_rand() {
        assert_eq!(raw(&mut Rng::new(1), 8), [41, 18467, 6334, 26500, 19169, 15724, 11478, 29358]);
        assert_eq!(raw(&mut Rng::default(), 5), [38, 7719, 21238, 2437, 8855], "0 at program start");
        assert_eq!(Rng::map_load().state(), 1);
    }

    #[test]
    fn random_n_is_the_15_bits_mod_n_and_always_draws() {
        let mut r = Rng::new(1);
        assert_eq!(r.random(10), 1, "41 mod 10");
        assert_eq!(r.random(0), 0, "Random(0) is 0 but steps the state");
        assert_eq!(r.random(1), 0);
        assert_eq!(r.random(100), 0, "26500 mod 100");
        // Ranges above 32768 never reach past 32767 (the music's 50 000 – 90 000).
        let mut r = Rng::new(1);
        assert_eq!(raw(&mut r.clone(), 8), (0..8).map(|_| r.random(90_000)).collect::<Vec<_>>());
        // A negative n acts as its absolute value.
        assert_eq!(Rng::new(1).random(-10), 1);
    }

    #[test]
    fn range_is_one_random_draw() {
        let mut r = Rng::new(1);
        assert_eq!(r.range(3, 7), 3 + 41 % 5);
        assert_eq!(r.range(5, 5), 5);
        assert_eq!(r.range(6, 2), 6, "an empty range gives lo");
        assert_eq!(r.state(), Rng::new(1).tap(3).state(), "three draws");
        for _ in 0..1000 {
            assert!((-4..=4).contains(&r.range(-4, 4)));
        }
    }

    impl Rng {
        fn tap(mut self, k: usize) -> Rng {
            (0..k).for_each(|_| {
                self.random(0);
            });
            self
        }
    }

    #[test]
    fn plants_reseed_the_stream_from_their_cell() {
        // Cell (0, 0): sin 0 = 0, cos 0 = 1, so the seed is 10 000 + the word.
        let tree = 9 << 8 | 3;
        assert_eq!(plant_hash(0, 0, tree), 10_000 + tree as u32);
        // Cell (0, 2): sin 1600 × 10⁶ + cos 2 × 10⁴ + 2 = −805 384.26; the cut is toward
        // zero, and S keeps the low 32 bits.
        assert_eq!(plant_hash(0, 2, 2), (-805_384i32) as u32);
        // Three draws after the last plant's seed; the cells before it do not matter. The
        // layer of a 3 × 2 map is 11 × 4 cells.
        let cells = plant_layer(3, 2, [(1, 0, 10, 5), (2, 1, 9, 7), (0, 1, 3, 1)]);
        assert_eq!(cells.len(), 11 * 4);
        assert_eq!((cells[1], cells[11 + 2]), (10 << 8 | 5, 9 << 8 | 7));
        assert_eq!(cells.iter().filter(|&&c| c != 0).count(), 2);
        let mut r = Rng::new(77);
        assert!(r.jitter_plants(3, &cells));
        assert_eq!(r.state(), Rng::new(plant_hash(2, 1, 9 << 8 | 7)).tap(3).state());
        // No plant (a rock of class 12 or a massif): the state is left alone.
        let mut r = Rng::new(77);
        assert!(!r.jitter_plants(3, &plant_layer(3, 2, [(0, 0, 12, 1), (1, 1, 5, 0)])));
        assert_eq!(r.state(), 77);
    }

    #[test]
    fn a_save_load_starts_the_stream_from_the_last_plant() {
        let cells = plant_layer(4, 3, [(1, 0, 9, 2), (3, 1, 11, 4), (0, 2, 5, 1)]);
        // The last plant's seed, its three jitter draws, one draw per army, the music's.
        let r = Rng::save_load(4, &cells, 2);
        assert_eq!(r.state(), Rng::new(plant_hash(3, 1, 11 << 8 | 4)).tap(3 + 2 + 1).state());
        assert_eq!(Rng::save_load(4, &cells, 2).state(), r.state(), "the same save replays the same stream");
        assert_eq!(Rng::save_load(4, &plant_layer(4, 3, []), 0).state(), Rng::default().tap(1).state(), "no plants, no armies");
    }

    #[test]
    fn the_plant_layer_is_the_originals_wider_cell_array() {
        // A 2 × 2 map has 10 × 4 cells: x = 2 (past the map's edge) keeps row 0, x = 10 is
        // row 1; a massif (class 4) keeps its own layer; the last object on a cell wins.
        let cells = plant_layer(2, 2, [(0, 0, 9, 1), (0, 0, 11, 2), (0, 0, 4, 9), (2, 0, 10, 3), (10, 0, 9, 4), (1, 3, 9, 5)]);
        assert_eq!(cells.len(), 40);
        assert_eq!([cells[0], cells[2], cells[10], cells[31]], [11 << 8 | 2, 10 << 8 | 3, 9 << 8 | 4, 9 << 8 | 5]);
        assert_eq!(cells.iter().filter(|&&c| c != 0).count(), 4);
        // The hash takes the column and row of the wider array: the last plant is (1, 3).
        let mut r = Rng::new(0);
        r.jitter_plants(2, &cells);
        assert_eq!(r.state(), Rng::new(plant_hash(1, 3, 9 << 8 | 5)).tap(3).state());
        let only = plant_layer(2, 2, [(10, 0, 9, 4)]);
        let mut r = Rng::new(0);
        r.jitter_plants(2, &only);
        assert_eq!(r.state(), Rng::new(plant_hash(0, 1, 9 << 8 | 4)).tap(3).state(), "x = 10 is (0, 1), not (2, 5)");
    }

    #[test]
    fn the_event_generator_and_its_retry_slip() {
        let step = |s: u32| s.wrapping_mul(0x0166_4525).wrapping_add(0x1390_4223);
        let mut e = EventRng::new(5);
        let s1 = step(5);
        assert_eq!(e.range(10, 19), 10 + (s1 % 10) as i32);
        // n = 2³¹ + 1: the limit is n itself, so a state at or above it is rejected and the
        // next limit is that state × n.
        // A state that the true limit would reject again but the slipped one accepts.
        let n = 0x8000_0001u32;
        let seed = (0u32..)
            .find(|&s| {
                let (r, t) = (step(s), step(step(s)));
                r >= n && t >= n && t < r.wrapping_mul(n)
            })
            .unwrap();
        let mut e = EventRng::new(seed);
        assert_eq!(e.range(0, i32::MIN), (step(step(seed)) - n) as i32);
        assert_eq!(e.0, step(step(seed)), "two draws, not three");
        assert_eq!(EventRng::new(9).range(4, 3), 4, "n = 0: lo");
    }
}
