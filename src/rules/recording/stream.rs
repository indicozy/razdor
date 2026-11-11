//! The replay input stream: what a replay needs to play a game again. It holds the player's
//! actions and the engine records (what real time decided: the event generator's seed, the
//! music rotation's draws, «Максимум»'s chosen moves, the battle patch globals at a battle's
//! start). Each entry has a [`Stamp`]: the stretch of time (`Game::stretches`) it happened in
//! and how many arrivals of that stretch had been asked for, so a draw made between two frames
//! lands at the same place when it is played again. Encoding: a version byte, then LEB128
//! varints (zigzag for signed values), about 3 to 4 bytes an action.

use crate::rules::battle::PatchGlobals;

/// The encoding's version (the first byte).
pub const STREAM_VERSION: u8 = 1;

/// Where in a game an entry happened: the stretch of time (`Game::stretches`, a walk step or a
/// wait tick), and the number of AI arrivals asked for inside it (0 between two stretches).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stamp {
    pub stretch: u64,
    pub ask: u32,
}

/// What the player did, in the form the rules take it (not the interface's clicks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerAction {
    /// A walk to map cell (x, y): the second click of the original.
    ClickMap { x: i32, y: i32 },
    /// A wait of 1 or 4 hours.
    Wait { hours: u32 },
    /// The answer to the question on screen.
    Answer { yes: bool },
    /// Closes the dialog on screen.
    Ok,
    /// In battle, a press on a card (`side` 1 the player's, 2 the enemy's; `row`, `col` as the grid).
    BattleAct { side: u8, row: i32, col: i32 },
    /// In battle, the space key.
    BattlePass,
    /// In battle, the auto-battle key.
    BattleAuto,
    /// Market, hire, heal, resurrect, learn, take and cast, equip: the row, unit or book entry.
    Buy { slot: u32 },
    Sell { slot: u32 },
    Hire { slot: u32 },
    Heal { unit: u32 },
    Resurrect { unit: u32 },
    Learn { slot: u32 },
    Take { slot: u32 },
    Cast { slot: u32, army: Option<i32> },
    Equip { slot: u32, unit: u32 },
}

/// A «Максимум» decision as it is recorded: the plan's action, its target and kind (the
/// kinds are numbered as [`crate::rules::battle::ActionKind`] orders them, see `max_ai`), a
/// move to a cell, a pass, or the skip of a unit's turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaxCode {
    Skip,
    Act { target: u32, kind: u8 },
    Move { row: u8, col: u8 },
    Pass,
}

/// The patch globals at a battle's start (`battle::PatchGlobals`, numbers as they are).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchRecord {
    pub battle: u32,
    pub hunger_counter: u32,
    pub hunger_seen: u32,
    pub screen_living: u32,
    pub side_blocks: [u32; 2],
}

/// What real time decided, recorded as it was decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineInput {
    /// A game starts (a new game, `load` false, or a load): the event generator's seed and the
    /// patch globals it starts with (the globals outlive a game, so a new game may not start
    /// from the defaults; `patch.battle` is not stored). Drawn at the start, in this order.
    GameStart { load: bool, seed: u32, patch: PatchRecord },
    /// The map music changed to track `pick` (the draw was made at the stamp).
    Music { pick: u8 },
    /// The enemy's «Максимум» decision number `decision` of battle `battle` (its start's number).
    Max { battle: u32, decision: u32, code: MaxCode },
    /// The patch globals at the start of battle `battle`.
    Patch(PatchRecord),
}

impl PatchRecord {
    /// The record of battle `battle`'s patch globals `g`.
    pub fn of(battle: u32, g: PatchGlobals) -> PatchRecord {
        PatchRecord {
            battle,
            hunger_counter: g.hunger_counter as u32,
            hunger_seen: g.hunger_seen as u32,
            screen_living: g.screen_living as u32,
            side_blocks: [g.side_blocks[0] as u32, g.side_blocks[1] as u32],
        }
    }

