//! «Максимум»'s memory (Razdor's, the owner's request): a key of everything in a battle that
//! decides how it goes on, the transposition table of searched positions and the cache of
//! rollouts, kept for the whole battle and shared by the threads that search it.

use crate::local;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::sync::Mutex;

use super::*;

/// A position's key: a 128-bit hash (two SipHash-1-3 digests) of every field of the battle
/// that its future depends on, the Community patch's globals and the searching side. Two
/// different positions share a key with a chance of about 2⁻¹²⁸ per pair: never in practice.
pub type StateKey = u128;

/// Collects the bytes `Hash` writes.
struct Sink<'a>(&'a mut Vec<u8>);

impl Hasher for Sink<'_> {
    fn write(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }
    fn finish(&self) -> u64 {
        0
    }
}

/// MurmurHash3 x64 128 (Austin Appleby's, public domain), seed 0: a well-mixed 128-bit hash
/// several times faster than SipHash; the keys are no secret, only collisions matter.
pub(super) fn murmur3_128(data: &[u8]) -> u128 {
    const C1: u64 = 0x87c3_7b91_1142_53d5;
    const C2: u64 = 0x4cf5_ad43_2745_937f;
    fn fmix(mut k: u64) -> u64 {
        k ^= k >> 33;
        k = k.wrapping_mul(0xff51_afd7_ed55_8ccd);
        k ^= k >> 33;
        k = k.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        k ^ (k >> 33)
    }
    let (mut h1, mut h2) = (0u64, 0u64);
    let mut blocks = data.chunks_exact(16);
    for b in &mut blocks {
        let k1 = u64::from_le_bytes(b[..8].try_into().unwrap_or_default());
        let k2 = u64::from_le_bytes(b[8..].try_into().unwrap_or_default());
        h1 ^= k1.wrapping_mul(C1).rotate_left(31).wrapping_mul(C2);
        h1 = h1.rotate_left(27).wrapping_add(h2).wrapping_mul(5).wrapping_add(0x52dc_e729);
        h2 ^= k2.wrapping_mul(C2).rotate_left(33).wrapping_mul(C1);
        h2 = h2.rotate_left(31).wrapping_add(h1).wrapping_mul(5).wrapping_add(0x3849_5ab5);
    }
    let tail = blocks.remainder();
    let mut t = [0u8; 16];
    t[..tail.len()].copy_from_slice(tail);
    let (k1, k2) = (u64::from_le_bytes(t[..8].try_into().unwrap_or_default()), u64::from_le_bytes(t[8..].try_into().unwrap_or_default()));
    if tail.len() > 8 {
        h2 ^= k2.wrapping_mul(C2).rotate_left(33).wrapping_mul(C1);
    }
    if !tail.is_empty() {
        h1 ^= k1.wrapping_mul(C1).rotate_left(31).wrapping_mul(C2);
    }
    let len = data.len() as u64;
    h1 ^= len;
    h2 ^= len;
    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);
    h1 = fmix(h1);
    h2 = fmix(h2);
    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);
    ((h2 as u128) << 64) | h1 as u128
}

