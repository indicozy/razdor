//! The field battle seam of [`Game`] (`rules::field`). A meeting with an army whose battle is a
//! script's (`[[army]] battle = "lua:<script>"`, or a trigger's `start_battle` with `rules`) opens
//! a field battle instead of the DT one. `start_battle` then returns a placeholder Battle that
//! must not be played: the screen checks [`Game::field_active`] and drives the battle with the
//! `field_*` methods. When it is over, [`Game::field_finish`] hands the shadow to
//! [`Game::resolve_battle`], as a DT battle's end does.
//!
//! The shadow (`rules::field`) is rebuilt from the squads and the armies on every load; the
//! state (`Game::field`) is saved and resumed on top of it.

use std::collections::BTreeMap;

use crate::rules::ai;
use crate::rules::battle::{Battle, Team};
use crate::rules::field::{self, Action, FieldBattle, FieldScript, FieldState, Side, Snapshot};
use crate::rules::content::UnitId;
use crate::rules::units::Unit;

use super::{troop_unit, BattleResult, Foe, Game};

/// A field battle in play: its state's owner (the shadow) and its hooks.
pub(crate) struct FieldPlay {
    pub(crate) fb: FieldBattle,
    pub(crate) hooks: Box<dyn FieldScript>,
}

/// The script a `rules` string names: `lua:<name>`. Any other rules are the DT's.
pub fn script_of(rules: &str) -> Option<&str> {
    rules.strip_prefix("lua:").filter(|n| !n.is_empty())
}

/// The shadow of a battle and what the field needs to write back after each action.
pub(crate) struct FieldShadow {
    pub(crate) battle: Battle,
    /// The enemy's troop records its fighters came from, in order (`Game::battle_troops`).
    pub(crate) enemy_troops: Vec<usize>,
    /// The building the hero defends, and the troops of it that fight for him.
    pub(crate) garrison: Option<usize>,
    pub(crate) joined: Vec<usize>,
}

impl Game {
    /// Whether a field battle is under way: the screen drives it with the `field_*` methods, and
    /// the battle [`Game::start_battle`] returned is a placeholder.
    pub fn field_active(&self) -> bool {
        self.field.is_some()
    }

    /// The round of the field battle under way.
    pub fn field_round(&self) -> Option<u32> {
        self.field.as_ref().map(|f| f.round)
    }

    /// The side that has won, once the field battle is decided.
    pub fn field_outcome(&self) -> Option<Side> {
        self.field.as_ref().and_then(|f| f.outcome)
    }

    /// The board, the units and the turn, for the screen (and the scripts' view of them).
    pub fn field_snapshot(&mut self) -> Option<Snapshot> {
        let p = self.field_live()?;
        let active = p.fb.acting();
        Some(p.fb.snapshot(active))
    }

    /// The unit due to act, starting or ending rounds as the battle needs (saved as it goes);
    /// `None` once the battle is over.
    pub fn field_acting(&mut self) -> Option<usize> {
        let p = self.field_live()?;
        let f = p.fb.next_actor(&mut *p.hooks);
        self.field_store();
        f
    }

    /// The DT unit a battle unit is drawn as.
    pub fn field_kind(&mut self, unit: usize) -> Option<UnitId> {
        self.field_live()?.fb.shadow.fighters.get(unit).map(|f| f.unit)
    }

    /// The coming units (see [`FieldBattle::queue`]).
    pub fn field_queue(&mut self) -> Vec<usize> {
        self.field_live().map_or_else(Vec::new, |p| p.fb.queue())
    }

    /// Every action the unit `unit` may take now, when it is the one to act: the cells it can
    /// walk to with or without a blow or shot from each, the gates it can break (see
    /// [`FieldBattle::legal`]). Empty for any other unit.
    pub fn field_legal(&mut self, unit: usize) -> Vec<Action> {
        self.field_live().map_or_else(Vec::new, |p| p.fb.legal(unit))
    }

