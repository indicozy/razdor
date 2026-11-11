//! Razdor's «Качество графики» (the owner's decision, 2026-11-12): one row of the map group sets the map's look in one
//! go: its resolution, the light, the shadows, the mist, the moving water and its rate and quality, the weather, the
//! living map and the map's animation. Nothing stores the choice. The row shows the first preset (low to high) whose
//! values all hold in the settings, or «Своё» when none does, so a setting changed by hand shows «Своё» by itself, and
//! an install that has its settings keeps them as they are.

use razdor::i18n::tr;

use super::audio::Settings;
use super::daylight::{LightMode, ShadowMode};
use super::lights::Quality as LightQuality;
use super::map_anim::Mode as AnimMode;
use super::mist::MistMode;
use super::water::{FrameRate, Quality as WaterQuality};

/// The three presets, low to high.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Low,
    Medium,
    High,
}

/// A preset's values. `None` is a value the preset does not say: it is neither compared nor set.
#[derive(Clone, Copy, Debug)]
struct Look {
    render_scale: Option<f32>,
    /// On: the light follows the game's clock (`LightMode::GameClock`) unless it already follows another mode.
    light_on: Option<bool>,
    light_quality: Option<LightQuality>,
    shadows: Option<ShadowMode>,
    mist: Option<MistMode>,
    live_water: Option<bool>,
    water_rate: Option<FrameRate>,
    water_quality: Option<WaterQuality>,
    weather: Option<bool>,
    living: Option<bool>,
    anim: Option<AnimMode>,
}

/// The values of each preset (the owner's decision, 2026-11-12).
fn look(p: Preset) -> Look {
    match p {
        Preset::Low => Look {
            render_scale: Some(0.75),
            light_on: Some(false),
            light_quality: None,
            shadows: Some(ShadowMode::Painted),
            mist: Some(MistMode::Flat),
            live_water: Some(false),
            water_rate: None,
            water_quality: None,
            weather: Some(false),
            living: Some(false),
            anim: Some(AnimMode::Near),
        },
        Preset::Medium => Look {
            render_scale: Some(1.0),
            light_on: Some(true),
            light_quality: Some(LightQuality::Medium),
            shadows: Some(ShadowMode::Sun),
            mist: Some(MistMode::Flat),
            live_water: Some(true),
            water_rate: Some(FrameRate::EveryOther),
            water_quality: Some(WaterQuality::Medium),
            weather: Some(true),
            living: Some(true),
            anim: Some(AnimMode::Near),
        },
        Preset::High => Look {
            render_scale: Some(1.0),
            light_on: Some(true),
            light_quality: Some(LightQuality::High),
            shadows: Some(ShadowMode::Sun),
            mist: Some(MistMode::Clouds),
            live_water: Some(true),
            water_rate: Some(FrameRate::EveryFrame),
            water_quality: Some(WaterQuality::High),
            weather: Some(true),
            living: Some(true),
            anim: Some(AnimMode::On),
        },
    }
}

impl Look {
    /// Whether every value this preset says is the one in `s`. A light quality of `None` is the device's
    /// (`lights::default_quality`), as the light uses it.
    fn holds(&self, s: &Settings) -> bool {
        self.render_scale.is_none_or(|v| s.map_render_scale == v)
            && self.light_on.is_none_or(|on| (s.map_light != LightMode::Off) == on)
            && self.light_quality.is_none_or(|q| s.map_light_quality.unwrap_or_else(|| super::lights::default_quality(razdor::platform::MOBILE)) == q)
            && self.shadows.is_none_or(|m| s.map_shadows == m)
            && self.mist.is_none_or(|m| s.map_mist == m)
            && self.live_water.is_none_or(|on| s.map_live_water == on)
            && self.water_rate.is_none_or(|r| s.map_water_rate == r)
            && self.water_quality.is_none_or(|q| s.map_water_quality == q)
            && self.weather.is_none_or(|on| s.map_weather == on)
            && self.living.is_none_or(|on| s.map_living == on)
            && self.anim.is_none_or(|m| s.map_anim == m)
    }
}

/// The preset the settings hold, `None` for «Своё».
pub fn current(s: &Settings) -> Option<Preset> {
    [Preset::Low, Preset::Medium, Preset::High].into_iter().find(|p| look(*p).holds(s))
}

/// Sets the settings to `p`'s values (the values the preset does not say are left as they are).
pub fn set(s: &mut Settings, p: Preset) {
    let l = look(p);
    if let Some(v) = l.render_scale {
        s.map_render_scale = v;
    }
    if let Some(on) = l.light_on {
        s.map_light = match (on, s.map_light) {
            (false, _) => LightMode::Off,
            (true, LightMode::Off) => LightMode::GameClock,
            (true, mode) => mode,
        };
    }
    if let Some(q) = l.light_quality {
        s.map_light_quality = Some(q);
    }
    if let Some(m) = l.shadows {
        s.map_shadows = m;
    }
    if let Some(m) = l.mist {
        s.map_mist = m;
    }
    if let Some(on) = l.live_water {
        s.map_live_water = on;
    }
    if let Some(r) = l.water_rate {
        s.map_water_rate = r;
    }
    if let Some(q) = l.water_quality {
        s.map_water_quality = q;
    }
    if let Some(on) = l.weather {
        s.map_weather = on;
    }
    if let Some(on) = l.living {
        s.map_living = on;
    }
    if let Some(m) = l.anim {
        s.map_anim = m;
    }
}

/// The preset a click on the row sets: low, medium, high, then low again; from «Своё», medium.
pub fn next(s: &Settings) -> Preset {
    match current(s) {
        Some(Preset::Low) => Preset::Medium,
        Some(Preset::Medium) => Preset::High,
        Some(Preset::High) => Preset::Low,
        None => Preset::Medium,
    }
}

/// The preset a fresh install starts on: high on the desktop, medium on a phone.
pub fn fresh_install() -> Preset {
    if razdor::platform::MOBILE {
        Preset::Medium
    } else {
        Preset::High
    }
}

/// The row's value: the preset's name, or «Своё».
pub fn label(p: Option<Preset>) -> &'static str {
    match p {
        Some(Preset::Low) => tr("Graphics preset low"),
        Some(Preset::Medium) => tr("Graphics preset medium"),
        Some(Preset::High) => tr("Graphics preset high"),
        None => tr("Graphics preset custom"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_preset_is_the_value_its_own_settings_show() {
        for p in [Preset::Low, Preset::Medium, Preset::High] {
            let mut s = Settings::default();
            set(&mut s, p);
            assert_eq!(current(&s), Some(p), "{p:?}");
        }
    }

    #[test]
    fn a_setting_changed_by_hand_shows_custom_and_the_next_click_is_medium() {
        let mut s = Settings::default();
        set(&mut s, Preset::High);
        s.map_weather = false;
        assert_eq!(current(&s), None);
        assert_eq!(next(&s), Preset::Medium);
    }

    #[test]
    fn a_fresh_install_shows_its_platforms_preset() {
        let mut s = Settings::default();
        set(&mut s, fresh_install());
        assert_eq!(current(&s), Some(fresh_install()));
    }
}
