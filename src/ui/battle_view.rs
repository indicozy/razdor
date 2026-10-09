//! The battle screen, laid out as the original's (video notes §3, refs 09 and 10): a red
//! marble window over the map titled with both armies, the acting (or hovered) unit's panel
//! on the left, and on the right the enemy's formation on top, a hint strip, and the
//! player's formation below, front rows facing each other in the middle. Each card is the
//! unit's portrait with the original's stat strip ("A: 45 D: 35/40 / Mnvr: 1 Ini: 12 /
//! Hits: 70"); empty cells show where each kind of unit belongs. The acting card has a green
//! frame, cells it can step to blue ones and its targets red (hostile, green under the
//! mouse) or blue (friendly).
//! Hovering a target previews the action ("Click to curse X" / "Initiative -5 Actions -1");
//! a click does it. As in the original each cell has one action: a hostile mage curses a
//! target without a negative modifier and strikes the others, a friendly one heals the
//! wounded and blesses the rest.

use std::collections::VecDeque;
use std::f32::consts::PI;

use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::rules::battle::{ActionKind, Battle, EndReason, Fighter, Hit, Outcome, Preview, Step, Team, XpAward};
use razdor::av::Echo;
use razdor::rules::content::{HeroClass, ItemId, MagicSchool, Stat};
use razdor::rules::custom::{Control, Session};
use razdor::rules::formation::{Row, Slot};
use razdor::rules::game::{BattleResult, Foe, Game};

use super::assets::Assets;
use super::audio::{cue, Cue};
use super::chrome::{self, shadow_centered, shadow_right, CellIcon, Skin, BLUE_TEXT, CREAM, GOLD, RED_TEXT};
use super::dialog::Dialog;
use super::unit_sheet::{self, Sheet};
use super::widgets::*;
use super::world_view;
use super::Screen;

const AI_DELAY: f32 = 0.45;
const STRIKE_TIME: f32 = 0.7;
const MOVE_TIME: f32 = 0.25;
const ACTIVE: Color = Color::new(0.35, 1.0, 0.35, 1.0);
const FRIENDLY: Color = Color::new(0.35, 0.55, 1.0, 1.0);
const HOSTILE: Color = Color::new(1.0, 0.35, 0.35, 1.0);
/// The paces of the watched battle.
const SPEEDS: [u8; 3] = [1, 2, 4];

thread_local! {
    /// The pace last chosen for watching: the next battle watches at it too.
    static SPEED: std::cell::Cell<u8> = const { std::cell::Cell::new(1) };
}

/// How a custom battle's result box is left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomEnd {
    /// The same armies again.
    Again,
    /// Back to the setup.
    Setup,
    MainMenu,
}

/// Window, panel and grid geometry, in screen pixels (from the 960×720 reference × `k`).
#[derive(Clone, Copy)]
struct Layout {
    k: f32,
    win: Rect,
    panel: Rect,
    /// Under the panel: the quick battle buttons.
    quick: Rect,
    strip: Rect,
    card: Vec2,
    pitch: Vec2,
    grid_x: f32,
    formation: razdor::rules::formation::Formation,
}

impl Layout {
    fn new(battle: &Battle) -> Layout {
        let k = chrome::k();
        let (sw, sh) = (screen_width(), screen_height());
        let (ww, wh) = ((836.0 * k).round(), (600.0 * k).round());
        let x = ((sw - ww) / 2.0).round();
        let y = ((sh - chrome::bar_height() - wh) / 2.0).max(2.0).round();
        let win = Rect::new(x, y, ww, wh);
        let panel = Rect::new(x + 2.0 * k, y + 27.0 * k, 244.0 * k, 534.0 * k);
        let quick = Rect::new(panel.x + 10.0 * k, panel.y + panel.h + 3.0 * k, panel.w - 20.0 * k, 30.0 * k);
        let (rx, rw) = (x + 248.0 * k, 586.0 * k);
        let strip = Rect::new(rx, y + 302.0 * k, rw, 20.0 * k);
        let f = battle.formation;
        let lines = f.display_lines() as f32;
        let c = 1.0f32.min(2.0 / lines).min(6.0 / f.display_cols() as f32);
        let card = vec2(88.0 * c * k, 128.0 * c * k).round();
        let pitch = vec2(96.0 * c * k, 133.0 * c * k);
        let grid_w = f.display_cols() as f32 * pitch.x - 8.0 * c * k;
        Layout { k, win, panel, quick, strip, card, pitch, grid_x: (rx + (rw - grid_w) / 2.0).round(), formation: f }
    }

    /// Top-left of a cell. Display line 0 is the front row: the enemy's is just above the
    /// strip, the player's just below it.
    fn cell_pos(&self, team: Team, slot: Slot) -> Vec2 {
        let (line, col) = self.formation.display(slot);
        let x = self.grid_x + col as f32 * self.pitch.x;
        let y = match team {
            Team::Enemy => self.strip.y - 9.0 * self.k - self.card.y - line as f32 * self.pitch.y,
            Team::Player => self.strip.y + self.strip.h + 10.0 * self.k + line as f32 * self.pitch.y,
        };
        vec2(x, y).round()
    }

    fn portrait(&self, p: Vec2) -> Rect {
        Rect::new(p.x, p.y, self.card.x, self.card.x)
    }
}

/// An action being animated.
enum FxKind {
    /// The actor lunges, the target shows the hit's animation and number; then, for a
    /// counterblow, the target lunges back and the actor shows the same animation, or, for a
    /// `DeathCurse` death of the killer, the actor shows the sorcery (`echo`, 0x4c43e8).
    Act { hit: Hit, echo: Option<Echo>, echo_cued: bool },
    /// The actor slides between two cells.
    Move { from: Vec2, to: Vec2 },
    /// A pass: a short pause (0x4afb54).
    Pass,
}

struct Fx {
    actor: usize,
    kind: FxKind,
    t: f32,
}

impl Fx {
    fn act(battle: &Battle, actor: usize, hit: Hit) -> Fx {
        let echo = razdor::av::echo(battle, actor, &hit);
        Fx { actor, kind: FxKind::Act { hit, echo, echo_cued: false }, t: 0.0 }
    }

    fn duration(&self) -> f32 {
        match self.kind {
            FxKind::Act { echo: Some(_), .. } => 2.0 * STRIKE_TIME,
            FxKind::Act { .. } => STRIKE_TIME,
            FxKind::Move { .. } => MOVE_TIME,
            FxKind::Pass => razdor::av::BATTLE_PASS_MS as f32 / 1000.0,
        }
    }

    /// The echo's half of an action is playing.
    fn echoing(&self) -> bool {
        matches!(self.kind, FxKind::Act { echo: Some(_), .. }) && self.t >= STRIKE_TIME
    }
}