    /// The acting unit's action, as the player gives it. An invalid part is logged and left out.
    pub fn field_act(&mut self, action: Action) -> Result<(), String> {
        self.field_step(Some(action))
    }

    /// The acting unit's action by the AI (the script's `ai_turn`, else the Rust AI).
    pub fn field_auto_step(&mut self) -> Result<(), String> {
        self.field_step(None)
    }

    /// Plays the rest of the field battle on the AI and hands it to [`Game::resolve_battle`].
    pub fn field_play_to_end(&mut self) -> Option<BattleResult> {
        loop {
            let p = self.field_live()?;
            if p.fb.outcome().is_some() || self.field_step(None).is_err() {
                break;
            }
        }
        self.field_finish()
    }

    /// The last `lines` lines of the field battle's log, in `lang`: a package's text for the key
    /// when it has one, else the built-in text (`rules::field::text`).
    pub fn field_log(&self, lines: usize, lang: &str) -> Vec<String> {
        let Some(st) = &self.field else { return Vec::new() };
        let empty: BTreeMap<String, String> = BTreeMap::new();
        let texts = self.story.as_deref().map_or(&empty, |s| &s.texts);
        let from = st.log.len().saturating_sub(lines);
        st.log[from..].iter().map(|l| field::text::render(l, lang, texts)).collect()
    }

    /// The end of a decided field battle: its shadow goes to [`Game::resolve_battle`], as a DT
    /// battle's does (deaths, experience, loot, the campaign). `None` while it is undecided.
    pub fn field_finish(&mut self) -> Option<BattleResult> {
        self.field_live()?.fb.outcome()?;
        let p = self.field_play.take()?;
        self.field = None;
        self.field_garrison = None;
        self.field_joined.clear();
        self.battle_troops.clear();
        let shadow = p.fb.finish()?;
        Some(self.resolve_battle(&shadow))
    }

    /// Opens a field battle for the pending foe, when its rules are a script's and a host is
    /// attached. `None`: a DT battle (not a field one, or no script host: the DT battle goes on).
    pub(crate) fn field_start(&mut self, request: Option<String>) -> Option<Battle> {
        let script = match request {
            Some(r) => script_of(&r).map(str::to_string),
            None => self.field_script_for_foe(),
        }?;
        self.field_host.as_ref()?;
        // The same bookkeeping a DT battle starts with (`start_battle`).
        self.formation_before_battle = Some(self.squad.iter().map(|u| u.slot).collect());
        self.armies_snap();
        self.battles += 1;
        if let Err(e) = self.field_build(&script, None) {
            crate::diag::play(&self.clock.label(), &format!("FIELD battle «{script}» does not open: {e}; the DT battle is used"));
            self.field = None;
            self.field_play = None;
            return None;
        }
        Some(self.field_shadow().battle)
    }

    /// The script of the army the hero meets: the army's own `battle`.
    fn field_script_for_foe(&self) -> Option<String> {
        let Some(Foe::Army(i)) = self.foe else { return None };
        let id = self.world.armies.get(i)?.id;
        self.story.as_ref()?.battles.get(&id).and_then(|r| script_of(r)).map(str::to_string)
    }

    /// The seed of a new field battle: the story's seed (the package's, or the sim's `--seed`),
    /// the clock and the battle count. Never the wall clock.
    fn field_seed(&self) -> u64 {
        let story = self.story.as_ref().map_or(0, |s| s.seed as u64);
        (self.clock.total_minutes() as u64) ^ self.battles.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (story << 32)
    }

    /// Opens the hooks and the battle: a new one (set up, round 1) or the saved state resumed.
    fn field_build(&mut self, script: &str, state: Option<FieldState>) -> Result<(), String> {
        let host = self.field_host.clone().ok_or_else(|| "no script host is attached".to_string())?;
        let shadow = self.field_shadow();
        let (fb, hooks) = match state {
            Some(st) => {
                let hooks = host.open(script, st.script_rng)?;
                (FieldBattle::resume(shadow.battle, st)?, hooks)
            }
            None => {
                let seed = self.field_seed();
                let mut hooks = host.open(script, seed)?;
                let mut fb = FieldBattle::new(shadow.battle, script, seed, &mut *hooks);
                fb.start_round(&mut *hooks);
                (fb, hooks)
            }
        };
        self.battle_troops = shadow.enemy_troops;
        self.field_garrison = shadow.garrison;
        self.field_joined = shadow.joined;
        self.field = Some(fb.state.clone());
        self.field_play = Some(FieldPlay { fb, hooks });
        Ok(())
    }

