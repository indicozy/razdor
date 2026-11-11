//! Memos for the 2.5D map's per-sprite work: a frame's yaw and pitch are the same for every
//! sprite, and a texture's size is the same every draw. (A plant's jitter is made once per object,
//! `Decoration::jitter`.)
//! Each memo returns exactly what the function it wraps returns: a hit is the value stored for
//! the same input, a miss computes it again. They are per-thread ([`razdor::local`]).

use std::cell::RefCell;

use razdor::local;

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