pub struct BattleView {
    battle: Battle,
    fx: Option<Fx>,
    ai_timer: f32,
    /// XP shares, computed once the battle is over.
    xp: Option<Vec<XpAward>>,
    /// The result box has shown and its music started (once).
    result_cued: bool,
    /// A won battle's hold (0x4b09e8): seconds since it began. The screen stays up with the
    /// experience on the cards, then closes; the report follows.
    hold: Option<f32>,
    /// The last line of the log shown in the strip, and for how long more.
    news: Option<(String, f32)>,
    /// The battle was just played out by a quick battle: the result box waits a frame, so
    /// the key that started it does not also close it.
    quick_played: bool,
    /// Esc opened "Варианты выхода из битвы" (and its restart question is open).
    exiting: bool,
    /// The window's red close button was clicked: the ways out open at the next frame, as
    /// with Esc (interface.md §12).
    close_clicked: bool,
    exit_asking: bool,
    /// What that window chose, for the app to carry out.
    pub exit: Option<super::saves::ExitChoice>,
    /// The battle as it began (the log's length, who acts, his actions left, the turn):
    /// until one changes nobody has acted, and the quick battle of Razdor's old deploy
    /// screen (Q / Enter) is offered.
    begun: (usize, Option<usize>, i32, u32),
    /// Who plays each side, by [`Team::index`]: in a campaign the player his own and the AI
    /// the enemy; a custom battle's setup chooses.
    control: [Control; 2],
    /// The watched quick battle (W): the AI plays the player's side too, on screen, at
    /// `speed` times the pace, until W gives the control back.
    watch: bool,
    speed: u8,
    /// A custom battle: no campaign behind it, no experience, its own result box.
    custom: bool,
    /// The cards' curse and blessing signs by fighter: set as a magic or a blessing effect
    /// ends on the card, kept to the end of the battle (unit +0xc5 / +0xc9, 4b00c1).
    signs: Vec<Signs>,
}

/// A card's curse and blessing signs.
#[derive(Clone, Copy, Default)]
struct Signs {
    curse: bool,
    bless: bool,
}

impl BattleView {
    /// The battle under way, for the cheat console (`win`, `lose`, `god`).
    pub fn battle_mut(&mut self) -> &mut Battle {
        &mut self.battle
    }
}

fn all_cells(battle: &Battle) -> Vec<(Team, Slot)> {
    [Team::Enemy, Team::Player].into_iter().flat_map(|t| battle.formation.slots().map(move |s| (t, s))).collect()
}


/// "Click to curse X" and the effect below it, as the original's hover box.
fn preview_lines(p: Preview, kind: ActionKind, name: &str, hp: i32) -> (String, String) {
    let head = match kind {
        ActionKind::Melee | ActionKind::LongStrike | ActionKind::Shot | ActionKind::Strike => {
            razdor::trf!("Click to attack \"{name}\"", name)
        }
        ActionKind::Curse => razdor::trf!("Click to curse \"{name}\"", name),
        ActionKind::Heal => razdor::trf!("Click to heal \"{name}\"", name),
        ActionKind::Bless => razdor::trf!("Click to bless \"{name}\"", name),
    };
    let effect = match p {
        Preview::Damage(d) if d >= hp => razdor::trf!("Damage -{d} hits (kills)", d),
        Preview::Damage(d) if kind == ActionKind::LongStrike => razdor::trf!("Long strike: damage -{d} hits", d),
        Preview::Damage(d) => razdor::trf!("Damage -{d} hits", d),
        Preview::Heal(h) => razdor::trf!("Heals +{h} hits", h),
        Preview::Buff(b) => {
            let parts: Vec<String> =
                [(tr("Attack"), b.attack), (tr("Defence"), b.defence), (tr("Initiative"), b.initiative), (tr("Actions"), b.actions)]
                    .iter()
                    .filter(|(_, v)| *v != 0)
                    .map(|(n, v)| format!("{n}: {v:+}"))
                    .collect();
            if parts.is_empty() {
                tr("No effect").to_string()
            } else {
                parts.join("  ")
            }
        }
    };
    (head, effect)
}

/// The sound of `actor`'s action `kind` (`razdor::av::BattleSound`): shooters with a ranged
/// attack of at least `ShotWeaponRange` fire cannon.
fn action_cue(battle: &Battle, actor: usize, kind: ActionKind) -> Cue {
    use razdor::av::BattleSound;
    match BattleSound::of(battle, actor, kind) {
        BattleSound::Fight => Cue::Fight,
        BattleSound::Cannon => Cue::Cannon,
        BattleSound::Shoot => Cue::Shoot,
        BattleSound::Cure => Cue::Cure,
        BattleSound::Bless => Cue::Bless,
        BattleSound::Sorcery => Cue::Sorcery,
    }
}

/// The original's animation for an action (`Graphics/Battle`, `Graphics/Spells`), its size
/// relative to the card and its tint.
fn effect_art(kind: ActionKind, school: Option<MagicSchool>) -> (&'static str, f32) {
    match kind {
        ActionKind::Melee | ActionKind::LongStrike => ("Battle/--KUSKI.ugs", 1.9),
        ActionKind::Shot => ("Battle/--KUSKIBIG.ugs", 1.7),
        ActionKind::Strike => match school {
            Some(MagicSchool::Life) => ("Spells/S-Light-Front.ugs", 1.5),
            Some(MagicSchool::Death) => ("Spells/S-Fog.ugs", 1.5),
            _ => ("Spells/S-Fire.ugs", 1.5),
        },
        ActionKind::Curse => ("Spells/S-Fontain.ugs", 1.5),
        ActionKind::Heal => ("Battle/--CURE.ugs", 1.6),
        ActionKind::Bless => ("Spells/S-Swirl.ugs", 1.4),
    }
}

/// "Battle: the army of hero Stings against Castle Morgen!"
fn battle_title(game: &Game) -> String {
    let enemy = match game.foe {
        Some(Foe::Army(i)) => game.world.armies.get(i).map(|a| if a.name.trim().is_empty() { a.leader_name.clone() } else { a.name.clone() }),
        Some(Foe::Garrison(l)) => game.world.locations.get(l).map(|l| l.name.clone()),
        None => None,
    }
    .filter(|n| !n.trim().is_empty())
    .unwrap_or_else(|| tr("the enemy").to_string());
    // The install's own words in Russian: "Сражаются: армия героя #HERONAME и #ARMYNAME!".
    let own = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| chrome::ui_text("Battle", "Title")).flatten();
    match own {
        Some(t) => t.replace("#HERONAME", &game.hero_name()).replace("#ARMYNAME", enemy.trim()),
        None => razdor::trf!("Battle: the army of hero {hero} against {enemy}!", hero = game.hero_name(), enemy = enemy.trim()),
    }
}

impl BattleView {

    /// The battle starts as the window opens: there is no deployment step, the formation is
    /// the one set in the army window beforehand (interface.md §12, 0x4daa80).
    pub fn new(mut battle: Battle) -> Self {
        battle.begin();
        let begun = Self::moment(&battle);
        BattleView {
            battle,
            fx: None,
            ai_timer: 0.0,
            xp: None,
            result_cued: false,
            news: None,
            quick_played: false,
            exiting: false,
            close_clicked: false,
            exit_asking: false,
            exit: None,
            hold: None,
            signs: Vec::new(),
            begun,
            control: [Control::Player, Control::Ai],
            watch: false,
            speed: SPEED.with(|s| s.get()),
            custom: false,
        }
    }

    /// A custom battle (`rules::custom`), each side played as `control` says.
    pub fn custom(battle: Battle, control: [Control; 2]) -> Self {
        BattleView { control, custom: true, ..BattleView::new(battle) }
    }

    /// The player plays `team`'s units now (not while he watches).
    fn human(&self, team: Team) -> bool {
        !self.watch && self.control[team.index()] == Control::Player
    }

    /// Nobody human plays: the watched quick battle, or a custom battle of AI against AI.
    fn watching(&self) -> bool {
        !Team::BOTH.into_iter().any(|t| self.human(t))
    }

    /// W: starts watching the AI play both sides, or stops and gives the player his side
    /// back (in a custom battle of AI against AI, the side below).
    fn toggle_watch(&mut self) {
        if self.watching() {
            self.watch = false;
            if !self.control.contains(&Control::Player) {
                self.control[Team::Player.index()] = Control::Player;
            }
        } else {
            self.watch = true;
            self.ai_timer = 0.0;
        }
    }