    /// The live field battle, rebuilt from the saved state when there is none (after a load).
    pub(super) fn field_live(&mut self) -> Option<&mut FieldPlay> {
        if self.field_play.is_none() {
            let st = self.field.clone()?;
            let script = st.script.clone();
            self.field_build(&script, Some(st)).ok()?;
        }
        self.field_play.as_mut()
    }

    /// One action of the acting unit (the given one, or the AI's), then the round's end and the
    /// next round's start when they are due (so the state shows them at once); the state and the
    /// squads are updated.
    fn field_step(&mut self, action: Option<Action>) -> Result<(), String> {
        let p = self.field_live().ok_or("no field battle is under way")?;
        let out = match action {
            None => {
                if p.fb.auto_step(&mut *p.hooks) {
                    Ok(())
                } else {
                    Err("no unit is to act".into())
                }
            }
            Some(a) => match p.fb.next_actor(&mut *p.hooks) {
                Some(f) => p.fb.act(f, a, &mut *p.hooks),
                None => Err("no unit is to act".into()),
            },
        };
        p.fb.next_actor(&mut *p.hooks);
        self.field_store();
        out
    }

    /// The live battle's state into the save, and its HP into the squads (as a DT battle's
    /// write-back does), after every change the screen can see.
    fn field_store(&mut self) {
        let Some(mut p) = self.field_play.take() else { return };
        p.fb.sync(&*p.hooks);
        self.field = Some(p.fb.state.clone());
        self.field_write_back(&p);
        self.field_play = Some(p);
    }

    /// The shadow's HP into the squad and the enemy's troops (as a DT battle's write-back), and
    /// the joined garrison's into its troops.
    fn field_write_back(&mut self, p: &FieldPlay) {
        self.battle_write_back(&p.fb.shadow);
        let Some(l) = self.field_garrison else { return };
        if self.field_joined.is_empty() {
            return;
        }
        let now = self.clock.total_minutes() as u64;
        let c = self.content.clone();
        let hp: Vec<i32> = p.fb.shadow.fighters.iter().filter(|f| f.team == Team::Player && f.squad_index.is_none()).map(|f| f.hp).collect();
        for (&k, &hp) in self.field_joined.iter().zip(&hp) {
            if let Some(t) = self.world.locations.get_mut(l).and_then(|loc| loc.garrison.get_mut(k)) {
                ai::write_hp(&c, t, hp, now);
            }
        }
    }

