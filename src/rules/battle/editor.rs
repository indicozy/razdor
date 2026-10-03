//! The map editor's battle engine as a ruleset of the game's (docs/reference/editor/testers.md
//! §4). DTMapEdit carries its own copy of the battle core, the pre-Community code: none of
//! the Community bonuses, Evasion, the Garrison fix or the mana floors of the patch; four
//! bonuses of its own (ids 22–25); poison −15 on a set of natures; narrower nature sets for
//! vampirism, the Life blessing and the Death heal; the AI's melee term with the right sign;
//! the knight's 90 %; the vanilla mana drain; and the battle tester's five rule switches,
//! which in the game are fixed. The battle tester and the AI viewer run it; the game never
//! does ([`Rules::Game`] is every game battle's).

use super::*;

/// The editor's knight damage share (0x5bb160, never loaded from the ini).
pub(super) const EDITOR_KNIGHT_PERCENT: i32 = 90;
/// The regeneration the editor's Poison sets (0x50293f, 0x502b57); the game's is −20.
const EDITOR_POISON_REGEN: i32 = -15;
/// The vanilla mana floors (the game's dead block 48446d): Life and Elemental 15, an undead
/// Death caster 25.
const VANILLA_FLOOR: i32 = 15;
const VANILLA_UNDEAD_DEATH_FLOOR: i32 = 25;

/// The battle tester's rule switches (testers.md §2.4, 0x5bb114–0x5bb124). The defaults
/// are the game's fixed values (4ed384–4ed394); only the tester's option panel changes them,
/// and they last for the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Switches {
    /// Counterblow units hit back after melee.
    pub counterblow: bool,
    /// Shooters and casters reach only the enemy cells in columns c−1..c+1 of rows 1–2.
    pub short_range: bool,
    /// Empty front rows collapse after deaths; off, the pull (map code 3) comes back.
    pub collapse: bool,
    /// A front-row warrior with nothing adjacent may strike the nearest enemy front unit;
    /// also switches the AI's retreat between the edge rule and the column scores.
    pub long_strike: bool,
    /// Every action also costs 1 current initiative.
    pub initiative_cost: bool,
}

impl Default for Switches {
    fn default() -> Switches {
        Switches { counterblow: true, short_range: false, collapse: true, long_strike: true, initiative_cost: false }
    }
}

/// Which battle engine a battle runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rules {
    /// The game's (Community) rules: every battle of the game.
    #[default]
    Game,
    /// The map editor's battle engine with these switches.
    Editor(Switches),
}

/// The editor's bonuses 22–25 (names at 0x5baf1c), which the game does not have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorBonus {
    /// ×2/3 physical damage; untouchable on turn 1 while it has actions; its vampiric heal
    /// is not capped and a kill gives it an action.
    OldVampiressGist,
    /// No battle effect found (testers.md §8).
    Chatty,
    /// Turn 1: the enemy front units in its column lose their actions.
    Terrible,
    /// A pass raises a guard: melee damage on it is 1 until it acts or is hit in melee.
    Parrying,
}

impl EditorBonus {
    pub fn of_id(id: u8) -> Option<EditorBonus> {
        match id {
            22 => Some(EditorBonus::OldVampiressGist),
            23 => Some(EditorBonus::Chatty),
            24 => Some(EditorBonus::Terrible),
            25 => Some(EditorBonus::Parrying),
            _ => None,
        }
    }
}

/// The editor's nature set {Normal, Hero} (0x5bafa0): vampirism, the Life blessing's effect,
/// and the natures its Death heal cannot help.
pub(super) fn vampire_nature(n: Nature) -> bool {
    matches!(n, Nature::Normal | Nature::Hero)
}

/// The natures the editor's Poison works on, {0, 3, 4, 5, 6} (0x5bafb0): not Undead or
/// Elemental.
fn poison_nature(n: Nature) -> bool {
    !matches!(n, Nature::Undead | Nature::Elemental)
}

impl Battle {
    /// The rules this battle runs.
    pub fn rules(&self) -> Rules {
        self.rules
    }

    /// The switches in force: the game's fixed values in a game battle.
    pub(super) fn switches(&self) -> Switches {
        match self.rules {
            Rules::Game => Switches::default(),
            Rules::Editor(s) => s,
        }
    }

    /// The strength field of `team`'s units in list order (the battle tester's gold costs),
    /// for the auto-arrange and the side strengths. Set before [`Battle::begin`].
    pub fn set_values(&mut self, team: Team, values: &[i32]) {
        let ids: Vec<usize> = (0..self.fighters.len()).filter(|&i| self.fighters[i].team == team).collect();
        for (&i, &v) in ids.iter().zip(values) {
            self.fighters[i].value = Some(v);
        }
    }

