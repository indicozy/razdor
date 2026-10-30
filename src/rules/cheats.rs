//! The cheat console (a Razdor extra, issue #1; `~` on the map and in battle, `ui::console`):
//! a typed command line parsed here and run on the game or the battle under way. Every
//! command goes through the normal rules where there are some: the time passes by the wait's
//! ticks, a battle won or lost ends by the normal end of a battle, a unit joins as a hired
//! one. A game in which a cheat worked says so in its saves ([`CheatState::used`]).

use std::fmt;

use crate::i18n::{n_, tr};
use crate::search;
use crate::trf;

use super::battle::{Battle, Outcome, Team};
use super::content::{Content, ItemId, UnitId};
use super::game::{Event, Game};
use super::units::Unit;

/// Highest level `level` and `unit` give.
pub const MAX_LEVEL: i32 = 99;
/// Longest wait `time` plays at once: 30 days.
pub const MAX_HOURS: u32 = 24 * 30;
/// Fastest walk `speed` gives.
pub const MAX_SPEED: u32 = 20;
/// Most gold, mana or XP one command gives (or takes).
pub const MAX_AMOUNT: i32 = 1_000_000;

/// What the console left in the game: saved with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CheatState {
    /// A cheat worked in this game (noted in its saves and in the play log).
    pub used: bool,
    /// `god`: the hero's army takes no damage in battle.
    pub god: bool,
    /// `speed`: the hero's walk multiplier; 0 or 1 is the normal pace.
    pub speed: u32,
    /// `noclip`: the hero's walk goes through everything ([`Game::plan`]); none in older saves.
    #[serde(default)]
    pub noclip: bool,
    /// `peace`: enemy armies do not attack or chase the hero (he may still attack them);
    /// none in older saves.
    #[serde(default)]
    pub peace: bool,
    /// `routes`: the minimap shows where the enemy armies are heading ([`routes`]); none in
    /// older saves.
    #[serde(default)]
    pub routes: bool,
}

impl CheatState {
    /// The walk multiplier, at least 1.
    pub fn speed(&self) -> u32 {
        self.speed.clamp(1, MAX_SPEED)
    }
}

/// A command of the console.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cheat {
    Help,
    Gold(i32),
    Mana(i32),
    Reveal,
    Heal,
    Xp(i32),
    Level(i32),
    Item(String),
    Spell(String),
    Unit(String, Option<i32>),
    Time(u32),
    Win,
    Lose,
    God,
    Speed(u32),
    Noclip,
    Peace,
    Routes,
}

/// The commands for `help`: (name and arguments, what it does).
pub const COMMANDS: [(&str, &str); 18] = [
    ("help", n_("this list")),
    ("gold N", n_("gives N gold (a negative N takes it)")),
    ("mana N", n_("gives N mana (a negative N takes it)")),
    ("reveal", n_("explores the whole map")),
    ("heal", n_("heals the army and raises its dead")),
    ("xp N", n_("gives N experience to every living unit of the army")),
    ("level N", n_("puts the hero at level N")),
    ("item <id | name>", n_("puts the item into the pack")),
    ("spell <id | name>", n_("writes the spell into the book")),
    ("unit <id | name> [level]", n_("the unit joins the army, if there is room")),
    ("time H", n_("lets H hours pass, as a wait")),
    ("win", n_("in battle: the enemy falls, you win")),
    ("lose", n_("in battle: your army falls, you lose")),
    ("god", n_("on / off: your army takes no damage in battle")),
    ("speed N", n_("the hero walks N times faster (1: normal)")),
    ("noclip", n_("on / off: the hero walks through anything to any cell")),
    ("peace", n_("on / off: enemy armies do not attack or chase the hero")),
    ("routes", n_("on / off: the minimap shows the enemy armies' routes and where they head")),
];

/// Why a line is not a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    /// No such command (the word typed).
    Unknown(String),
    /// The command's arguments are wrong: its usage line from [`COMMANDS`].
    Usage(&'static str),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Empty => f.write_str(tr("Type a command; help lists them.")),
            ParseError::Unknown(word) => f.write_str(&trf!("Unknown command \"{word}\"; help lists them.", word)),
            ParseError::Usage(usage) => f.write_str(&trf!("Usage: {usage}", usage)),
        }
    }
}

fn usage(name: &str) -> ParseError {
    ParseError::Usage(COMMANDS.iter().find(|(u, _)| u.split(' ').next() == Some(name)).map_or("help", |c| c.0))
}