local! {
    static KEY_BYTES: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl Battle {
    /// The key of the battle as it stands for `side`'s search (with this thread's patch
    /// globals). Every field is named (no `..`), so a field added to `Battle` or `Fighter`
    /// has to be placed here before the code builds; only what never changes how the battle
    /// goes on is left out: the content (one per battle), the log, the AI's record of old
    /// reads (`stale_reads`), the «Максимум» switch, and the fighters' names and what the
    /// cards alone show (the stats at the start, the items, the world spells, the potion
    /// sign), which the rules never read.
    pub(crate) fn max_key(&self, side: Team) -> StateKey {
        KEY_BYTES.with(|buf| {
            let mut buf = buf.borrow_mut();
            buf.clear();
            self.write_key(&mut Sink(&mut buf), side);
            murmur3_128(&buf)
        })
    }

    fn write_key(&self, h: &mut impl Hasher, side: Team) {
        let Battle {
            content: _,
            formation,
            fighters,
            round,
            log: _,
            attacker,
            building_defence,
            strength_defence,
            bare_strengths,
            knight,
            cells,
            mean_initiative,
            deploying,
            ended,
            threshold,
            first_threshold,
            cursor,
            current,
            start,
            xp_correction,
            interactive,
            screen_object,
            splash,
            crippled,
            ai_level,
            surrender_mana,
            stale,
            start_count,
            stale_reads: _,
            predict,
            predicted,
            turn_lost,
            max_turn_lost,
            bench,
            god,
            max_ai: _,
            // One value for the whole battle, as the battle's tables are; the evaluations
            // that read it (the forced-win search) keep no table entries.
            max_stakes: _,
        } = self;
        side.hash(h);
        patch_globals().hash(h);
        formation.hash(h);
        round.hash(h);
        attacker.hash(h);
        building_defence.hash(h);
        strength_defence.hash(h);
        bare_strengths.hash(h);
        knight.hash(h);
        cells.hash(h);
        for m in mean_initiative {
            m.to_bits().hash(h);
        }
        deploying.hash(h);
        ended.hash(h);
        threshold.hash(h);
        first_threshold.hash(h);
        cursor.hash(h);
        current.hash(h);
        start.hash(h);
        xp_correction.hash(h);
        interactive.hash(h);
        screen_object.hash(h);
        splash.hash(h);
        crippled.hash(h);
        ai_level.hash(h);
        surrender_mana.hash(h);
        stale.hash(h);
        start_count.hash(h);
        predict.hash(h);
        predicted.hash(h);
        turn_lost.hash(h);
        max_turn_lost.hash(h);
        bench.hash(h);
        god.hash(h);
        fighters.len().hash(h);
        for f in fighters {
            let Fighter {
                unit,
                name: _,
                team,
                hp,
                slot,
                is_hero,
                squad_index,
                named,
                level,
                xp,
                base,
                stats,
                at_start: _,
                power,
                mods,
                items: _,
                spells: _,
                drain,
                potion: _,
                blessed,
                cursed,
                actions,
                cur_initiative,
                regen,
                bleed,
                reserve_move,
                suicided,
                surrender,
                surrendered,
                tactical,
                role,
                useful,
                taken,
                lost,
                ai_power,
                ai_role,
            } = f;
            (unit, team, hp, slot, is_hero, squad_index, named, level, xp).hash(h);
            (base, stats, power, mods, drain, blessed, cursed).hash(h);
            (actions, cur_initiative, regen, bleed, reserve_move, suicided, surrender, surrendered).hash(h);
            (tactical, role, useful, taken, lost, ai_power, ai_role).hash(h);
        }
    }
}

/// A `HashMap` hasher for keys that are hashes already: their low 64 bits.
#[derive(Default)]
struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = self.0.rotate_left(8) ^ b as u64;
        }
    }
    fn write_u128(&mut self, k: u128) {
        self.0 = k as u64;
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

type KeyMap<V> = HashMap<StateKey, V, BuildHasherDefault<KeyHasher>>;

/// Two generations: a lookup that finds a key in the old one brings it to the new one; when
/// the new one is full it becomes the old one (the previous old one is dropped). At most
/// twice `cap` entries, the ones used lately kept.
struct Gen<V> {
    new: KeyMap<V>,
    old: KeyMap<V>,
}

impl<V> Default for Gen<V> {
    fn default() -> Self {
        Gen { new: KeyMap::default(), old: KeyMap::default() }
    }
}

impl<V: Copy> Gen<V> {
    fn get(&mut self, k: StateKey, cap: usize) -> Option<V> {
        if let Some(v) = self.new.get(&k) {
            return Some(*v);
        }
        let v = self.old.remove(&k)?;
        self.insert(k, v, cap);
        Some(v)
    }

    fn insert(&mut self, k: StateKey, v: V, cap: usize) {
        if self.new.len() >= cap && !self.new.contains_key(&k) {
            self.old = std::mem::take(&mut self.new);
        }
        self.new.insert(k, v);
    }

    fn len(&self) -> usize {
        self.new.len() + self.old.len()
    }
}

/// What a search proved of a position's score at one depth: `lower ≤ score ≤ upper`
/// (equal when exact), and whether a line was cut by the depth (`cut` false: every line
/// ended in the battle's end, so the bounds hold at any greater depth too).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Bound {
    pub depth: u8,
    /// The leaves' rollout class (`Search::class`).
    pub class: u8,
    pub cut: bool,
    pub lower: i64,
    pub upper: i64,
}

