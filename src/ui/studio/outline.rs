//! The Сюжет workspace (UX5, spec §4.2): the outline at the left (tabs Сюжет | Персонажи | Переменные) and
//! the centre's switch between the story graph of the shown map and the campaign graph. The outline lists the
//! triggers and dialogs by map, with the trigger's `when` (or the dialog's title) under each; a click selects the
//! node, opens its form in the Inspector and centres the graph on it. Персонажи and Переменные are the
//! characters and variables screens (`scenarios.rs`, `vars.rs`).

use egui::{CollapsingHeader, Id, ScrollArea, TextEdit, Ui};
use egui_macroquad::egui;
use egui_phosphor::regular as ic;
use razdor::i18n::tr;
use razdor::studio::nav::{outline, OutlineKind, OutlineRow, EVERY_MAP};
use razdor::studio::story_graph as sg;

use super::story::StorySel;
use super::{Viewer, ERROR_RED};

/// The centre's view of the Сюжет workspace.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StoryView {
    #[default]
    Story,
    Campaign,
}

/// The outline's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutlineTab {
    #[default]
    Story,
    Characters,
    Vars,
}

/// The outline's search field (its id: Ctrl+F in the graph asks for the focus).
pub const SEARCH_ID: &str = "studio-outline-search";

impl Viewer<'_> {
    /// The Сюжет centre: the switch (the story graph or the campaign graph), then the view.
    pub(super) fn story_centre(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for (view, label) in [(StoryView::Story, tr("Story graph")), (StoryView::Campaign, tr("Campaign graph"))] {
                if ui.selectable_label(self.scen.story_view == view, label).clicked() {
                    self.scen.story_view = view;
                }
            }
        });
        ui.separator();
        match self.scen.story_view {
            StoryView::Story => self.story_screen(ui),
            StoryView::Campaign => self.campaign_screen(ui),
        }
    }

    /// The outline column: its three tabs and the one shown.
    pub(super) fn outline_panel(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for (tab, label) in [(OutlineTab::Story, tr("Story")), (OutlineTab::Characters, tr("Characters")), (OutlineTab::Vars, tr("Variables"))] {
                if ui.selectable_label(self.scen.outline_tab == tab, label).clicked() {
                    self.scen.outline_tab = tab;
                }
            }
        });
        ui.separator();
        match self.scen.outline_tab {
            OutlineTab::Story => self.outline_list(ui),
            OutlineTab::Characters => self.characters_dock(ui),
            OutlineTab::Vars => self.vars_dock(ui),
        }
    }

    /// The Сюжет tab: the search, then the triggers and dialogs by map.
    fn outline_list(&mut self, ui: &mut Ui) {
        let id = Id::new(SEARCH_ID);
        ui.add(TextEdit::singleline(&mut self.scen.outline_query).id(id).hint_text(tr("Search triggers and dialogs")).desired_width(f32::INFINITY));
        if std::mem::take(&mut self.scen.focus_find) {
            ui.memory_mut(|m| m.request_focus(id));
        }
        let Some(pkg) = self.pkg.clone() else {
            ui.colored_label(ERROR_RED, tr("The package could not be read: see Problems."));
            return;
        };
        let groups = outline(&pkg, &self.scen.outline_query);
        if groups.is_empty() {
            ui.weak(tr("Nothing matches the search."));
            return;
        }
        let texts = pkg.texts.get(razdor::pack::open::lang()).or_else(|| pkg.texts.values().next());
        let current = self.story_sel_id();
        let mut pick: Option<(String, OutlineRow)> = None;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for g in &groups {
                let title = if g.map == EVERY_MAP { tr("Every map").to_string() } else { g.map.clone() };
                // The shown map's group opens; the others stay folded until a search or a click opens them.
                let open = self.maps.map.as_deref() == Some(g.map.as_str()) || !self.scen.outline_query.trim().is_empty();
                CollapsingHeader::new(title).id_salt(("outline-map", g.map.as_str())).default_open(open).show(ui, |ui| {
                    for row in &g.rows {
                        let node = match row.kind {
                            OutlineKind::Trigger => sg::trigger_id(&row.id),
                            OutlineKind::Dialog => sg::dialog_id(&row.id),
                        };
                        let icon = if row.kind == OutlineKind::Trigger { ic::LIGHTNING } else { ic::CHAT_CIRCLE_TEXT };
                        let sub = match row.kind {
                            OutlineKind::Trigger => row.sub.clone(),
                            OutlineKind::Dialog => texts.and_then(|t| t.get(&row.sub)).cloned().unwrap_or_else(|| row.sub.clone()),
                        };
                        ui.horizontal(|ui| {
                            ui.label(icon);
                            let clicked = ui.selectable_label(current.as_deref() == Some(node.as_str()), row.id.as_str()).clicked();
                            if clicked {
                                pick = Some((g.map.clone(), row.clone()));
                            }
                        });
                        if !sub.is_empty() {
                            ui.indent(("outline-sub", node.as_str()), |ui| ui.weak(sub));
                        }
                    }
                });
            }
        });
        if let Some((map, row)) = pick {
            self.outline_pick(&map, &row);
        }
    }

    /// A click in the outline: the node is selected (its form in the Inspector), the map it is on is shown, and
    /// the graph centres on it in the Story view.
    fn outline_pick(&mut self, map: &str, row: &OutlineRow) {
        if map != EVERY_MAP && self.maps.map.as_deref() != Some(map) {
            self.scen.want_map = Some(map.to_string());
        }
        let (sel, node) = match row.kind {
            OutlineKind::Trigger => (StorySel::Trigger(row.id.clone()), sg::trigger_id(&row.id)),
            OutlineKind::Dialog => (StorySel::Dialog(row.id.clone()), sg::dialog_id(&row.id)),
        };
        self.story_goto(sel);
        self.scen.story.centre = Some(node);
        self.scen.story_view = StoryView::Story;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outline_tabs_and_the_views_start_on_the_story() {
        assert_eq!(StoryView::default(), StoryView::Story);
        assert_eq!(OutlineTab::default(), OutlineTab::Story);
    }
}
