//! A battle's start state (replays, hole 8): everything `Game::start_battle` decides before the
//! battle is played, so that a battle can be rebuilt from its own record (a cut-out battle, a
//! replay's battle) and play out as the live one did. The spike rebuilt a battle from the two
//! unit lists alone and it differed in 2 of 8 army battles (a few HP): the missing field is the
//! enemy's building defence of an army that stands in a hostile building (`army_home`), which
//! `start_battle` sets and the spike did not. The patch globals are part of the start too.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::rules::ai;
use crate::rules::battle::{patch_globals, set_patch_globals, Battle, PatchGlobals, Team};
use crate::rules::content::Content;
use crate::rules::formation::Slot;
use crate::rules::game::{troop_unit, Foe, Game};
use crate::rules::units::Unit;

/// What a battle starts with (see the module's comment).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BattleStart {
    /// The battle's number in its game (`Game::battles` after the start).
    pub ordinal: u32,
    pub attacker: Team,
    /// The player's fighters: their squad index and the unit.
    pub player: Vec<(usize, Unit)>,
    /// The enemy's units, as `ai::army_units` or the garrison gives them.
    pub enemy: Vec<Unit>,
    /// The squad's cells that stay out of the battle (the reserve of its formation).
    pub bench: Vec<Slot>,
    pub xp_correction: i32,
    pub improved_ai: bool,
    pub max_ai: bool,
    pub max_guard: bool,
    pub god: bool,
    /// The enemy is a garrison whose strengths are bare (`set_bare_strengths`).
    pub bare_enemy: bool,
    /// The building defence of the player's side and of the enemy's (0: none).
    pub building_defence: [i32; 2],
    /// The patch globals the battle starts with.
    pub patch: PatchGlobals,
}

impl Game {
    /// The start of the battle `start_battle` would begin now, read without changing the game.
    /// Panics without a foe, as `start_battle` would have nothing to fight.
    pub fn battle_start(&self) -> BattleStart {
        let correction = match self.foe {
            Some(Foe::Army(i)) => self.world.armies[i].ai.exp_correction,
            _ => 100,
        };
        let (enemy, attacker, defence) = match self.foe {
            Some(Foe::Garrison(l)) => {
                let loc = &self.world.locations[l];
                (loc.garrison.iter().filter(|t| t.alive()).map(|t| troop_unit(&self.content, t)).collect(), Team::Player, loc.garrison_defence)
            }
            Some(Foe::Army(i)) => (ai::army_units(&self.content, &self.world.armies[i]), Team::Enemy, 0),
            None => (Vec::new(), Team::Player, 0),
        };
        let player: Vec<(usize, Unit)> = self
            .squad
            .iter()
            .enumerate()
            .filter(|(i, u)| *i == 0 || (u.alive() && !(self.allow_unit_benching && u.battle_disabled) && (attacker != Team::Player || !u.unpaid)))
            .map(|(i, u)| (i, u.clone()))
            .collect();
        let fighting: Vec<usize> = player.iter().map(|p| p.0).collect();
        let bench = (0..self.squad.len()).filter(|i| !fighting.contains(i)).map(|i| self.squad[i].slot).collect();
        let army_home = match self.foe {
            Some(Foe::Army(i)) => self
                .world
                .location_covering(self.world.armies[i].tile(&self.world.map))
                .map(|l| &self.world.locations[l])
                .filter(|l| l.hostile())
                .map_or(0, |l| l.garrison_defence),
            _ => 0,
        };
        BattleStart {
            ordinal: (self.battles + 1) as u32,
            attacker,
            player,
            enemy,
            bench,
            xp_correction: correction,
            improved_ai: self.improved_ai,
            max_ai: self.max_ai,
            max_guard: self.max_guard,
            god: self.cheats.god,
            bare_enemy: matches!(self.foe, Some(Foe::Garrison(l)) if self.world.locations[l].strengths_bare),
            building_defence: [self.hero_building_defence(), defence.max(army_home)],
            patch: patch_globals(),
        }
    }
}

