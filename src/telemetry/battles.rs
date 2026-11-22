//! Battle records (the analytics' section 3, `docs/superpowers/specs/2026-11-22-analytics-design.md`):
//! one battle the player fought, packed as its own record and described by its upload header.
//!
//! The record is the battle's header, its start (`BattleStart`) and its entries from the start to
//! the end: a battle cut (`rules::recording::file::cut_bytes`), the same blocks a replay uses. The
//! upload header is `razdor_wire::analytics::BattleUpload`, which the server checks.
//!
//! Pure: nothing here reads the clock, the install or the queue's folder. The caller passes the
//! time; [`submit`] queues through the hub and keeps the hour's cadence and cap.

use razdor_wire::analytics::{BattleOutcome, BattleTag, BattleUpload};
use razdor_wire::{limits, paths, InstallId, Validate, SCHEMA};
use sha2::{Digest, Sha256};

use super::queue::Class;
use super::state::Consent;
use super::{Hub, Refused};
use crate::rules::recording::battle_start::BattleStart;
use crate::rules::recording::file::cut_bytes;
use crate::rules::recording::header::ReplayHeader;
use crate::rules::recording::stream::Entry;

/// What the upload header takes from the run beside the record.
pub struct Facts<'a> {
    /// The install id (32 hex digits).
    pub install_id: &'a str,
    /// The game id (`dt`, or the active mod's id).
    pub game: &'a str,
    /// The hub mod's hash, when a mod is played; none for the base game.
    pub mod_hash: Option<String>,
    /// The experiments that are on, by name.
    pub experiments: Vec<String>,
    pub tags: Vec<BattleTag>,
    pub outcome: BattleOutcome,
    /// Unix seconds: the battle's start and its end.
    pub started_at: u64,
    pub ended_at: u64,
}

/// A packed battle: the record's bytes and the upload header that describes them.
#[derive(Debug, PartialEq)]
pub struct Packed {
    pub upload: BattleUpload,
    pub record: Vec<u8>,
}

/// Packs one battle: its record (`cut_bytes`) and the upload header with the record's hash and length.
/// The header is checked (`BattleUpload::validate`) before it is returned. Deterministic: the same input
/// gives the same bytes.
pub fn pack(header: &ReplayHeader, start: &BattleStart, entries: &[Entry], facts: &Facts) -> Result<Packed, String> {
    let map = header.map.clone().ok_or("a battle with no map is not recorded")?;
    let map_hash = header.map_hash.ok_or("a battle with no map hash is not recorded")?;
    let record = cut_bytes(header, start, entries)?;
    let record_sha256: String = Sha256::digest(&record).iter().map(|b| format!("{b:02x}")).collect();
    let upload = BattleUpload {
        schema: SCHEMA,
        install_id: InstallId::parse(facts.install_id).map_err(|e| e.to_string())?,
        razdor: env!("CARGO_PKG_VERSION").to_string(),
        platform: super::platform(),
        game: facts.game.to_string(),
        map,
        map_hash: format!("{map_hash:016x}"),
        mod_hash: facts.mod_hash.clone(),
        rules: header.rules,
        experiments: facts.experiments.clone(),
        tags: facts.tags.clone(),
        outcome: facts.outcome,
        started_at: facts.started_at,
        ended_at: facts.ended_at,
        record_sha256,
        record_len: record.len() as u64,
    };
    upload.validate().map_err(|e| e.to_string())?;
    Ok(Packed { upload, record })
}

/// The next UTC hour boundary after `now` (unix seconds): when a battle record is sent. A battle at
/// 10:59:59 waits for 11:00:00, and one at 11:00:00 for 12:00:00.
pub fn send_at(now: u64) -> u64 {
    now / 3600 * 3600 + 3600
}

