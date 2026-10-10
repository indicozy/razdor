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
use super::game::{Event, Game, PACK_SIZE, SPELL_BOOK_SIZE};
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
}

/// The commands for `help`: (name and arguments, what it does).
pub const COMMANDS: [(&str, &str); 17] = [
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
            if game.pack.len() >= PACK_SIZE {
                return Err(tr("The pack is full.").into());
            }
            game.pack.push(item);
            Done::line(trf!("{item} is in the pack.", item = among(&name, hits)))
        }
        Cheat::Spell(q) => {
            let (id, name, hits) = find_spell(&c, q)?;
            let Ok(byte) = u8::try_from(id) else { return Err(trf!("Nothing has id {id}.", id)) };
            if game.knows_spell(id) {
                return Err(trf!("{spell} is in the book already.", spell = name));
            }
            if game.spells.len() >= SPELL_BOOK_SIZE {
                return Err(tr("The spell book is full.").into());
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
        Cheat::Help | Cheat::Win | Cheat::Lose | Cheat::God => unreachable!("run handles it"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::content::HeroClass;
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
        assert_eq!(COMMANDS.len(), 17);
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
        assert_eq!(g.pack.last(), Some(&ItemId(item.id)));
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
    fn the_mark_is_saved() {
        let mut g = game();
        g.set_origin(crate::rules::save::ScenarioRef::Demo);
        cheat(&mut g, "speed 2").unwrap();
        cheat(&mut g, "god").unwrap();
        cheat(&mut g, "noclip").unwrap();
        cheat(&mut g, "peace").unwrap();
        let back = crate::rules::save::tests::roundtrip(&g, g.content.clone(), None);
        assert_eq!(back.cheats, CheatState { used: true, god: true, speed: 2, noclip: true, peace: true });
        assert!(crate::rules::save::meta_of(&g, crate::rules::save::SaveKind::Manual, "x").unwrap().cheats);
    }
}
