//! The 2.5D cards of the map's building sprites: each sprite cut into a few pieces, each
//! standing on its own ground point. The numbers come from `tools/depth` (an offline run of a
//! depth model) and ship in `data/cards/objects.json`; only numbers, no pixels. Pure data:
//! no macroquad.

use std::collections::HashMap;
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
}

#[derive(Deserialize)]
struct RawSprite {
    w: u32,
    h: u32,
    cards: Vec<RawCard>,
}

#[derive(Deserialize)]
struct RawCard {
    foot: [f32; 2],
    verts: Vec<f32>,
    tris: Vec<u32>,
}

/// A card as the file gives it, or `None` when its data cannot stand: an odd vertex list, no
/// triangle or a triangle list not in threes, or an index past the vertices.
fn checked(raw: RawCard) -> Option<Card> {
    if !raw.verts.len().is_multiple_of(2) || raw.tris.is_empty() || !raw.tris.len().is_multiple_of(3) {
        return None;
    }
    let n = raw.verts.len() / 2;
    let tris = raw.tris.iter().map(|&i| usize::try_from(i).ok().filter(|&i| i < n).and_then(|i| u16::try_from(i).ok())).collect::<Option<Vec<u16>>>()?;
    Some(Card { foot: (raw.foot[0], raw.foot[1]), verts: raw.verts.chunks_exact(2).map(|v| (v[0], v[1])).collect(), tris })
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

/// The bundled cards (`data/cards/objects.json`), parsed once.
pub fn builtin() -> &'static HashMap<u64, SpriteCards> {
    static CARDS: OnceLock<HashMap<u64, SpriteCards>> = OnceLock::new();
    CARDS.get_or_init(|| parse(include_str!("../../data/cards/objects.json")))
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

    /// The player's install; skipped without `RAZDOR_DT_DIR`. The keys of the bundled cards are
    /// the install's building sprites' keys, with their size.
    #[test]
    fn bundled_keys_match_the_installs_buildings() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let sprites = install.map_objects().expect("map objects decode");
        let buildings: Vec<&super::super::gfx::ObjectSprite> = sprites.sprites.iter().filter(|s| s.section == super::super::gfx::ObjectSprite::BUILDINGS).collect();
        let matched = buildings
            .iter()
            .filter(|s| builtin().get(&sprite_key(&s.image)).is_some_and(|k| k.w == s.image.width && k.h == s.image.height))
            .count();
        println!("{matched} of {} building sprites have cards", buildings.len());
        assert!(matched > 0, "no building sprite of the install has cut data");
    }

    #[test]
    fn bundled_cards_load() {
        let cards = builtin();
        assert!(!cards.is_empty());
        assert!(cards.values().all(|s| !s.cards.is_empty()));
    }
}