    /// The vanilla mana drain from turn 2 (StartTurn 0x4f6510): the school's DecSpell…, then
    /// Life floor 15; Elemental floor 15, but 0 for an Undead or Rogue caster that went below
    /// 0; Death floor 25 for an Undead caster only; never below 0. A caster with no school
    /// has no drain.
    pub(super) fn editor_drain(&mut self, i: usize) {
        let o = &self.content.options;
        let f = &self.fighters[i];
        let Some(school) = f.stats.magic else { return };
        let nature = f.base.nature;
        let mut p = f.power.wrapping_sub(o.dec_spell(school));
        match school {
            MagicSchool::Life => p = p.max(VANILLA_FLOOR),
            MagicSchool::Elemental if p < 0 && matches!(nature, Nature::Undead | Nature::Rogue) => p = 0,
            MagicSchool::Elemental => p = p.max(VANILLA_FLOOR),
            MagicSchool::Death if nature == Nature::Undead => p = p.max(VANILLA_UNDEAD_DEATH_FLOOR),
            MagicSchool::Death => {}
        }
        self.fighters[i].power = p.max(0);
    }

    /// Terrible, at the end of turn 1's start (0x4f6510): each enemy front-row unit in the
    /// column of a front-row Terrible unit loses all its actions.
    pub(super) fn terrible(&mut self) {
        let fronts: Vec<(Team, u8)> = self
            .fighters
            .iter()
            .filter(|f| f.standing() && f.extra == Some(EditorBonus::Terrible) && f.slot.row == Row::Front)
            .map(|f| (f.team, f.slot.col))
            .collect();
        for (team, col) in fronts {
            for f in self.fighters.iter_mut().filter(|f| f.listed() && f.team != team && f.slot == Slot::new(Row::Front, col)) {
                f.actions = 0;
            }
        }
    }

    /// The editor's Poison (0x50293f, 0x502b57): a hit above 1 sets −15 on a target that is
    /// not Undead or Elemental.
    pub(super) fn editor_poison(&mut self, id: usize, target: usize, raw: i32) {
        if raw > 1 && self.fighters[id].has(Bonus::Poison) && poison_nature(self.fighters[target].stats.nature) {
            self.fighters[target].regen = EDITOR_POISON_REGEN;
        }
    }

    /// The editor's vampirism after a melee hit or a Death strike (0x501054): only on Normal
    /// and Hero targets. An OldVampiressGist's heal is not capped at its max HP and a kill
    /// gives it one more action (the cap sits in the other branch: the original's).
    pub(super) fn editor_vampirism(&mut self, id: usize, target: usize, raw: i32) {
        if !vampire_nature(self.fighters[target].stats.nature) {
            return;
        }
        let killed = !self.fighters[target].alive();
        let f = &mut self.fighters[id];
        let heal = raw.wrapping_mul(f.stats[Stat::Vampirizm]) / 100;
        if f.extra == Some(EditorBonus::OldVampiressGist) {
            f.hp = f.hp.wrapping_add(heal);
            if killed {
                f.actions += 1;
            }
        } else {
            f.hp = f.hp.wrapping_add(heal).min(f.max_hp());
        }
    }

    /// The editor's melee after the damage (0x501054): Poison, vampirism, then the target's
    /// Parrying guard drops.
    pub(super) fn editor_melee(&mut self, id: usize, target: usize, raw: i32) {
        self.editor_poison(id, target, raw);
        self.editor_vampirism(id, target, raw);
        self.fighters[target].guard = false;
    }

    /// The enemy back-row unit fighter `id` could pull into its front row (map code 3, only
    /// with the collapse switch off, 0x4f7f83): the one in its own column, from the front
    /// row. The original marks the enemy front cell of that column whether it is empty or
    /// not; a pull onto an occupied cell would put two units on one cell, so Razdor offers it
    /// only when the cell is empty.
    pub fn pull_target(&self, id: usize) -> Option<usize> {
        let f = &self.fighters[id];
        if self.switches().collapse || !f.standing() || f.slot.row != Row::Front {
            return None;
        }
        let enemy = f.team.other();
        let front = Slot::new(Row::Front, f.slot.col);
        if self.at(enemy, front).is_some() || !self.is_open(enemy, front) {
            return None;
        }
        self.at(enemy, Slot::new(Row::Back, f.slot.col)).filter(|&t| self.fighters[t].standing())
    }

    /// The active fighter pulls `target` (its [`Battle::pull_target`]) into the enemy front
    /// row, same column; one action. `free`: the AI's, which first gives the actor the
    /// action back (0x4fa04e), so its pull costs nothing.
    pub(super) fn pull(&mut self, target: usize, free: bool) -> Result<(), ActionError> {
        let id = self.active().ok_or(ActionError::NotYourTurn)?;
        if self.pull_target(id) != Some(target) {
            return Err(ActionError::InvalidTarget);
        }
        if free {
            self.fighters[id].actions += 1;
        }
        if self.start_action(id) {
            self.fighters[target].slot.row = Row::Front;
            let msg = crate::trf!("{name} pulls {tname} forward", name = self.fighters[id].name, tname = self.fighters[target].name);
            self.log.push(msg);
        }
        self.finish_action(id);
        Ok(())
    }