/// Reads a command line: the command's name (any case) and its arguments.
pub fn parse(line: &str) -> Result<Cheat, ParseError> {
    let line = line.trim();
    let (word, rest) = match line.split_once(char::is_whitespace) {
        Some((w, r)) => (w, r.trim()),
        None => (line, ""),
    };
    if word.is_empty() {
        return Err(ParseError::Empty);
    }
    let name = word.to_lowercase();
    let bad = || usage(&name);
    let number = |min: i64, max: i64| -> Result<i64, ParseError> {
        rest.parse::<i64>().ok().filter(|n| (min..=max).contains(n)).ok_or_else(bad)
    };
    let none = || if rest.is_empty() { Ok(()) } else { Err(bad()) };
    let text = || if rest.is_empty() { Err(bad()) } else { Ok(rest.to_string()) };
    let amount = MAX_AMOUNT as i64;
    Ok(match name.as_str() {
        "help" | "?" => none().map(|_| Cheat::Help)?,
        "gold" => Cheat::Gold(number(-amount, amount)? as i32),
        "mana" => Cheat::Mana(number(-amount, amount)? as i32),
        "reveal" => none().map(|_| Cheat::Reveal)?,
        "heal" => none().map(|_| Cheat::Heal)?,
        "xp" => Cheat::Xp(number(1, amount)? as i32),
        "level" => Cheat::Level(number(1, MAX_LEVEL as i64)? as i32),
        "item" => Cheat::Item(text()?),
        "spell" => Cheat::Spell(text()?),
        "unit" => {
            // A last word that is a number is the level, unless it is the only word (an id).
            let words: Vec<&str> = rest.split_whitespace().collect();
            match words.as_slice() {
                [] => return Err(bad()),
                [.., last] if words.len() > 1 && last.parse::<i64>().is_ok() => {
                    let level = last.parse::<i64>().ok().filter(|l| (1..=MAX_LEVEL as i64).contains(l)).ok_or_else(bad)?;
                    Cheat::Unit(words[..words.len() - 1].join(" "), Some(level as i32))
                }
                _ => Cheat::Unit(rest.to_string(), None),
            }
        }
        "time" => Cheat::Time(number(1, MAX_HOURS as i64)? as u32),
        "win" => none().map(|_| Cheat::Win)?,
        "lose" => none().map(|_| Cheat::Lose)?,
        "god" => none().map(|_| Cheat::God)?,
        "speed" => Cheat::Speed(number(1, MAX_SPEED as i64)? as u32),
        "noclip" => none().map(|_| Cheat::Noclip)?,
        "peace" => none().map(|_| Cheat::Peace)?,
        "routes" => none().map(|_| Cheat::Routes)?,
        _ => return Err(ParseError::Unknown(word.to_string())),
    })
}

/// What a command did: its lines for the console and the events of the time it let pass
/// (for the interface to show, as after a wait).
#[derive(Debug, Default)]
pub struct Done {
    pub lines: Vec<String>,
    pub events: Vec<Event>,
}

impl Done {
    fn line(s: String) -> Done {
        Done { lines: vec![s], events: Vec::new() }
    }
}

/// Finds one of `records` (id, record, name) by `query`: a number is an id; else the name
/// that is the query (any case), else the first that starts with it, else the first that
/// holds it. Also gives how many names hold it.
fn pick<T: Copy>(query: &str, records: impl Iterator<Item = (u32, T, String)>) -> Result<(T, String, usize), String> {
    let all: Vec<(u32, T, String)> = records.collect();
    if let Ok(id) = query.trim().parse::<u32>() {
        return all.into_iter().find(|r| r.0 == id).map(|r| (r.1, r.2, 1)).ok_or_else(|| trf!("Nothing has id {id}.", id));
    }
    let q = search::fold(query.trim());
    let hits: Vec<&(u32, T, String)> = all.iter().filter(|r| search::find(&r.2, &q).is_some()).collect();
    let exact = hits.iter().find(|r| search::fold(&r.2) == q);
    let prefix = hits.iter().find(|r| search::fold(&r.2).starts_with(&q));
    match exact.or(prefix).or(hits.first()) {
        Some(r) => Ok((r.1, r.2.clone(), hits.len())),
        None => Err(trf!("Nothing is called \"{query}\".", query = query.trim())),
    }
}

/// The note on a name picked among several.
fn among(name: &str, hits: usize) -> String {
    if hits > 1 {
        trf!("{name} (of {hits} that match; type more of the name or the id)", name, hits)
    } else {
        name.to_string()
    }
}

pub fn find_item(content: &Content, query: &str) -> Result<(ItemId, String, usize), String> {
    pick(query, content.items.iter().map(|a| (a.id, ItemId(a.id), a.name.clone())))
}

pub fn find_spell(content: &Content, query: &str) -> Result<(u32, String, usize), String> {
    pick(query, content.spells.iter().map(|s| (s.id, s.id, s.name.clone())))
}

pub fn find_unit(content: &Content, query: &str) -> Result<(UnitId, String, usize), String> {
    pick(query, content.units.iter().filter(|u| u.hits > 0).map(|u| (u.id, UnitId(u.id), u.name.clone())))
}

/// The help lines: each command with what it does.
pub fn help() -> Vec<String> {
    COMMANDS.iter().map(|(u, what)| format!("{u} — {}", tr(what))).collect()
}

/// Most rows the console's completion list shows.
pub const MOST_SUGGESTIONS: usize = 8;

/// The names the completion of `item`, `spell` and `unit` draws from: the current game's, as
/// [`find_item`], [`find_spell`] and [`find_unit`] search them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Names {
    pub items: Vec<String>,
    pub spells: Vec<String>,
    pub units: Vec<String>,
}

