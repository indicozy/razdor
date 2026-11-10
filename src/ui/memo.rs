//! Memos for the 2.5D map's per-sprite work: a frame's yaw and pitch are the same for every
//! sprite, a plant's jitter is the same every frame, and a texture's size is the same every draw.
//! Each memo returns exactly what the function it wraps returns: a hit is the value stored for
//! the same input, a miss computes it again. They are per-thread ([`razdor::local`]).

use std::cell::RefCell;

use razdor::local;
use razdor::rules::rng::plant_jitter;

/// Folds a key into a slot of a table of `n` (> 0) slots.
fn slot(key: u64, n: usize) -> usize {
    let h = (key ^ (key >> 29)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    ((h >> 32) as usize) % n
}

#[derive(Clone, Copy)]
struct Trig {
    bits: u32,
    sin: f32,
    cos: f32,
}

/// A direct-mapped memo of `to_radians().sin_cos()`, keyed by the angle's bits.
pub struct TrigMemo<const N: usize> {
    slots: [Option<Trig>; N],
}

impl<const N: usize> TrigMemo<N> {
    pub const fn new() -> Self {
        TrigMemo { slots: [None; N] }
    }

    /// `deg.to_radians().sin_cos()`, from the memo when the same angle was asked for last.
    pub fn sin_cos_deg(&mut self, deg: f32) -> (f32, f32) {
        let bits = deg.to_bits();
        let i = slot(u64::from(bits), N);
        if let Some(t) = self.slots[i] {
            if t.bits == bits {
                return (t.sin, t.cos);
            }
        }
        let (sin, cos) = deg.to_radians().sin_cos();
        self.slots[i] = Some(Trig { bits, sin, cos });
        (sin, cos)
    }
}

local! {
    static TRIG: RefCell<TrigMemo<16>> = RefCell::new(TrigMemo::new());
}

/// `deg.to_radians().sin_cos()` (the yaw and pitch of the 2.5D camera, asked for by every sprite).
pub fn sin_cos_deg(deg: f32) -> (f32, f32) {
    TRIG.with(|m| m.borrow_mut().sin_cos_deg(deg))
}

#[derive(Clone, Copy)]
struct Jitter {
    x: i32,
    y: i32,
    word: u16,
    out: (i32, i32, i32),
}

/// A direct-mapped memo of [`plant_jitter`], keyed by (x, y, word) with the full key checked.
pub struct JitterMemo {
    slots: Box<[Option<Jitter>]>,
}

impl JitterMemo {
    /// A memo of `n` (> 0) slots.
    pub fn with_slots(n: usize) -> JitterMemo {
        JitterMemo { slots: vec![None; n.max(1)].into_boxed_slice() }
    }

    /// [`plant_jitter`] of the same key, from the memo when it is there.
    pub fn plant_jitter(&mut self, x: i32, y: i32, word: u16) -> (i32, i32, i32) {
        let h = (x as u32 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u32 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ u64::from(word).wrapping_mul(0x1656_67B1_9E37_79F9);
        let i = slot(h, self.slots.len());
        if let Some(j) = self.slots[i] {
            if j.x == x && j.y == y && j.word == word {
                return j.out;
            }
        }
        let out = plant_jitter(x, y, word);
        self.slots[i] = Some(Jitter { x, y, word, out });
        out
    }
}

local! {
    static JITTER: RefCell<JitterMemo> = RefCell::new(JitterMemo::with_slots(4096));
}

/// [`plant_jitter`] (the plant's x and y offsets and sway phase), from the frame's memo.
pub fn plant_jitter_memo(x: i32, y: i32, word: u16) -> (i32, i32, i32) {
    JITTER.with(|m| m.borrow_mut().plant_jitter(x, y, word))
}

/// The last `N` values by key, replaced round robin: a hit returns the stored value without calling
/// `make`.
pub struct Recent<K, V, const N: usize> {
    slots: [Option<(K, V)>; N],
    next: usize,
}

impl<K: Copy + PartialEq, V: Copy, const N: usize> Recent<K, V, N> {
    pub const fn new() -> Self {
        Recent { slots: [None; N], next: 0 }
    }

    pub fn get_or_insert_with(&mut self, key: K, make: impl FnOnce() -> V) -> V {
        for &(k, v) in self.slots.iter().flatten() {
            if k == key {
                return v;
            }
        }
        let v = make();
        self.slots[self.next] = Some((key, v));
        self.next = (self.next + 1) % N;
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trig_memo_gives_the_uncached_sin_cos_bit_for_bit() {
        // Four slots for many angles: most lookups collide and must recompute.
        let mut m = TrigMemo::<4>::new();
        let angles: Vec<f32> = (-400..400).map(|i| i as f32 * 0.7 - 0.0).chain([0.0, -0.0, 45.0, f32::NAN]).collect();
        for _ in 0..2 {
            for &a in &angles {
                let (s, c) = m.sin_cos_deg(a);
                let (ws, wc) = a.to_radians().sin_cos();
                assert_eq!((s.to_bits(), c.to_bits()), (ws.to_bits(), wc.to_bits()), "angle {a}");
            }
        }
        // The shared memo agrees too.
        for a in [0.0f32, 30.0, -135.5, 359.9] {
            let (s, c) = sin_cos_deg(a);
            assert_eq!((s.to_bits(), c.to_bits()), (a.to_radians().sin().to_bits(), a.to_radians().cos().to_bits()));
        }
    }

    #[test]
    fn the_jitter_memo_gives_the_rules_jitter_including_collisions() {
        // One slot: every key but the last one asked for collides and is recomputed.
        let mut one = JitterMemo::with_slots(1);
        // 4096 slots: the same keys mostly hit.
        let mut many = JitterMemo::with_slots(4096);
        let words = [0u16, 9 << 8 | 2, 10 << 8 | 7, 11 << 8 | 255];
        for _ in 0..2 {
            for x in -3..40 {
                for y in -2..30 {
                    for &w in &words {
                        let want = plant_jitter(x, y, w);
                        assert_eq!(one.plant_jitter(x, y, w), want, "({x}, {y}, {w})");
                        assert_eq!(many.plant_jitter(x, y, w), want, "({x}, {y}, {w})");
                    }
                }
            }
        }
        assert_eq!(plant_jitter_memo(5, 4, 10 << 8 | 1), plant_jitter(5, 4, 10 << 8 | 1));
    }

    #[test]
    fn the_recent_memo_keeps_the_last_keys_and_replaces_round_robin() {
        let mut calls = 0;
        let mut m = Recent::<u32, u32, 2>::new();
        let get = |m: &mut Recent<u32, u32, 2>, k: u32, calls: &mut u32| {
            m.get_or_insert_with(k, || {
                *calls += 1;
                k * 10
            })
        };
        assert_eq!(get(&mut m, 1, &mut calls), 10);
        assert_eq!(get(&mut m, 2, &mut calls), 20);
        assert_eq!(get(&mut m, 1, &mut calls), 10);
        assert_eq!(calls, 2, "a hit does not make the value again");
        // 3 pushes 1 out (round robin), so 1 is made again.
        assert_eq!(get(&mut m, 3, &mut calls), 30);
        assert_eq!(get(&mut m, 2, &mut calls), 20);
        assert_eq!(calls, 3);
        assert_eq!(get(&mut m, 1, &mut calls), 10);
        assert_eq!(calls, 4);
    }
}
