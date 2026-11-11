//! The depth model's bookkeeping that needs no model: which building sprites still need a cut,
//! the input size each one runs at, the jobs and answers that cross the worker threads, and the
//! cards the atlas draws from the model. Pure Rust, no tract and no macroquad, so it is tested in
//! every build.

use std::collections::HashMap;

use crate::dt::cards::Card;
use crate::dt::gfx::Image;

/// A building picture as the atlas keys its sprites: (section, category, index).
pub type Id = (u32, u32, u32);

/// The model's input side for a small sprite. The file's own size.
pub const SMALL: usize = 266;
/// The input side for a big sprite: the same file, a second plan made on first use. The parity
/// run showed that bigger sprites lose their cards at [`SMALL`].
pub const LARGE: usize = 518;
/// A sprite whose longer side is at most this many pixels runs at [`SMALL`].
const SMALL_MAX_SIDE: u32 = 120;

/// The model's input side for a `w` x `h` sprite.
pub fn input_size(w: u32, h: u32) -> usize {
    if w.max(h) <= SMALL_MAX_SIDE {
        SMALL
    } else {
        LARGE
    }
}

/// What the atlas knows about one object sprite before the model runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Known {
    /// A building sprite: the only kind the model cuts.
    pub building: bool,
    /// Has shipped cut data of its size, or is a one-card building of the bundled list.
    pub shipped: bool,
    /// Has a cached cut, one card or more or none (an empty cut is a cut too).
    pub cached: bool,
}

/// The positions (in the order given) of the sprites the model must still cut: buildings with
/// neither shipped nor cached cut data.
pub fn unknown(sprites: impl IntoIterator<Item = Known>) -> Vec<usize> {
    sprites
        .into_iter()
        .enumerate()
        .filter(|(_, k)| k.building && !k.shipped && !k.cached)
        .map(|(i, _)| i)
        .collect()
}

/// A sprite for a worker: its picture and its pixels.
pub struct Job {
    pub id: Id,
    pub img: Image,
}

/// A worker's answer: the cards of the sprite, empty for one that stays one card.
pub struct Done {
    pub id: Id,
    pub cards: Vec<Card>,
}

/// The model's cards by picture, drawn in place of a billboard. Each picture is answered once and
/// the atlas lives for the whole run, so a picture's cards are leaked once (bounded by the install's
/// building sprites). An empty answer is no entry: that picture stays one billboard.
#[derive(Default)]
pub struct Overlay {
    cards: HashMap<Id, &'static [Card]>,
}

impl Overlay {
    /// The cards of a picture, if the model (or the cache) gave it some.
    pub fn get(&self, id: Id) -> Option<&'static [Card]> {
        self.cards.get(&id).copied()
    }

    /// Keeps the cards of one picture. Empty cards are not kept.
    pub fn insert(&mut self, id: Id, cards: Vec<Card>) {
        if !cards.is_empty() {
            self.cards.insert(id, Box::leak(cards.into_boxed_slice()));
        }
    }

    /// Keeps the cards of each answer that has any.
    pub fn add(&mut self, done: impl IntoIterator<Item = Done>) {
        for d in done {
            self.insert(d.id, d.cards);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(x: f32) -> Card {
        Card { foot: (x, 1.0), verts: vec![(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)], tris: vec![0, 1, 2], ground: false }
    }

    #[test]
    fn the_input_size_follows_the_longer_side() {
        assert_eq!(input_size(64, 50), SMALL);
        assert_eq!(input_size(120, 10), SMALL, "120 px is still small");
        assert_eq!(input_size(121, 10), LARGE);
        assert_eq!(input_size(10, 310), LARGE, "either side counts");
        assert_eq!(input_size(0, 0), SMALL);
    }

    #[test]
    fn the_queue_takes_only_unknown_buildings() {
        let k = |building, shipped, cached| Known { building, shipped, cached };
        let sprites = [
            k(true, false, false), // unknown building: queued
            k(false, false, false), // a decoration: never cut
            k(true, true, false), // shipped: known
            k(true, false, true), // cached (empty or not): known
            k(true, false, false), // unknown building: queued
        ];
        assert_eq!(unknown(sprites), vec![0, 4]);
        assert_eq!(unknown(std::iter::empty()), Vec::<usize>::new());
    }

    #[test]
    fn the_overlay_keeps_cards_and_leaves_an_empty_cut_as_one_billboard() {
        let mut overlay = Overlay::default();
        overlay.add([Done { id: (1, 2, 3), cards: vec![card(1.0), card(2.0)] }, Done { id: (1, 2, 4), cards: Vec::new() }]);
        let got = overlay.get((1, 2, 3)).expect("two cards");
        assert_eq!(got.len(), 2);
        assert_eq!(got[1].foot, (2.0, 1.0));
        assert!(overlay.get((1, 2, 4)).is_none(), "an empty answer is no entry");
        assert!(overlay.get((9, 9, 9)).is_none());
        overlay.insert((1, 2, 3), vec![card(5.0)]);
        assert_eq!(overlay.get((1, 2, 3)).map(|c| c[0].foot.0), Some(5.0), "a later answer replaces the first");
    }
}