    /// The patch globals the record holds.
    pub fn globals(&self) -> PatchGlobals {
        PatchGlobals {
            hunger_counter: self.hunger_counter as usize,
            hunger_seen: self.hunger_seen as usize,
            screen_living: self.screen_living as usize,
            side_blocks: [self.side_blocks[0] as usize, self.side_blocks[1] as usize],
        }
    }
}

/// One entry of the stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Player(PlayerAction),
    Engine(EngineInput),
}

/// An input and the place it happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub at: Stamp,
    pub input: Input,
}

/// Appends `v` as an unsigned LEB128 varint.
pub fn put_u(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Appends `v` as a zigzag varint.
pub fn put_i(out: &mut Vec<u8>, v: i64) {
    put_u(out, ((v << 1) ^ (v >> 63)) as u64);
}

/// Reads what [`put_u`] and [`put_i`] wrote.
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, at: 0 }
    }

    pub fn u(&mut self) -> Result<u64, String> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = *self.bytes.get(self.at).ok_or("the stream ends inside a number")?;
            self.at += 1;
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("a number longer than 64 bits".into())
    }

    pub fn i(&mut self) -> Result<i64, String> {
        let v = self.u()?;
        Ok((v >> 1) as i64 ^ -((v & 1) as i64))
    }

    /// A number that must fit in 32 bits.
    fn u32(&mut self) -> Result<u32, String> {
        u32::try_from(self.u()?).map_err(|_| "a number out of range".to_string())
    }

    fn i32(&mut self) -> Result<i32, String> {
        i32::try_from(self.i()?).map_err(|_| "a number out of range".to_string())
    }

    fn u8(&mut self) -> Result<u8, String> {
        u8::try_from(self.u()?).map_err(|_| "a number out of range".to_string())
    }

    fn bool(&mut self) -> Result<bool, String> {
        match self.u()? {
            0 => Ok(false),
            1 => Ok(true),
            n => Err(format!("a flag of {n}")),
        }
    }

    pub fn is_done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

/// A stamp as its difference from the one before (zigzag) and its ask count.
fn put_stamp(out: &mut Vec<u8>, prev: Stamp, at: Stamp) {
    put_i(out, at.stretch.wrapping_sub(prev.stretch) as i64);
    put_u(out, at.ask as u64);
}

fn put_player(out: &mut Vec<u8>, a: PlayerAction) {
    match a {
        PlayerAction::ClickMap { x, y } => {
            put_u(out, 1);
            put_i(out, x as i64);
            put_i(out, y as i64);
        }
        PlayerAction::Wait { hours } => {
            put_u(out, 2);
            put_u(out, hours as u64);
        }
        PlayerAction::Answer { yes } => {
            put_u(out, 3);
            put_u(out, yes as u64);
        }
        PlayerAction::Ok => put_u(out, 4),
        PlayerAction::BattleAct { side, row, col } => {
            put_u(out, 5);
            put_u(out, side as u64);
            put_i(out, row as i64);
            put_i(out, col as i64);
        }
        PlayerAction::BattlePass => put_u(out, 6),
        PlayerAction::BattleAuto => put_u(out, 7),
        PlayerAction::Buy { slot } => put_slot(out, 8, slot),
        PlayerAction::Sell { slot } => put_slot(out, 9, slot),
        PlayerAction::Hire { slot } => put_slot(out, 10, slot),
        PlayerAction::Heal { unit } => put_slot(out, 11, unit),
        PlayerAction::Resurrect { unit } => put_slot(out, 12, unit),
        PlayerAction::Learn { slot } => put_slot(out, 13, slot),
        PlayerAction::Take { slot } => put_slot(out, 14, slot),
        PlayerAction::Cast { slot, army } => {
            put_u(out, 15);
            put_u(out, slot as u64);
            put_u(out, army.is_some() as u64);
            if let Some(a) = army {
                put_i(out, a as i64);
            }
        }
        PlayerAction::Equip { slot, unit } => {
            put_u(out, 16);
            put_u(out, slot as u64);
            put_u(out, unit as u64);
        }
    }
}