    /// The watched battle at 4× (the `custom-battle:watch` snapshot scene).
    pub fn watch_for_snapshot(&mut self) {
        self.watch = true;
        self.speed = 4;
    }

    /// S: the next pace, 1× → 2× → 4× → 1×.
    fn next_speed(&mut self) {
        let i = SPEEDS.iter().position(|&s| s == self.speed).unwrap_or(0);
        self.speed = SPEEDS[(i + 1) % SPEEDS.len()];
        SPEED.with(|s| s.set(self.speed));
    }

    /// How much faster than normal the AI's moves and their animations run.
    fn pace(&self) -> f32 {
        if self.watching() { self.speed as f32 } else { 1.0 }
    }

    /// Nobody has acted yet: the battle's first moment, where Razdor's deploy screen stood
    /// before the parity pass. Its quick battle is offered there (Q or Enter, the button
    /// "Quick battle"); afterwards Q and "Finish automatically" play out the rest.
    fn untouched(&self) -> bool {
        Self::moment(&self.battle) == self.begun && self.fx.is_none() && self.battle.outcome() == Outcome::Ongoing
    }

    fn moment(b: &Battle) -> (usize, Option<usize>, i32, u32) {
        (b.log.len(), b.active(), b.actions_left(), b.round)
    }

    /// The battle is won and its result is up: the triumph has started.
    pub fn won(&self) -> bool {
        self.result_cued && self.battle.outcome() == Outcome::Victory
    }

    fn cell_under_mouse(&self, l: &Layout) -> Option<(Team, Slot)> {
        all_cells(&self.battle).into_iter().find(|&(t, s)| {
            let p = l.cell_pos(t, s);
            mouse_in(p.x, p.y, l.card.x, l.card.y)
        })
    }

    fn fighter_under_mouse(&self, l: &Layout) -> Option<usize> {
        let (team, slot) = self.cell_under_mouse(l)?;
        self.battle.at(team, slot)
    }

    /// Quick battle (Razdor extra): the rest of the battle is played at once by the battle AI
    /// on both sides (`Battle::auto_play_to_end`); the result box follows. While watching it
    /// skips to the end, with the result the watching would have reached.
    fn quick_battle(&mut self) {
        self.fx = None;
        self.news = None;
        self.battle.auto_play_to_end();
        self.quick_played = true;
    }

    /// One move of the AI for the active unit, whichever side it is on: the quick battle's
    /// step ([`Battle::auto_step`]), so a watched battle plays exactly as the instant one.
    fn ai_move(&mut self) -> Option<Step> {
        let step = self.battle.auto_step();
        self.note_log();
        step
    }

    /// Remembers the newest log line for the strip.
    fn note_log(&mut self) {
        if let Some(line) = self.battle.log.last() {
            if !line.starts_with("--") && self.news.as_ref().is_none_or(|(n, _)| n != line) {
                self.news = Some((line.clone(), 2.5));
            }
        }
    }

    /// The fight's own part of a frame, the same in a campaign and in a custom battle: the
    /// animations run, the keys and the AI act. Returns whether the battle is over (its last
    /// animation played) and whether the ways out opened this frame.
    fn advance(&mut self, l: &Layout) -> (bool, bool) {
        let dt = get_frame_time() * self.pace();
        if let Some(fx) = &mut self.fx {
            let before = fx.t;
            fx.t += dt;
            // An effect that ends marks its card: the magic effect (a strike or a curse) with
            // the curse sign, the blessing's with the blessing sign (4afe7c, effects 2 and 3).
            if let FxKind::Act { hit, echo, .. } = &fx.kind {
                let ended = |end: f32| before < end && fx.t >= end;
                let mut marks = Vec::new();
                if ended(STRIKE_TIME) {
                    marks.push((hit.target, hit.kind));
                }
                match echo {
                    Some(Echo::Counter) if ended(2.0 * STRIKE_TIME) => marks.push((fx.actor, hit.kind)),
                    Some(Echo::Curse) if ended(2.0 * STRIKE_TIME) => marks.push((fx.actor, ActionKind::Strike)),
                    _ => {}
                }
                for (id, kind) in marks {
                    if self.signs.len() < self.battle.fighters.len() {
                        self.signs.resize(self.battle.fighters.len(), Signs::default());
                    }
                    match kind {
                        ActionKind::Strike | ActionKind::Curse => self.signs[id].curse = true,
                        ActionKind::Bless => self.signs[id].bless = true,
                        _ => {}
                    }
                }
            }
            // The echo's sound as its half begins: the action's own, or the sorcery.
            let echoing = fx.echoing();
            if let FxKind::Act { hit, echo: Some(e), echo_cued } = &mut fx.kind {
                if echoing && !*echo_cued {
                    *echo_cued = true;
                    cue(match e {
                        Echo::Counter => action_cue(&self.battle, fx.actor, hit.kind),
                        Echo::Curse => Cue::Sorcery,
                    });
                }
            }
            if fx.t >= fx.duration() {
                self.fx = None;
            }
        }
        if let Some((_, t)) = &mut self.news {
            *t -= dt;
            if *t <= 0.0 {
                self.news = None;
            }
        }

        self.quick_played = false;
        let ongoing = self.battle.outcome() == Outcome::Ongoing;
        // Esc opens the ways out, also while a strike or spell plays. The window opens on the
        // next frame, so the Esc that opened it does not also close it.
        let mut just_opened = false;
        if !self.exiting && ongoing && (key(KeyCode::Escape) || std::mem::take(&mut self.close_clicked)) {
            self.exiting = true;
            just_opened = true;
        }
        if self.exiting {
            // The battle stands still under the window.
            set_input_blocked(true);
        } else if ongoing && (key(KeyCode::Q) || (self.untouched() && (key(KeyCode::Enter) || key(KeyCode::KpEnter)))) {
            self.quick_battle();
        } else if ongoing && key(KeyCode::W) {
            self.toggle_watch();
        } else if ongoing && self.watching() && key(KeyCode::S) {
            self.next_speed();
        } else if self.fx.is_none() {
            if let Some(active) = self.battle.active() {
                if self.human(self.battle.fighters[active].team) {
                    self.player_input(l, active);
                } else {
                    self.ai_timer += dt;
                    if self.ai_timer >= AI_DELAY {
                        self.ai_timer = 0.0;
                        self.fx = match self.ai_move() {
                            Some(Step::Act { actor, hit }) => {
                                cue(action_cue(&self.battle, actor, hit.kind));
                                Some(Fx::act(&self.battle, actor, hit))
                            }
                            Some(Step::Move { actor, from, to }) => {
                                cue(Cue::CardMove);
                                let team = self.battle.fighters[actor].team;
                                let kind = FxKind::Move { from: l.cell_pos(team, from), to: l.cell_pos(team, to) };
                                Some(Fx { actor, kind, t: 0.0 })
                            }
                            Some(Step::Wait { .. }) | None => None,
                        };
                    }
                }
            }
        }

        let over = self.battle.outcome() != Outcome::Ongoing && self.fx.is_none();
        if over && self.xp.is_none() && !self.custom {
            // What the player's units gain: only a victory pays (experience.md §3).
            self.xp = Some(self.battle.player_xp());
        }
        (over, just_opened)
    }

