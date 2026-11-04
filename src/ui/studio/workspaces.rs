//! Razdor Studio's four workspaces (UX1, spec §4): Карта, Сюжет, Данные and Тест. Each has its own dock
//! layout (an egui_dock tree of the studio's panels, saved per user as JSON in the settings) and its
//! default, built for the window's width: the Inspector is the right column of every workspace, at
//! 380 px, and the other side columns take their widths of spec §5. The drawer (Проблемы, Вывод, Claude)
//! is not a dock: it is the frame's bottom panel (`mod.rs`).

use egui_dock::{DockState, NodeIndex};
use razdor::i18n::n_;
use egui_phosphor::regular as ic;

use super::theme;
use super::Tab;

/// The width the layouts are first made at, before the window's width is known (points).
pub const NOMINAL_W: f32 = 1280.0;
/// The Сюжет outline and the Данные navigation columns (points).
const OUTLINE_W: f32 = 260.0;
const NAV_W: f32 = 150.0;
/// Below this window width the Map workspace folds its Слои | Сцена column into the Inspector column's tabs,
/// with the minimap pinned on top (spec §5.8; the canvas keeps about 650 px at 1280).
pub const FOLD_W: f32 = 1600.0;
/// The layouts' versions: a saved layout is keyed by its version, so a change of a workspace's layout
/// (here UX3's Map, UX5's Story and Data) resets the layouts the earlier version saved.
const MAP_LAYOUT: &str = "map-ux3";
const STORY_LAYOUT: &str = "story-ux5";
const DATA_LAYOUT: &str = "data-ux5";

/// One of the four workspaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WsKind {
    Map,
    Story,
    Data,
    Test,
}

impl WsKind {
    /// The workspaces in the switcher's order (Alt+1 to Alt+4).
    pub const ALL: [WsKind; 4] = [WsKind::Map, WsKind::Story, WsKind::Data, WsKind::Test];

    /// The index into the screen's array of docks.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The key the layout is saved under (also the dock's id).
    pub fn key(self) -> &'static str {
        match self {
            WsKind::Map => MAP_LAYOUT,
            WsKind::Story => STORY_LAYOUT,
            WsKind::Data => DATA_LAYOUT,
            WsKind::Test => "test",
        }
    }

    /// The command that switches to this workspace (`ws-map` and so on).
    pub fn command(self) -> &'static str {
        match self {
            WsKind::Map => "ws-map",
            WsKind::Story => "ws-story",
            WsKind::Data => "ws-data",
            WsKind::Test => "ws-test",
        }
    }

    /// The workspace of a switcher command, if the id is one.
    pub fn of_command(id: &str) -> Option<WsKind> {
        WsKind::ALL.into_iter().find(|k| k.command() == id)
    }

    /// The English name (the catalogue key).
    pub fn name(self) -> &'static str {
        match self {
            WsKind::Map => n_("Map workspace"),
            WsKind::Story => n_("Story workspace"),
            WsKind::Data => n_("Data workspace"),
            WsKind::Test => n_("Test workspace"),
        }
    }

    /// The Phosphor icon of the switcher's button.
    pub fn icon(self) -> &'static str {
        match self {
            WsKind::Map => ic::MAP_TRIFOLD,
            WsKind::Story => ic::FLOW_ARROW,
            WsKind::Data => ic::TABLE,
            WsKind::Test => ic::FLASK,
        }
    }
}

/// The drawer's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drawer {
    Problems,
    Output,
    Claude,
}

impl Drawer {
    pub const ALL: [Drawer; 3] = [Drawer::Problems, Drawer::Output, Drawer::Claude];

    pub fn name(self) -> &'static str {
        match self {
            Drawer::Problems => n_("Problems"),
            Drawer::Output => n_("Output"),
            Drawer::Claude => "Claude",
        }
    }
}

/// The share of the first node when a node of `of` points is split into `px` points and the rest:
/// egui_dock's fraction (clamped so that neither side vanishes).
pub fn share(px: f32, of: f32) -> f32 {
    if of <= 1.0 {
        return 0.5;
    }
    (px / of).clamp(0.05, 0.95)
}

