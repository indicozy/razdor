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
pub mod dump;
pub mod events;
pub mod files;
pub mod geometry;
pub mod grid;
pub mod mapcheck;
pub mod mapfile;
pub mod playability;
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
    use crate::editor::mapfile;
    use crate::rules::content::Content;

    fn install() -> Option<DtInstall> {
        let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
        Some(DtInstall::load(std::path::Path::new(&dir)).expect("install loads"))
    }

    #[test]
    fn every_shipped_map_saves_as_the_original_editor() {
        let Some(dt) = install() else { return };
        let game_dir = crate::dt::install::find_path(&dt.dir, crate::dt::install::MAPS_DIR).unwrap();
        let names = Names::from_content(&Content::from_dt(&dt));
        let palette = Palette::from_sprites(&dt.map_objects().unwrap());
        let out = files::tests::temp_dir("shipped");
        assert_eq!(dt.maps.len(), 15);
        for m in &dt.maps {
            let mut d = EditorDoc::open_with(&m.path, Some(&game_dir), Some(&palette), names.artefacts.len()).unwrap_or_else(|e| panic!("{}: {e}", m.name));
            assert!(matches!(d.origin, Origin::Game(_)), "{}", m.name);
            assert!(!d.dirty(), "{}: a version-4 map opens unmodified", m.name);
            let errors: Vec<String> = d.issues(Some(&names), Some(&palette)).iter().filter(|i| i.severity == Severity::Error).map(|i| i.to_string()).collect();
            assert!(errors.is_empty(), "{}: {errors:#?}", m.name);
            let before = d.file_bytes(Some(&names), Some(&palette)).unwrap();
            let target = out.join(format!("{}.DTm", m.name));
            assert_eq!(d.save_to(&target, Some(&names), Some(&palette)).unwrap(), target);
            let saved = std::fs::read(&target).unwrap();
            assert!(saved == before, "{}: the save writes what file_bytes promised", m.name);
            // The shipped maps were saved consistently: a save changes only the save
            // counter, and the strings the loader trims (only "Устье Трейна" has any).
            let original = crate::dt::container::decode(&std::fs::read(&m.path).unwrap()).unwrap().payload;
            let payload = crate::dt::container::decode(&saved).unwrap().payload;
            let mut want = crate::dt::dtm::Scenario::parse_payload(&original).unwrap();
            mapfile::trim_strings(&mut want, &mut []);
            want.header.set_save_counter(1);
            assert!(payload == want.to_payload(), "{}: saved payload differs", m.name);
            if !m.name.starts_with("Устье") {
                let changed: Vec<usize> = (0..original.len()).filter(|i| original[*i] != payload[*i]).collect();
                assert_eq!((original.len(), changed), (payload.len(), vec![0x124]), "{}", m.name);
            }
            // The save counter goes up with every save; the document is clean.
            assert!(!d.dirty());
            d.save_to(&target, Some(&names), Some(&palette)).unwrap();
            assert_eq!(crate::dt::dtm::Scenario::load(&target).unwrap().header.save_counter(), 2, "{}", m.name);
            // An edit and its undo give the same file again.
            let again = d.file_bytes(Some(&names), Some(&palette)).unwrap();
            d.apply(Command::PaintTerrain { x: 1, y: 1, size: 9, code: 15 }).unwrap();
            let _ = d.apply(Command::DeleteBuilding { id: 1 });
            let _ = d.apply(Command::DeleteArmy { id: 1 });
            while d.undo() {
                if !d.dirty() {
                    break;
                }
            }
            assert!(d.file_bytes(Some(&names), Some(&palette)).unwrap() == again, "{}: undo is not exact", m.name);
        }
    }

    #[test]
    fn the_map_check_runs_on_every_shipped_map() {
        let Some(dt) = install() else { return };
        let names = Names::from_content(&Content::from_dt(&dt));
        assert_eq!(names.facts.recruit_div, 2);
        let mut total = 0;
        for m in &dt.maps {
            let d = EditorDoc::open(&m.path, None).unwrap();
            let rows = mapcheck::check_map(&d.scenario, &names);
            for r in &rows {
                // Every row leads to an existing record.
                let n = match r.kind {
                    mapcheck::CheckKind::Army => d.scenario.armies.len(),
                    mapcheck::CheckKind::Building => d.scenario.buildings.len(),
                    mapcheck::CheckKind::Event => d.scenario.events.len(),
                    mapcheck::CheckKind::Point => d.scenario.points.len(),
                };
                assert!((1..=n).contains(&(r.id as usize)), "{}: {r:?}", m.name);
            }
            total += rows.len();
        }
        assert!(total > 0, "the shipped maps have remarks");
    }

    #[test]
    fn every_shipped_map_scores() {
        let Some(dt) = install() else { return };
        let names = Names::from_content(&Content::from_dt(&dt));
        let mut scores = Vec::new();
        for m in &dt.maps {
            let mut d = EditorDoc::open(&m.path, None).unwrap();
            // The shipped maps were never scored, or were cleaned.
            assert_eq!((d.scenario.header.playability(), d.scenario.header.quest_count()), (0, 0), "{}", m.name);
            let r = d.score_playability(&names).unwrap_or_else(|e| panic!("{}: {e:?}", m.name));
            let quests = d.scenario.events.iter().filter(|e| e.kind == 3).count();
            assert_eq!((r.quests as usize, d.scenario.header.quest_count() as usize), (quests, quests), "{}", m.name);
            assert_eq!(d.scenario.header.playability(), r.score);
            assert!(r.line.starts_with(&d.scenario.title) && r.line.ends_with('|'), "{}", r.line);
            if (r.score, r.quests) != (0, 0) {
                assert!(d.undo() && d.scenario.header.playability() == 0, "scoring is one undo step");
            }
            scores.push(r.score);
        }
        // The campaign and story maps score above the tutorials' floor.
        assert!(scores.iter().filter(|s| **s > 0).count() >= 10, "{scores:?}");
    }

    #[test]
    fn the_estuary_map_opens_with_its_blank_questions_trimmed() {
        // Events 36 to 38 of "Устье Трейна" have a question that is only a line break: the
        // loader trims it to nothing and keeps all 211 events (§3.5).
        let Some(dt) = install() else { return };
        let m = dt.maps.iter().find(|m| m.name.starts_with("Устье")).expect("the map is shipped");
        let raw = crate::dt::dtm::Scenario::load(&m.path).unwrap();
        assert_eq!((36..=38).map(|i| raw.events[i - 1].question.as_str()).collect::<Vec<_>>(), ["\r\n"; 3]);
        let d = EditorDoc::open(&m.path, None).unwrap();
        assert_eq!(d.scenario.events.len(), 211);
        assert!((36..=38).all(|i| d.scenario.events[i - 1].question.is_empty()));
    }

    #[test]
    fn shipped_maps_survive_event_edits() {
        let Some(dt) = install() else { return };
        let names = Names::from_content(&Content::from_dt(&dt));
        let palette = Palette::from_sprites(&dt.map_objects().unwrap());
        let content = std::sync::Arc::new(Content::from_dt(&dt));
        for m in &dt.maps {
            let mut d = EditorDoc::open(&m.path, None).unwrap();
            let original = d.file_bytes(Some(&names), Some(&palette)).unwrap();
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

