//! Razdor's «Качество графики» (the owner's decision, 2026-11-12): one row of the map group sets the map's look in one
//! go: its resolution, the light, the shadows, the mist, the moving water and its rate and quality, the weather, the
//! living map, the map's animation, and (the owner's table, 2026-11-19) the natural ground, the cloud shadows and the
//! decorations, and «Снегопад» (the falling snow, off on «Низкое»). Nothing stores the choice. The row shows the first
//! preset whose values all hold in the settings, or «Своё» when none does, so a setting changed by hand shows «Своё» by itself, and an install that
//! has its settings keeps them as they are. A new install's settings are the original's look («Оригинал»).

use razdor::i18n::tr;

use super::audio::Settings;
use super::daylight::{LightMode, ShadowMode};
use super::lights::Quality as LightQuality;
use super::map_anim::Mode as AnimMode;
use super::mist::MistMode;
use super::terrain::NaturalGround;
use super::water::{FrameRate, Quality as WaterQuality};
use super::world_view::decor::DecorLevel;

/// The presets, in the row's order: «Оригинал» first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// The original's look, with no new effects; a new install's settings.
    Original,
    Low,
    Medium,
    High,
}

/// The presets in the row's cycle order.
const ORDER: [Preset; 4] = [Preset::Original, Preset::Low, Preset::Medium, Preset::High];

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
    /// «Улучшенная земля» (`Settings::map_natural_ground`). «Оригинал» says Off, so a non-original look
    /// never shows as «Оригинал».
    natural: Option<NaturalGround>,
    /// «Тени облаков» (`Settings::map_cloud_shadows`).
    clouds: Option<bool>,
    /// «Украшения карты» (`Settings::map_decor`).
    decor: Option<DecorLevel>,
    /// «Снегопад» (`Settings::map_snowfall`): off on «Низкое», on from «Среднее» up; «Оригинал» does not say.
    snowfall: Option<bool>,
}

/// The values of each preset (the owner's decisions, 2026-11-12).
fn look(p: Preset) -> Look {
    match p {
        Preset::Original => Look {
            render_scale: Some(1.0),
            light_on: Some(false),
            light_quality: None,
            shadows: Some(ShadowMode::Painted),
            mist: Some(MistMode::Off),
            live_water: Some(false),
            water_rate: None,
            water_quality: None,
            weather: Some(false),
            living: Some(false),
            anim: Some(AnimMode::On),
            natural: Some(NaturalGround::Off),
            clouds: Some(false),
            decor: Some(DecorLevel::Off),
            snowfall: None,
        },
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
            natural: Some(NaturalGround::Off),
            clouds: Some(false),
            decor: Some(DecorLevel::Off),
            snowfall: Some(false),
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
            natural: Some(NaturalGround::Soft),
            clouds: Some(true),
            decor: Some(DecorLevel::Few),
            snowfall: Some(true),
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
            natural: Some(NaturalGround::Strong),
            clouds: Some(true),
            decor: Some(DecorLevel::Normal),
            snowfall: Some(true),
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
            && self.natural.is_none_or(|n| s.map_natural_ground == n)
            && self.clouds.is_none_or(|on| s.map_cloud_shadows == on)
            && self.decor.is_none_or(|d| s.map_decor == d)
            && self.snowfall.is_none_or(|on| s.map_snowfall == on)
    }
}

/// The preset the settings hold, `None` for «Своё». «Смешивание земли» (`Settings::map_ground_blend`) is in no
/// preset: `Look` has no field for it, so `holds` never compares it and `set` never changes it. A player who
/// turns it off keeps «Низкое» (or any other preset) showing.
pub fn current(s: &Settings) -> Option<Preset> {
    ORDER.into_iter().find(|p| look(*p).holds(s))
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
    if let Some(n) = l.natural {
        s.map_natural_ground = n;
    }
    if let Some(on) = l.clouds {
        s.map_cloud_shadows = on;
    }
    if let Some(d) = l.decor {
        s.map_decor = d;
    }
    if let Some(on) = l.snowfall {
        s.map_snowfall = on;
    }
}

/// The preset a click on the row sets: «Оригинал», «Низкое», «Среднее», «Высокое», then «Оригинал» again; from «Своё»,
/// «Оригинал».
pub fn next(s: &Settings) -> Preset {
    match current(s) {
        Some(Preset::Original) => Preset::Low,
        Some(Preset::Low) => Preset::Medium,
        Some(Preset::Medium) => Preset::High,
        Some(Preset::High) => Preset::Original,
        None => Preset::Original,
    }
}