/// The default layout of a workspace for a window `width` points wide. `side` puts the Script beside
/// the Карта's map (the snapshot option `side`).
pub fn default_dock(kind: WsKind, width: f32, side: bool) -> DockState<Tab> {
    let w = if width > 1.0 { width } else { NOMINAL_W };
    let inspector = theme::INSPECTOR_W;
    match kind {
        WsKind::Map => {
            // The palette column (Палитра | Штампы) at the left; the canvas; at the right the minimap over Слои |
            // Сцена, then the Inspector. Narrower than FOLD_W, the Слои and Сцена tabs join the Inspector's column,
            // and the minimap is pinned above them. The tool strip is a panel of its own (`studio/mod.rs`).
            let dock_w = (w - theme::TOOL_W).max(1.0);
            let mut dock = DockState::new(vec![Tab::Map]);
            let tree = dock.main_surface_mut();
            if width < FOLD_W {
                let [centre, column] = tree.split_right(NodeIndex::root(), share(dock_w - inspector, dock_w), vec![Tab::Inspector, Tab::Layers, Tab::Scene]);
                let _ = tree.split_above(column, 0.35, vec![Tab::Minimap]);
                let [centre, _palette] = tree.split_left(centre, share(theme::PALETTE_W, dock_w - inspector), vec![Tab::Palette, Tab::Stamps]);
                if side {
                    let _ = tree.split_right(centre, 0.5, vec![Tab::Script]);
                }
            } else {
                let [centre, _inspector] = tree.split_right(NodeIndex::root(), share(dock_w - inspector, dock_w), vec![Tab::Inspector]);
                let column_room = dock_w - inspector;
                let [centre, column] = tree.split_right(centre, share(column_room - theme::RIGHT_W, column_room), vec![Tab::Minimap]);
                let _ = tree.split_below(column, 0.4, vec![Tab::Layers, Tab::Scene]);
                let palette_room = column_room - theme::RIGHT_W;
                let [centre, _palette] = tree.split_left(centre, share(theme::PALETTE_W, palette_room), vec![Tab::Palette, Tab::Stamps]);
                if side {
                    let _ = tree.split_right(centre, 0.5, vec![Tab::Script]);
                }
            }
            dock
        }
        WsKind::Story => {
            // The graph or the campaign graph in the centre, the outline (Сюжет | Персонажи | Переменные) at the
            // left, the Inspector at the right (spec §4.2).
            let mut dock = DockState::new(vec![Tab::Story]);
            let tree = dock.main_surface_mut();
            let [centre, _inspector] = tree.split_right(NodeIndex::root(), share(w - inspector, w), vec![Tab::Inspector]);
            let _ = tree.split_left(centre, share(OUTLINE_W, w - inspector), vec![Tab::Outline]);
            dock
        }
        WsKind::Data => {
            // The view in the centre (spreadsheets, texts, package, files), its navigation at the left, the Inspector.
            let mut dock = DockState::new(vec![Tab::Data]);
            let tree = dock.main_surface_mut();
            let [centre, _inspector] = tree.split_right(NodeIndex::root(), share(w - inspector, w), vec![Tab::Inspector]);
            let _ = tree.split_left(centre, share(NAV_W, w - inspector), vec![Tab::DataNav]);
            dock
        }
        WsKind::Test => {
            let mut dock = DockState::new(vec![Tab::Battle, Tab::Scores]);
            let tree = dock.main_surface_mut();
            let _ = tree.split_right(NodeIndex::root(), share(w - inspector, w), vec![Tab::Inspector]);
            dock
        }
    }
}

/// A layout as the settings keep it: egui_dock's laid-out rectangles (`rect`, `viewport`) are not part of
/// it. The next frame lays the tabs out again, so they are written as zeros; a layout not drawn yet has
/// infinite rectangles, which JSON cannot hold, so they are zeros from the start. Two layouts that are
/// arranged the same give the same text.
pub fn to_json(dock: &DockState<Tab>) -> Option<String> {
    let mut v = serde_json::to_value(dock).ok()?;
    tidy(&mut v);
    serde_json::to_string(&v).ok()
}