fn put_slot(out: &mut Vec<u8>, tag: u64, slot: u32) {
    put_u(out, tag);
    put_u(out, slot as u64);
}

fn put_code(out: &mut Vec<u8>, c: MaxCode) {
    match c {
        MaxCode::Skip => put_u(out, 0),
        MaxCode::Act { target, kind } => {
            put_u(out, 1);
            put_u(out, target as u64);
            put_u(out, kind as u64);
        }
        MaxCode::Move { row, col } => {
            put_u(out, 2);
            put_u(out, row as u64);
            put_u(out, col as u64);
        }
        MaxCode::Pass => put_u(out, 3),
    }
}

/// The patch globals' numbers (not the battle's).
fn put_patch(out: &mut Vec<u8>, p: PatchRecord) {
    put_u(out, p.hunger_counter as u64);
    put_u(out, p.hunger_seen as u64);
    put_u(out, p.screen_living as u64);
    put_u(out, p.side_blocks[0] as u64);
    put_u(out, p.side_blocks[1] as u64);
}

fn get_patch(r: &mut Reader, battle: u32) -> Result<PatchRecord, String> {
    Ok(PatchRecord {
        battle,
        hunger_counter: r.u32()?,
        hunger_seen: r.u32()?,
        screen_living: r.u32()?,
        side_blocks: [r.u32()?, r.u32()?],
    })
}

fn put_engine(out: &mut Vec<u8>, e: EngineInput) {
    match e {
        EngineInput::GameStart { load, seed, patch } => {
            put_u(out, 32);
            put_u(out, load as u64);
            put_u(out, seed as u64);
            put_patch(out, patch);
        }
        EngineInput::Music { pick } => {
            put_u(out, 33);
            put_u(out, pick as u64);
        }
        EngineInput::Max { battle, decision, code } => {
            put_u(out, 34);
            put_u(out, battle as u64);
            put_u(out, decision as u64);
            put_code(out, code);
        }
        EngineInput::Patch(p) => {
            put_u(out, 35);
            put_u(out, p.battle as u64);
            put_patch(out, p);
        }
    }
}

/// The stream's bytes: the version, the count, then each entry.
pub fn encode(entries: &[Entry]) -> Vec<u8> {
    let mut out = vec![STREAM_VERSION];
    put_u(&mut out, entries.len() as u64);
    let mut prev = Stamp::default();
    for e in entries {
        put_stamp(&mut out, prev, e.at);
        prev = e.at;
        match e.input {
            Input::Player(a) => {
                put_u(&mut out, 0);
                put_player(&mut out, a);
            }
            Input::Engine(x) => {
                put_u(&mut out, 1);
                put_engine(&mut out, x);
            }
        }
    }
    out
}

fn get_player(r: &mut Reader) -> Result<PlayerAction, String> {
    Ok(match r.u()? {
        1 => PlayerAction::ClickMap { x: r.i32()?, y: r.i32()? },
        2 => PlayerAction::Wait { hours: r.u32()? },
        3 => PlayerAction::Answer { yes: r.bool()? },
        4 => PlayerAction::Ok,
        5 => PlayerAction::BattleAct { side: r.u8()?, row: r.i32()?, col: r.i32()? },
        6 => PlayerAction::BattlePass,
        7 => PlayerAction::BattleAuto,
        8 => PlayerAction::Buy { slot: r.u32()? },
        9 => PlayerAction::Sell { slot: r.u32()? },
        10 => PlayerAction::Hire { slot: r.u32()? },
        11 => PlayerAction::Heal { unit: r.u32()? },
        12 => PlayerAction::Resurrect { unit: r.u32()? },
        13 => PlayerAction::Learn { slot: r.u32()? },
        14 => PlayerAction::Take { slot: r.u32()? },
        15 => {
            let slot = r.u32()?;
            let army = if r.bool()? { Some(r.i32()?) } else { None };
            PlayerAction::Cast { slot, army }
        }
        16 => PlayerAction::Equip { slot: r.u32()?, unit: r.u32()? },
        t => return Err(format!("an unknown action {t}")),
    })
}