    /// The buttons under the panel while the battle goes on (a Razdor extra the players
    /// asked for): the quick battle, played out at once from its first moment or the rest of
    /// it, and the watched one; while watching, its pace, the skip to the end and the way
    /// back to playing.
    fn controls(&mut self, l: &Layout, over: bool) {
        if over || self.battle.outcome() != Outcome::Ongoing {
            return;
        }
        let (q, gap) = (l.quick, 4.0 * l.k);
        if self.watching() {
            let w = (q.w - 2.0 * gap) / 3.0;
            let x = |i: f32| q.x + i * (w + gap);
            if button(x(0.0), q.y, w, q.h, &razdor::trf!("Speed {n}x (S)", n = self.speed), true) {
                self.next_speed();
            }
            if button(x(1.0), q.y, w, q.h, tr("Skip (Q)"), true) {
                self.quick_battle();
            }
            if button(x(2.0), q.y, w, q.h, tr("Take over (W)"), true) {
                self.toggle_watch();
            }
            return;
        }
        let label = if self.untouched() { tr("Quick battle (Q)") } else { tr("Finish automatically (Q)") };
        let w = (q.w - gap) * 0.62;
        if button(q.x, q.y, w, q.h, label, true) {
            self.quick_battle();
        }
        if button(q.x + w + gap, q.y, q.w - w - gap, q.h, tr("Watch (W)"), true) {
            self.toggle_watch();
        }
    }

    /// The result box shows: the first time, a victory's triumph starts (it carries on over
    /// the map afterwards, a sting is not cut by the move to the map). True that first time.
    fn result_shown(&mut self) -> bool {
        if self.result_cued {
            return false;
        }
        self.result_cued = true;
        if self.battle.outcome() == Outcome::Victory {
            cue(Cue::Triumph);
        }
        true
    }

    pub fn frame(&mut self, game: &mut Game, assets: &Assets, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
        let l = Layout::new(&self.battle);
        // Input the app holds back (the cheat console) stays held for the exit window too.
        let outer = input_blocked();
        let (over, just_opened) = self.advance(&l);
        // Every action is written back into the armies as it is taken (0x4c4f8c after the
        // player's, 0x4c57bc after each of the enemy's).
        game.battle_write_back(&self.battle);
        world_view::backdrop(game, assets);
        self.close_clicked |= self.draw(&l, &battle_title(game), game.clock.total_minutes() as u64, assets);
        self.controls(&l, over);
        if over && !self.quick_played {
            self.result_shown();
            let outcome = self.battle.outcome();
            if outcome == Outcome::Victory {
                // The won battle's hold: 2.5 s with the experience on the cards and no input,
                // then the screen closes and the report follows (interface.md §9.9, §12).
                let held = self.hold.get_or_insert(0.0);
                *held += get_frame_time();
                let done = *held * 1000.0 >= razdor::av::BATTLE_END_HOLD_MS as f32;
                return done.then(|| self.close_won(game, message, dialogs));
            }
            return self.result_overlay(&l, game, message, dialogs, outcome);
        }
        if self.exiting && !just_opened {
            set_input_blocked(outer);
            match super::saves::battle_exit_dialog(&mut self.exit_asking) {
                (Some(choice), _) => {
                    self.exiting = false;
                    self.exit = Some(choice);
                }
                (None, true) => self.exiting = false,
                (None, false) => {}
            }
        }
        None
    }

    /// A frame of a custom battle, over the main menu's ruins: the round is counted in
    /// `session` when its result shows. Esc asks before leaving for the setup.
    pub fn frame_custom(&mut self, assets: &Assets, session: &mut Session) -> Option<CustomEnd> {
        let l = Layout::new(&self.battle);
        // Input the app holds back (the cheat console) stays held for the exit window too.
        let outer = input_blocked();
        let (over, just_opened) = self.advance(&l);
        super::main_menu::backdrop();
        self.close_clicked |= self.draw(&l, tr("Custom battle"), 0, assets);
        self.controls(&l, over);
        if over && !self.quick_played {
            if self.result_shown() {
                session.record(self.battle.outcome());
            }
            return self.custom_result(&l, session);
        }
        if self.exiting && !just_opened {
            set_input_blocked(outer);
            match super::saves::question(tr("Leave the battle"), tr("The battle is not over. Leave it for the setup of the armies?")) {
                Some(true) => {
                    self.exiting = false;
                    return Some(CustomEnd::Setup);
                }
                Some(false) => self.exiting = false,
                None => {}
            }
        }
        None
    }

    fn player_input(&mut self, l: &Layout, active: usize) {
        // The space key does what a click on the active unit's own card does (4c4f8c): one
        // action, a self-cast or a pass.
        let space = key(KeyCode::Space);
        if !space && !clicked() {
            return;
        }
        let under = if space { Some(active) } else { self.fighter_under_mouse(l) };
        if let Some(t) = under {
            let opts = self.battle.options(active, t);
            if let Some(&kind) = opts.first() {
                if let Ok(hit) = self.battle.act_with(t, kind) {
                    cue(action_cue(&self.battle, active, kind));
                    self.fx = Some(Fx::act(&self.battle, active, hit));
                    self.note_log();
                }
            } else if t == active {
                // A click on its own card passes one action, with its short pause, as in the
                // original.
                self.battle.pass();
                self.fx = Some(Fx { actor: active, kind: FxKind::Pass, t: 0.0 });
            }
        } else if let Some((team, to)) = self.cell_under_mouse(l).filter(|&(t, _)| t == self.battle.fighters[active].team) {
            let from = self.battle.fighters[active].slot;
            if self.battle.move_active(to).is_ok() {
                cue(Cue::CardMove);
                let kind = FxKind::Move { from: l.cell_pos(team, from), to: l.cell_pos(team, to) };
                self.fx = Some(Fx { actor: active, kind, t: 0.0 });
            }
        }
    }