impl Names {
    pub fn of(content: &Content) -> Names {
        Names {
            items: content.items.iter().map(|a| a.name.clone()).collect(),
            spells: content.spells.iter().map(|s| s.name.clone()).collect(),
            units: content.units.iter().filter(|u| u.hits > 0).map(|u| u.name.clone()).collect(),
        }
    }
}

/// A row of the console's completion list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    /// The line once completed: `gold `, `item Long Sword`, `unit Bandit 3`.
    pub completion: String,
    /// What the row shows: a command's usage, or a name.
    pub label: String,
    /// A command's description, untranslated (`tr` it to show); empty for a name.
    pub hint: &'static str,
    /// A command that needs no argument: Enter runs it once it is completed.
    pub alone: bool,
}

/// The completion list for the console's line. While the first word is typed (no space yet),
/// the commands whose name starts with it (any case); after `item `, `spell ` or `unit `, the
/// names of the game that start with the typed text, then those that contain it. An empty
/// line, an empty name query, no game (no names) or another command give none. At most
/// [`MOST_SUGGESTIONS`] rows.
pub fn suggestions(line: &str, names: &Names) -> Vec<Suggestion> {
    let typed = line.trim_start();
    let Some(at) = typed.find(char::is_whitespace) else {
        if typed.is_empty() {
            return Vec::new();
        }
        let word = search::fold(typed);
        return COMMANDS
            .iter()
            .filter(|c| search::fold(command_name(c.0)).starts_with(&word))
            .take(MOST_SUGGESTIONS)
            .map(|c| Suggestion { completion: format!("{} ", command_name(c.0)), label: c.0.to_string(), hint: c.1, alone: !c.0.contains(' ') })
            .collect();
    };
    let cmd = typed[..at].to_lowercase();
    let list = match cmd.as_str() {
        "item" => &names.items,
        "spell" => &names.spells,
        "unit" => &names.units,
        _ => return Vec::new(),
    };
    // `unit <name> <level>`: a last number is the level, kept as typed; the name is what before it.
    let mut words: Vec<&str> = typed[at..].split_whitespace().collect();
    let mut tail = String::new();
    if cmd == "unit" && words.len() > 1 && words.last().is_some_and(|w| w.parse::<i64>().is_ok()) {
        tail = format!(" {}", words.pop().unwrap_or_default());
    }
    let query = words.join(" ");
    if query.is_empty() {
        return Vec::new();
    }
    let q = search::fold(&query);
    let starts = |n: &&String| search::fold(n).starts_with(&q);
    let contains = |n: &&String| search::find(n, &query).is_some();
    let first = list.iter().filter(starts);
    let then = list.iter().filter(|n| !starts(n) && contains(n));
    first
        .chain(then)
        .take(MOST_SUGGESTIONS)
        .map(|n| Suggestion { completion: format!("{cmd} {n}{tail}"), label: n.clone(), hint: "", alone: false })
        .collect()
}

/// The name of a command's usage: `gold` for `gold N`.
fn command_name(usage: &str) -> &str {
    usage.split(' ').next().unwrap_or(usage)
}

/// The row the selection goes to from `current` (None: none picked yet) by one step: forward
/// to the next row (the first from none), back to the previous (the last from none); wrapping
/// at both ends. None for an empty list.
pub fn cycle(current: Option<usize>, len: usize, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match (current, forward) {
        (None, true) => 0,
        (None, false) => len - 1,
        (Some(i), true) => (i + 1) % len,
        (Some(i), false) => (i + len - 1) % len,
    })
}

/// Runs `cheat` on the game (`None` in a custom battle) and the battle on screen (`None` on
/// the map). The commands that change the army, its experience or the time wait for the
/// battle to end; `win` and `lose` need one. A command that worked marks the game.
pub fn run(cheat: &Cheat, game: Option<&mut Game>, battle: Option<&mut Battle>) -> Result<Done, String> {
    if *cheat == Cheat::Help {
        return Ok(Done { lines: help(), events: Vec::new() });
    }
    let in_battle = battle.is_some();
    match (cheat, battle) {
        (Cheat::Win | Cheat::Lose, None) => return Err(tr("Only in battle.").into()),
        // A battle already decided keeps its result (a second side falling would turn it).
        (Cheat::Win | Cheat::Lose, Some(b)) if b.outcome() != Outcome::Ongoing => return Err(tr("The battle is over.").into()),
        (Cheat::Win, Some(b)) => {
            b.force_end(Team::Player);
            mark(game);
            return Ok(Done::line(tr("The enemy falls: the battle is won.").into()));
        }
        (Cheat::Lose, Some(b)) => {
            b.force_end(Team::Enemy);
            mark(game);
            return Ok(Done::line(tr("Your army falls: the battle is lost.").into()));
        }
        (Cheat::God, b) => {
            let on = match game {
                Some(g) => {
                    g.cheats.god = !g.cheats.god;
                    g.cheats.used = true;
                    g.cheats.god
                }
                None => !b.as_ref().is_some_and(|b| b.god),
            };
            if let Some(b) = b {
                b.god = on;
            }
            return Ok(Done::line(if on { tr("God mode on: your army takes no damage.") } else { tr("God mode off.") }.into()));
        }
        _ => {}
    }
    let Some(game) = game else { return Err(tr("Only in a game (not in a custom battle).").into()) };
    if in_battle && matches!(cheat, Cheat::Heal | Cheat::Xp(_) | Cheat::Level(_) | Cheat::Unit(..) | Cheat::Time(_)) {
        return Err(tr("Not during a battle: the army is fighting.").into());
    }
    let done = run_on_game(cheat, game)?;
    game.cheats.used = true;
    Ok(done)
}