fn get_code(r: &mut Reader) -> Result<MaxCode, String> {
    Ok(match r.u()? {
        0 => MaxCode::Skip,
        1 => MaxCode::Act { target: r.u32()?, kind: r.u8()? },
        2 => MaxCode::Move { row: r.u8()?, col: r.u8()? },
        3 => MaxCode::Pass,
        t => return Err(format!("an unknown decision {t}")),
    })
}

fn get_engine(r: &mut Reader) -> Result<EngineInput, String> {
    Ok(match r.u()? {
        32 => EngineInput::GameStart { load: r.bool()?, seed: r.u32()?, patch: get_patch(r, 0)? },
        33 => EngineInput::Music { pick: r.u8()? },
        34 => EngineInput::Max { battle: r.u32()?, decision: r.u32()?, code: get_code(r)? },
        35 => {
            let battle = r.u32()?;
            EngineInput::Patch(get_patch(r, battle)?)
        }
        t => return Err(format!("an unknown record {t}")),
    })
}

/// The entries [`encode`] wrote; an error for bytes it could not have written.
pub fn decode(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    if bytes.first() != Some(&STREAM_VERSION) {
        return Err("not a replay stream of this version".into());
    }
    let mut r = Reader::new(&bytes[1..]);
    let n = r.u()?;
    let mut out = Vec::new();
    let mut prev = Stamp::default();
    for _ in 0..n {
        let stretch = prev.stretch.wrapping_add(r.i()? as u64);
        let at = Stamp { stretch, ask: r.u32()? };
        prev = at;
        let input = match r.u()? {
            0 => Input::Player(get_player(&mut r)?),
            1 => Input::Engine(get_engine(&mut r)?),
            t => return Err(format!("an unknown input kind {t}")),
        };
        out.push(Entry { at, input });
    }
    if !r.is_done() {
        return Err("bytes after the last entry".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> PatchRecord {
        PatchRecord { battle: 0, hunger_counter: 3, hunger_seen: 3, screen_living: 0, side_blocks: [0, 0] }
    }

    fn sample() -> Vec<Entry> {
        let at = |stretch, ask| Stamp { stretch, ask };
        vec![
            Entry { at: at(0, 0), input: Input::Engine(EngineInput::GameStart { load: false, seed: 0x9e37_79b9, patch: fresh() }) },
            Entry { at: at(0, 0), input: Input::Player(PlayerAction::ClickMap { x: -3, y: 250 }) },
            Entry { at: at(7, 0), input: Input::Engine(EngineInput::Music { pick: 5 }) },
            Entry { at: at(7, 12), input: Input::Engine(EngineInput::Music { pick: 0 }) },
            Entry { at: at(u64::MAX, u32::MAX), input: Input::Player(PlayerAction::Wait { hours: 4 }) },
            Entry { at: at(9, 1), input: Input::Player(PlayerAction::Answer { yes: true }) },
            Entry { at: at(9, 1), input: Input::Player(PlayerAction::Ok) },
            Entry { at: at(9, 2), input: Input::Player(PlayerAction::BattleAct { side: 1, row: 2, col: -1 }) },
            Entry { at: at(9, 2), input: Input::Player(PlayerAction::BattlePass) },
            Entry { at: at(9, 2), input: Input::Player(PlayerAction::BattleAuto) },
            Entry { at: at(10, 0), input: Input::Player(PlayerAction::Cast { slot: 3, army: None }) },
            Entry { at: at(10, 0), input: Input::Player(PlayerAction::Cast { slot: 0, army: Some(-7) }) },
            Entry { at: at(10, 0), input: Input::Player(PlayerAction::Equip { slot: 12, unit: 4 }) },
            Entry { at: at(10, 0), input: Input::Player(PlayerAction::Take { slot: 1 }) },
            Entry { at: at(10, 0), input: Input::Player(PlayerAction::Buy { slot: 2 }) },
            Entry { at: at(10, 0), input: Input::Player(PlayerAction::Sell { slot: 2 }) },
            Entry { at: at(10, 0), input: Input::Player(PlayerAction::Hire { slot: 2 }) },
            Entry { at: at(10, 0), input: Input::Player(PlayerAction::Heal { unit: 2 }) },
            Entry { at: at(10, 0), input: Input::Player(PlayerAction::Resurrect { unit: 2 }) },
            Entry { at: at(10, 0), input: Input::Player(PlayerAction::Learn { slot: 2 }) },
            Entry { at: at(11, 0), input: Input::Engine(EngineInput::Max { battle: 3, decision: 0, code: MaxCode::Skip }) },
            Entry { at: at(11, 0), input: Input::Engine(EngineInput::Max { battle: 3, decision: 1, code: MaxCode::Act { target: 40, kind: 6 } }) },
            Entry { at: at(11, 0), input: Input::Engine(EngineInput::Max { battle: 3, decision: 2, code: MaxCode::Move { row: 2, col: 5 } }) },
            Entry { at: at(11, 0), input: Input::Engine(EngineInput::Max { battle: 3, decision: 3, code: MaxCode::Pass }) },
            Entry {
                at: at(11, 0),
                input: Input::Engine(EngineInput::Patch(PatchRecord { battle: 3, hunger_counter: 3, hunger_seen: 3, screen_living: 14, side_blocks: [7, 9] })),
            },
            Entry { at: at(12, 0), input: Input::Engine(EngineInput::GameStart { load: true, seed: u32::MAX, patch: PatchRecord { battle: 0, hunger_counter: 9, hunger_seen: 2, screen_living: 7, side_blocks: [1, 0] } }) },
        ]
    }

    #[test]
    fn every_kind_round_trips() {
        let entries = sample();
        let bytes = encode(&entries);
        assert_eq!(decode(&bytes).unwrap(), entries);
        assert_eq!(decode(&encode(&[])).unwrap(), Vec::<Entry>::new());
    }

    #[test]
    fn numbers_are_varints_and_signs_are_zigzag() {
        for v in [0u64, 1, 127, 128, 300, 16_383, 16_384, u32::MAX as u64, u64::MAX] {
            let mut out = Vec::new();
            put_u(&mut out, v);
            assert_eq!(Reader::new(&out).u().unwrap(), v);
        }
        for v in [0i64, -1, 1, -64, 63, i32::MIN as i64, i32::MAX as i64, i64::MIN, i64::MAX] {
            let mut out = Vec::new();
            put_i(&mut out, v);
            assert_eq!(Reader::new(&out).i().unwrap(), v);
        }
        let mut out = Vec::new();
        put_i(&mut out, -1);
        assert_eq!(out, [1], "-1 is one byte");
    }

    #[test]
    fn actions_take_a_few_bytes_each() {
        // A walk: a stamp and a cell near the start of a map, about 4 bytes each.
        let entries: Vec<Entry> = (0..1000)
            .map(|i| Entry { at: Stamp { stretch: 40 + i / 3, ask: 0 }, input: Input::Player(PlayerAction::ClickMap { x: 12 + (i as i32 % 30), y: 40 - (i as i32 % 20) }) })
            .collect();
        let bytes = encode(&entries);
        let per = bytes.len() as f64 / entries.len() as f64;
        assert!(per < 6.5, "{per:.2} bytes an action");
        assert!(per > 3.0, "the sample is not empty ({per:.2} bytes)");
    }

    #[test]
    fn damaged_streams_are_refused() {
        let bytes = encode(&sample());
        assert!(decode(&[]).is_err());
        assert!(decode(&[9]).is_err(), "an unknown version");
        assert!(decode(&bytes[..bytes.len() - 1]).is_err(), "truncated");
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode(&extra).is_err(), "bytes after the last entry");
        let mut bad = encode(&[Entry { at: Stamp::default(), input: Input::Player(PlayerAction::Ok) }]);
        *bad.last_mut().unwrap() = 99;
        assert!(decode(&bad).is_err(), "an unknown action");
    }
}
