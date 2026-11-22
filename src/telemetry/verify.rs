//! The battle verifier (`razdor-verify`, the analytics design's «battle replay check»): one battle the
//! server holds is replayed with the real engine on the owner's install, and its verdict says whether
//! the record could have been played.
//!
//! The input is the battle as `GET /v1/battles/{id}` returns it (the metadata, then the record). The
//! replay: the battle is rebuilt from its start (`BattleStart::rebuild`), each recorded player act is
//! applied with [`try_apply_battle`] (an act the engine refuses at its moment is suspicious), the
//! enemy's turns run the battle AI between the acts as the playback does (`play.rs`), and the battle's
//! result (winner, turns, losses) must be the metadata's.
//!
//! Three verdicts. `ok`: the record replays and ends with the stated result. `suspicious`: a check
//! failed. `unverified`: a battle this verifier cannot replay (a mod's battle, «Максимум» on the enemy
//! side, whose recorded moves are not replayed here); such a battle is not training data either.

use std::sync::Arc;

use razdor_wire::analytics::{BattleOutcome, Side};
use razdor_wire::frame;

use crate::rules::battle::{Battle, Outcome, Team};
use crate::rules::content::Content;
use crate::rules::recording::actions::{try_apply_battle, BattleAct};
use crate::rules::recording::battle_start::BattleStart;
use crate::rules::recording::file::read_cut;
use crate::rules::recording::play::plays_turn;
use crate::rules::recording::stream::{Entry, EngineInput, Input, PlayerAction};

/// The most battle AI steps one enemy run may take (a battle ends long before this).
const MAX_ENEMY_STEPS: usize = 10_000;

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    Suspicious(String),
    Unverified(String),
}

impl Verdict {
    /// The one line the verifier prints: `{"verdict": ..., "reason": ...}`.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Verdict::Ok => serde_json::json!({"verdict": "ok", "reason": ""}),
            Verdict::Suspicious(r) => serde_json::json!({"verdict": "suspicious", "reason": r}),
            Verdict::Unverified(r) => serde_json::json!({"verdict": "unverified", "reason": r}),
        }
    }
}

/// The result of a battle as the metadata states it: the winner, the turns and each side's losses
/// (the fighters at 0 HP, as the chronicle counts them).
pub fn result_of(b: &Battle) -> Result<BattleOutcome, String> {
    let winner = match b.outcome() {
        Outcome::Victory => Side::Player,
        Outcome::Defeat => Side::Foe,
        Outcome::Ongoing => return Err("the battle does not end".into()),
    };
    let losses = |team: Team| b.fighters.iter().filter(|f| f.team == team && f.hp <= 0).count() as u32;
    Ok(BattleOutcome { winner, turns: b.round, player_losses: losses(Team::Player), foe_losses: losses(Team::Enemy) })
}

/// What the server keeps of a battle beside its record (`GET /v1/battles/{id}`: the metadata as
/// JSON, then the record's bytes, framed). The record's hash was checked when the server took it.
#[derive(Debug, PartialEq, Eq)]
pub struct Claim {
    pub game: String,
    pub mod_hash: Option<String>,
    pub rules: u64,
    pub outcome: BattleOutcome,
}

impl Claim {
    /// Reads the metadata object the server sends (`battle_meta`).
    pub fn from_meta(meta: &serde_json::Value) -> Result<Claim, String> {
        let text = |k: &str| meta.get(k).and_then(|v| v.as_str()).map(str::to_string).ok_or(format!("the metadata has no {k}"));
        let num = |k: &str| meta.get(k).and_then(|v| v.as_u64()).ok_or(format!("the metadata has no {k}"));
        let winner: Side = serde_json::from_value(meta.get("winner").cloned().unwrap_or_default()).map_err(|e| format!("the winner: {e}"))?;
        Ok(Claim {
            game: text("game")?,
            mod_hash: meta.get("mod_hash").and_then(|v| v.as_str()).map(str::to_string),
            rules: num("rules")?,
            outcome: BattleOutcome {
                winner,
                turns: u32::try_from(num("turns")?).map_err(|_| "the turns")?,
                player_losses: u32::try_from(num("player_losses")?).map_err(|_| "the player's losses")?,
                foe_losses: u32::try_from(num("foe_losses")?).map_err(|_| "the foe's losses")?,
            },
        })
    }
}

