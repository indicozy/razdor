//! The cut of each sprite, cached in a folder the caller gives: one JSON file per sprite key
//! (`<key>.json`, the key as 16 hex digits). An entry has the shape of a sprite in
//! `data/cards/objects.json` plus the model tag. An empty card list is an entry too, so a sprite
//! with one region is not cut again. Writes go to a temporary file and are renamed into place.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::dt::cards::{checked, Card, RawCard};

/// The model the cached cards came from. An entry with another tag is not read.
pub const MODEL: &str = "v2-small-266";

#[derive(Serialize)]
struct EntryOut {
    w: u32,
    h: u32,
    model: &'static str,
    cards: Vec<CardOut>,
}

#[derive(Serialize)]
struct CardOut {
    foot: [f32; 2],
    verts: Vec<f32>,
    tris: Vec<u16>,
}

#[derive(Deserialize)]
struct EntryIn {
    w: u32,
    h: u32,
    model: String,
    cards: Vec<RawCard>,
}

fn path(dir: &Path, key: u64) -> PathBuf {
    dir.join(format!("{key:016x}.json"))
}

/// The cards cached for this key, `Some(empty)` for a sprite with one region. `None` when there is
/// no entry, the entry is for another model or another size, or it does not read back whole.
pub fn read(dir: &Path, key: u64, w: u32, h: u32) -> Option<Vec<Card>> {
    let text = std::fs::read_to_string(path(dir, key)).ok()?;
    let entry: EntryIn = serde_json::from_str(&text).ok()?;
    if entry.model != MODEL || entry.w != w || entry.h != h {
        return None;
    }
    entry.cards.into_iter().map(checked).collect()
}

/// Caches the cards of this key. The entry appears whole or not at all.
pub fn write(dir: &Path, key: u64, w: u32, h: u32, cards: &[Card]) -> std::io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let entry = EntryOut {
        w,
        h,
        model: MODEL,
        cards: cards
            .iter()
            .map(|c| CardOut {
                foot: [c.foot.0, c.foot.1],
                verts: c.verts.iter().flat_map(|v| [v.0, v.1]).collect(),
                tris: c.tris.clone(),
            })
            .collect(),
    };
    let text = serde_json::to_string(&entry).map_err(std::io::Error::other)?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{key:016x}.{}.{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path(dir, key)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the system temp dir, removed by the test.
    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Dir {
            let p = std::env::temp_dir().join(format!("razdor-depth-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            Dir(p)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn card(x: f32) -> Card {
        Card { foot: (x, 3.5), verts: vec![(0.0, 0.0), (4.0, 0.0), (0.0, 4.0)], tris: vec![0, 1, 2] }
    }

    #[test]
    fn cards_round_trip_and_an_empty_list_is_cached() {
        let dir = Dir::new("roundtrip");
        assert_eq!(read(&dir.0, 7, 4, 4), None, "no entry yet");
        write(&dir.0, 7, 4, 4, &[card(1.5), card(2.0)]).unwrap();
        assert_eq!(read(&dir.0, 7, 4, 4), Some(vec![card(1.5), card(2.0)]));
        write(&dir.0, 9, 4, 4, &[]).unwrap();
        assert_eq!(read(&dir.0, 9, 4, 4), Some(Vec::new()), "one region is cached as no cards");
    }

    #[test]
    fn another_size_or_model_is_not_read() {
        let dir = Dir::new("mismatch");
        write(&dir.0, 7, 4, 4, &[card(1.5)]).unwrap();
        assert_eq!(read(&dir.0, 7, 5, 4), None);
        let text = std::fs::read_to_string(path(&dir.0, 7)).unwrap();
        std::fs::write(path(&dir.0, 7), text.replace(MODEL, "v2-large-518")).unwrap();
        assert_eq!(read(&dir.0, 7, 4, 4), None);
    }

    #[test]
    fn a_damaged_card_makes_the_entry_invalid() {
        let dir = Dir::new("damaged");
        std::fs::create_dir_all(&dir.0).unwrap();
        let text = format!(
            r#"{{"w":4,"h":4,"model":"{MODEL}","cards":[{{"foot":[1,2],"verts":[0,0,4,0,0,4],"tris":[0,1,9]}}]}}"#
        );
        std::fs::write(path(&dir.0, 3), text).unwrap();
        assert_eq!(read(&dir.0, 3, 4, 4), None, "an index past the vertices");
    }
}
