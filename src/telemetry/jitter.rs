//! Send-time jitter for the analytics: the battle upload's offset within its hour slot, and the retry
//! backoff's ±25%. It has its own small generator (splitmix64, seeded from the install id), so it is
//! not the rules' RNG (`crate::rules::rng`): the functions take plain numbers and never touch the rules'
//! state, and this module imports nothing from `rules`.

/// The largest offset a battle upload waits past its hour boundary, seconds (15 minutes).
pub const MAX_OFFSET_SECS: i64 = 900;

/// Separates the retry draws from the slot draws for the same seed.
const RETRY_DOMAIN: u64 = 0x5245_5452_5900_0000;

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// One draw for `seed` and `salt`.
fn draw(seed: u64, salt: u64) -> u64 {
    splitmix64(seed ^ splitmix64(salt))
}

/// The jitter seed of an install: FNV-1a over its id (the id is random, so its bytes are the entropy).
pub fn seed_of(install_id: &str) -> u64 {
    install_id.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The offset past its hour boundary of the battle uploads of hour `slot` (the hour's index since the
/// epoch), in `0..=MAX_OFFSET_SECS`. The same `seed` and `slot` always give the same offset.
pub fn slot_offset_secs(seed: u64, slot: i64) -> i64 {
    (draw(seed, slot as u64) % (MAX_OFFSET_SECS as u64 + 1)) as i64
}

/// The retry wait `base_secs` with ±25% jitter for the `attempt`-th retry: within
/// `base*0.75 ..= base*1.25` (in whole seconds, the bounds rounded inwards). A base of 0 or less is 0.
pub fn jittered(base_secs: i64, seed: u64, attempt: u32) -> i64 {
    if base_secs <= 0 {
        return 0;
    }
    let span = base_secs / 4;
    let r = draw(seed ^ RETRY_DOMAIN, u64::from(attempt));
    base_secs - span + (r % (2 * span as u64 + 1)) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    // The rules' RNG (`crate::rules::rng`) cannot be passed in: these signatures take numbers only.
    #[test]
    fn the_functions_take_plain_numbers_not_the_rules_rng() {
        let _: fn(u64, i64) -> i64 = slot_offset_secs;
        let _: fn(i64, u64, u32) -> i64 = jittered;
        let _: fn(&str) -> u64 = seed_of;
    }

    #[test]
    fn the_slot_offset_stays_within_zero_to_fifteen_minutes() {
        for seed in [0u64, 1, 0xdead_beef, u64::MAX] {
            for slot in 0..10_000i64 {
                let o = slot_offset_secs(seed, slot);
                assert!((0..=MAX_OFFSET_SECS).contains(&o), "seed {seed} slot {slot}: {o}");
            }
        }
    }

    #[test]
    fn the_slot_offsets_are_spread_not_all_equal() {
        let seed = seed_of("0123456789abcdef0123456789abcdef");
        let distinct: BTreeSet<i64> = (1_000..1_200).map(|slot| slot_offset_secs(seed, slot)).collect();
        assert!(distinct.len() > 100, "only {} distinct offsets in 200 slots", distinct.len());
    }

    #[test]
    fn the_same_seed_and_slot_give_the_same_offset_and_another_seed_differs() {
        let a = seed_of("0123456789abcdef0123456789abcdef");
        let b = seed_of("fedcba9876543210fedcba9876543210");
        assert_eq!(slot_offset_secs(a, 490_000), slot_offset_secs(a, 490_000));
        assert_ne!(a, b);
        assert!((0..200).any(|slot| slot_offset_secs(a, slot) != slot_offset_secs(b, slot)));
    }

    #[test]
    fn the_jitter_stays_within_plus_and_minus_a_quarter() {
        let seed = seed_of("0123456789abcdef0123456789abcdef");
        for base in [1i64, 2, 3, 60, 61, 120, 3600, 21_600] {
            let mut seen = BTreeSet::new();
            for attempt in 0..2_000u32 {
                let v = jittered(base, seed, attempt);
                assert!(4 * v >= 3 * base && 4 * v <= 5 * base, "base {base} attempt {attempt}: {v}");
                seen.insert(v);
            }
            if base >= 60 {
                assert!(seen.len() > 10, "base {base}: the jitter does not spread ({} values)", seen.len());
            }
        }
        assert_eq!(jittered(0, seed, 3), 0);
        assert_eq!(jittered(-5, seed, 3), 0);
    }

    #[test]
    fn the_jitter_is_seeded_and_repeats_for_the_same_input() {
        let seed = seed_of("0123456789abcdef0123456789abcdef");
        assert_eq!(jittered(60, seed, 2), jittered(60, seed, 2));
    }
}
