//! The map editor's model: a scenario document with typed, undoable edits, validation and
//! safe saving. Pure: no macroquad, so every behaviour is tested here; the window is
//! `ui::editor` in the app.
//!
//! Design: `docs/superpowers/specs/2026-09-25-map-editor-design.md`. The editor writes
//! `.DTm` files with [`crate::dt::dtm::Scenario::to_payload`] and the `AIpf` container, so
//! they load in the original game and in Razdor; opening and saving an unchanged shipped
//! map gives the same bytes.

pub mod brush;
pub mod catalog;
pub mod cells;
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
pub mod menus;
pub mod naming;
pub mod options;
pub mod playability;
pub mod palette;
pub mod records;
pub mod refs;
pub mod tools;
pub mod validate;

pub use command::{Command, Settings};
pub use defaults::NewMap;
pub use doc::{Applied, EditError, EditorDoc, Origin, SaveError, Target};
pub use palette::{Names, Palette};
pub use tools::{Held, Kit, Page, Press, TerrainShape, ToolState};
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
    fn the_install_palette_as_the_original_groups_it() {
        let Some(dt) = install() else { return };
        let palette = Palette::from_sprites(&dt.map_objects().unwrap());
        // Every picture's brush (its width in cells) covers at least one cell.
        assert!(palette.buildings.iter().all(|b| b.brush >= 1 && b.brush >= b.size.0.min(b.size.1)));
        // Plants by family of twelve: live trees, dead trees (all in the last family),
        // thickets; no bushes below sprite 120.
        let f = palette.forest_facts();
        assert_eq!(f.counts, [[9, 9, 6, 6, 3, 6, 9, 3, 0, 0], [0, 0, 0, 0, 0, 0, 0, 0, 0, 9], [9, 9, 6, 6, 0, 0, 0, 3, 0, 9], [0; 10]]);
        assert_eq!((1..=6).map(|n| palette.hills(n).len()).collect::<Vec<_>>(), [38, 36, 24, 16, 16, 3]);
        assert_eq!((palette.forests(1).len(), palette.forests(2).len()), (102, 15));
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
    fn shipped_armies_save_as_the_army_window_saves_them() {
        let Some(dt) = install() else { return };
        let c = Content::from_dt(&dt);
        let div = Names::from_content(&c).facts.recruit_div;
        let (mut armies, mut rated, mut same_cost, mut same_side) = (0, 0, 0, 0);
        for m in &dt.maps {
            let s = crate::dt::dtm::Scenario::load(&m.path).unwrap();
            for a in &s.armies {
                armies += 1;
                // The model byte and the faction are derived (records.md §13: all agree).
                let saved = records::save_army(a, Some(&c), div);
                assert_eq!((saved.model, saved.faction), (a.model, a.faction), "{} army {}", m.name, a.id);
                assert!(a.tactical_cost_1 <= records::COST_CAP && a.tactical_cost_2 <= records::COST_CAP);
                let cost = records::army_cost(a, &c, div).expect("no shipped army passes 12 units");
                rated += 1;
                // Where the unit table still rates the army as when it was saved, the side
                // strength mostly agrees too (older editor builds formed sides differently).
                if records::stored_cost(cost.tactical) == a.tactical_cost_1 {
                    same_cost += 1;
                    same_side += (records::stored_cost(cost.side) == a.tactical_cost_2) as usize;
                }
            }
        }
        assert_eq!((armies, rated), (403, 403));
        assert!(same_cost >= 140 && same_side * 10 >= same_cost * 9, "{same_cost} / {same_side}");
    }

    #[test]
    fn shipped_buildings_save_as_the_building_window_saves_them() {
        let Some(dt) = install() else { return };
        let palette = Palette::from_sprites(&dt.map_objects().unwrap());
        let content = std::sync::Arc::new(Content::from_dt(&dt));
        let (mut buildings, mut above_50, mut clamped, mut tested) = (0, 0, 0, 0);
        for m in &dt.maps {
            let d = EditorDoc::open_with(&m.path, None, Some(&palette), 0).unwrap();
            let s = &d.scenario;
            for (i, b) in s.buildings.iter().enumerate() {
                buildings += 1;
                let footprint = palette.picture(b.picture_type, b.picture_variant).map(|p| p.size);
                let saved = records::save_building(b, s, footprint);
                // Byte 294 is derived (records.md §13: every shipped building agrees); the
                // footprint is the picture's, as the loader set it.
                assert_eq!((saved.has_barracks, saved.size_x, saved.size_y), (b.has_barracks, b.size_x, b.size_y), "{} building {}", m.name, i + 1);
                // The stale goods copy at 296–301 survives a save but for the 0..12 clamp.
                assert_eq!(saved.stale_artifacts[1..], b.stale_artifacts[1..]);
                clamped += (saved.stale_artifacts[0] != b.stale_artifacts[0]) as usize;
                above_50 += (saved.garrison_extra_defence != b.garrison_extra_defence) as usize;
                if records::BuildingPages::of(b.kind).market && records::market_test_ready(b) && tested < 3 {
                    let goods = records::market_test(s, i as u16 + 1, b, content.clone());
                    assert_eq!(goods.len(), 12);
                    assert!(goods.iter().flatten().count() > 0, "{} building {}: the restock stocks something", m.name, i + 1);
                    tested += 1;
                }
            }
        }
        assert_eq!((buildings, above_50, tested), (1082, 6, 3));
        assert!(clamped > 0, "some shipped buildings hold a stale byte above 12 at 296");
    }

    #[test]
    fn shipped_scenario_parameters_fit_the_original_page() {
        let Some(dt) = install() else { return };
        for m in &dt.maps {
            let s = crate::dt::dtm::Scenario::load(&m.path).unwrap();
            // The built-in picture is one of the page's six; every preset's experience is 0
            // (records.md §13) and its gold and mana are words of the page's range.
            assert!(s.header.scenario_picture_index < 6, "{}", m.name);
            for h in &s.header.heroes {
                assert_eq!(records::preset_experience(h), 0, "{}", m.name);
                assert!((0..=records::PRESET_MAX).contains(&(records::preset_word(h.gold) as i64)) && h.gold >> 16 == 0, "{}", m.name);
            }
            // Shifting the start and back leaves every event as it was.
            let mut events = s.events.clone();
            records::shift_event_starts(&mut events, s.header.start_time, s.header.start_time.wrapping_add(43_200));
            assert!(events.iter().zip(&s.events).all(|(a, b)| a.start_time == b.start_time || a.start_time == b.start_time.wrapping_add(43_200)));
            records::shift_event_starts(&mut events, s.header.start_time.wrapping_add(43_200), s.header.start_time);
            assert!(events == s.events, "{}", m.name);
        }
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
        let mut quirks = 0;
        for m in &dt.maps {
            let mut d = EditorDoc::open(&m.path, None).unwrap();
            let original = d.file_bytes(Some(&names), Some(&palette)).unwrap();
            let n = d.scenario.events.len() as u16;
            // A list holding a 0 in its counted part is shortened by every event delete (the
            // original's quirk): such maps are compared but for those counts.
            let zero_lists = |s: &crate::dt::dtm::Scenario| {
                s.buildings.iter().filter(|b| b.event_slots[..b.event_count as usize].contains(&0)).count()
                    + s.points.iter().filter(|p| p.event_slots[..p.event_count as usize].contains(&0)).count()
            };
            let quirk = zero_lists(&d.scenario) > 0;
            let start = d.scenario.clone();
            let same = |d: &EditorDoc, what: &str| {
                if quirk {
                    let strip = |s: &crate::dt::dtm::Scenario| {
                        let mut s = s.clone();
                        s.buildings.iter_mut().for_each(|b| b.event_count = 0);
                        s.points.iter_mut().for_each(|p| p.event_count = 0);
                        s
                    };
                    assert!(strip(&d.scenario) == strip(&start), "{}: {what}", m.name);
                } else {
                    assert!(d.file_bytes(Some(&names), Some(&palette)).unwrap() == original, "{}: {what}", m.name);
                }
            };
            // Adding and deleting an event, or duplicating one and deleting the copy, gives
            // the same file.
            d.apply(Command::NewEvent { kind: 1, repeat: false }).unwrap();
            d.apply(Command::DeleteEvent { id: n + 1 }).unwrap();
            same(&d, "add + delete");
            d.apply(Command::DuplicateEvent { id: n, next: None }).unwrap();
            d.apply(Command::DeleteEvent { id: n + 1 }).unwrap();
            same(&d, "duplicate + delete");
            // A copy of event 1 goes in at 2, renumbering every later reference; deleting it
            // renumbers them back.
            assert_eq!(d.apply(Command::DuplicateEvent { id: 1, next: Some(2) }).unwrap().new_id, Some(2));
            d.apply(Command::DeleteEvent { id: 2 }).unwrap();
            same(&d, "copy in the middle + delete");
            // Moving the last event to the top and back.
            d.apply(Command::MoveEvent { from: n, to: 1 }).unwrap();
            d.apply(Command::MoveEvent { from: 1, to: n }).unwrap();
            same(&d, "move and back");
            quirks += quirk as usize;
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
        // Some shipped map has a list with an empty slot that event deletes shorten.
        assert!(quirks > 0);
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

