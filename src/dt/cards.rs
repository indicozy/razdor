//! The 2.5D cards of the map's building sprites: each sprite cut into a few pieces, each
//! standing on its own ground point. The numbers come from `tools/depth` (an offline run of a
//! depth model) and ship in `data/cards/objects.json`; only numbers, no pixels. Pure data:
//! no macroquad.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::Deserialize;

use super::gfx::Image;

/// One card: a piece of a sprite that stands on its own ground point.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    /// The ground point, in sprite pixels (origin top-left).
    pub foot: (f32, f32),
    /// The outline's vertices, in sprite pixels.
    pub verts: Vec<(f32, f32)>,
    /// Triangles over `verts` (indices into `verts`, three a triangle).
    pub tris: Vec<u16>,
    /// Ground, not standing (the 2.5D fold): the piece lies flat on the map (courtyards, paving, grass, fields);
    /// the tilted views draw it on the ground and leave it out of the standing layers. False for the cards
    /// of the older data and of the in-game cut.
    pub ground: bool,
}

/// A sprite's cards, back to front, with the size they were cut at.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteCards {
    pub w: u32,
    pub h: u32,
    pub cards: Vec<Card>,
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// The key of a sprite: FNV-1a 64 over `u32le(w) ++ u32le(h) ++ rgba`, as `tools/depth` keys it.
pub fn sprite_key(img: &Image) -> u64 {
    let mut h = FNV_OFFSET;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(FNV_PRIME);
        }
    };
    feed(&img.width.to_le_bytes());
    feed(&img.height.to_le_bytes());
    feed(&img.rgba);
    h
}

#[derive(Deserialize)]
struct File {
    sprites: HashMap<String, RawSprite>,
    /// Building sprites left with no entry: one region, or too small to cut. They stand as one
    /// billboard, and the depth model is not asked about them.
    #[serde(default)]
    one_card: Vec<String>,
}

#[derive(Deserialize)]
struct RawSprite {
    w: u32,
    h: u32,
    cards: Vec<RawCard>,
}

#[derive(Deserialize)]
pub(crate) struct RawCard {
    foot: [f32; 2],
    verts: Vec<f32>,
    tris: Vec<u32>,
    /// Version 2 of the data: the piece is ground (see [`Card::ground`]); absent in version 1.
    #[serde(default)]
    ground: bool,
}

/// A card as the file gives it, or `None` when its data cannot stand: an odd vertex list, no
/// triangle or a triangle list not in threes, or an index past the vertices.
pub(crate) fn checked(raw: RawCard) -> Option<Card> {
    if !raw.verts.len().is_multiple_of(2) || raw.tris.is_empty() || !raw.tris.len().is_multiple_of(3) {
        return None;
    }
    let n = raw.verts.len() / 2;
    let tris = raw.tris.iter().map(|&i| usize::try_from(i).ok().filter(|&i| i < n).and_then(|i| u16::try_from(i).ok())).collect::<Option<Vec<u16>>>()?;
    Some(Card { foot: (raw.foot[0], raw.foot[1]), verts: raw.verts.chunks_exact(2).map(|v| (v[0], v[1])).collect(), tris, ground: raw.ground })
}

/// The cards of every sprite in `json` (the format of `data/cards/objects.json`), keyed by the
/// sprite's key. A card with bad data is dropped; a sprite left with no card, or a key that is not
/// 16 hex digits, has no entry. Unreadable JSON gives none.
pub fn parse(json: &str) -> HashMap<u64, SpriteCards> {
    let Ok(file) = serde_json::from_str::<File>(json) else { return HashMap::new() };
    file.sprites
        .into_iter()
        .filter_map(|(key, s)| {
            let key = u64::from_str_radix(&key, 16).ok().filter(|_| key.len() == 16)?;
            let cards: Vec<Card> = s.cards.into_iter().filter_map(checked).collect();
            (!cards.is_empty()).then_some((key, SpriteCards { w: s.w, h: s.h, cards }))
        })
        .collect()
}

/// The keys listed as one-card buildings in `json` (16 hex digits each; others are dropped).
pub fn parse_one_card(json: &str) -> HashSet<u64> {
    let Ok(file) = serde_json::from_str::<File>(json) else { return HashSet::new() };
    file.one_card.iter().filter(|k| k.len() == 16).filter_map(|k| u64::from_str_radix(k, 16).ok()).collect()
}

/// The bundled cards (`data/cards/objects.json`), parsed once.
pub fn builtin() -> &'static HashMap<u64, SpriteCards> {
    static CARDS: OnceLock<HashMap<u64, SpriteCards>> = OnceLock::new();
    CARDS.get_or_init(|| parse(include_str!("../../data/cards/objects.json")))
}

/// The bundled one-card keys (see [`parse_one_card`]), parsed once.
pub fn builtin_one_card() -> &'static HashSet<u64> {
    static KEYS: OnceLock<HashSet<u64>> = OnceLock::new();
    KEYS.get_or_init(|| parse_one_card(include_str!("../../data/cards/objects.json")))
}