/// Verifies one battle as the server returns it: the framed metadata and the record.
pub fn verify(fetched: &[u8], content: Arc<Content>) -> Verdict {
    let (meta, record) = match frame::decode::<serde_json::Value>(fetched) {
        Ok(x) => x,
        Err(e) => return Verdict::Suspicious(format!("not a battle: {e:?}")),
    };
    let claim = match Claim::from_meta(&meta) {
        Ok(c) => c,
        Err(e) => return Verdict::Suspicious(e),
    };
    if claim.game != "dt" || claim.mod_hash.is_some() {
        return Verdict::Unverified("a mod's battle: the verifier replays the base game only".into());
    }
    let (header, start, entries) = match read_cut(record) {
        Ok(x) => x,
        Err(e) => return Verdict::Suspicious(format!("the record does not read: {e}")),
    };
    if header.rules != claim.rules {
        return Verdict::Suspicious("the record's rules are not the metadata's".into());
    }
    if start.max_ai || entries.iter().any(|e| matches!(e.input, Input::Engine(EngineInput::Max { .. }))) {
        return Verdict::Unverified("«Максимум» on the enemy side: its recorded moves are not replayed here".into());
    }
    // The engine is not built to take any record: a panic on a record is a record the engine cannot
    // have played, so it is suspicious, and the nightly run goes on with the next battle.
    let played = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| replay(&start, content, &entries)));
    let battle = match played {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => return Verdict::Suspicious(e),
        Err(_) => return Verdict::Suspicious("the replay panicked: the record is not one the engine plays".into()),
    };
    match result_of(&battle) {
        Ok(got) if got == claim.outcome => Verdict::Ok,
        Ok(got) => Verdict::Suspicious(format!("the result is {got:?}, the metadata says {:?}", claim.outcome)),
        Err(e) => Verdict::Suspicious(e),
    }
}

/// The battle rebuilt from its start, with each recorded player act applied in order and the
/// enemy's turns between them (`play.rs`'s `battle_act`, without the «Максимум» moves).
fn replay(start: &BattleStart, content: Arc<Content>, entries: &[Entry]) -> Result<Battle, String> {
    let acts: Vec<&BattleAct> = entries
        .iter()
        .filter_map(|e| match &e.input {
            Input::Player(PlayerAction::Battle(a)) => Some(a),
            _ => None,
        })
        .collect();
    let mut b = start.rebuild(content);
    for (i, act) in acts.iter().enumerate() {
        // A battle recorded before the deployment step has no `Begin`: its first action began it.
        let legacy = b.is_deploying() && !matches!(act, BattleAct::Begin | BattleAct::MoveCard { .. } | BattleAct::SitOut { .. });
        if legacy {
            b.begin();
            run_enemy(&mut b, Some(act))?;
        }
        try_apply_battle(&mut b, act).map_err(|e| format!("act {} ({act:?}) is illegal: {e}", i + 1))?;
        run_enemy(&mut b, acts.get(i + 1).copied())?;
    }
    Ok(b)
}

