//! The x87's extended precision, as the original's generator computes (newmap.md
//! conventions): a 64-bit mantissa, every operation rounded to nearest with ties to even,
//! and `Round` to an integer with ties to even. Only what the generator needs: the four
//! operations, the integer rounding, and the cosine and sine of a whole number of degrees
//! taken through `a·π ÷ 180` as the original does.
//!
//! Where a formula is a single quotient of integers the generator rounds it exactly with
//! [`round_div`] instead (the extended result cannot land on a tie the exact one misses).

use std::cmp::Ordering;

/// A finite extended value: `mant × 2^(exp − 63)` with the top bit of `mant` set, or zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ext {
    neg: bool,
    exp: i32,
    mant: u64,
}

/// π as the original's constant holds it (0x5283cc): π rounded to 64 bits.
pub const PI: Ext = Ext { neg: false, exp: 1, mant: 0xC90F_DAA2_2168_C235 };

impl Ext {
    pub const ZERO: Ext = Ext { neg: false, exp: 0, mant: 0 };

    /// An integer, exactly (`fild`).
    pub fn int(v: i64) -> Ext {
        Ext::round(v < 0, v.unsigned_abs() as u128, 0, false)
    }

    pub fn is_zero(self) -> bool {
        self.mant == 0
    }

    /// `v × 2^e0` (plus a little more when `sticky`), rounded to 64 bits, nearest even.
    fn round(neg: bool, v: u128, e0: i32, sticky: bool) -> Ext {
        if v == 0 {
            return Ext::ZERO;
        }
        let top = 127 - v.leading_zeros() as i32;
        if top <= 63 {
            debug_assert!(!sticky, "an inexact value needs guard bits");
            return Ext { neg, exp: e0 + top, mant: (v << (63 - top)) as u64 };
        }
        let s = (top - 63) as u32;
        let mut keep = v >> s;
        let rem = v & ((1u128 << s) - 1);
        let half = 1u128 << (s - 1);
        let up = rem > half || (rem == half && (sticky || keep & 1 == 1));
        let mut exp = e0 + s as i32 + 63;
        if up {
            keep += 1;
            if keep >> 64 != 0 {
                keep >>= 1;
                exp += 1;
            }
        }
        Ext { neg, exp, mant: keep as u64 }
    }

    pub fn neg(self) -> Ext {
        Ext { neg: !self.neg && !self.is_zero(), ..self }
    }

    pub fn mul(self, o: Ext) -> Ext {
        if self.is_zero() || o.is_zero() {
            return Ext::ZERO;
        }
        Ext::round(self.neg != o.neg, self.mant as u128 * o.mant as u128, self.exp + o.exp - 126, false)
    }

    /// `self ÷ o`; `None` for a division by zero (the original's control word does not mask
    /// it: the run stops).
    pub fn div(self, o: Ext) -> Option<Ext> {
        if o.is_zero() {
            return None;
        }
        if self.is_zero() {
            return Some(Ext::ZERO);
        }
        let (a, b) = (self.mant as u128, o.mant as u128);
        let (q1, r1) = ((a << 64) / b, (a << 64) % b);
        let (q, r) = ((q1 << 2) | ((r1 << 2) / b), (r1 << 2) % b);
        Some(Ext::round(self.neg != o.neg, q, self.exp - o.exp - 66, r != 0))
    }

    pub fn add(self, o: Ext) -> Ext {
        if self.is_zero() {
            return o;
        }
        if o.is_zero() {
            return self;
        }
        let (big, small) = if (self.exp, self.mant) >= (o.exp, o.mant) { (self, o) } else { (o, self) };
        let b = (big.mant as u128) << 62;
        let d = (big.exp - small.exp) as u32;
        let wide = (small.mant as u128) << 62;
        let (s, sticky) = if d >= 127 { (0, true) } else { (wide >> d, wide & ((1u128 << d) - 1) != 0) };
        let e0 = big.exp - 125;
        if big.neg == small.neg {
            Ext::round(big.neg, b + s, e0, sticky)
        } else if sticky {
            // The true difference lies just above `b − s − 1`.
            Ext::round(big.neg, b - s - 1, e0, true)
        } else {
            Ext::round(big.neg, b - s, e0, false)
        }
    }

    pub fn sub(self, o: Ext) -> Ext {
        self.add(o.neg())
    }

    /// `Round`: to the nearest integer, ties to even.
    pub fn round_int(self) -> i64 {
        if self.is_zero() || self.exp < -1 {
            return 0;
        }
        let m = self.mant as u128;
        let v = if self.exp >= 63 {
            (m << (self.exp - 63).min(64)) as i64
        } else {
            let s = (63 - self.exp) as u32;
            let (int, frac, half) = (m >> s, m & ((1u128 << s) - 1), 1u128 << (s - 1));
            (int + (frac > half || (frac == half && int & 1 == 1)) as u128) as i64
        };
        if self.neg {
            -v
        } else {
            v
        }
    }

