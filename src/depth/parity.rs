//! Parity with the shipped cards. Runs only when `RAZDOR_DEPTH_MODEL` (the shape-fixed model file)
//! and `RAZDOR_DT_DIR` (the install) are set. Prints a table of five building sprites: the shipped
//! cards (Depth Anything V2 Large at 4 times the size, `data/cards/objects.json`) against this
//! engine's (Small at 266 px). The two models differ, so the counts and feet are reported, not
//! compared; the test checks only that the cards stay inside their sprite.
//!
//! Run: `RAZDOR_DEPTH_MODEL=<file> cargo test --release --lib depth::parity -- --nocapture`

use std::path::Path;
use std::time::Instant;

use super::cut::cut;
use super::{cards_for, DepthModel};
use crate::dt::cards::{builtin, sprite_key, SpriteCards};
use crate::dt::gfx::{decode_objects_ugs, ObjectSprite, MAP_OBJECTS};
use crate::dt::install::ENV_VAR;

fn feet(cards: &[crate::dt::cards::Card]) -> String {
    if cards.is_empty() {
        return "none (one card)".into();
    }
    let feet: Vec<String> = cards.iter().map(|c| format!("({:.1},{:.1})", c.foot.0, c.foot.1)).collect();
    format!("{}: {}", cards.len(), feet.join(" "))
}

#[test]
fn small_model_cards_against_the_shipped_ones() {
    let (Some(model_path), Some(dt_dir)) = (std::env::var_os("RAZDOR_DEPTH_MODEL"), std::env::var_os(ENV_VAR)) else {
        return;
    };
    let bytes = std::fs::read(Path::new(&dt_dir).join(MAP_OBJECTS)).expect("Objects.ugs is readable");
    let sprites = decode_objects_ugs(&bytes).expect("Objects.ugs decodes");
    let model = DepthModel::load(Path::new(&model_path)).expect("the model file loads");

    // Five building sprites that have shipped cards, spread through the list.
    let shipped_for = |s: &ObjectSprite| -> Option<&SpriteCards> {
        builtin().get(&sprite_key(&s.image)).filter(|c| c.w == s.image.width && c.h == s.image.height)
    };
    let with_cards: Vec<&ObjectSprite> = sprites
        .iter()
        .filter(|s| s.section == ObjectSprite::BUILDINGS && shipped_for(s).is_some())
        .collect();
    assert!(with_cards.len() >= 5, "the install has at least five building sprites with shipped cards");
    let picks: Vec<&ObjectSprite> = (0..5).map(|i| with_cards[i * (with_cards.len() - 1) / 4]).collect();

    // The first run pays for paging in the weights: not timed.
    let _ = model.depth(&picks[0].image);

    let cache_dir = std::env::temp_dir().join(format!("razdor-depth-parity-{}", std::process::id()));
    let mut times = Vec::new();
    println!("| key | size | shipped (Large, 4x) | small (266 px) | ms |");
    println!("|---|---|---|---|---|");
    for s in &picks {
        let (w, h) = (s.image.width, s.image.height);
        let shipped = shipped_for(s).expect("picked with shipped cards");

        let t = Instant::now();
        let depth = model.depth(&s.image);
        assert_eq!(depth.len(), (w * h) as usize, "one depth value per sprite pixel");
        let small = cut(&s.image, &depth);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        times.push(ms);

        // The cache gives back what the cut gave, and the second call does not run the model.
        let first = cards_for(&model, &s.image, Some(&cache_dir));
        let again = cards_for(&model, &s.image, Some(&cache_dir));
        assert_eq!(first, small, "cards_for matches the cut");
        assert_eq!(again, first, "the cache returns the cards it stored");

        for c in &small {
            assert!(c.foot.0 >= 0.0 && c.foot.0 < w as f32 && c.foot.1 >= 0.0 && c.foot.1 < h as f32, "a foot inside the sprite");
            for v in &c.verts {
                assert!(v.0 >= 0.0 && v.0 < w as f32 && v.1 >= 0.0 && v.1 < h as f32, "an outline vertex inside the sprite");
            }
        }
        println!(
            "| {:016x} | {}x{} | {} | {} | {:.0} |",
            sprite_key(&s.image),
            w,
            h,
            feet(&shipped.cards),
            feet(&small),
            ms
        );
    }
    let _ = std::fs::remove_dir_all(&cache_dir);

    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    println!(
        "model + cut per sprite: median {:.0} ms (min {:.0}, max {:.0}, n = {})",
        sorted[sorted.len() / 2],
        sorted[0],
        sorted[sorted.len() - 1],
        sorted.len()
    );
}