/// The enemy's turns up to the player's turn or the end. Stops at a turn the next recorded act
/// plays itself (`plays_turn`), as the playback does.
fn run_enemy(b: &mut Battle, next: Option<&BattleAct>) -> Result<(), String> {
    let claimed = next.is_some_and(plays_turn);
    for _ in 0..MAX_ENEMY_STEPS {
        if b.outcome() != Outcome::Ongoing {
            return Ok(());
        }
        let Some(id) = b.active() else { return Ok(()) };
        if b.fighters[id].team != Team::Enemy || claimed {
            return Ok(());
        }
        b.ai_step();
    }
    Err("the battle does not end".into())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::rules::content::Content;
    use crate::rules::game::Game;
    use crate::rules::recording::battle_start::tests::{base, biggest, fresh};
    use crate::rules::recording::file::cut_bytes;
    use crate::rules::recording::header::{platform, ReplayHeader, HEADER_FORMAT};
    use crate::rules::recording::snapshot::Settings;
    use crate::rules::recording::stream::Stamp;
    use crate::rules::game::Foe;
    use crate::rules::content::ExperimentalRules;
    use razdor_wire::analytics::BattleOutcome;
    use razdor_wire::frame;

    fn header() -> ReplayHeader {
        ReplayHeader {
            format: HEADER_FORMAT,
            razdor: "9.9.9".into(),
            rules: 42,
            install: 7,
            map: Some("RK1.DTm".into()),
            map_hash: Some(0xabc),
            platform: platform(),
            settings: Settings { improved_ai: true, max_ai: false, max_guard: false, keep_xp_on_promotion: false, keep_formation: true, allow_unit_benching: false, pursue: true, walk_pace: 1.0 },
            experimental: ExperimentalRules::default(),
            language: "ru".into(),
        }
    }

    /// Plays a real battle of the biggest map's first army fight as the playback would: the player's
    /// acts are recorded (`Begin`, then each accepted act or a pass), the enemy's turns are not.
    /// Returns the battle's start and its acts.
    fn played(c: &Arc<Content>, dt: &crate::dt::install::DtInstall) -> Option<(BattleStart, Vec<BattleAct>, BattleOutcome)> {
        let (m, s) = biggest(dt);
        for i in 0..s.armies.len().min(8) {
            let mut g0: Game = fresh(c, &m, &s);
            g0.foe = Some(Foe::Army(i));
            let start = g0.battle_start();
            let mut live = g0.start_battle();
            let mut acts = Vec::new();
            if live.is_deploying() {
                live.begin();
                acts.push(BattleAct::Begin);
            }
            for _ in 0..2000 {
                if live.outcome() != Outcome::Ongoing {
                    break;
                }
                let Some(id) = live.active() else { break };
                if live.fighters[id].team == Team::Enemy {
                    live.ai_step();
                    continue;
                }
                let target = (0..live.fighters.len()).find(|&t| live.act(t).is_ok());
                match target {
                    Some(t) => acts.push(BattleAct::Act { target: t as u32 }),
                    None => {
                        live.pass();
                        acts.push(BattleAct::Pass);
                    }
                }
            }
            if live.outcome() != Outcome::Ongoing {
                let result = result_of(&live).unwrap();
                return Some((start, acts, result));
            }
        }
        None
    }

    fn entries(acts: &[BattleAct]) -> Vec<Entry> {
        acts.iter().enumerate().map(|(i, a)| Entry { at: Stamp { stretch: i as u64, ask: 0 }, input: Input::Player(PlayerAction::Battle(a.clone())) }).collect()
    }

    /// The fetched frame of a record of `acts` with `outcome`, as the server sends it.
    fn fetched(start: &BattleStart, acts: &[BattleAct], outcome: BattleOutcome) -> Vec<u8> {
        let record = cut_bytes(&header(), start, &entries(acts)).unwrap();
        let meta = serde_json::json!({
            "game": "dt",
            "mod_hash": null,
            "rules": header().rules,
            "winner": outcome.winner,
            "turns": outcome.turns,
            "player_losses": outcome.player_losses,
            "foe_losses": outcome.foe_losses,
        });
        frame::encode(&meta, &record).unwrap()
    }

    #[test]
    fn a_played_battle_verifies_and_its_tampered_copies_do_not() {
        let Some(dt) = base() else { return };
        let c = Arc::new(Content::from_dt(&dt));
        let (start, acts, result) = played(&c, &dt).expect("a battle of the biggest map ends");
        assert!(!acts.is_empty());

        // The honest record: every act legal, the result the battle's own.
        assert_eq!(verify(&fetched(&start, &acts, result), c.clone()), Verdict::Ok);

        // A changed result: the metadata claims a turn more than the battle had.
        let claimed = BattleOutcome { turns: result.turns + 1, ..result };
        match verify(&fetched(&start, &acts, claimed), c.clone()) {
            Verdict::Suspicious(r) => assert!(r.contains("the result is"), "{r}"),
            other => panic!("a changed result must be suspicious, got {other:?}"),
        }

        // An illegal act: a target outside the battle, placed where the battle has begun.
        let mut bad = acts.clone();
        bad.insert(usize::from(matches!(bad.first(), Some(BattleAct::Begin))), BattleAct::Act { target: 9_999 });
        match verify(&fetched(&start, &bad, result), c.clone()) {
            Verdict::Suspicious(r) => assert!(r.contains("illegal"), "{r}"),
            other => panic!("an illegal act must be suspicious, got {other:?}"),
        }
    }

    #[test]
    fn a_frame_that_is_not_a_battle_is_suspicious() {
        let Some(dt) = base() else { return };
        let c = Arc::new(Content::from_dt(&dt));
        assert!(matches!(verify(&[1, 0, 0], c), Verdict::Suspicious(_)));
    }

    #[test]
    fn the_verdict_line_is_the_json_the_script_reads() {
        assert_eq!(Verdict::Ok.to_json(), serde_json::json!({"verdict": "ok", "reason": ""}));
        assert_eq!(Verdict::Suspicious("x".into()).to_json()["verdict"], "suspicious");
        assert_eq!(Verdict::Unverified("y".into()).to_json()["verdict"], "unverified");
    }

}
