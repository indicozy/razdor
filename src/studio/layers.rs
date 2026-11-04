//! The Map workspace's layers (spec §5.4). A layer is a definition in data: an id, a name and a
//! kind. Each map keeps an instance of each layer, keyed by the id (LDtk's definitions and instances):
//! its eye (visible) and its lock. Only the fixed layers exist now; a user layer is one more
//! definition and one more instance, with no change to this model. Nothing is drawn here, and nothing
//! is saved: the instances live with the open map (the canvas keeps them).
//!
//! Click filtering: a hidden or locked layer takes no pick and no paint or erase; [`Set::first_pickable`]
//! picks the first of a stack of candidates that a visible, unlocked layer owns.
use std::collections::BTreeMap;

use crate::i18n::n_;

use super::map::Selection;

/// What a layer holds. The kind decides which picks and edits belong to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Terrain cells (paint, fill, select).
    Terrain,
    /// Decorative and blocking objects: hills, mountains, stones, plants (select, move, erase).
    Objects,
    /// Buildings, by footprint (select, place).
    Buildings,
    /// Army starts (select, place).
    Armies,
    /// Points: heroes, spawns, lanterns (select, place).
    Points,
    /// Trigger areas (`hero_enters`): not on the canvas yet.
    Triggers,
}

/// A layer's definition: its id (the instances are keyed by it), its name (a catalogue key) and kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Def {
    pub id: String,
    pub name: &'static str,
    pub kind: Kind,
}

/// The fixed layers, in the panel's order.
pub fn fixed() -> Vec<Def> {
    [
        ("terrain", n_("Terrain"), Kind::Terrain),
        ("objects", n_("Objects"), Kind::Objects),
        ("buildings", n_("Buildings"), Kind::Buildings),
        ("armies", n_("Armies"), Kind::Armies),
        ("points", n_("Points"), Kind::Points),
        ("triggers", n_("Trigger zones"), Kind::Triggers),
    ]
    .into_iter()
    .map(|(id, name, kind)| Def { id: id.to_string(), name, kind })
    .collect()
}

/// The id of the fixed layer a kind belongs to.
pub fn id_of(kind: Kind) -> &'static str {
    match kind {
        Kind::Terrain => "terrain",
        Kind::Objects => "objects",
        Kind::Buildings => "buildings",
        Kind::Armies => "armies",
        Kind::Points => "points",
        Kind::Triggers => "triggers",
    }
}

/// The layer a selection belongs to: an army, a building, a point or the hero (a point), a terrain
/// object.
pub fn layer_of(sel: &Selection) -> &'static str {
    match sel {
        Selection::Army(_) => id_of(Kind::Armies),
        Selection::Building(_) => id_of(Kind::Buildings),
        Selection::Point(_) | Selection::Hero => id_of(Kind::Points),
        Selection::Object(_) => id_of(Kind::Objects),
    }
}

/// One layer's instance on a map: its eye and its lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub visible: bool,
    pub locked: bool,
}

impl Default for State {
    fn default() -> State {
        State { visible: true, locked: false }
    }
}

/// The instances of one map: one state per layer id, and the active layer (the row the user last
/// picked; a pick prefers its objects when several layers are under the pointer).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Set {
    pub active: String,
    states: BTreeMap<String, State>,
}

impl Default for Set {
    fn default() -> Set {
        let states = fixed().into_iter().map(|d| (d.id, State::default())).collect();
        Set { active: id_of(Kind::Terrain).to_string(), states }
    }
}

impl Set {
    /// The state of a layer (a layer with no instance yet is visible and unlocked).
    pub fn state(&self, id: &str) -> State {
        self.states.get(id).copied().unwrap_or_default()
    }

    fn state_mut(&mut self, id: &str) -> &mut State {
        self.states.entry(id.to_string()).or_default()
    }

    pub fn visible(&self, id: &str) -> bool {
        self.state(id).visible
    }

    pub fn locked(&self, id: &str) -> bool {
        self.state(id).locked
    }

    /// Whether a layer takes clicks, paint and erase: visible and not locked.
    pub fn pickable(&self, id: &str) -> bool {
        let s = self.state(id);
        s.visible && !s.locked
    }