    /// The shadow of the pending fight: the squad (as DT picks its fighters), the foe's units,
    /// and for an army met at a building the hero owns, that building's living garrison.
    pub(super) fn field_shadow(&self) -> FieldShadow {
        let c = self.content.clone();
        let attacker = if matches!(self.foe, Some(Foe::Garrison(_))) { Team::Player } else { Team::Enemy };
        let mut enemies: Vec<Unit> = Vec::new();
        let mut enemy_troops: Vec<usize> = Vec::new();
        let mut enemy_defence = 0;
        let mut correction = 100;
        match self.foe {
            Some(Foe::Garrison(l)) => {
                let loc = &self.world.locations[l];
                enemy_troops = (0..loc.garrison.len()).filter(|&k| loc.garrison[k].alive()).collect();
                enemies = loc.garrison.iter().filter(|t| t.alive()).map(|t| troop_unit(&c, t)).collect();
                enemy_defence = loc.garrison_defence;
            }
            Some(Foe::Army(i)) => {
                let a = &self.world.armies[i];
                enemy_troops = (0..a.troops.len()).filter(|&k| a.troops[k].alive()).collect();
                enemies = ai::army_units(&c, a);
                correction = a.ai.exp_correction;
            }
            None => {}
        }
        // The hero's own building (or a friend's) fights for him, and its living garrison joins him.
        let mut garrison = None;
        let mut joined = Vec::new();
        let mut joiners: Vec<Unit> = Vec::new();
        if let (Some(Foe::Army(_)), Some(l)) = (self.foe, self.field_defended_building()) {
            for (k, t) in self.world.locations[l].garrison.iter().enumerate() {
                if t.alive() {
                    joiners.push(troop_unit(&c, t));
                    joined.push(k);
                }
            }
            garrison = Some(l);
        }
        let player: Vec<(usize, &Unit)> = self
            .squad
            .iter()
            .enumerate()
            .filter(|(i, u)| *i == 0 || (u.alive() && !(self.allow_unit_benching && u.battle_disabled) && (attacker != Team::Player || !u.unpaid)))
            .collect();
        let mut battle = field::shadow_of(c.clone(), &player, &joiners, &enemies, attacker);
        // Names as a DT battle gives them (`start_battle`).
        let (mut jn, mut en) = (0, 0);
        for f in battle.fighters.iter_mut() {
            match (f.squad_index, f.team) {
                (Some(i), _) => f.name = self.squad_label(i),
                (None, Team::Player) => {
                    f.name = self.unit_label(&joiners[jn]);
                    jn += 1;
                }
                (None, Team::Enemy) => {
                    f.name = self.unit_label(&enemies[en]);
                    en += 1;
                }
            }
        }
        battle.set_xp_correction(correction);
        if enemy_defence > 0 {
            battle.set_building_defence(Team::Enemy, enemy_defence);
        }
        let own = self.hero_building_defence();
        if own > 0 {
            battle.set_building_defence(Team::Player, own);
        }
        FieldShadow { battle, enemy_troops, garrison, joined }
    }

    /// The building the hero defends when an army meets him there: one he owns or a friend's
    /// (the same test as [`Game::hero_building_defence`]).
    fn field_defended_building(&self) -> Option<usize> {
        let here = self.location.or_else(|| self.world.location_covering(self.tile()))?;
        let l = &self.world.locations[here];
        (l.owned() || l.attitude > 0).then_some(here)
    }
}