impl BattleStart {
    /// The battle as `Game::start_battle` built it: the same calls in the same order. It sets the
    /// thread's patch globals to the start's (a battle reads them as they are when it begins).
    pub fn rebuild(&self, content: Arc<Content>) -> Battle {
        set_patch_globals(self.patch);
        let player: Vec<(usize, &Unit)> = self.player.iter().map(|(i, u)| (*i, u)).collect();
        let mut b = Battle::new(content, &player, &self.enemy, self.attacker);
        if self.bare_enemy {
            b.set_bare_strengths(Team::Enemy);
        }
        b.set_bench(self.bench.clone());
        b.set_xp_correction(self.xp_correction);
        b.set_improved_ai(self.improved_ai);
        b.set_max_ai(self.max_ai);
        b.set_max_guard(self.max_guard);
        b.god = self.god;
        if self.building_defence[1] > 0 {
            b.set_building_defence(Team::Enemy, self.building_defence[1]);
        }
        if self.building_defence[0] > 0 {
            b.set_building_defence(Team::Player, self.building_defence[0]);
        }
        b.auto_arrange(Team::Enemy);
        b.ordinal = self.ordinal;
        b
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use super::super::hook;
    use super::super::stream::{Entry, EngineInput, Input, PatchRecord, Stamp};
    use super::*;
    use crate::dt::dtm::Scenario;
    use crate::dt::install::DtInstall;
    use crate::rules::battle::{MaxPoll, MaxThinker, Outcome};
    use crate::rules::content::HeroClass;
    use crate::rules::save;

    fn base() -> Option<DtInstall> {
        crate::dt::install::load_dotenv();
        let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
        DtInstall::load(Path::new(&dir)).ok()
    }

    /// The map of `dt` with the largest area (and its scenario).
    fn biggest(dt: &DtInstall) -> (crate::dt::install::MapEntry, Scenario) {
        let mut best: Option<(u64, crate::dt::install::MapEntry, Scenario)> = None;
        for m in &dt.maps {
            let Ok(s) = m.load() else { continue };
            let area = s.width() as u64 * s.height() as u64;
            if best.as_ref().is_none_or(|b| area > b.0) {
                best = Some((area, m.clone(), s));
            }
        }
        let (_, m, s) = best.unwrap();
        (m, s)
    }

    /// A new game on the map, its event generator and patch globals fixed (a playback's start).
    fn fresh(c: &Arc<Content>, m: &crate::dt::install::MapEntry, s: &Scenario) -> Game {
        let start = Entry {
            at: Stamp::default(),
            input: Input::Engine(EngineInput::GameStart { load: false, seed: 5, patch: PatchRecord { battle: 0, hunger_counter: 3, hunger_seen: 3, screen_living: 0, side_blocks: [0, 0] } }),
        };
        hook::install_playback(&[start]);
        let mut g = Game::from_scenario(c.clone(), s, HeroClass::Knight);
        hook::take_playback();
        let (origin, bytes) = save::ScenarioRef::of_map(&m.path, &m.name).unwrap();
        g.set_map(origin, bytes);
        g.drain_events();
        g
    }

    /// The battle's fighters' state as the rules hold it, for a field-by-field comparison. The
    /// names are the interface's labels (`start_battle` sets them after the rules), so they are
    /// left out.
    fn fighters_text(b: &Battle) -> String {
        let text = format!("{:?}", b.fighters);
        let mut out = String::new();
        let mut rest = text.as_str();
        while let Some(at) = rest.find("name: \"") {
            out.push_str(&rest[..at]);
            let after = &rest[at + 7..];
            rest = after.find('"').map_or("", |end| &after[end + 1..]);
        }
        out.push_str(rest);
        out
    }

    /// The final state of a played battle: outcome, each fighter's HP and cell.
    fn final_state(b: &Battle) -> (Outcome, Vec<(i32, String)>) {
        (b.outcome(), b.fighters.iter().map(|f| (f.hp, format!("{:?}", f.slot))).collect())
    }

    #[test]
    fn a_rebuilt_army_battle_is_the_live_one_in_every_case() {
        let Some(dt) = base() else { return };
        let c = Arc::new(Content::from_dt(&dt));
        let (m, s) = biggest(&dt);
        let mut checked = 0;
        for i in 0..s.armies.len().min(8) {
            let mut g0 = fresh(&c, &m, &s);
            g0.foe = Some(Foe::Army(i));
            let start = g0.battle_start();
            let mut live = g0.start_battle();
            // The rebuilt battle, from the start alone, is the same before a move.
            let re = start.rebuild(c.clone());
            assert_eq!(fighters_text(&re), fighters_text(&live), "army {i}: the fighters at the start");
            assert_eq!(re.building_defence(Team::Enemy), live.building_defence(Team::Enemy), "army {i}: the enemy's building defence");
            assert_eq!(re.building_defence(Team::Player), live.building_defence(Team::Player), "army {i}: the player's building defence");
            let mut re = re;
            let outcome = live.auto_play_to_end();
            let back = re.auto_play_to_end();
            assert_eq!(final_state(&re), final_state(&live), "army {i}: the same battle to its end ({outcome:?})");
            assert_eq!(back, outcome);
            g0.resolve_battle(&live);
            checked += 1;
        }
        assert!(checked > 0, "the biggest map has armies");
        println!("REBUILT {checked} army battles match the live ones");
    }

    /// Hole 8's field, shown: the spike's rebuild gave an army in a hostile building no building
    /// defence on its side, and it fought another battle (armies 1 and 6 of the biggest map).
    #[test]
    fn the_enemy_building_defence_is_part_of_the_start() {
        let Some(dt) = base() else { return };
        let c = Arc::new(Content::from_dt(&dt));
        let (m, s) = biggest(&dt);
        let mut changed = Vec::new();
        for i in 0..fresh(&c, &m, &s).world.armies.len().min(8) {
            let mut g = fresh(&c, &m, &s);
            g.foe = Some(Foe::Army(i));
            let start = g.battle_start();
            let mut live = g.start_battle();
            live.auto_play_to_end();
            let mut without = start.clone();
            without.building_defence[1] = 0;
            let mut rebuilt = without.rebuild(c.clone());
            rebuilt.auto_play_to_end();
            if final_state(&rebuilt) != final_state(&live) {
                assert!(start.building_defence[1] > 0, "army {i} changes without a defence");
                changed.push(i);
            }
        }
        assert!(changed.len() >= 2, "the armies in a hostile building fight with its defence: {changed:?}");
    }

    #[test]
    fn a_battle_start_survives_its_bytes() {
        let Some(dt) = base() else { return };
        let c = Arc::new(Content::from_dt(&dt));
        let (m, s) = biggest(&dt);
        let mut g = fresh(&c, &m, &s);
        g.foe = Some(Foe::Army(0));
        let start = g.battle_start();
        let text = serde_json::to_string(&start).unwrap();
        let back: BattleStart = serde_json::from_str(&text).unwrap();
        assert_eq!(back, start);
        let (mut a, mut b) = (back.rebuild(c.clone()), start.rebuild(c.clone()));
        let (oa, ob) = (a.auto_play_to_end(), b.auto_play_to_end());
        assert_eq!(oa, ob);
        assert_eq!(final_state(&a), final_state(&b), "the same bytes give the same battle");
    }

    /// A battle the enemy plays by «Максимум» on the thinker (the real time search): its
    /// decisions are recorded, and the rebuilt battle plays them back from the records.
    fn play_max(b: &mut Battle) -> Outcome {
        let mut thinker = MaxThinker::new(Team::Enemy);
        for _ in 0..3000 {
            if b.outcome() != Outcome::Ongoing {
                break;
            }
            let Some(id) = b.active() else { break };
            if b.max_ai() && b.fighters[id].team == Team::Enemy {
                loop {
                    match thinker.poll(b) {
                        MaxPoll::Ready(r) => {
                            b.apply_max(r);
                            break;
                        }
                        MaxPoll::Thinking => std::thread::sleep(Duration::from_millis(1)),
                        MaxPoll::Failed => {
                            b.auto_step();
                            break;
                        }
                    }
                }
            } else {
                b.auto_step();
            }
        }
        b.outcome()
    }

    #[derive(Clone, Default)]
    struct Shared(Rc<RefCell<Vec<Entry>>>);

    impl hook::Recorder for Shared {
        fn record(&mut self, entry: Entry) {
            self.0.borrow_mut().push(entry);
        }
    }

    #[test]
    fn a_max_battle_rebuilds_from_its_start_and_its_records() {
        let Some(dt) = base() else { return };
        let c = Arc::new(Content::from_dt(&dt));
        let (m, s) = biggest(&dt);
        let mut g = fresh(&c, &m, &s);
        g.max_ai = true;
        g.foe = Some(Foe::Army(0));
        let start = g.battle_start();
        assert!(start.max_ai, "the battle is fought by «Максимум»");
        let shared = Shared::default();
        hook::install_recorder(Box::new(shared.clone()));
        let mut live = g.start_battle();
        live.begin();
        let outcome = play_max(&mut live);
        hook::take_recorder();
        let records: Vec<Entry> = shared.0.borrow().clone();
        let decisions = records.iter().filter(|e| matches!(e.input, Input::Engine(EngineInput::Max { .. }))).count();
        assert!(decisions > 0, "«Максимум» decided moves");
        assert!(records.iter().any(|e| matches!(e.input, Input::Engine(EngineInput::Patch(_)))), "the start's patch globals are recorded");

        // The rebuilt battle plays the same decisions from the records, not from a search.
        hook::install_playback(&records);
        let mut re = start.rebuild(c.clone());
        re.begin();
        let back = play_max(&mut re);
        hook::take_playback();
        assert_eq!(back, outcome);
        assert_eq!(final_state(&re), final_state(&live), "the rebuilt battle matches the live one");
        g.resolve_battle(&live);
    }
}
