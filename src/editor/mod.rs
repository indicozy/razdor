//! The map editor's model: a scenario document with typed, undoable edits, validation and
//! safe saving. Pure: no macroquad, so every behaviour is tested here; the window is
//! `ui::editor` in the app.
//!
//! Design: `docs/superpowers/specs/2026-09-25-map-editor-design.md`. The editor writes
//! `.DTm` files with [`crate::dt::dtm::Scenario::to_payload`] and the `AIpf` container, so
//! they load in the original game and in Razdor; opening and saving an unchanged shipped
//! map gives the same bytes.

pub mod command;
pub mod defaults;
pub mod doc;
pub mod events;
pub mod files;
pub mod geometry;
pub mod palette;
pub mod records;
pub mod refs;
pub mod tools;
pub mod validate;

pub use command::{Command, ObjectFilter, Settings};
pub use defaults::NewMap;
pub use doc::{Applied, EditError, EditorDoc, Origin, SaveError, Target};
pub use palette::{Names, Palette};
pub use tools::{TerrainShape, Tool, ToolState};
pub use validate::{Issue, Place, Severity};

#[cfg(test)]
mod real_maps {
    //! Against the player's install; skipped without `RAZDOR_DT_DIR`.
    use super::*;
    use crate::dt::install::DtInstall;
    use crate::rules::content::Content;

    fn install() -> Option<DtInstall> {
        let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
        let mut dt = DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        dt.maps.retain(|m| !crate::dt::install::is_translated_copy(&m.name));
        Some(dt)
    }

    #[test]
    fn every_shipped_map_saves_byte_identically() {
        let Some(dt) = install() else { return };
        let game_dir = crate::dt::install::find_path(&dt.dir, crate::dt::install::MAPS_DIR).unwrap();
        let names = Names::from_content(&Content::from_dt(&dt));
        let palette = Palette::from_sprites(&dt.map_objects().unwrap());
        let out = files::tests::temp_dir("shipped");
        assert_eq!(dt.maps.len(), 15);
        for m in &dt.maps {
            let mut d = EditorDoc::open(&m.path, Some(&game_dir)).unwrap_or_else(|e| panic!("{}: {e}", m.name));
            assert!(matches!(d.origin, Origin::Game(_)), "{}", m.name);
            let errors: Vec<String> = d.issues(Some(&names), Some(&palette)).iter().filter(|i| i.severity == Severity::Error).map(|i| i.to_string()).collect();
            assert!(errors.is_empty(), "{}: {errors:#?}", m.name);
            let target = out.join(format!("{}.DTm", m.name));
            d.save_to(&target, Some(&names), Some(&palette)).unwrap_or_else(|e| panic!("{}: {e}", m.name));
            let original = std::fs::read(&m.path).unwrap();
            assert!(std::fs::read(&target).unwrap() == original, "{}: saved file differs", m.name);
            // An edit and its undo also give the original bytes.
            d.apply(Command::PaintTerrain { x: 1, y: 1, size: 9, code: 15 }).unwrap();
            let _ = d.apply(Command::DeleteBuilding { id: 1 });
            let _ = d.apply(Command::DeleteArmy { id: 1 });
            while d.undo() {}
            assert!(d.file_bytes(Some(&names), Some(&palette)).unwrap() == original, "{}: undo is not exact", m.name);
        }
    }

    #[test]
    fn shipped_maps_survive_event_edits() {
        let Some(dt) = install() else { return };
        let names = Names::from_content(&Content::from_dt(&dt));
        let palette = Palette::from_sprites(&dt.map_objects().unwrap());
        let content = std::sync::Arc::new(Content::from_dt(&dt));
        for m in &dt.maps {
            let original = std::fs::read(&m.path).unwrap();
            let mut d = EditorDoc::open(&m.path, None).unwrap();
            let n = d.scenario.events.len() as u16;
            // Adding and deleting an event, or duplicating one and deleting the copy, gives
            // the same file.
            d.apply(Command::NewEvent { kind: 1 }).unwrap();
            d.apply(Command::DeleteEvent { id: n + 1 }).unwrap();
            assert!(d.file_bytes(Some(&names), Some(&palette)).unwrap() == original, "{}: add + delete", m.name);
            d.apply(Command::DuplicateEvent { id: 1 }).unwrap();
            d.apply(Command::DeleteEvent { id: n + 1 }).unwrap();
            assert!(d.file_bytes(Some(&names), Some(&palette)).unwrap() == original, "{}: duplicate + delete", m.name);
            // Deleting the most referred-to event leaves no dangling reference.
            let busiest = (1..=n).max_by_key(|id| events::references_to(&d.scenario, *id).len()).unwrap();
            let refs = events::references_to(&d.scenario, busiest).len();
            d.apply(Command::DeleteEvent { id: busiest }).unwrap();
            let errors: Vec<String> = d.issues(Some(&names), Some(&palette)).iter().filter(|i| i.severity == Severity::Error).map(|i| i.to_string()).collect();
            assert!(errors.is_empty(), "{}: deleting event {busiest} ({refs} references): {errors:#?}", m.name);
            // Every field edit of the panel round-trips: the first event with a new title.
            let mut e = d.scenario.events[0].clone();
            e.title = events::with_flags(&events::with_name(&e.title, "Проверка"), "+ПроверкаФлаг", "");
            d.apply(Command::SetEvent { id: 1, event: Box::new(e) }).unwrap();
            let bytes = d.file_bytes(Some(&names), Some(&palette)).unwrap();
            let s = crate::dt::dtm::Scenario::from_file_bytes(&bytes).unwrap();
            assert_eq!(s.events, d.scenario.events, "{}", m.name);
            // The event engine runs the edited map.
            let mut g = crate::rules::game::Game::from_scenario(content.clone(), &s, crate::rules::content::HeroClass::Knight);
            for _ in 0..6 {
                g.drain_events();
                for _ in 0..8 {
                    if g.pending_question().is_none() {
                        break;
                    }
                    g.answer_question(true);
                }
                g.wait(12);
            }
            assert!(g.script().is_some(), "{}", m.name);
        }
    }

    #[test]
    fn edits_of_a_shipped_map_play() {
        let Some(dt) = install() else { return };
        let m = dt.maps.iter().find(|m| m.name.starts_with("РК1")).expect("РК1 present");
        let mut d = EditorDoc::open(&m.path, None).unwrap();
        d.apply(Command::DeleteArmy { id: 1 }).unwrap();
        d.apply(Command::DeletePoint { id: 1 }).unwrap();
        d.apply(Command::DeleteBuilding { id: 2 }).unwrap();
        let names = Names::from_content(&Content::from_dt(&dt));
        let palette = Palette::from_sprites(&dt.map_objects().unwrap());
        let errors: Vec<String> = d.issues(Some(&names), Some(&palette)).iter().filter(|i| i.severity == Severity::Error).map(|i| i.to_string()).collect();
        assert!(errors.is_empty(), "{errors:#?}");
        let bytes = d.file_bytes(Some(&names), Some(&palette)).unwrap();
        let s = crate::dt::dtm::Scenario::from_file_bytes(&bytes).unwrap();
        assert_eq!(s.armies.len() + 1, EditorDoc::open(&m.path, None).unwrap().scenario.armies.len());
        let content = std::sync::Arc::new(Content::from_dt(&dt));
        let g = crate::rules::game::Game::from_scenario(content, &s, crate::rules::content::HeroClass::Knight);
        assert_eq!(g.world.locations.len(), s.buildings.len());
    }
}