    /// The player's pull, of a click on the empty enemy front cell (the tester's).
    pub fn pull_active(&mut self, target: usize) -> Result<(), ActionError> {
        self.pull(target, false)
    }

    /// The AI's pull (head of 0x4f9f40, collapse switch off): a front-row unit with no
    /// melee target pulls the enemy back-row unit of its column when that one is not a
    /// warrior.
    pub(super) fn ai_pull(&self, id: usize) -> Option<Plan> {
        let t = self.pull_target(id)?;
        if self.fighters[t].ai_role == AiRole::Warrior {
            return None;
        }
        let melee = self.all_options(id).iter().any(|o| o.1.is_melee() && self.fighters[o.0].team != self.fighters[id].team);
        (!melee).then_some(Plan::Pull(t))
    }

    /// The AI's retreat scores with the long-strike switch off (the block 0x4fa52e–0x4fa745
    /// in place of the edge rule): every front cell of the own side starts at 1000, and one
    /// the unit can step to, its own cell and an ally's cell it can tend lose the power of
    /// each enemy front unit in columns c−1..c+1; each back cell gets its column's value on
    /// top of its retreat score, and the unit's own front cell 1 more. The cells need not
    /// exist (the picker does not ask): a pick without a code of its own spends the action.
    pub(super) fn retreat_scores(&self, id: usize, cands: &mut Vec<(Slot, f64, Slot)>) {
        let f = &self.fighters[id];
        let cols = self.formation.cols;
        let moves = self.moves(id);
        let tends = |s: Slot| self.at(f.team, s).is_some_and(|t| t != id && self.option_at(id, f.slot, t).is_some_and(|k| !k.is_hostile()));
        let value = |c: u8| {
            let s = Slot::new(Row::Front, c);
            let coded = moves.contains(&s) || s == f.slot || tends(s);
            if !coded {
                return 1000;
            }
            let near = (c.saturating_sub(1)..=(c + 1).min(cols - 1)).filter_map(|k| self.at(f.team.other(), Slot::new(Row::Front, k)));
            1000 - near.map(|e| self.fighters[e].ai_power as i64).sum::<i64>()
        };
        for c in 0..cols {
            let v = value(c);
            let back = Slot::new(Row::Back, c);
            match cands.iter_mut().find(|x| x.0 == back) {
                Some(x) => x.1 += v as f64,
                None => cands.push((back, v as f64, back)),
            }
            let front = Slot::new(Row::Front, c);
            cands.push((front, (v + i64::from(front == f.slot)) as f64, front));
        }
    }

    /// The AI's level (battle B+8): 0, nobody gets the full killable test; 1, side 1 (the
    /// player's side) does; 2, both.
    pub fn set_ai_level(&mut self, level: u8) {
        self.ai_level = level;
    }

    /// HP `team` has lost through the damage routine (side +0xC).
    pub fn hp_lost(&self, team: Team) -> i64 {
        self.side_lost(team)
    }

    /// The cell the AI would act on with the active fighter: a target's, a step's, the own
    /// cell for a pass, the empty enemy front cell for a pull.
    pub fn ai_target_cell(&self) -> Option<(Team, Slot)> {
        let id = self.active()?;
        let f = &self.fighters[id];
        Some(match self.ai_plan()? {
            Plan::Act(t, _) => (self.fighters[t].team, self.fighters[t].slot),
            Plan::Move(s) => (f.team, s),
            Plan::Pass => (f.team, f.slot),
            Plan::Pull(t) => (self.fighters[t].team, Slot::new(Row::Front, self.fighters[t].slot.col)),
        })
    }

    /// The editor's battle side of an army built at less than full strength (0x58af74,
    /// `fullStrength` 0): its unpaid units fight, with melee, ranged and magic attack ×3/4
    /// (×3, then shifted right by 2) and their base initiative halved. `unpaid` follows
    /// `team`'s units in list order. Before [`Battle::begin`].
    pub fn weaken_unpaid(&mut self, team: Team, unpaid: &[bool]) {
        let ids: Vec<usize> = (0..self.fighters.len()).filter(|&i| self.fighters[i].team == team).collect();
        for (&i, _) in ids.iter().zip(unpaid).filter(|(_, &u)| u) {
            let f = &mut self.fighters[i];
            for st in [Stat::AttackBlow, Stat::AttackShot, Stat::MagicPower] {
                f.base[st] = f.base[st].wrapping_mul(3) >> 2;
            }
            f.power = f.power.wrapping_mul(3) >> 2;
            f.base[Stat::Initiative] /= 2;
            f.at_start = f.base.clone();
            self.refresh(i);
        }
    }
}
