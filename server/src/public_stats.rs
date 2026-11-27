//! The public play stats for the site's counter (`GET /v1/public/stats`): aggregates of the last
//! seven days, with no install id and no player counts. A map or a game shows only when at least
//! five distinct installs played it, so one install's habits never show. Computed at most once an
//! hour (see `AppState::public_stats`).

use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::civil::day_of;
use crate::http::{json, ApiError};
use crate::state::AppState;

const WINDOW_DAYS: i64 = 7;
/// A map or a game needs this many distinct installs in the window.
const MIN_INSTALLS: i64 = 5;
/// The hardest-maps board: at least this many starts, and this many decided games (wins + losses).
const HARD_MIN_STARTS: i64 = 20;
const HARD_MIN_DECIDED: i64 = 10;
const BOARD_ROWS: usize = 5;
/// The stats are recomputed when the stored ones are older than this (seconds).
const CACHE_SECS: i64 = 3600;

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct TopMap {
    pub map: String,
    pub starts: i64,
    pub wins: i64,
    pub losses: i64,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct HardMap {
    pub map: String,
    pub starts: i64,
    pub wins: i64,
    pub win_rate: f64,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct TopGame {
    pub game: String,
    pub players: i64,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct PublicStats {
    /// Seconds since 1970 when these were computed.
    pub generated_at: i64,
    pub days: i64,
    pub top_maps: Vec<TopMap>,
    pub hard_maps: Vec<HardMap>,
    pub top_games: Vec<TopGame>,
}

/// One map's window totals: (map, starts, wins, losses), only maps with enough installs.
fn map_totals(c: &Connection, from: i64) -> rusqlite::Result<Vec<(String, i64, i64, i64)>> {
    c.prepare(
        "SELECT map,
                SUM(kind = 'map_start'),
                SUM(kind = 'map_end' AND outcome = 'win'),
                SUM(kind = 'map_end' AND outcome = 'loss')
         FROM events
         WHERE quarantined = 0 AND day >= ?1 AND kind IN ('map_start', 'map_end') AND map <> ''
         GROUP BY map
         HAVING COUNT(DISTINCT CASE WHEN kind = 'map_start' THEN install_id END) >= ?2",
    )?
    .query_map(params![from, MIN_INSTALLS], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
    .collect()
}

/// The window's aggregates. `now` is seconds since 1970.
pub fn compute(c: &Connection, now: i64) -> rusqlite::Result<PublicStats> {
    let from = day_of(now) - (WINDOW_DAYS - 1);
    let totals = map_totals(c, from)?;

    let mut top: Vec<TopMap> = totals
        .iter()
        .map(|(map, starts, wins, losses)| TopMap { map: map.clone(), starts: *starts, wins: *wins, losses: *losses })
        .collect();
    top.sort_by(|a, b| b.starts.cmp(&a.starts).then(a.map.cmp(&b.map)));
    top.truncate(BOARD_ROWS);

    let mut hard: Vec<HardMap> = totals
        .iter()
        .filter(|(_, starts, wins, losses)| *starts >= HARD_MIN_STARTS && wins + losses >= HARD_MIN_DECIDED)
        .map(|(map, starts, wins, losses)| HardMap {
            map: map.clone(),
            starts: *starts,
            wins: *wins,
            win_rate: *wins as f64 / (wins + losses) as f64,
        })
        .collect();
    hard.sort_by(|a, b| {
        a.win_rate
            .total_cmp(&b.win_rate)
            .then(b.starts.cmp(&a.starts))
            .then(a.map.cmp(&b.map))
    });
    hard.truncate(BOARD_ROWS);

    let top_games = c
        .prepare(
            "SELECT game, COUNT(DISTINCT install_id) FROM pings
             WHERE quarantined = 0 AND day >= ?1
             GROUP BY game
             HAVING COUNT(DISTINCT install_id) >= ?2
             ORDER BY 2 DESC, game
             LIMIT ?3",
        )?
        .query_map(params![from, MIN_INSTALLS, BOARD_ROWS as i64], |r| {
            Ok(TopGame { game: r.get(0)?, players: r.get(1)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(PublicStats { generated_at: now, days: WINDOW_DAYS, top_maps: top, hard_maps: hard, top_games })
}

/// `GET /v1/public/stats`: no key, GET only, cached for ten minutes by browsers and at most once an
/// hour on the server. The site's GitHub Pages copy reads it from another origin.
pub async fn get_public_stats(State(st): State<AppState>) -> Result<Response, ApiError> {
    let now = st.clock.now();
    let mut cache = st.public_stats.lock().await;
    let fresh = matches!(cache.as_ref(), Some(s) if now - s.generated_at < CACHE_SECS);
    let stats = if fresh {
        cache.clone().expect("fresh means filled")
    } else {
        let stats = st.stats.read(move |c| Ok(compute(c, now)?)).await?;
        *cache = Some(stats.clone());
        stats
    };
    drop(cache);
    Ok((
        [(header::CACHE_CONTROL, "public, max-age=600"), (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
        json(&stats),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civil::DAY;
    use crate::schema;

    const NOW: i64 = 1_769_000_000;

    fn db() -> Connection {
        let c = Connection::open_in_memory().expect("opens");
        c.execute_batch(schema::STATS).expect("schema");
        c
    }

    fn event(c: &Connection, install: &str, at: i64, kind: &str, map: &str, outcome: &str) {
        c.execute(
            "INSERT INTO events (install_id, received_at, day, razdor, kind, map, outcome, last_screen, payload)
             VALUES (?1, ?2, ?3, '1.6.2', ?4, ?5, ?6, '', '{}')",
            params![install, at, day_of(at), kind, map, outcome],
        )
        .expect("inserts");
    }

    /// `n` installs each start `map` once, the first `wins` of them win and the rest lose.
    fn played(c: &Connection, map: &str, n: usize, wins: usize, at: i64) {
        for i in 0..n {
            let install = format!("{map}-{i}");
            event(c, &install, at, "map_start", map, "");
            let outcome = if i < wins { "win" } else { "loss" };
            event(c, &install, at, "map_end", map, outcome);
        }
    }

    fn ping(c: &Connection, install: &str, at: i64, game: &str) {
        c.execute(
            "INSERT INTO pings (install_id, day, received_at, razdor, platform, game, lang)
             VALUES (?1, ?2, ?3, '1.6.2', 'linux', ?4, 'ru')",
            params![install, day_of(at), at, game],
        )
        .expect("inserts");
    }

    #[test]
    fn a_map_with_fewer_than_five_installs_is_hidden() {
        let c = db();
        played(&c, "few", 4, 0, NOW);
        played(&c, "enough", 5, 5, NOW);
        let s = compute(&c, NOW).expect("computes");
        let names: Vec<&str> = s.top_maps.iter().map(|m| m.map.as_str()).collect();
        assert_eq!(names, vec!["enough"]);
        assert!(s.hard_maps.is_empty(), "5 starts is below the hardest-maps minimum");
    }

    #[test]
    fn the_hardest_maps_are_ordered_by_win_rate_then_starts() {
        let c = db();
        played(&c, "hard", 30, 3, NOW); // 3 of 30 won: 10%
        played(&c, "harder", 25, 2, NOW); // 2 of 25 won: 8%
        played(&c, "middle", 20, 10, NOW); // 50%
        let s = compute(&c, NOW).expect("computes");
        let order: Vec<&str> = s.hard_maps.iter().map(|m| m.map.as_str()).collect();
        assert_eq!(order, vec!["harder", "hard", "middle"]);
        assert_eq!(s.hard_maps[0].win_rate, 2.0 / 25.0);
    }

    #[test]
    fn a_map_with_too_few_decided_games_is_not_on_the_hardest_board() {
        let c = db();
        for i in 0..25 {
            event(&c, &format!("open-{i}"), NOW, "map_start", "open", "");
        }
        let s = compute(&c, NOW).expect("computes");
        assert_eq!(s.top_maps[0].map, "open");
        assert!(s.hard_maps.is_empty(), "25 starts but no finished games");
    }

    #[test]
    fn events_older_than_seven_days_are_dropped() {
        let c = db();
        played(&c, "old", 6, 0, NOW - 7 * DAY);
        played(&c, "new", 5, 0, NOW - 6 * DAY);
        let s = compute(&c, NOW).expect("computes");
        let names: Vec<&str> = s.top_maps.iter().map(|m| m.map.as_str()).collect();
        assert_eq!(names, vec!["new"]);
    }

    #[test]
    fn quarantined_rows_do_not_count() {
        let c = db();
        played(&c, "banned", 6, 0, NOW);
        c.execute("UPDATE events SET quarantined = 1 WHERE install_id = 'banned-0'", []).expect("updates");
        let s = compute(&c, NOW).expect("computes");
        assert_eq!(s.top_maps[0].starts, 5, "one install's rows are gone, five installs remain");
        c.execute("UPDATE events SET quarantined = 1 WHERE install_id = 'banned-1'", []).expect("updates");
        assert!(compute(&c, NOW).expect("computes").top_maps.is_empty(), "four installs left");
    }

    #[test]
    fn games_need_five_installs_and_are_ordered_by_players() {
        let c = db();
        for i in 0..7 {
            ping(&c, &format!("dt-{i}"), NOW, "dt");
        }
        for i in 0..5 {
            ping(&c, &format!("mod-{i}"), NOW, "evo");
        }
        for i in 0..4 {
            ping(&c, &format!("tiny-{i}"), NOW, "tiny");
        }
        let s = compute(&c, NOW).expect("computes");
        assert_eq!(
            s.top_games,
            vec![TopGame { game: "dt".into(), players: 7 }, TopGame { game: "evo".into(), players: 5 }]
        );
    }

    #[test]
    fn the_json_has_the_documented_shape() {
        let c = db();
        played(&c, "hristos0_-_VeilspielDungeon_1_5_1", 25, 0, NOW);
        let s = compute(&c, NOW).expect("computes");
        let v = serde_json::to_value(&s).expect("serialises");
        assert_eq!(v["generated_at"], NOW);
        assert_eq!(v["days"], 7);
        assert_eq!(v["top_maps"][0]["starts"], 25);
        assert_eq!(v["top_maps"][0]["losses"], 25);
        assert_eq!(v["hard_maps"][0]["win_rate"], 0.0);
        assert!(v["top_games"].is_array());
        assert!(v.get("install_id").is_none());
    }
}