fn mark(game: Option<&mut Game>) {
    if let Some(g) = game {
        g.cheats.used = true;
    }
}

fn run_on_game(cheat: &Cheat, game: &mut Game) -> Result<Done, String> {
    let c = game.content.clone();
    Ok(match cheat {
        Cheat::Gold(n) => {
            game.gold = game.gold.saturating_add(*n);
            Done::line(trf!("Gold: {gold}.", gold = game.gold))
        }
        Cheat::Mana(n) => {
            game.mana = game.mana.saturating_add(*n);
            Done::line(trf!("Mana: {mana}.", mana = game.mana))
        }
        Cheat::Reveal => {
            if !game.fog.enabled {
                return Ok(Done::line(tr("This map has no fog: everything is in sight.").into()));
            }
            for y in 0..game.fog.h {
                for x in 0..game.fog.w {
                    game.fog.mark((x, y));
                }
            }
            Done::line(tr("The whole map is explored.").into())
        }
        Cheat::Heal => {
            let mut raised = 0;
            for u in &mut game.squad {
                if !u.alive() {
                    raised += 1;
                    u.died_at = None;
                }
                u.heal_full(&c);
            }
            Done::line(trf!("The army is healed; {raised} raised from the dead.", raised))
        }
        Cheat::Xp(n) => {
            let mut levels = 0;
            for u in game.squad.iter_mut().filter(|u| u.alive()) {
                levels += u.gain_xp(&c, *n);
            }
            Done::line(trf!("+{n} XP to every living unit; {levels} levels gained.", n, levels))
        }
        Cheat::Level(n) => {
            let hero = &mut game.squad[0];
            let before = hero.max_hp(&c);
            hero.level = *n;
            hero.xp = 0;
            hero.follow_max(&c, before);
            Done::line(trf!("The hero is at level {n}.", n))
        }
        Cheat::Item(q) => {
            let (item, name, hits) = find_item(&c, q)?;
            if game.pack.is_full() {
                return Err(tr("The pack is full.").into());
            }
            game.pack.add(item);
            Done::line(trf!("{item} is in the pack.", item = among(&name, hits)))
        }
        Cheat::Spell(q) => {
            let (id, name, hits) = find_spell(&c, q)?;
            let Ok(byte) = u8::try_from(id) else { return Err(trf!("Nothing has id {id}.", id)) };
            if game.knows_spell(id) {
                return Err(trf!("{spell} is in the book already.", spell = name));
            }
            game.spells.push(byte);
            Done::line(trf!("{spell} is written into the book.", spell = among(&name, hits)))
        }
        Cheat::Unit(q, level) => {
            let (kind, name, hits) = find_unit(&c, q)?;
            let taken: Vec<_> = game.squad.iter().map(|u| u.slot).collect();
            let slot = match c.formation.new_unit_slot(&taken) {
                Some(slot) if game.squad.len() < game.max_squad() => slot,
                _ => return Err(tr("No room in the army.").into()),
            };
            let mut u = Unit::new(&c, kind, slot);
            u.level = level.unwrap_or(1);
            u.heal_full(&c);
            u.last_paid = game.clock.total_minutes() as u64;
            game.squad.push(u);
            Done::line(trf!("{unit} (level {level}) joins the army.", unit = among(&name, hits), level = level.unwrap_or(1)))
        }
        Cheat::Time(h) => {
            let from = game.clock.label();
            let events = game.wait(*h);
            Done { lines: vec![trf!("Time passes: {from} → {to}.", from, to = game.clock.label())], events }
        }
        Cheat::Speed(n) => {
            game.cheats.speed = *n;
            Done::line(trf!("The hero walks {n} times faster.", n))
        }
        Cheat::Noclip => {
            game.cheats.noclip = !game.cheats.noclip;
            // A route planned for the other mode is dropped: the next click plans again.
            game.cut_walk();
            Done::line(if game.cheats.noclip { tr("Noclip on: the hero walks through anything.") } else { tr("Noclip off.") }.into())
        }
        Cheat::Peace => {
            game.cheats.peace = !game.cheats.peace;
            Done::line(if game.cheats.peace { tr("Peace on: enemy armies leave the hero alone.") } else { tr("Peace off.") }.into())
        }
        Cheat::Routes => {
            game.cheats.routes = !game.cheats.routes;
            Done::line(if game.cheats.routes { tr("Routes on: the minimap (M) shows where the enemy armies are heading.") } else { tr("Routes off.") }.into())
        }
        Cheat::Help | Cheat::Win | Cheat::Lose | Cheat::God => unreachable!("run handles it"),
    })
}