/// Queues a packed battle at `now` (unix seconds), to be sent at [`send_at`]. Nothing is queued
/// without the consent yes. When the hour's cap (`BATTLES_PER_HOUR` records for one boundary) is full,
/// the record is dropped and counted (`Hub::battles_dropped`), not queued.
pub fn submit(hub: &Hub, packed: &Packed, now: u64) -> Result<u64, Refused> {
    if hub.consent() != Consent::Yes {
        return Err(Refused::NoConsent);
    }
    packed.upload.validate().map_err(|e| Refused::Invalid(e.to_string()))?;
    if packed.upload.record_len != packed.record.len() as u64 {
        return Err(Refused::Invalid("the record's length is not the header's".into()));
    }
    let at = send_at(now);
    // The items already queued for this boundary (a restart keeps them: they are on disk).
    let queued = hub.queue().metas().filter(|m| m.path == paths::BATTLES && m.not_before == at).count();
    if queued >= limits::BATTLES_PER_HOUR as usize {
        hub.count_battle_dropped();
        return Err(Refused::BattleCap);
    }
    let header = serde_json::to_value(&packed.upload).map_err(|e| Refused::Invalid(e.to_string()))?;
    hub.enqueue_frame_at(paths::BATTLES, Class::Consent, header, packed.record.clone(), None, None, at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::battle::{patch_globals, Team};
    use crate::rules::content::ExperimentalRules;
    use crate::rules::recording::header::{platform, HEADER_FORMAT};
    use crate::rules::recording::snapshot::Settings;
    use crate::rules::recording::stream::{EngineInput, Input, Stamp};
    use razdor_wire::analytics::Side;
    use std::path::PathBuf;

    const INSTALL: &str = "0123456789abcdef0123456789abcdef";

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-battles-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn hub(name: &str, consent: Consent) -> (Hub, PathBuf) {
        let dir = dir(name);
        let hub = Hub::open(&dir, limits::CLIENT_QUEUE_BYTES).unwrap();
        hub.set_consent(consent, "test");
        (hub, dir)
    }

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

    /// A battle start with no units (the units' own serialisation is the rules'): the team and the
    /// patch globals are the real ones.
    fn start(ordinal: u32) -> BattleStart {
        serde_json::from_value(serde_json::json!({
            "ordinal": ordinal,
            "attacker": Team::Player,
            "player": [],
            "enemy": [],
            "bench": [],
            "xp_correction": 0,
            "improved_ai": false,
            "max_ai": false,
            "max_guard": false,
            "god": false,
            "bare_enemy": false,
            "building_defence": [0, 0],
            "patch": patch_globals(),
        }))
        .unwrap()
    }

    fn entries() -> Vec<Entry> {
        (0..5u32).map(|i| Entry { at: Stamp { stretch: u64::from(i), ask: 2 * i }, input: Input::Engine(EngineInput::Check { hash: 100 + u64::from(i) }) }).collect()
    }

    fn facts(tags: Vec<BattleTag>, ended_at: u64) -> Facts<'static> {
        Facts {
            install_id: INSTALL,
            game: "dt",
            mod_hash: None,
            experiments: vec!["fast_ai".into()],
            tags,
            outcome: BattleOutcome { winner: Side::Player, turns: 12, player_losses: 2, foe_losses: 9 },
            started_at: ended_at - 300,
            ended_at,
        }
    }

    #[test]
    fn packing_is_deterministic_for_the_same_input() {
        let a = pack(&header(), &start(3), &entries(), &facts(vec![], 1_800_000_000)).unwrap();
        let b = pack(&header(), &start(3), &entries(), &facts(vec![], 1_800_000_000)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn the_header_validates_and_the_hash_and_length_match_the_bytes() {
        let p = pack(&header(), &start(3), &entries(), &facts(vec![BattleTag::Autobattle], 1_800_000_000)).unwrap();
        assert!(p.upload.validate().is_ok());
        assert_eq!(p.upload.record_len, p.record.len() as u64);
        let sha: String = Sha256::digest(&p.record).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(p.upload.record_sha256, sha);
        assert_eq!(p.upload.map_hash, "0000000000000abc");
        assert_eq!(p.upload.rules, 42);
        assert_eq!(p.upload.mod_hash, None);
    }

    #[test]
    fn the_record_reads_back_as_the_battle_it_packs() {
        let p = pack(&header(), &start(3), &entries(), &facts(vec![], 1_800_000_000)).unwrap();
        let (h, s, e) = crate::rules::recording::file::read_cut(&p.record).unwrap();
        assert_eq!(h, header());
        assert_eq!(s, start(3));
        assert_eq!(e, entries());
    }

    #[test]
    fn a_battle_with_no_map_is_not_packed() {
        let mut h = header();
        h.map = None;
        assert!(pack(&h, &start(1), &entries(), &facts(vec![], 1_800_000_000)).is_err());
    }

    #[test]
    fn the_send_time_is_the_next_utc_hour() {
        assert_eq!(send_at(10 * 3600 + 59 * 60 + 59), 11 * 3600);
        assert_eq!(send_at(11 * 3600), 12 * 3600);
        assert_eq!(send_at(11 * 3600 + 1), 12 * 3600);
    }

    #[test]
    fn nothing_is_queued_without_consent() {
        let (hub, folder) = hub("no-consent", Consent::No);
        let p = pack(&header(), &start(1), &entries(), &facts(vec![], 1_800_000_000)).unwrap();
        assert_eq!(submit(&hub, &p, 1_800_000_000), Err(Refused::NoConsent));
        assert!(hub.queue().is_empty());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn two_battles_in_one_hour_share_a_send_time_and_one_at_the_end_waits_for_the_next() {
        let (hub, folder) = hub("cadence", Consent::Yes);
        let hour = 1_800_000_000 / 3600 * 3600;
        let p = pack(&header(), &start(1), &entries(), &facts(vec![], hour + 600)).unwrap();
        submit(&hub, &p, hour + 600).unwrap();
        submit(&hub, &p, hour + 3000).unwrap();
        submit(&hub, &p, hour + 3599).unwrap();
        let at: Vec<u64> = hub.queue().metas().map(|m| m.not_before).collect();
        assert_eq!(at, vec![hour + 3600; 3]);
        let late = pack(&header(), &start(2), &entries(), &facts(vec![], hour + 3599)).unwrap();
        submit(&hub, &late, hour + 3599).unwrap();
        assert_eq!(hub.queue().metas().last().unwrap().not_before, hour + 3600);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn the_hours_cap_drops_and_counts_the_record_beyond_sixty() {
        let (hub, folder) = hub("cap", Consent::Yes);
        let now = 1_800_000_000;
        let p = pack(&header(), &start(1), &entries(), &facts(vec![], now)).unwrap();
        for _ in 0..limits::BATTLES_PER_HOUR {
            submit(&hub, &p, now).unwrap();
        }
        assert_eq!(submit(&hub, &p, now), Err(Refused::BattleCap));
        assert_eq!(hub.battles_dropped(), 1);
        assert_eq!(hub.queue().metas().count(), limits::BATTLES_PER_HOUR as usize);
        // The next hour starts with an empty cap.
        assert!(submit(&hub, &p, now + 3600).is_ok());
        let _ = std::fs::remove_dir_all(folder);
    }
}
