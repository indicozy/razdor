//! Razdor's «Погода» (`Settings::map_weather`, off by default): the weather follows the land. Snow
//! over snow and ice, rain over swamps and lowlands, heat haze over deserts, ash over fire lands and
//! cloud shadows everywhere, each biome's weather drawn for its day. Style only: no rule, fog, path,
//! AI or save reads it, and nothing is kept (the weather is recomputed from the map and the clock).

use std::sync::atomic::{AtomicBool, Ordering};

/// The setting (`Settings::map_weather`), pushed every frame by `display::follow_settings`.
static ON: AtomicBool = AtomicBool::new(false);

/// Takes the setting «Погода» in (every frame).
pub fn set_pref(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Whether the weather is drawn (the setting, or `RAZDOR_WEATHER=1`).
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_off_until_it_is_set() {
        set_pref(false);
        assert!(!on());
        set_pref(true);
        assert!(on());
        set_pref(false);
    }
}