    /// Draws the battle; true when the window's close button was clicked.
    fn draw(&self, l: &Layout, title: &str, now: u64, assets: &Assets) -> bool {
        let b = &self.battle;
        let k = l.k;
        let active = b.active();
        // The side whose unit the player moves now (both sides, turn about, in a custom
        // battle of player against player).
        let acting = active.map(|a| b.fighters[a].team);
        let player_turn = acting.is_some_and(|t| self.human(t)) && self.fx.is_none();
        let (targets, moves) = match (player_turn, active) {
            // The player's battle grid has no blocked cells (4d2233), but the screen shows
            // only the formation's cells, as the original's does.
            (true, Some(a)) => (b.targets(a), b.moves(a).into_iter().filter(|&m| b.formation.contains(m)).collect()),
            _ => (Vec::new(), Vec::new()),
        };
        let hovered_cell = self.cell_under_mouse(l);

        // The window: red marble, the title with both armies, the turn in the corner.
        // Its red close button asks to leave the battle, as Esc does (4daa80).
        let ongoing = b.outcome() == Outcome::Ongoing;
        let (_, close) = chrome::window(l.win, title, Skin::Red, ongoing);
        let close = close && !self.exiting && !input_blocked();
        // The frame between the panel and the formations.
        draw_line(l.panel.x + l.panel.w + 1.0, l.panel.y, l.panel.x + l.panel.w + 1.0, l.panel.y + l.panel.h, 1.5 * k, chrome::SILVER);

        // Empty cells, with the lit cells a step can go to.
        for (team, slot) in all_cells(b) {
            let p = l.cell_pos(team, slot);
            if b.at(team, slot).is_some_and(|i| b.fighters[i].alive()) {
                continue;
            }
            chrome::empty_cell(Rect::new(p.x, p.y, l.card.x, l.card.y), CellIcon::of(b.formation, slot), true);
            let is_move = Some(team) == acting && moves.contains(&slot);
            let sq = l.portrait(p);
            if is_move {
                chrome::glow_frame(sq, FRIENDLY, false);
            }
            let lit = is_move && hovered_cell == Some((team, slot));
            if lit {
                draw_rectangle(sq.x, sq.y, sq.w, sq.h, Color::new(0.4, 0.6, 1.0, 0.18));
            }
        }

        // The strip between the formations: what to do, or what just happened.
        let (hint, color) = self.strip_text(player_turn, &targets, &moves);
        chrome::divider(l.strip);
        let text_r = Rect { w: l.strip.w - 60.0 * k, x: l.strip.x + 30.0 * k, ..l.strip };
        chrome::hint_text(text_r, &hint, color);
        let limit = b.content().options.battle_end_turn;
        let size = (11.0 * k).round();
        shadow_right(&razdor::trf!("Turn {round}/{limit}", round = b.round, limit), l.strip.x + l.strip.w - 6.0 * k, l.strip.y + l.strip.h * 0.5 + size * 0.36, size, GOLD);

        // Cards: the order of the next units after the active one, as small numbers.
        let queue: Vec<usize> = if b.outcome() == Outcome::Ongoing { b.queue().skip(1).take(3).collect() } else { Vec::new() };
        for (i, f) in b.fighters.iter().enumerate() {
            let in_fx = self.fx.as_ref().is_some_and(|fx| matches!(&fx.kind, FxKind::Act { hit, .. } if hit.target == i) || fx.actor == i);
            if !f.alive() && !in_fx {
                continue;
            }
            let mut p = l.cell_pos(f.team, f.slot);
            if let Some(fx) = &self.fx {
                let dir = if f.team == Team::Player { -1.0 } else { 1.0 };
                let lunge = |t: f32| dir * 14.0 * k * ((t / (STRIKE_TIME * 0.5)).min(1.0) * PI).sin();
                match &fx.kind {
                    FxKind::Act { .. } if fx.actor == i && !fx.echoing() => p.y += lunge(fx.t),
                    // The counterblow: the target lunges back at the actor.
                    FxKind::Act { hit, echo: Some(Echo::Counter), .. } if hit.target == i && fx.echoing() => p.y += lunge(fx.t - STRIKE_TIME),
                    FxKind::Move { from, to } if fx.actor == i => p = from.lerp(*to, (fx.t / MOVE_TIME).min(1.0)),
                    _ => {}
                }
            }
            let hovered = hovered_cell == Some((f.team, f.slot));
            let frame = if Some(i) == active && self.fx.is_none() {
                Some((ACTIVE, true))
            } else if targets.contains(&i) {
                // The enemy under the mouse is framed green, as in the original; the other
                // enemies in reach stay faintly red.
                let c = match (Some(f.team) == acting, hovered) {
                    (true, _) => FRIENDLY,
                    (_, true) => ACTIVE,
                    (_, false) => HOSTILE,
                };
                Some((if hovered { c } else { Color { a: 0.6, ..c } }, hovered))
            } else {
                None
            };
            let order = queue.iter().position(|&q| q == i);
            self.draw_card(l, assets, i, p, frame, hovered && targets.contains(&i), order, Some(f.team) == acting, now);
        }

        if let Some(fx) = &self.fx {
            self.draw_fx(l, fx);
        }
        if let Some(xp) = &self.xp {
            for a in xp {
                let f = &b.fighters[a.fighter];
                let sq = l.portrait(l.cell_pos(f.team, f.slot));
                let y = sq.y + sq.h * 0.38;
                draw_rectangle(sq.x + 4.0 * k, y, sq.w - 8.0 * k, 20.0 * k, Color::new(0.0, 0.12, 0.16, 0.8));
                shadow_centered(&razdor::trf!("XP +{xp}", xp = a.xp), sq.x + sq.w / 2.0, y + 15.0 * k, (15.0 * k).round(), XP_COLOR);
                if self.levels_gained(a) > 0 {
                    let y = y + 22.0 * k;
                    draw_rectangle(sq.x + 4.0 * k, y, sq.w - 8.0 * k, 18.0 * k, Color::new(0.3, 0.22, 0.02, 0.85));
                    shadow_centered(tr("Level up!"), sq.x + sq.w / 2.0, y + 14.0 * k, (13.0 * k).round(), GOLD);
                }
            }
        }
        // The left panel: the hovered unit, else the one acting.
        if let Some(id) = self.fighter_under_mouse(l).filter(|&i| b.fighters[i].alive()).or(active).or_else(|| b.fighters.iter().position(|f| f.is_hero)) {
            self.draw_panel(l, assets, id);
        } else {
            chrome::parchment(l.panel, true);
        }
        // One hint box at a time: a spell badge's hint replaces the hover box (49e710).
        if player_turn && !super::spell_badges::hovered() {
            self.draw_preview(l, active.expect("player turn"));
        }
        close
    }