    #[cfg(test)]
    pub fn to_f64(self) -> f64 {
        let v = self.mant as f64 * 2f64.powi(self.exp - 63);
        if self.neg {
            -v
        } else {
            v
        }
    }
}

impl PartialOrd for Ext {
    fn partial_cmp(&self, o: &Ext) -> Option<Ordering> {
        let key = |e: &Ext| if e.is_zero() { (0, 0, 0) } else { (if e.neg { -1 } else { 1 }, e.exp as i64, e.mant as i128) };
        let (a, b) = (key(self), key(o));
        Some(match (a.0, b.0) {
            (x, y) if x != y => x.cmp(&y),
            (0, _) => Ordering::Equal,
            (1, _) => (a.1, a.2).cmp(&(b.1, b.2)),
            _ => (b.1, b.2).cmp(&(a.1, a.2)),
        })
    }
}

/// `Round(num ÷ den)` of an exact quotient, ties to even.
pub fn round_div(num: i64, den: i64) -> i64 {
    let (n, d) = ((num as i128) * den.signum() as i128, (den as i128).abs());
    let (q, r) = (n.div_euclid(d), n.rem_euclid(d));
    let up = 2 * r > d || (2 * r == d && q & 1 == 1);
    (q + up as i128) as i64
}

// --- The cosine and sine of the generator's headings -------------------------------------
//
// The original takes `fcos`/`fsin` of x = (a·π) ÷ 180 in extended precision. They are
// modelled as correctly rounded: x differs from the exact a° by a tiny ε, so
// cos x = cos a° · cos ε − sin a° · sin ε, with cos a° and sin a° in 116-bit fixed point.
// A processor's fcos may differ from this in the last bit; that only shows where cos·len is
// within a bit of a half (a heading at a multiple of 30° with an odd step).

const FX: i32 = 116;
/// π · 2^116 and π/180 · 2^116.
const PI_FX: u128 = 0x3243f6a8885a308d313198a2e03707;
const PI180_FX: u128 = 0x477d1a894a74e4570762fb374a42;

/// `(a × b) >> s` for 128-bit `a`, `b` whose shifted product fits 128 bits.
fn mul_shr(a: u128, b: u128, s: u32) -> u128 {
    let (a1, a0, b1, b0) = (a >> 64, a & u64::MAX as u128, b >> 64, b & u64::MAX as u128);
    let (hh, hl, lh, ll) = (a1 * b1, a1 * b0, a0 * b1, a0 * b0);
    let mid = (ll >> 64) + (hl & u64::MAX as u128) + (lh & u64::MAX as u128);
    let lo = (ll & u64::MAX as u128) | (mid << 64);
    let hi = hh + (hl >> 64) + (lh >> 64) + (mid >> 64);
    if s >= 128 {
        hi >> (s - 128)
    } else if s == 0 {
        lo
    } else {
        (hi << (128 - s)) | (lo >> s)
    }
}

fn fx_mul(a: i128, b: i128) -> i128 {
    let m = mul_shr(a.unsigned_abs(), b.unsigned_abs(), FX as u32) as i128;
    if (a < 0) != (b < 0) {
        -m
    } else {
        m
    }
}

/// cos and sin of `b`° for 0 ≤ b ≤ 45, by their series.
fn cos_sin_small(b: i32) -> (i128, i128) {
    let t = PI180_FX as i128 * b as i128;
    let t2 = fx_mul(t, t);
    let one = 1i128 << FX;
    let (mut c, mut s) = (one, t);
    let (mut tc, mut ts) = (one, t);
    for n in 1..40i128 {
        tc = -fx_mul(tc, t2) / ((2 * n - 1) * (2 * n));
        ts = -fx_mul(ts, t2) / ((2 * n) * (2 * n + 1));
        if tc == 0 && ts == 0 {
            break;
        }
        c += tc;
        s += ts;
    }
    (c, s)
}

/// cos and sin of `a`° in fixed point.
fn cos_sin_deg(a: i32) -> (i128, i128) {
    let r = a.rem_euclid(360);
    let (half, r) = if r >= 180 { (true, r - 180) } else { (false, r) };
    let (c, s) = if r >= 90 {
        let (c, s) = quarter(r - 90);
        (-s, c)
    } else {
        quarter(r)
    };
    if half {
        (-c, -s)
    } else {
        (c, s)
    }
}

fn quarter(r: i32) -> (i128, i128) {
    if r <= 45 {
        cos_sin_small(r)
    } else {
        let (c, s) = cos_sin_small(90 - r);
        (s, c)
    }
}