    /// Whether a pick of this selection is allowed (its layer is pickable).
    pub fn allows(&self, sel: &Selection) -> bool {
        self.pickable(layer_of(sel))
    }

    /// The first candidate a pickable layer owns (the candidates are in the order of the hit: top
    /// first). A hidden or locked layer's candidates are passed over.
    pub fn first_pickable(&self, candidates: &[Selection]) -> Option<Selection> {
        candidates.iter().find(|s| self.allows(s)).cloned()
    }

    /// The eye: shows or hides a layer.
    pub fn toggle_visible(&mut self, id: &str) {
        let s = self.state_mut(id);
        s.visible = !s.visible;
    }

    /// The lock: locks or unlocks a layer.
    pub fn toggle_locked(&mut self, id: &str) {
        let s = self.state_mut(id);
        s.locked = !s.locked;
    }

    /// Alt+click on an eye: shows only this layer. A second Alt+click, when it is the only one
    /// shown, shows them all again.
    pub fn solo(&mut self, id: &str, all: &[String]) {
        let only = all.iter().all(|other| other == id || !self.visible(other));
        for other in all {
            self.state_mut(other).visible = only || other == id;
        }
    }
}

/// The derived view of one map's layers: the instances keyed by the map's id (the canvas keeps one
/// per map, so switching maps keeps each map's eyes and locks).
pub type PerMap = BTreeMap<String, Set>;

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> Vec<String> {
        fixed().into_iter().map(|d| d.id).collect()
    }

    #[test]
    fn the_fixed_layers_have_unique_ids_and_match_their_kinds() {
        let defs = fixed();
        assert_eq!(defs.len(), 6);
        let mut seen: Vec<&str> = defs.iter().map(|d| d.id.as_str()).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 6, "ids are unique");
        for d in &defs {
            assert_eq!(id_of(d.kind), d.id, "{} is the id of its kind", d.name);
        }
    }

    #[test]
    fn a_pick_belongs_to_the_layer_of_its_kind() {
        assert_eq!(layer_of(&Selection::Army("a".into())), "armies");
        assert_eq!(layer_of(&Selection::Building("b".into())), "buildings");
        assert_eq!(layer_of(&Selection::Point(0)), "points");
        assert_eq!(layer_of(&Selection::Hero), "points");
        assert_eq!(layer_of(&Selection::Object(3)), "objects");
    }

    #[test]
    fn a_hidden_layer_is_never_picked() {
        let mut set = Set::default();
        set.toggle_visible("objects");
        let stack = [Selection::Object(2), Selection::Army("a".into())];
        assert_eq!(set.first_pickable(&stack), Some(Selection::Army("a".into())), "the object is passed over");
        set.toggle_visible("objects");
        assert_eq!(set.first_pickable(&stack), Some(Selection::Object(2)), "shown again, it is picked");
    }

    #[test]
    fn a_locked_layer_is_never_picked_either() {
        let mut set = Set::default();
        set.toggle_locked("armies");
        assert!(!set.pickable("armies"));
        assert!(set.visible("armies"), "lock and eye are separate");
        assert_eq!(set.first_pickable(&[Selection::Army("a".into())]), None);
        assert_eq!(set.first_pickable(&[Selection::Army("a".into()), Selection::Point(1)]), Some(Selection::Point(1)));
    }

    #[test]
    fn alt_click_on_an_eye_shows_only_that_layer_and_again_shows_all() {
        let all = ids();
        let mut set = Set::default();
        set.solo("buildings", &all);
        for id in &all {
            assert_eq!(set.visible(id), id == "buildings", "{id}: only buildings shown");
        }
        set.solo("buildings", &all);
        for id in &all {
            assert!(set.visible(id), "{id}: the second solo shows them all");
        }
        set.solo("armies", &all);
        set.toggle_visible("points");
        assert!(set.visible("armies") && set.visible("points") && !set.visible("buildings"), "a plain eye click adds one layer");
    }

    #[test]
    fn a_map_without_an_instance_has_every_layer_visible_and_unlocked() {
        let set = Set::default();
        assert_eq!(set.state("not-a-layer"), State::default());
        assert!(set.pickable("terrain"));
        assert_eq!(set.active, "terrain");
    }
}