fn tidy(v: &mut serde_json::Value) {
    use serde_json::{json, Value};
    match v {
        Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if key == "rect" || key == "viewport" {
                    *val = json!({ "min": { "x": 0.0, "y": 0.0 }, "max": { "x": 0.0, "y": 0.0 } });
                } else {
                    tidy(val);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(tidy),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(dock: &DockState<Tab>) -> Vec<Tab> {
        dock.iter_all_tabs().map(|(_, t)| *t).collect()
    }

    #[test]
    fn every_workspace_has_the_inspector_and_its_own_tabs() {
        for kind in WsKind::ALL {
            let t = tabs(&default_dock(kind, 1280.0, false));
            assert!(t.contains(&Tab::Inspector), "{:?} has the Inspector", kind);
        }
        assert!(tabs(&default_dock(WsKind::Map, 1280.0, false)).contains(&Tab::Map));
        assert!(tabs(&default_dock(WsKind::Story, 1280.0, false)).contains(&Tab::Story));
        assert!(tabs(&default_dock(WsKind::Story, 1280.0, false)).contains(&Tab::Outline));
        assert!(tabs(&default_dock(WsKind::Data, 1280.0, false)).contains(&Tab::Data));
        assert!(tabs(&default_dock(WsKind::Data, 1280.0, false)).contains(&Tab::DataNav));
        assert!(tabs(&default_dock(WsKind::Test, 1280.0, false)).contains(&Tab::Battle));
    }

    #[test]
    fn the_side_option_puts_the_script_beside_the_map() {
        assert!(!tabs(&default_dock(WsKind::Map, 1280.0, false)).contains(&Tab::Script));
        assert!(tabs(&default_dock(WsKind::Map, 1280.0, true)).contains(&Tab::Script));
    }

    #[test]
    fn a_saved_layout_round_trips_with_its_stable_tab_keys() {
        let dock = default_dock(WsKind::Data, 1920.0, false);
        let json = to_json(&dock).expect("the layout is JSON");
        let back: DockState<Tab> = serde_json::from_str(&json).expect("and reads back");
        assert_eq!(to_json(&back).as_deref(), Some(json.as_str()), "a saved layout is the same text again");
        assert_eq!(tabs(&back), tabs(&dock));
        // The keys are the tabs' fixed keys, not their titles: a change of language cannot break them.
        assert!(json.contains("DataNav"), "{json}");
        assert_eq!(Tab::Units.key(), "units");
    }

    #[test]
    fn the_map_layout_has_its_palette_layers_and_minimap_at_both_widths() {
        for width in [1280.0, 1920.0] {
            let t = tabs(&default_dock(WsKind::Map, width, false));
            for tab in [Tab::Palette, Tab::Stamps, Tab::Minimap, Tab::Layers, Tab::Scene, Tab::Inspector, Tab::Map] {
                assert!(t.contains(&tab), "{tab:?} at {width}");
            }
        }
    }

    #[test]
    fn the_map_layout_is_keyed_by_its_version() {
        assert_eq!(WsKind::Map.key(), "map-ux3", "a change of the Map layout bumps the key, so saved layouts reset");
        assert_eq!(WsKind::Story.key(), "story-ux5");
        assert_eq!(WsKind::Data.key(), "data-ux5");
    }

    #[test]
    fn the_workspace_keys_and_commands_are_fixed() {
        assert_eq!(WsKind::of_command("ws-data"), Some(WsKind::Data));
        assert_eq!(WsKind::Test.key(), "test");
        assert_eq!(WsKind::Map.index(), 0);
        assert_eq!(WsKind::Test.index(), 3);
    }

    #[test]
    fn share_clamps_and_handles_a_tiny_node() {
        assert!((share(380.0, 1280.0) - 0.296875).abs() < 1e-6);
        assert_eq!(share(2000.0, 1000.0), 0.95);
        assert_eq!(share(10.0, 0.5), 0.5);
    }
}