    /// The strip's text: the last action, or what the player can do.
    fn strip_text(&self, player_turn: bool, targets: &[usize], moves: &[Slot]) -> (String, Color) {
        let b = &self.battle;
        if b.outcome() != Outcome::Ongoing {
            return (tr("The battle is over").into(), GOLD);
        }
        if let Some((line, _)) = &self.news {
            if !player_turn || self.fx.is_some() {
                return (line.clone(), CREAM);
            }
        }
        if player_turn {
            // The original's line while the player's unit waits; the hover box says what a
            // click does. With nothing in reach, what is left to do.
            let hint = match (targets.is_empty(), moves.is_empty()) {
                (false, _) => {
                    let own = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| chrome::ui_text("Battle", "ExitHint")).flatten();
                    return (own.unwrap_or_else(|| tr("To leave the battle, press ESC").to_string()), GOLD);
                }
                (true, false) => tr("Nothing in reach: step to a lit cell, or press SPACE"),
                (true, true) => tr("Nothing to do: press SPACE to pass an action"),
            };
            return (hint.into(), Color::new(1.0, 0.55, 0.25, 1.0));
        }
        let waiting = if self.watching() {
            tr("The AI plays both sides (W: take over)")
        } else if b.active().is_some_and(|a| b.fighters[a].team == Team::Player) {
            tr("The AI moves your army...")
        } else {
            tr("The enemy moves...")
        };
        (waiting.into(), Color::new(1.0, 0.55, 0.25, 1.0))
    }

    /// XP needed for fighter `f`'s next level, as the battle began.
    fn need(&self, f: &Fighter) -> i32 {
        self.battle.content().xp_to_next(f.unit, f.level)
    }

    /// Levels the award `a` will add to its fighter.
    fn levels_gained(&self, a: &XpAward) -> i32 {
        let f = &self.battle.fighters[a.fighter];
        let c = self.battle.content();
        razdor::rules::experience::add_xp(f.level, f.xp, a.xp, |l| c.xp_to_next(f.unit, l)).2
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_card(&self, l: &Layout, assets: &Assets, id: usize, p: Vec2, frame: Option<(Color, bool)>, aimed: bool, order: Option<usize>, friendly: bool, now: u64) {
        let f = &self.battle.fighters[id];
        // Against the start of the battle, so every gain or loss shows (blue or red), with
        // the building's defence in the D values.
        let s = &self.battle.shown_stats(id);
        let base = &f.at_start;
        let k = l.k;
        let p = p.round();
        let (w, h) = (l.card.x, l.card.y);
        // The card's shadow, the portrait, the stat strip.
        draw_rectangle(p.x + 4.0 * k, p.y + 4.0 * k, w, h, Color::new(0.0, 0.0, 0.0, 0.45));
        let sq = l.portrait(p);
        assets.draw_portrait(f.unit, f.team, sq);
        chrome::wounds(sq, f.hp, f.max_hp());
        if aimed {
            let tint = if friendly { Color::new(0.3, 0.5, 1.0, 0.22) } else { Color::new(1.0, 0.1, 0.05, 0.25) };
            draw_rectangle(sq.x, sq.y, sq.w, sq.h, tint);
        }
        draw_rectangle_lines(sq.x, sq.y, sq.w, sq.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
        let strip = Rect::new(p.x, p.y + w, w, h - w);
        let row2 = if f.slot.row == Row::Back { self.battle.content().options.row2_def } else { 0 };
        let (caster, place) = (unit_sheet::caster(self.battle.content(), f.unit), unit_sheet::strip_place(self.battle.formation, f.slot));
        // The hero's and the enemy's first unit's strip is red, a named character's blue
        // (0x49462c, in battle for both sides).
        let first_enemy = f.team == Team::Enemy && self.battle.fighters.iter().find(|x| x.team == Team::Enemy).is_some_and(|x| std::ptr::eq(x, f));
        let panel = if f.is_hero || first_enemy {
            unit_sheet::StripPanel::Hero
        } else if f.named > 0 {
            unit_sheet::StripPanel::Named
        } else {
            unit_sheet::StripPanel::Plain
        };
        unit_sheet::stat_strip(strip, s, base, f.power, caster, place, f.hp, row2, frame.is_some_and(|(c, _)| c == ACTIVE), panel);

        // The original's signs (493a64): a drunk potion, then the blessing, from the top
        // left; poison (a negative regeneration), then the curse, from the top right; 23 px
        // apart, 1 px below the portrait's top. The spell badges along its bottom.
        // The original keeps the blessing and curse signs to the battle's end; Razdor shows
        // them only while the effect lasts, until the round start clears the modifiers (a
        // choice of the user's, 2026-10-09).
        let signs = self.signs.get(id).copied().unwrap_or_default();
        let m = f.mods;
        let blessed = signs.bless && (m.attack > 0 || m.defence > 0 || m.initiative > 0 || m.actions > 0);
        let cursed = signs.curse && (m.attack < 0 || m.defence < 0 || m.initiative < 0 || m.actions < 0);
        chrome::card_signs(sq, true, &[(f.potion, "sign-potion", GREEN), (blessed, "sign-bless", BLUE_TEXT)]);
        chrome::card_signs(sq, false, &[(f.poisoned(), "sign-poison", GREEN), (cursed, "sign-curse", PURPLE)]);
        // The turn order (Razdor's) in the bottom right corner, clear of the spell badges.
        if let Some(n) = order {
            let (ox, oy) = (sq.x + sq.w - 16.0 * k, sq.y + sq.h - 16.0 * k);
            draw_rectangle(ox, oy, 13.0 * k, 13.0 * k, Color::new(0.0, 0.0, 0.0, 0.55));
            shadow_centered(&(n + 1).to_string(), ox + 6.5 * k, oy + 11.0 * k, (11.0 * k).round(), CREAM);
        }
        let fighting = self.battle.outcome() == Outcome::Ongoing;
        if fighting && f.alive() && f.slot.row != Row::Reserve && self.battle.helpless(id) {
            draw_rectangle(sq.x, sq.y + sq.h - 16.0 * k, sq.w, 15.0 * k, Color::new(0.0, 0.0, 0.0, 0.5));
            shadow_centered(tr("can't reach"), sq.x + sq.w / 2.0, sq.y + sq.h - 4.0 * k, (11.0 * k).round(), Color::new(0.8, 0.8, 0.75, 1.0));
        }
        // In battle a dead unit's card shows no badges (493a64: HP 0).
        if f.alive() {
            super::spell_badges::draw(sq, &f.spells, f.drain, now, self.battle.content());
        }
        if let Some((c, strong)) = frame {
            chrome::glow_frame(sq, c, strong);
        }
    }

    fn draw_fx(&self, l: &Layout, fx: &Fx) {
        let FxKind::Act { hit, echo, .. } = &fx.kind else { return };
        if fx.echoing() {
            // The echo on the actor: the action's own picture for a counterblow, the sorcery
            // for a curse.
            let k = (fx.t - STRIKE_TIME) / STRIKE_TIME;
            let a = &self.battle.fighters[fx.actor];
            let q = l.portrait(l.cell_pos(a.team, a.slot));
            let art = match echo {
                Some(Echo::Curse) => effect_art(ActionKind::Strike, Some(MagicSchool::Death)).0,
                _ => effect_art(hit.kind, self.battle.fighters[hit.target].stats.magic).0,
            };
            chrome::effect(art, vec2(q.x + q.w / 2.0, q.y + q.h / 2.0), q.w * 1.7, k, WHITE);
            draw_rectangle(q.x, q.y, q.w, q.h, Color::new(1.0, 0.1, 0.1, 0.35 * (1.0 - k)));
            if let Some(c) = hit.counter {
                shadow_centered(&razdor::trf!("-{c} counter", c), q.x + q.w / 2.0, q.y + q.h * 0.45 - 26.0 * l.k * k, (18.0 * l.k).round(), RED);
            }
            return;
        }
        let k = fx.t / STRIKE_TIME;
        let f = &self.battle.fighters[hit.target];
        let sq = l.portrait(l.cell_pos(f.team, f.slot));
        let school = self.battle.fighters[fx.actor].stats.magic;
        let (art, size) = effect_art(hit.kind, school);
        let centre = vec2(sq.x + sq.w / 2.0, sq.y + sq.h / 2.0);
        let drawn = chrome::effect(art, centre, sq.w * size, k, WHITE);
        let (flash, label, color) = match hit.kind {
            ActionKind::Heal => (Color::new(0.2, 1.0, 0.3, 0.4 * (1.0 - k)), format!("+{}", hit.amount), GREEN),
            ActionKind::Bless => (Color::new(0.4, 0.7, 1.0, 0.4 * (1.0 - k)), tr("blessed").into(), BLUE_TEXT),
            ActionKind::Curse => (Color::new(0.7, 0.2, 0.9, 0.4 * (1.0 - k)), tr("cursed").into(), PURPLE),
            ActionKind::LongStrike => (Color::new(1.0, 0.1, 0.1, 0.45 * (1.0 - k)), razdor::trf!("-{amount} long!", amount = hit.amount), WHITE),
            ActionKind::Strike => (Color::new(1.0, 0.5, 0.1, 0.45 * (1.0 - k)), format!("-{}", hit.amount), ORANGE),
            _ => (Color::new(1.0, 0.1, 0.1, 0.45 * (1.0 - k)), format!("-{}", hit.amount), WHITE),
        };
        if !drawn || hit.kind.is_hostile() {
            draw_rectangle(sq.x, sq.y, sq.w, sq.h, Color { a: flash.a * if drawn { 0.5 } else { 1.0 }, ..flash });
        }
        let y = sq.y + sq.h * 0.45 - 26.0 * l.k * k;
        shadow_centered(&label, sq.x + sq.w / 2.0, y, (22.0 * l.k).round(), color);
    }

    /// The hover box over a target (or the acting unit's own card, or a cell to step to):
    /// what a click would do.
    fn draw_preview(&self, l: &Layout, active: usize) {
        let b = &self.battle;
        let (head, effect, effect_color) = if let Some(t) = self.fighter_under_mouse(l) {
            let opts = b.options(active, t);
            match opts.first() {
                Some(&kind) => {
                    let (h, e) = preview_lines(b.preview(active, t, kind), kind, &b.fighters[t].name, b.fighters[t].hp);
                    let c = if kind.is_hostile() { Color::new(0.75, 0.05, 0.02, 1.0) } else { Color::new(0.05, 0.25, 0.75, 1.0) };
                    (h, Some(e), c)
                }
                None if t == active => (razdor::trf!("Click (or press SPACE) to pass one action of \"{name}\"", name = b.fighters[t].name), None, BLACK),
                None => return,
            }
        } else if let Some((_, slot)) = self.cell_under_mouse(l).filter(|&(t, _)| t == b.fighters[active].team) {
            if !b.moves(active).contains(&slot) {
                return;
            }
            (razdor::trf!("Click to move \"{name}\" here", name = b.fighters[active].name), None, BLACK)
        } else {
            return;
        };
        let k = l.k;
        let size = (13.0 * k).round();
        let lh = 16.0 * k;
        let w = measure(&head, size).width.max(effect.as_ref().map_or(0.0, |e| measure(e, size).width)) + 14.0 * k;
        let h = lh * if effect.is_some() { 2.0 } else { 1.0 } + 8.0 * k;
        let (mx, my) = crate::ui::widgets::pointer();
        let x = (mx - w * 0.4).clamp(2.0, (screen_width() - w - 2.0).max(2.0));
        let y = (my - h - 6.0 * k).max(2.0);
        draw_rectangle(x, y, w, h, Color::new(0.93, 0.89, 0.72, 0.95));
        draw_rectangle_lines(x, y, w, h, 1.0, Color::new(0.2, 0.15, 0.1, 1.0));
        text(&head, x + 7.0 * k, y + lh, size, Color::new(0.08, 0.05, 0.02, 1.0));
        if let Some(e) = effect {
            text(&e, x + 7.0 * k, y + 2.0 * lh, size, effect_color);
        }
    }

    /// The unit panel on the left, as the original's.
    fn draw_panel(&self, l: &Layout, assets: &Assets, id: usize) {
        let b = &self.battle;
        let f = &b.fighters[id];
        // What it wears, an enemy's too (an army wears its items, `ai::army_units`).
        let items: [Option<ItemId>; 4] = f.items;
        let mut status = Vec::new();
        if b.active() == Some(id) {
            status.push((razdor::trf!("Acting: {left} of {total} actions left", left = b.actions_left(), total = f.stats[Stat::Manevres].max(b.actions_left())), Color::new(0.5, 1.0, 0.5, 1.0)));
        }
        if !f.mods.is_empty() {
            status.push((razdor::trf!("This turn: {mods}", mods = f.mods.describe()), BLUE_TEXT));
        }
        if f.poisoned() {
            status.push((tr("Poisoned").into(), Color::new(0.5, 1.0, 0.4, 1.0)));
        }
        if f.bleed > 0 {
            status.push((tr("Bleeding").into(), RED_TEXT));
        }
        let hero = f.is_hero.then(|| HeroClass::ALL.into_iter().find(|h| h.unit() == f.unit)).flatten();
        let sheet = Sheet {
            kind: f.unit,
            name: &f.name,
            named: f.named > 0,
            level: f.level,
            xp: f.xp,
            need: self.need(f),
            hp: f.hp,
            now: &self.battle.shown_stats(id),
            start: &f.at_start,
            power: f.power,
            wage: if f.is_hero || f.team == Team::Enemy || self.custom { 0 } else { b.content().wage(f.unit) },
            items,
            back_row: f.slot.row == Row::Back,
            building: b.building_defence(f.team),
            hero,
            status,
            battle: true,
        };
        let mut hover = None;
        unit_sheet::draw(assets, b.content(), l.panel, &sheet, false, &mut hover);
    }

    /// The won battle's hold is over: the battle is settled and the screen closes; the
    /// victory report follows on the map 250 ms later (a level gained has no sound here:
    /// `Unit-Upgrade` is the promotion screen's).
    fn close_won(&self, game: &mut Game, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Screen {
        let result = game.resolve_battle(&self.battle);
        if game.won() {
            return Screen::Victory;
        }
        // The report is the chained step 250 ms after the screen closes (0x4af658).
        let due = macroquad::time::get_time() + razdor::av::BATTLE_REPORT_GAP_MS as f64 / 1000.0;
        dialogs.extend(Dialog::victory(game, &result).map(|d| Dialog { not_before: Some(due), ..d }));
        *message = None;
        Screen::WorldMap
    }

    /// The result box over the unit panel (a defeat, or a battle nobody won), so the XP
    /// badges on the cards stay visible.
    fn result_overlay(&self, l: &Layout, game: &mut Game, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>, outcome: Outcome) -> Option<Screen> {
        let k = l.k;
        let r = Rect::new(l.panel.x + 8.0 * k, l.panel.y + 150.0 * k, l.panel.w - 16.0 * k, 200.0 * k);
        let (title, sub, color) = match (outcome, self.battle.end_reason()) {
            (Outcome::Victory, Some(EndReason::Surrender(_))) => (tr("Victory!"), tr("The enemy surrenders."), GOLD),
            (Outcome::Victory, Some(EndReason::TurnLimit)) => (tr("Victory!"), tr("The turns run out; the field is yours."), GOLD),
            (Outcome::Victory, _) => (tr("Victory!"), "", GOLD),
            (_, Some(EndReason::Surrender(_))) => (tr("Defeat"), tr("Your army surrenders."), RED_TEXT),
            _ => (tr("Defeat"), tr("Your whole army has fallen."), RED_TEXT),
        };
        let window_title = if outcome == Outcome::Victory { tr("Victory over the enemy!") } else { tr("Defeat in battle!") };
        let (inner, _) = chrome::window(r, window_title, Skin::Marble, false);
        shadow_centered(title, inner.x + inner.w / 2.0, inner.y + 40.0 * k, (32.0 * k).round(), color);
        for (i, line) in wrap(sub, inner.w - 16.0 * k, (13.0 * k).round()).iter().enumerate() {
            shadow_centered(line, inner.x + inner.w / 2.0, inner.y + 64.0 * k + i as f32 * 15.0 * k, (13.0 * k).round(), CREAM);
        }
        let total: i32 = self.xp.iter().flatten().map(|a| a.xp).sum();
        if total > 0 {
            shadow_centered(&razdor::trf!("Experience gained: {total}", total), inner.x + inner.w / 2.0, inner.y + 100.0 * k, (14.0 * k).round(), XP_COLOR);
        }
        let (bw, bh) = (120.0 * k, 30.0 * k);
        let pressed = button(inner.x + (inner.w - bw) / 2.0, inner.y + inner.h - bh - 10.0 * k, bw, bh, "OK", true) || key(KeyCode::Enter);
        if !pressed {
            return None;
        }
        let result = game.resolve_battle(&self.battle);
        match result {
            BattleResult::Defeat => Some(Screen::GameOver),
            BattleResult::Victory { .. } if game.won() => Some(Screen::Victory),
            victory @ BattleResult::Victory { .. } => {
                dialogs.extend(Dialog::victory(game, &victory));
                *message = None;
                Some(Screen::WorldMap)
            }
            BattleResult::Withdrew { lost } => {
                *message = Some(if lost > 0 {
                    razdor::trf!("Nobody breaks. You withdraw, {lost} fell; no experience without a victory.", lost)
                } else {
                    tr("Nobody breaks. You withdraw; no experience without a victory.").to_string()
                });
                Some(Screen::WorldMap)
            }
        }
    }
}

impl BattleView {
    /// A custom battle's result box over the unit panel: who won and how, the session's
    /// score, and the ways on: "Again" (Enter), "Change armies" (Esc), "Main menu".
    fn custom_result(&self, l: &Layout, session: &Session) -> Option<CustomEnd> {
        let k = l.k;
        let r = Rect::new(l.panel.x + 8.0 * k, l.panel.y + 130.0 * k, l.panel.w - 16.0 * k, 250.0 * k);
        let won = self.battle.outcome() == Outcome::Victory;
        let (title, color) = if won { (tr("Your army wins!"), GOLD) } else { (tr("The enemy army wins!"), RED_TEXT) };
        let sub = match self.battle.end_reason() {
            Some(EndReason::Surrender(Team::Player)) => tr("Your army surrenders."),
            Some(EndReason::Surrender(Team::Enemy)) => tr("The enemy surrenders."),
            Some(EndReason::TurnLimit) => tr("The turns run out; the field is yours."),
            _ if won => tr("The whole enemy army has fallen."),
            _ => tr("Your whole army has fallen."),
        };
        let (inner, _) = chrome::window(r, tr("Custom battle"), Skin::Marble, false);
        let cx = inner.x + inner.w / 2.0;
        shadow_centered(title, cx, inner.y + 34.0 * k, fit_size(title, inner.w - 12.0 * k, (24.0 * k).round()), color);
        for (i, line) in wrap(sub, inner.w - 16.0 * k, (13.0 * k).round()).iter().enumerate() {
            shadow_centered(line, cx, inner.y + 58.0 * k + i as f32 * 15.0 * k, (13.0 * k).round(), CREAM);
        }
        let score = razdor::trf!("Rounds won: {won}, lost: {lost}", won = session.won, lost = session.lost);
        shadow_centered(&score, cx, inner.y + 96.0 * k, (13.0 * k).round(), GOLD);
        let (bw, bh) = (inner.w - 30.0 * k, 28.0 * k);
        let bx = inner.x + 15.0 * k;
        let by = |i: f32| inner.y + inner.h - (3.0 - i) * (bh + 6.0 * k) - 4.0 * k;
        if button(bx, by(0.0), bw, bh, tr("Again (Enter)"), true) || key(KeyCode::Enter) || key(KeyCode::KpEnter) {
            return Some(CustomEnd::Again);
        }
        if button(bx, by(1.0), bw, bh, tr("Change armies (Esc)"), true) || key(KeyCode::Escape) {
            return Some(CustomEnd::Setup);
        }
        if button(bx, by(2.0), bw, bh, tr("Main menu"), true) {
            return Some(CustomEnd::MainMenu);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use razdor::rules::content::{Content, HeroClass};
    use razdor::rules::game::{Foe, Game};

    use super::*;

    #[test]
    fn the_battle_starts_as_its_window_opens() {
        let mut g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        let battle = g.start_battle();
        assert!(battle.is_deploying());
        let view = BattleView::new(battle);
        assert!(!view.battle.is_deploying(), "no deployment step");
        assert!(view.battle.active().is_some(), "the first unit acts at once");
    }

    #[test]
    fn the_quick_battle_is_offered_until_someone_acts() {
        let mut g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        let mut view = BattleView::new(g.start_battle());
        assert!(view.untouched(), "the deploy screen's moment: Q / Enter for a quick battle");
        let active = view.battle.active().unwrap();
        if view.battle.fighters[active].team == Team::Player {
            view.battle.act_with(active, view.battle.options(active, active)[0]).unwrap();
        } else {
            view.battle.ai_step();
        }
        assert!(!view.untouched(), "once someone acted, Q finishes the rest");
        view.quick_battle();
        assert_ne!(view.battle.outcome(), Outcome::Ongoing);
    }

    fn bandit_camp() -> BattleView {
        let mut g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        BattleView::new(g.start_battle())
    }

    #[test]
    fn the_watched_quick_battle_ends_as_the_instant_one() {
        let mut instant = bandit_camp();
        let mut watched = bandit_camp();
        let mut skipped = bandit_camp();
        instant.quick_battle();
        assert_ne!(instant.battle.outcome(), Outcome::Ongoing);
        // W: the AI plays both sides, move by move as the screen shows them.
        watched.toggle_watch();
        assert!(watched.watching() && !watched.human(Team::Player));
        let mut moves = 0;
        while watched.battle.outcome() == Outcome::Ongoing {
            assert!(watched.ai_move().is_some());
            moves += 1;
        }
        assert_eq!(watched.battle.outcome(), instant.battle.outcome());
        assert_eq!(watched.battle.log, instant.battle.log);
        assert_eq!(watched.battle.player_results(), instant.battle.player_results());
        assert_eq!(watched.battle.player_xp(), instant.battle.player_xp());
        // Watched for a while, then skipped to the end (Q): the same result.
        skipped.toggle_watch();
        for _ in 0..moves / 2 {
            skipped.ai_move();
        }
        skipped.quick_battle();
        assert_eq!(skipped.battle.log, instant.battle.log);
        assert_eq!(skipped.battle.player_results(), instant.battle.player_results());
    }

    #[test]
    fn watching_stops_to_give_the_control_back() {
        let mut view = bandit_camp();
        assert!(!view.watching() && view.human(Team::Player) && !view.human(Team::Enemy));
        assert_eq!(view.pace(), 1.0, "a battle the player plays runs at the normal pace");
        view.toggle_watch();
        assert!(view.watching());
        view.speed = 1;
        view.next_speed();
        assert_eq!(view.pace(), 2.0);
        view.next_speed();
        assert_eq!(view.pace(), 4.0);
        view.next_speed();
        assert_eq!(view.pace(), 1.0, "4x wraps round to 1x");
        view.toggle_watch();
        assert!(!view.watching() && view.human(Team::Player), "W again: the player plays on");
        // A custom battle of AI against AI: stopping takes the side below.
        let battle = bandit_camp().battle;
        let mut view = BattleView::custom(battle, [Control::Ai, Control::Ai]);
        assert!(view.watching() && view.custom);
        view.toggle_watch();
        assert!(view.human(Team::Player) && !view.human(Team::Enemy));
        // Player against player: each side's units are the player's.
        let view = BattleView::custom(bandit_camp().battle, [Control::Player, Control::Player]);
        assert!(view.human(Team::Player) && view.human(Team::Enemy) && !view.watching());
    }

    /// РК1's ruins (`rk1-day1.jsonl` to step 29): the militia's blow on the swordsman at
    /// 2:1:2 is answered by a counterblow, so the action plays twice as long, the second half
    /// the swordsman's lunge back and the effect on the militia (interface.md §12).
    #[test]
    fn a_counterblow_adds_the_slide_back_and_the_effect_on_the_actor() {
        use razdor::difftest::{parse_actions, Runner, Source};
        let Some(dir) = std::env::var_os(razdor::dt::install::ENV_VAR) else { return };
        let dt = razdor::dt::install::DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let actions = parse_actions(include_str!("../../tools/difftest/rk1-day1.jsonl")).unwrap();
        let mut r = Runner::new(Source::Install(&dt));
        for a in &actions[..30] {
            r.apply(a).unwrap();
        }
        let (_, Some(mut b), _) = r.into_view().unwrap() else { panic!("no battle at step 29") };
        let actor = b.active().unwrap();
        let target = b.at(Team::Enemy, Slot::new(Row::Front, 1)).unwrap();
        let kind = b.options(actor, target)[0];
        let hit = b.act_with(target, kind).unwrap();
        let mut fx = Fx::act(&b, actor, hit);
        assert!(matches!(fx.kind, FxKind::Act { echo: Some(Echo::Counter), .. }));
        assert_eq!(fx.duration(), 2.0 * STRIKE_TIME);
        assert!(!fx.echoing());
        fx.t = STRIKE_TIME + 0.01;
        assert!(fx.echoing());
    }
}