#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::pack::files::Files;
    use crate::pack::load::Package;
    use crate::pack::open::test_support;
    use crate::pack::terrain::{encode_rgb, DEFAULT_PALETTE};
    use crate::rules::content::{Content, HeroClass};

    /// A package with one map `m`: the hero (militiamen) and an army of bandits `e` at (30, 30)
    /// whose battle is `lua:field`, and the script `field.lua` (`script`).
    fn package(script: &str) -> Package {
        let manifest = "id = \"field-test\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"story.title\"\nauthor = \"Razdor\"\nlicense = \"MIT\"\n";
        let map = "title = \"map.title\"\nstart_date = \"1300-04-01\"\nrelations = [[3,3,0,-3],[3,3,0,-3],[0,0,3,-3],[-3,-3,-3,3]]\n\n[hero]\ncell = [5, 5]\nclasses = [\"knight\"]\ngold = 100\ntroops = [{ unit = \"dt:militiaman\", level = 1, count = 3 }]\n\n[[army]]\nid = \"e\"\nname = \"army.e\"\ncell = [30, 30]\nfaction = \"enemy\"\nmodel = \"bandits\"\nbattle = \"lua:field\"\ntroops = [{ unit = \"dt:militiaman\", level = 1, count = 2 }]\n\n[victory]\nkind = \"reach\"\ncell = [45, 45]\nmessage = \"win.message\"\n";
        let mut f = Files::default();
        f.insert("package.toml", manifest);
        f.insert("texts/en.toml", "[story]\ntitle = \"Field test\"\n\n[map]\ntitle = \"Map\"\n\n[win]\nmessage = \"Won\"\n\n[army]\ne = \"Bandits\"\n");
        f.insert("maps/m/map.toml", map);
        f.insert("maps/m/terrain.png", encode_rgb(50, 50, |_, _| DEFAULT_PALETTE[6].1));
        f.insert("scripts/field.lua", script);
        Package::parse(f).0
    }

    /// The game of the package's map, started, with the army `e` as the foe.
    fn game(script: &str) -> Game {
        let pkg = package(&format!("function setup(s) end\nfunction check_end(s) return nil end\n{script}"));
        let o = test_support::opened(&pkg);
        let content = Arc::new(Content::builtin().with_package(o.units.clone(), o.items.clone()));
        let mut g = Game::from_scenario(content, &o.scenario, HeroClass::Knight);
        o.start(&mut g);
        g.foe = Some(Foe::Army(0));
        g
    }

    const ENDS_AT_ROUND_3: &str = "function check_end(s)\n  if s.round >= 3 then return \"attackers\" end\n  return nil\nend\n";

    #[test]
    fn a_rules_string_names_a_script_only_with_the_lua_prefix() {
        assert_eq!(script_of("lua:siege"), Some("siege"));
        assert_eq!(script_of("lua:"), None);
        assert_eq!(script_of("dt"), None);
    }

    #[test]
    fn a_scripted_army_opens_a_field_battle_that_ends_in_resolve_battle() {
        let mut g = game(ENDS_AT_ROUND_3);
        let placeholder = g.start_battle();
        assert!(g.field_active(), "the army's script opens a field battle");
        assert_eq!(g.field_round(), Some(1));
        assert!(g.field_snapshot().is_some());
        assert!(placeholder.fighters.len() >= 2, "the placeholder names both sides");
        let result = g.field_play_to_end();
        assert!(result.is_some(), "a decided field battle resolves");
        assert!(!g.field_active(), "the field battle is over");
        assert!(g.foe.is_none(), "the foe is settled, as after a DT battle");
    }

    #[test]
    fn a_save_taken_mid_battle_resumes_to_the_same_log() {
        let mut straight = game(ENDS_AT_ROUND_3);
        straight.start_battle();
        while straight.field_outcome().is_none() {
            straight.field_auto_step().unwrap();
        }
        let whole = straight.field.clone().unwrap().log;

        let mut g = game(ENDS_AT_ROUND_3);
        g.start_battle();
        for _ in 0..3 {
            g.field_auto_step().unwrap();
        }
        let saved = serde_json::to_string(&g.field).unwrap();
        // A load: the saved state is back, the live battle is not (rebuilt from it).
        g.field = serde_json::from_str(&saved).unwrap();
        g.field_play = None;
        assert!(g.field_active());
        while g.field_outcome().is_none() {
            g.field_auto_step().unwrap();
        }
        assert_eq!(g.field.clone().unwrap().log, whole, "the resumed fight is the same fight");
    }

    #[test]
    fn a_trigger_request_for_dt_beats_the_armys_script() {
        let mut g = game(ENDS_AT_ROUND_3);
        g.start_field_battle(1, "dt");
        let b = g.start_battle();
        assert!(!g.field_active(), "a DT battle by the trigger's request");
        assert!(b.fighters.len() >= 2);
        assert!(g.field_request.is_none(), "the request is taken by the battle's start");
    }

    #[test]
    fn a_failing_script_ends_the_field_battle_and_settles_it() {
        let mut g = game("function ai_turn(s)\n  error('nope')\nend\n");
        g.start_battle();
        let result = g.field_play_to_end();
        assert!(result.is_some());
        assert!(!g.field_active());
    }

    #[test]
    fn the_log_reads_in_the_language_of_the_package_first() {
        let mut g = game(ENDS_AT_ROUND_3);
        g.start_battle();
        g.field_auto_step().unwrap();
        let en = g.field_log(5, "en");
        assert!(en.iter().any(|l| l.starts_with("turn 1")), "{en:?}");
        let ru = g.field_log(5, "ru");
        assert!(ru.iter().any(|l| l.starts_with("ход 1")), "{ru:?}");
    }
}