/// An extended value in fixed point (truncated below 2^-116).
fn to_fx(x: Ext) -> i128 {
    let sh = x.exp - 63 + FX;
    let m = x.mant as i128;
    let v = if sh >= 0 { m << sh } else if sh > -128 { m >> -sh } else { 0 };
    if x.neg {
        -v
    } else {
        v
    }
}

fn from_fx(v: i128) -> Ext {
    Ext::round(v < 0, v.unsigned_abs(), -FX, false)
}

/// `(a·π) ÷ 180` as the original computes it.
pub fn radians(a: i32) -> Ext {
    Ext::int(a as i64).mul(PI).div(Ext::int(180)).expect("180 is not zero")
}

/// The original's `Cos` and `Sin` of `(a·π) ÷ 180` (headings of a few thousand degrees at
/// most).
pub fn cos_sin(a: i32) -> (Ext, Ext) {
    let x = radians(a);
    let (c, s) = cos_sin_deg(a);
    // ε = x − a·π/180, from π to 116 bits; full turns taken off first so it fits.
    let turns = a.div_euclid(360) as i128;
    let exact = 2 * PI_FX as i128 * turns + PI180_FX as i128 * a.rem_euclid(360) as i128;
    let eps = to_fx(x) - exact;
    let eps2 = fx_mul(eps, eps) / 2;
    (from_fx(c - fx_mul(s, eps) - fx_mul(c, eps2)), from_fx(s + fx_mul(c, eps) - fx_mul(s, eps2)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(v: i64) -> Ext {
        Ext::int(v)
    }

    #[test]
    fn the_operations_round_to_64_bits_nearest_even() {
        let third = e(1).div(e(3)).unwrap();
        assert_eq!((third.mant, third.exp), (0xAAAA_AAAA_AAAA_AAAB, -2));
        assert_eq!(e(2).div(e(3)).unwrap().mant, 0xAAAA_AAAA_AAAA_AAAB);
        assert_eq!(e(10).div(e(4)).unwrap().to_f64(), 2.5);
        // 2^64 + 1 needs 65 bits: the tie goes to the even mantissa.
        let big = e(1 << 62).mul(e(4)).add(e(1));
        assert_eq!((big.mant, big.exp), (1 << 63, 64));
        let big3 = e(1 << 62).mul(e(4)).add(e(3));
        assert_eq!(big3.mant, (1 << 63) + 2);
        assert_eq!(e(7).sub(e(7)), Ext::ZERO);
        assert_eq!(e(5).sub(e(9)).to_f64(), -4.0);
        assert_eq!(e(3).div(e(0)), None);
        for (a, b) in [(10_000i64, 7i64), (123_457, 1000), (-99, 13), (1, 1_000_003)] {
            let q = e(a).div(e(b)).unwrap();
            assert!((q.to_f64() - a as f64 / b as f64).abs() <= (a as f64 / b as f64).abs() * 1e-15);
            let back = q.mul(e(b));
            assert!((back.to_f64() - a as f64).abs() <= a.abs() as f64 * 1e-15);
        }
        assert!(e(3) > e(2) && e(-3) < e(-2) && e(-1) < Ext::ZERO && Ext::ZERO < e(1));
    }

    #[test]
    fn round_is_half_to_even() {
        let half = |n: i64| e(n).div(e(2)).unwrap().round_int();
        assert_eq!((half(1), half(3), half(5), half(-1), half(-3), half(7)), (0, 2, 2, 0, -2, 4));
        assert_eq!(e(7).div(e(3)).unwrap().round_int(), 2);
        assert_eq!(e(8).div(e(3)).unwrap().round_int(), 3);
        assert_eq!((round_div(5, 2), round_div(7, 2), round_div(-5, 2), round_div(999, 1000), round_div(1, 3)), (2, 4, -2, 1, 0));
        assert_eq!(round_div(54 * 37, 100), 20);
    }

    #[test]
    fn headings_follow_the_degrees() {
        for a in [-90, 0, 17, 30, 45, 60, 90, 135, 200, 300, 359, 405, 777] {
            let (c, s) = cos_sin(a);
            let r = (a as f64).to_radians();
            assert!((c.to_f64() - r.cos()).abs() < 1e-15, "cos {a}");
            assert!((s.to_f64() - r.sin()).abs() < 1e-15, "sin {a}");
        }
        assert!((radians(180).to_f64() - std::f64::consts::PI).abs() < 1e-15);
        // At 60°, x is a hair off π/3, so cos x is not exactly ½.
        let (c60, _) = cos_sin(60);
        assert_ne!(c60, e(1).div(e(2)).unwrap());
        assert_eq!(cos_sin(0), (e(1), Ext::ZERO));
    }
}