/// An enemy army's route for the `routes` cheat: the cell it stands on, then the cells of
/// the path its AI planned (`Army::path`, the original's path buffer, `rules::ai`), the last
/// one being where it heads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub from: (i32, i32),
    pub path: Vec<(i32, i32)>,
}

impl Route {
    /// Where the army heads: its path's last cell.
    pub fn destination(&self) -> (i32, i32) {
        self.path.last().copied().unwrap_or(self.from)
    }
}

/// The routes of the hostile armies that have a path, for the `routes` cheat (fog or not:
/// it is a cheat).
pub fn routes(game: &Game) -> Vec<Route> {
    let map = &game.world.map;
    game.world.armies.iter().filter(|a| a.hostile() && !a.path.is_empty()).map(|a| Route { from: a.tile(map), path: a.path.clone() }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::content::HeroClass;
    use crate::rules::game::STEP_SECONDS;
    use std::sync::Arc;

    fn game() -> Game {
        let mut g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        g.world.armies.clear();
        g
    }

    fn cheat(g: &mut Game, line: &str) -> Result<Done, String> {
        run(&parse(line).map_err(|e| e.to_string())?, Some(g), None)
    }

    #[test]
    fn parses_every_command() {
        assert_eq!(parse("help"), Ok(Cheat::Help));
        assert_eq!(parse("  GOLD   500 "), Ok(Cheat::Gold(500)));
        assert_eq!(parse("mana -20"), Ok(Cheat::Mana(-20)));
        assert_eq!(parse("reveal"), Ok(Cheat::Reveal));
        assert_eq!(parse("heal"), Ok(Cheat::Heal));
        assert_eq!(parse("xp 100"), Ok(Cheat::Xp(100)));
        assert_eq!(parse("level 7"), Ok(Cheat::Level(7)));
        assert_eq!(parse("item Long Sword"), Ok(Cheat::Item("Long Sword".into())));
        assert_eq!(parse("spell 3"), Ok(Cheat::Spell("3".into())));
        assert_eq!(parse("unit 12"), Ok(Cheat::Unit("12".into(), None)), "a lone number is the id");
        assert_eq!(parse("unit 12 5"), Ok(Cheat::Unit("12".into(), Some(5))));
        assert_eq!(parse("unit лучник 3"), Ok(Cheat::Unit("лучник".into(), Some(3))));
        assert_eq!(parse("time 24"), Ok(Cheat::Time(24)));
        assert_eq!(parse("win"), Ok(Cheat::Win));
        assert_eq!(parse("Lose"), Ok(Cheat::Lose));
        assert_eq!(parse("god"), Ok(Cheat::God));
        assert_eq!(parse("speed 3"), Ok(Cheat::Speed(3)));
        assert_eq!(parse("NoClip"), Ok(Cheat::Noclip));
        assert_eq!(parse("noclip 1"), Err(ParseError::Usage("noclip")));
        assert_eq!(parse("Peace"), Ok(Cheat::Peace));
        assert_eq!(parse("peace now"), Err(ParseError::Usage("peace")));
        assert_eq!(parse("Routes"), Ok(Cheat::Routes));
        assert_eq!(parse("routes on"), Err(ParseError::Usage("routes")));
        assert_eq!(COMMANDS.len(), 18);
        for (u, _) in COMMANDS {
            let name = u.split(' ').next().unwrap();
            assert!(!matches!(parse(name), Err(ParseError::Unknown(_))), "{name} is a command");
        }
    }

    #[test]
    fn refuses_what_is_not_a_command() {
        assert_eq!(parse("   "), Err(ParseError::Empty));
        assert_eq!(parse("money 5"), Err(ParseError::Unknown("money".into())));
        assert_eq!(parse("gold"), Err(ParseError::Usage("gold N")));
        assert_eq!(parse("gold lots"), Err(ParseError::Usage("gold N")));
        assert_eq!(parse("xp 0"), Err(ParseError::Usage("xp N")));
        assert_eq!(parse("level 1000"), Err(ParseError::Usage("level N")));
        assert_eq!(parse("time 0"), Err(ParseError::Usage("time H")));
        assert_eq!(parse("speed 99"), Err(ParseError::Usage("speed N")));
        assert_eq!(parse("item"), Err(ParseError::Usage("item <id | name>")));
        assert_eq!(parse("unit bandit 0"), Err(ParseError::Usage("unit <id | name> [level]")));
        assert_eq!(parse("win now"), Err(ParseError::Usage("win")));
        assert!(parse("money").unwrap_err().to_string().contains("money"));
    }

    #[test]
    fn gold_mana_and_the_mark() {
        let mut g = game();
        assert!(!g.cheats.used);
        cheat(&mut g, "help").unwrap();
        assert!(!g.cheats.used, "help is no cheat");
        assert!(cheat(&mut g, "item nothing-is-called-so").is_err());
        assert!(!g.cheats.used, "a command that failed leaves no mark");
        let (gold, mana) = (g.gold, g.mana);
        cheat(&mut g, "gold 500").unwrap();
        cheat(&mut g, "mana 40").unwrap();
        assert_eq!((g.gold, g.mana), (gold + 500, mana + 40));
        assert!(g.cheats.used);
        cheat(&mut g, "gold -100").unwrap();
        assert_eq!(g.gold, gold + 400);
    }

    #[test]
    fn reveal_explores_every_cell() {
        let mut g = game();
        g.fog = crate::rules::fog::Fog::new(g.world.map.w, g.world.map.h);
        assert!(g.fog.explored_count() < (g.fog.w * g.fog.h) as usize);
        cheat(&mut g, "reveal").unwrap();
        assert_eq!(g.fog.explored_count(), (g.fog.w * g.fog.h) as usize);
    }

    #[test]
    fn heal_raises_the_dead() {
        let mut g = game();
        let c = g.content.clone();
        cheat(&mut g, "unit 4").unwrap();
        g.squad[0].hp = 1;
        g.squad[1].hp = 0;
        g.squad[1].died_at = Some(5);
        cheat(&mut g, "heal").unwrap();
        for u in &g.squad {
            assert_eq!(u.hp, u.max_hp(&c));
            assert_eq!(u.died_at, None);
        }
    }

    #[test]
    fn xp_and_level() {
        let mut g = game();
        let c = g.content.clone();
        let need = g.squad[0].xp_to_next(&c);
        cheat(&mut g, &format!("xp {need}")).unwrap();
        assert_eq!(g.squad[0].level, 2);
        cheat(&mut g, "level 10").unwrap();
        assert_eq!((g.squad[0].level, g.squad[0].xp), (10, 0));
        assert_eq!(g.squad[0].hp, g.squad[0].max_hp(&c), "an unhurt hero stays unhurt");
    }

    #[test]
    fn items_spells_and_units_by_id_or_name() {
        let mut g = game();
        let c = g.content.clone();
        let item = &c.items[0];
        cheat(&mut g, &format!("item {}", item.id)).unwrap();
        assert_eq!(g.pack.items().last(), Some(&ItemId(item.id)));
        let part: String = item.name.chars().skip(1).take(4).collect::<String>().to_uppercase();
        cheat(&mut g, &format!("item {part}")).unwrap();
        assert!(g.pack.len() >= 2);
        let spell = &c.spells[0];
        cheat(&mut g, &format!("spell {}", spell.name)).unwrap();
        assert!(g.knows_spell(spell.id));
        assert!(cheat(&mut g, &format!("spell {}", spell.id)).is_err(), "known already");
        let before = g.squad.len();
        let unit = c.units.iter().find(|u| u.hits > 0 && u.id > 3).unwrap();
        cheat(&mut g, &format!("unit {} 4", unit.name)).unwrap();
        assert_eq!(g.squad.len(), before + 1);
        let u = g.squad.last().unwrap();
        assert_eq!((u.def, u.level, u.hp), (UnitId(unit.id), 4, u.max_hp(&c)));
        while g.squad.len() < g.max_squad() {
            cheat(&mut g, &format!("unit {}", unit.id)).unwrap();
        }
        assert!(cheat(&mut g, &format!("unit {}", unit.id)).is_err(), "no room");
    }

    #[test]
    fn time_passes_by_the_wait() {
        let mut g = game();
        let before = g.clock.total_minutes();
        cheat(&mut g, "time 5").unwrap();
        assert_eq!(g.clock.total_minutes(), before + 300.0);
    }

    #[test]
    fn speed_shortens_the_steps() {
        let mut g = game();
        let normal = g.step_time((2, 2), (3, 2));
        cheat(&mut g, "speed 4").unwrap();
        assert_eq!(g.step_time((2, 2), (3, 2)), normal / 4.0);
        cheat(&mut g, "speed 1").unwrap();
        assert_eq!(g.step_time((2, 2), (3, 2)), normal);
    }

    /// The game minutes of one real step of the hero, on a cell he came onto (his step base
    /// is set then, so the walk prices it from that base), with `speed N` on or off. The
    /// armies' clock runs by these minutes, so `speed` shortens only the hero's walk.
    #[test]
    fn speed_shortens_every_hero_step_so_the_armies_move_less_meanwhile() {
        // One real step to a passable neighbour: the game minutes it took.
        fn step(g: &mut Game) -> f64 {
            let from = g.tile();
            let to = [(1, 0), (0, 1), (-1, 0), (0, -1)]
                .iter()
                .map(|&(dx, dy)| (from.0 + dx, from.1 + dy))
                .find(|&t| g.set_destination(t))
                .expect("a passable neighbour");
            assert_eq!(g.path, vec![to]);
            let t0 = g.clock.total_minutes();
            while g.tile() == from {
                g.tick(STEP_SECONDS / 10.0);
            }
            g.clock.total_minutes() - t0
        }
        let minutes = |speed: Option<u32>| {
            let mut g = game();
            if let Some(n) = speed {
                cheat(&mut g, &format!("speed {n}")).unwrap();
            }
            step(&mut g);
            // The second step is priced from the cell the first one brought him onto.
            step(&mut g)
        };
        let normal = minutes(None);
        assert!(normal > 0.0);
        assert_eq!(minutes(Some(4)), normal / 4.0, "a quarter of the minutes, the hero's step only");
        assert_eq!(minutes(Some(1)), normal);
    }

    /// A battle against three spearmen.
    fn battle(g: &mut Game) -> Battle {
        let foes = vec![Unit::new(&g.content, UnitId(4), crate::rules::formation::Slot::new(crate::rules::formation::Row::Front, 0)); 3];
        let player: Vec<(usize, &Unit)> = g.squad.iter().enumerate().collect();
        let mut b = Battle::new(g.content.clone(), &player, &foes, Team::Player);
        b.auto_arrange(Team::Enemy);
        b
    }

    #[test]
    fn win_and_lose_end_the_battle() {
        let mut g = game();
        let mut b = battle(&mut g);
        assert!(run(&Cheat::Win, Some(&mut g), None).is_err(), "not on the map");
        assert!(run(&Cheat::Heal, Some(&mut g), Some(&mut b)).is_err(), "not during a battle");
        run(&Cheat::Win, Some(&mut g), Some(&mut b)).unwrap();
        assert_eq!(b.outcome(), Outcome::Victory);
        assert!(g.cheats.used);
        let mut b = battle(&mut g);
        run(&Cheat::Lose, Some(&mut g), Some(&mut b)).unwrap();
        assert_eq!(b.outcome(), Outcome::Defeat);
        assert!(run(&Cheat::Win, Some(&mut g), Some(&mut b)).is_err(), "a lost battle stays lost");
        assert_eq!(b.outcome(), Outcome::Defeat);
        assert!(run(&Cheat::Lose, Some(&mut g), Some(&mut b)).is_err());
        // A custom battle has no game behind it.
        let mut b = battle(&mut g);
        run(&Cheat::Win, None, Some(&mut b)).unwrap();
        assert_eq!(b.outcome(), Outcome::Victory);
        assert!(run(&Cheat::Gold(5), None, Some(&mut battle(&mut g))).is_err());
    }

    #[test]
    fn god_keeps_the_army_whole() {
        let mut g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        run(&Cheat::God, Some(&mut g), None).unwrap();
        assert!(g.cheats.god);
        g.foe = Some(crate::rules::game::Foe::Army(0));
        let mut b = g.start_battle();
        assert!(b.god, "the battle takes the switch");
        let hp: Vec<i32> = b.fighters.iter().filter(|f| f.team == Team::Player).map(|f| f.hp).collect();
        b.auto_play_to_end();
        let after: Vec<i32> = b.fighters.iter().filter(|f| f.team == Team::Player).map(|f| f.hp).collect();
        assert_eq!(hp, after, "no damage taken");
        assert_ne!(b.outcome(), Outcome::Defeat);
        run(&Cheat::God, Some(&mut g), Some(&mut b)).unwrap();
        assert!(!g.cheats.god && !b.god, "a second god switches it off");
    }

    #[test]
    fn peace_switches_on_and_off() {
        let mut g = game();
        assert!(!g.cheats.peace);
        cheat(&mut g, "peace").unwrap();
        assert!(g.cheats.peace && g.cheats.used);
        cheat(&mut g, "PEACE").unwrap();
        assert!(!g.cheats.peace, "a second peace switches it off");
    }

    #[test]
    fn routes_switch_on_and_show_the_enemies_planned_paths() {
        let mut g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        assert!(g.world.armies.len() >= 2, "the demo has gangs");
        cheat(&mut g, "routes").unwrap();
        assert!(g.cheats.routes && g.cheats.used);
        let map = g.world.map.clone();
        for a in &mut g.world.armies {
            a.path.clear();
        }
        let from = g.world.armies[0].tile(&map);
        g.world.armies[0].attitude = -3;
        g.world.armies[0].path = vec![(from.0 + 1, from.1), (from.0 + 2, from.1 + 1)];
        g.world.armies[1].attitude = 3;
        g.world.armies[1].path = vec![(1, 1)];
        let r = routes(&g);
        assert_eq!(r, vec![Route { from, path: vec![(from.0 + 1, from.1), (from.0 + 2, from.1 + 1)] }], "a friend's route is not shown");
        assert_eq!(r[0].destination(), (from.0 + 2, from.1 + 1));
        cheat(&mut g, "routes").unwrap();
        assert!(!g.cheats.routes, "a second routes switches it off");
        assert!(help().iter().any(|l| l.starts_with("routes")), "help lists it");
    }

    #[test]
    fn the_mark_is_saved() {
        let mut g = game();
        g.set_origin(crate::rules::save::ScenarioRef::Demo);
        cheat(&mut g, "speed 2").unwrap();
        cheat(&mut g, "god").unwrap();
        cheat(&mut g, "noclip").unwrap();
        cheat(&mut g, "peace").unwrap();
        cheat(&mut g, "routes").unwrap();
        let back = crate::rules::save::tests::roundtrip(&g, g.content.clone(), None);
        assert_eq!(back.cheats, CheatState { used: true, god: true, speed: 2, noclip: true, peace: true, routes: true });
        assert!(crate::rules::save::meta_of(&g, crate::rules::save::SaveKind::Manual, "x").unwrap().cheats);
    }

    fn labels(line: &str, names: &Names) -> Vec<String> {
        suggestions(line, names).into_iter().map(|s| s.label).collect()
    }

    fn completions(line: &str, names: &Names) -> Vec<String> {
        suggestions(line, names).into_iter().map(|s| s.completion).collect()
    }

    fn names() -> Names {
        Names {
            items: vec!["Iron Helm".into(), "Long Sword".into(), "Sword of Lore".into(), "Lodestone".into()],
            spells: vec!["Fireball".into(), "Heal Wounds".into()],
            units: vec!["Bandit".into(), "Archer".into()],
        }
    }

    #[test]
    fn a_command_prefix_lists_the_commands_it_starts_case_insensitively() {
        let none = Names::default();
        assert_eq!(labels("go", &none), vec!["gold N", "god"], "gold and god, in COMMANDS order");
        assert_eq!(labels("GO", &none), labels("go", &none), "any case");
        assert_eq!(completions("go", &none), vec!["gold ", "god "], "a command completes to its name and a space");
        let rows = suggestions("god", &none);
        assert_eq!(rows[0].hint, "on / off: your army takes no damage in battle", "the dim description");
        assert!(rows[0].alone, "god needs no argument");
        assert!(!suggestions("gold", &none)[0].alone, "gold needs its N");
        assert!(suggestions("", &none).is_empty(), "an empty line shows no list");
        assert!(suggestions("   ", &none).is_empty(), "nor a blank one");
        assert!(suggestions("zzz", &none).is_empty(), "no command starts with it");
        assert_eq!(labels("s", &none), vec!["spell <id | name>", "speed N"], "spell and speed");
        assert!(labels("t", &none).len() <= MOST_SUGGESTIONS);
    }

    #[test]
    fn item_spell_and_unit_names_match_the_prefix_first_then_the_inside() {
        let n = names();
        assert_eq!(labels("item lo", &n), vec!["Long Sword", "Lodestone", "Sword of Lore"], "prefix first, then contains");
        assert_eq!(completions("item lo", &n), vec!["item Long Sword", "item Lodestone", "item Sword of Lore"]);
        assert_eq!(labels("ITEM LO", &n), labels("item lo", &n), "the command and the name in any case");
        assert_eq!(completions("ITEM LO", &n)[0], "item Long Sword", "the command completes in lower case");
        assert_eq!(labels("spell heal", &n), vec!["Heal Wounds"]);
        assert_eq!(completions("spell fire", &n), vec!["spell Fireball"]);
        assert_eq!(completions("unit ban", &n), vec!["unit Bandit"]);
        assert_eq!(completions("unit ban 3", &n), vec!["unit Bandit 3"], "a typed level is kept");
        let hit = suggestions("item lo", &n)[0].clone();
        assert!(!hit.alone && hit.hint.is_empty(), "a name is not a command and has no description");
    }

    #[test]
    fn names_are_at_most_eight_and_need_a_query_and_a_game() {
        let many = Names { items: (0..12).map(|i| format!("Amulet {i}")).collect(), ..Default::default() };
        assert_eq!(suggestions("item am", &many).len(), MOST_SUGGESTIONS, "at most 8");
        assert!(suggestions("item lo", &Names::default()).is_empty(), "no game, no names");
        assert!(suggestions("item ", &many).is_empty(), "no query, no names");
        assert!(suggestions("item", &many).len() <= MOST_SUGGESTIONS, "the bare word is a command prefix");
        assert!(suggestions("gold 5", &many).is_empty(), "a command with no name list");
        assert!(suggestions("foo bar", &many).is_empty(), "an unknown command with a space");
        assert!(suggestions("item zzz", &names()).is_empty(), "no name holds it");
    }

    #[test]
    fn the_names_of_the_content_are_the_ones_the_commands_find() {
        let c = Content::builtin();
        let n = Names::of(&c);
        assert_eq!(n.items.len(), c.items.len());
        assert_eq!(n.spells.len(), c.spells.len());
        assert_eq!(n.units.len(), c.units.iter().filter(|u| u.hits > 0).count());
        let spell = &c.spells[0].name;
        let part: String = spell.chars().take(3).collect();
        assert!(suggestions(&format!("spell {part}"), &n).iter().any(|s| s.label == *spell), "{spell} is offered");
    }

    #[test]
    fn the_selection_wraps_forwards_and_backwards() {
        assert_eq!(cycle(None, 3, true), Some(0), "Tab with no row picked: the first");
        assert_eq!(cycle(None, 3, false), Some(2), "Shift+Tab with none: the last");
        assert_eq!(cycle(Some(0), 3, true), Some(1));
        assert_eq!(cycle(Some(2), 3, true), Some(0), "wraps to the first");
        assert_eq!(cycle(Some(0), 3, false), Some(2), "wraps to the last");
        assert_eq!(cycle(Some(0), 1, true), Some(0), "one row stays");
        assert_eq!(cycle(None, 0, true), None, "no rows, no selection");
    }
}