/// A position in the transposition table: the best move its deepest search found (for the
/// move ordering only) and the bounds of its score at a few depths.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct TtEntry {
    pub best: Option<MaxMove>,
    pub best_depth: u8,
    pub bounds: [Option<Bound>; 3],
}

impl TtEntry {
    /// The bounds that hold at `depth` with leaves of `class`: those searched at that
    /// depth, or an uncut search at a smaller one.
    pub fn bound(&self, depth: u8, class: u8) -> Option<Bound> {
        self.bounds.iter().flatten().copied().find(|b| b.class == class && (b.depth == depth || (!b.cut && b.depth <= depth)))
    }

    /// Adds what a search at `b.depth` found (bounds of the same search depth and class
    /// meet); the shallowest record gives way when all three are taken.
    pub fn add(&mut self, b: Bound, best: Option<MaxMove>) {
        if best.is_some() && b.depth >= self.best_depth {
            self.best = best;
            self.best_depth = b.depth;
        }
        if let Some(old) = self.bounds.iter_mut().flatten().find(|o| o.depth == b.depth && o.class == b.class) {
            old.lower = old.lower.max(b.lower);
            old.upper = old.upper.min(b.upper);
            old.cut |= b.cut;
            return;
        }
        let slot = match self.bounds.iter().position(Option::is_none) {
            Some(i) => i,
            None => (0..self.bounds.len()).min_by_key(|&i| self.bounds[i].map_or(0, |o| o.depth)).unwrap_or(0),
        };
        self.bounds[slot] = Some(b);
    }
}

const SHARDS: usize = 16;
/// Entries per generation, all shards together: about 30 MB of positions and 8 MB of
/// rollouts at most.
const TT_CAP: usize = 1 << 17;
const ROLLOUT_CAP: usize = 1 << 17;

#[derive(Default)]
struct Shard {
    tt: Gen<TtEntry>,
    rollouts: Gen<i64>,
}

/// The transposition table and the rollout cache of one battle, safe to share between
/// threads (sharded locks). Whatever is in them only saves work: a search finds the same
/// scores with them or without.
pub struct MaxTables {
    shards: Box<[Mutex<Shard>]>,
}

impl Default for MaxTables {
    fn default() -> Self {
        MaxTables { shards: (0..SHARDS).map(|_| Mutex::new(Shard::default())).collect() }
    }
}

impl MaxTables {
    fn shard(&self, k: StateKey) -> std::sync::MutexGuard<'_, Shard> {
        let i = (k >> 120) as usize % SHARDS;
        self.shards[i].lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(super) fn tt_get(&self, k: StateKey) -> Option<TtEntry> {
        self.shard(k).tt.get(k, TT_CAP / SHARDS)
    }

    pub(super) fn tt_add(&self, k: StateKey, b: Bound, best: Option<MaxMove>) {
        let mut s = self.shard(k);
        let mut e = s.tt.get(k, TT_CAP / SHARDS).unwrap_or_default();
        e.add(b, best);
        s.tt.insert(k, e, TT_CAP / SHARDS);
    }

    pub(super) fn rollout_get(&self, k: StateKey) -> Option<i64> {
        self.shard(k).rollouts.get(k, ROLLOUT_CAP / SHARDS)
    }

    pub(super) fn rollout_add(&self, k: StateKey, v: i64) {
        self.shard(k).rollouts.insert(k, v, ROLLOUT_CAP / SHARDS);
    }

    /// Positions and rollouts held.
    pub fn len(&self) -> (usize, usize) {
        self.shards.iter().fold((0, 0), |(a, b), s| {
            let s = s.lock().unwrap_or_else(|e| e.into_inner());
            (a + s.tt.len(), b + s.rollouts.len())
        })
    }

    pub fn is_empty(&self) -> bool {
        self.len() == (0, 0)
    }
}