/// Whether the bundled data knows a sprite: it has cut data of its size, or it is a one-card one.
pub fn builtin_knows(img: &Image) -> bool {
    let key = sprite_key(img);
    builtin().get(&key).is_some_and(|k| k.w == img.width && k.h == img.height) || builtin_one_card().contains(&key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: u32, h: u32, rgba: Vec<u8>) -> Image {
        Image { width: w, height: h, rgba }
    }

    #[test]
    fn key_matches_the_tool_test_vector() {
        // tools/depth's vector: w = 1, h = 1, one pixel (1, 2, 3, 4).
        assert_eq!(sprite_key(&image(1, 1, vec![1, 2, 3, 4])), 0xef73_dc3a_80c8_da6d);
        assert_eq!(format!("{:016x}", sprite_key(&image(1, 1, vec![1, 2, 3, 4]))), "ef73dc3a80c8da6d");
    }

    #[test]
    fn parses_cards_and_drops_bad_ones() {
        let json = r#"{"version":1,"sprites":{
            "0000000000000001":{"w":4,"h":4,"cards":[
                {"foot":[1.5,3.0],"verts":[0,0,4,0,0,4],"tris":[0,1,2]},
                {"foot":[2.0,3.0],"verts":[0,0,4,0,0,4],"tris":[0,1,3]},
                {"foot":[2.0,3.0],"verts":[0,0,4,0,0,4],"tris":[0,1]}
            ]},
            "00000000000000002":{"w":4,"h":4,"cards":[{"foot":[0,0],"verts":[0,0,1,1],"tris":[]}]},
            "0000000000000003":{"w":4,"h":4,"cards":[{"foot":[0,0],"verts":[0,0,1,1],"tris":[]}]}
        }}"#;
        let parsed = parse(json);
        assert_eq!(parsed.len(), 1, "a 17-digit key and a sprite with no card are dropped");
        let s = &parsed[&1];
        assert_eq!((s.w, s.h), (4, 4));
        assert_eq!(s.cards.len(), 1, "an index past the vertices and a short triangle are dropped");
        assert_eq!(s.cards[0].foot, (1.5, 3.0));
        assert_eq!(s.cards[0].verts, vec![(0.0, 0.0), (4.0, 0.0), (0.0, 4.0)]);
        assert_eq!(s.cards[0].tris, vec![0, 1, 2]);
        assert!(!parsed.contains_key(&3), "a card with no triangles is no card");
        assert!(parse("not json").is_empty());
    }

    /// The player's install; skipped without `RAZDOR_DT_DIR`. Every building sprite of the install
    /// is known to the bundled data (a cut, or one card), so the depth model is never asked about
    /// a base-game building.
    #[test]
    fn bundled_keys_match_the_installs_buildings() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let sprites = install.map_objects().expect("map objects decode");
        let buildings: Vec<&super::super::gfx::ObjectSprite> = sprites.sprites.iter().filter(|s| s.section == super::super::gfx::ObjectSprite::BUILDINGS).collect();
        let cut = buildings.iter().filter(|s| builtin().get(&sprite_key(&s.image)).is_some_and(|k| k.w == s.image.width && k.h == s.image.height)).count();
        println!("{cut} of {} building sprites have cards", buildings.len());
        assert!(cut > 0, "no building sprite of the install has cut data");
        let unknown: Vec<u64> = buildings.iter().filter(|s| !builtin_knows(&s.image)).map(|s| sprite_key(&s.image)).collect();
        assert!(unknown.is_empty(), "building sprites the bundled data does not know: {unknown:x?}");
    }

    #[test]
    fn one_card_keys_are_read_and_bad_ones_dropped() {
        let json = r#"{"version":1,"one_card":["0000000000000007","xyz","00000000000000080"],"sprites":{}}"#;
        let keys = parse_one_card(json);
        assert_eq!(keys, HashSet::from([7]));
        assert!(parse_one_card(r#"{"sprites":{}}"#).is_empty(), "the list is optional");
        assert!(builtin_one_card().len() >= 37, "the bundled list is there");
    }

    #[test]
    fn the_ground_flag_is_read_and_defaults_to_standing() {
        let json = r#"{"version":2,"sprites":{
            "0000000000000001":{"w":4,"h":4,"cards":[
                {"foot":[1.0,3.0],"verts":[0,0,4,0,0,4],"tris":[0,1,2],"ground":true},
                {"foot":[2.0,3.0],"verts":[0,0,4,0,0,4],"tris":[0,1,2]}
            ]}
        }}"#;
        let parsed = parse(json);
        let cards = &parsed[&1].cards;
        assert!(cards[0].ground, "a version 2 card reads its ground flag");
        assert!(!cards[1].ground, "a card with no flag (the version 1 data) stands");
    }

    #[test]
    fn bundled_buildings_stand_on_their_ground() {
        // The fold (2.5D): every sprite keeps a standing piece, so a building never turns into a flat
        // picture on the ground, and some buildings have ground pieces to lie flat.
        let cards = builtin();
        let mixed = cards.values().filter(|s| s.cards.iter().any(|c| c.ground)).count();
        let all_ground = cards.values().filter(|s| s.cards.iter().all(|c| c.ground)).count();
        assert!(mixed > 0, "some buildings have ground pieces");
        assert_eq!(all_ground, 0, "no sprite is all ground");
    }

    #[test]
    fn bundled_cards_load() {
        let cards = builtin();
        assert!(!cards.is_empty());
        assert!(cards.values().all(|s| !s.cards.is_empty()));
    }
}
