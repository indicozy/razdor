//! The light points of the map's building sprites: boxes of windows, doors, gates, chimneys, flags
//! and mill sails, found offline by `tools/depth/detect_points.py` and shipped as numbers in
//! `data/lights/objects.json` (no pixels). Each sprite is keyed by its pixel hash
//! ([`cards::sprite_key`](super::cards::sprite_key)), the same key as the depth cards. Pure data:
//! no macroquad.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// What a light point marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Window,
    Door,
    Gate,
    Chimney,
    Flag,
    Sails,
}

impl Kind {
    fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "window" => Kind::Window,
            "door" => Kind::Door,
            "gate" => Kind::Gate,
            "chimney" => Kind::Chimney,
            "flag" => Kind::Flag,
            "sails" => Kind::Sails,
            _ => return None,
        })
    }
}

/// One point: its kind and its box `[x0, y0, x1, y1]` in sprite pixels (origin top-left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub kind: Kind,
    pub rect: [f32; 4],
}

/// A sprite's points, with the size they were found at.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteLights {
    pub w: u32,
    pub h: u32,
    pub points: Vec<Point>,
}

#[derive(Deserialize)]
struct File {
    sprites: HashMap<String, RawSprite>,
}

#[derive(Deserialize)]
struct RawSprite {
    w: u32,
    h: u32,
    points: Vec<RawPoint>,
}

#[derive(Deserialize)]
struct RawPoint {
    kind: String,
    #[serde(rename = "box")]
    rect: [f32; 4],
}

/// The points of every sprite in `json` (the format of `data/lights/objects.json`), keyed by the
/// sprite's key. A point of an unknown kind or with an empty box is dropped; a sprite left with no
/// point has no entry, and so does a key that is not 16 hex digits. Unreadable JSON gives none.
pub fn parse(json: &str) -> HashMap<u64, SpriteLights> {
    let Ok(file) = serde_json::from_str::<File>(json) else { return HashMap::new() };
    file.sprites
        .into_iter()
        .filter_map(|(key, s)| {
            let key = u64::from_str_radix(&key, 16).ok().filter(|_| key.len() == 16)?;
            let points: Vec<Point> = s
                .points
                .iter()
                .filter_map(|p| {
                    let kind = Kind::parse(&p.kind)?;
                    let [x0, y0, x1, y1] = p.rect;
                    (x1 > x0 && y1 > y0).then_some(Point { kind, rect: p.rect })
                })
                .collect();
            (!points.is_empty()).then_some((key, SpriteLights { w: s.w, h: s.h, points }))
        })
        .collect()
}

/// The bundled light points (`data/lights/objects.json`), parsed once.
pub fn builtin() -> &'static HashMap<u64, SpriteLights> {
    static LIGHTS: OnceLock<HashMap<u64, SpriteLights>> = OnceLock::new();
    LIGHTS.get_or_init(|| parse(include_str!("../../data/lights/objects.json")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_points_by_key_and_drops_bad_ones() {
        let json = r#"{"version":1,"model":"test","sprites":{
            "0000000000000001":{"w":40,"h":30,"points":[
                {"kind":"chimney","box":[10,0,14,6],"score":1.0},
                {"kind":"sails","box":[2,2,20,20],"score":1.0},
                {"kind":"unknown","box":[1,1,5,5],"score":1.0},
                {"kind":"flag","box":[9,9,9,12],"score":1.0}
            ]},
            "00000000000000002":{"w":4,"h":4,"points":[{"kind":"door","box":[0,0,2,2],"score":1.0}]},
            "0000000000000003":{"w":4,"h":4,"points":[{"kind":"window","box":[0,0,2,2],"score":1.0}]},
            "0000000000000004":{"w":4,"h":4,"points":[{"kind":"gate","box":[0,0,0,2],"score":1.0}]}
        }}"#;
        let parsed = parse(json);
        assert_eq!(parsed.len(), 2, "a 17-digit key and a sprite with no usable point are dropped");
        let s = &parsed[&1];
        assert_eq!((s.w, s.h), (40, 30));
        assert_eq!(
            s.points,
            vec![Point { kind: Kind::Chimney, rect: [10.0, 0.0, 14.0, 6.0] }, Point { kind: Kind::Sails, rect: [2.0, 2.0, 20.0, 20.0] }],
            "an unknown kind and an empty box are dropped, the order kept"
        );
        assert_eq!(parsed[&3].points[0].kind, Kind::Window);
        assert!(parse("not json").is_empty());
    }

    #[test]
    fn bundled_points_load_and_hold_the_living_kinds() {
        let lights = builtin();
        assert!(lights.len() > 90, "the bundled detector run is there: {}", lights.len());
        let living = lights.values().flat_map(|s| &s.points).filter(|p| matches!(p.kind, Kind::Chimney | Kind::Flag | Kind::Sails)).count();
        // 12 chimneys on castle towers were removed by the owner's corrections (backlog 1600): 159 before.
        assert!(living > 140, "chimneys, flags and sails are shipped: {living}");
    }

    /// Backlog 1600: a castle's round tower carries a cone roof, and the detector marked its top as a
    /// chimney, so smoke rose from the tower. The corrections (`tools/depth/lights_overrides.json`) remove
    /// those chimneys; these castles (the orange-roofed tower of the report, and its twin in the other
    /// install) keep none of them. The wall-tower tops of the same castle family are pinned too.
    #[test]
    fn castle_towers_have_no_chimney_points() {
        for key in ["83a0e13facd9012d", "e1b73f452d96b345", "2dd78dde7f9809d6", "780f04f891a58a92"] {
            let key = u64::from_str_radix(key, 16).unwrap();
            let chimneys = builtin().get(&key).map_or(0, |s| s.points.iter().filter(|p| p.kind == Kind::Chimney).count());
            assert_eq!(chimneys, 0, "sprite {key:016x}: a tower top is not a chimney (backlog 1600)");
        }
    }

    /// The player's install; skipped without `RAZDOR_DT_DIR`. Every building sprite of the install
    /// that has light points has the size they were found at.
    #[test]
    fn bundled_light_sizes_match_the_installs_buildings() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let sprites = install.map_objects().expect("map objects decode");
        let mut matched = 0;
        for s in sprites.sprites.iter().filter(|s| s.section == super::super::gfx::ObjectSprite::BUILDINGS) {
            if let Some(l) = builtin().get(&super::super::cards::sprite_key(&s.image)) {
                assert_eq!((l.w, l.h), (s.image.width, s.image.height), "the light points' size is the sprite's");
                matched += 1;
            }
        }
        println!("{matched} building sprites have light points");
        assert!(matched > 0, "no building sprite of the install has light points");
    }
}