/// The row's value: the preset's name, or «Своё».
pub fn label(p: Option<Preset>) -> &'static str {
    match p {
        // The catalog keys are the English names (the English UI shows them as they are).
        Some(Preset::Original) => tr("Original"),
        Some(Preset::Low) => tr("Low"),
        Some(Preset::Medium) => tr("Medium"),
        Some(Preset::High) => tr("High"),
        None => tr("Custom settings"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ground_blend_is_in_no_preset() {
        // «Смешивание земли» is no preset's value: every preset still shows with it off, and setting a preset leaves it.
        for p in ORDER {
            let mut s = Settings::default();
            set(&mut s, p);
            s.map_ground_blend = false;
            assert_eq!(current(&s), Some(p), "{p:?} still holds with the blend off");
            set(&mut s, p);
            assert!(!s.map_ground_blend, "{p:?} does not set the blend");
        }
    }

    #[test]
    fn the_effects_follow_the_owners_table() {
        // «Улучшенная земля», «Тени облаков», «Украшения карты» (the owner's table, 2026-11-19).
        let table = [
            (Preset::Original, NaturalGround::Off, false, DecorLevel::Off),
            (Preset::Low, NaturalGround::Off, false, DecorLevel::Off),
            (Preset::Medium, NaturalGround::Soft, true, DecorLevel::Few),
            (Preset::High, NaturalGround::Strong, true, DecorLevel::Normal),
        ];
        for (p, natural, clouds, decor) in table {
            let mut s = Settings { map_natural_ground: NaturalGround::Strong, map_cloud_shadows: !clouds, map_decor: DecorLevel::Normal, ..Settings::default() };
            set(&mut s, p);
            assert_eq!((s.map_natural_ground, s.map_cloud_shadows, s.map_decor), (natural, clouds, decor), "{p:?} sets its three effects");
            assert_eq!(current(&s), Some(p), "{p:?} shows with its effects");
        }
    }

    #[test]
    fn a_changed_effect_shows_custom() {
        // Each of the three effects is part of the presets: a hand change of one shows «Своё» for «Среднее» and «Высокое».
        for p in [Preset::Medium, Preset::High] {
            let mut s = Settings::default();
            set(&mut s, p);
            assert_eq!(current(&s), Some(p));
            s.map_cloud_shadows = !s.map_cloud_shadows;
            assert_eq!(current(&s), None, "{p:?} with clouds changed");
            s.map_cloud_shadows = !s.map_cloud_shadows;
            s.map_decor = DecorLevel::Off;
            assert_eq!(current(&s), None, "{p:?} with decorations changed");
        }
    }

    #[test]
    fn the_low_preset_turns_the_snowfall_off() {
        // «Снегопад» is off on «Низкое», on from «Среднее» up; «Оригинал» does not say (not compared, not set).
        let mut s = Settings { map_snowfall: true, ..Settings::default() };
        set(&mut s, Preset::Low);
        assert!(!s.map_snowfall, "Low turns the falling snow off");
        assert_eq!(current(&s), Some(Preset::Low));
        for p in [Preset::Medium, Preset::High] {
            set(&mut s, p);
            assert!(s.map_snowfall, "{p:?} turns the falling snow on");
            assert_eq!(current(&s), Some(p));
        }
        set(&mut s, Preset::High);
        s.map_snowfall = false;
        assert_eq!(current(&s), None, "High with the snowfall off shows custom");
        let mut s = Settings { map_snowfall: false, ..Settings::default() };
        set(&mut s, Preset::Original);
        assert!(!s.map_snowfall, "Original does not set the snowfall");
        assert_eq!(current(&s), Some(Preset::Original), "and Original holds with it off");
    }

    #[test]
    fn an_install_from_before_the_effects_keeps_its_low_and_original_look() {
        // An older file has none of the keys: the defaults (no effects, snowfall on). «Оригинал» still holds; an old
        // «Низкое», «Среднее» or «Высокое» shows «Своё» until the player picks it again (the plain comparison).
        let s = Settings::default();
        assert_eq!(current(&s), Some(Preset::Original));
        let mut s = Settings::default();
        set(&mut s, Preset::Low);
        s.map_snowfall = true;
        assert_eq!(current(&s), None, "an old «Низкое» has the falling snow on, which Low says off");
        let mut s = Settings::default();
        set(&mut s, Preset::Medium);
        s.map_natural_ground = NaturalGround::Off;
        s.map_cloud_shadows = false;
        s.map_decor = DecorLevel::Off;
        assert_eq!(current(&s), None, "the old «Среднее» values without the three effects");
    }

    #[test]
    fn each_preset_is_the_value_its_own_settings_show() {
        for p in ORDER {
            let mut s = Settings::default();
            set(&mut s, p);
            assert_eq!(current(&s), Some(p), "{p:?}");
        }
    }

    #[test]
    fn a_new_install_is_the_original_look() {
        // The default settings (no file) are «Оригинал»: a fresh install looks like the original game.
        assert_eq!(current(&Settings::default()), Some(Preset::Original));
    }

    #[test]
    fn a_setting_changed_by_hand_shows_custom_and_the_next_click_is_the_original() {
        let mut s = Settings::default();
        set(&mut s, Preset::High);
        s.map_weather = false;
        assert_eq!(current(&s), None);
        assert_eq!(next(&s), Preset::Original);
    }

    #[test]
    fn the_row_cycles_original_low_medium_high() {
        let mut s = Settings::default();
        let mut seen = Vec::new();
        for _ in 0..5 {
            let p = next(&s);
            set(&mut s, p);
            seen.push(p);
        }
        assert_eq!(seen, vec![Preset::Low, Preset::Medium, Preset::High, Preset::Original, Preset::Low]);
    }
}
