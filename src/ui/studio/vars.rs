//! The Variables dock (P4): every variable of the package with where it is set and where it is
//! read; a variable nothing sets or nothing reads is warned. A usage's click selects its trigger or
//! dialog on the Story screen. Renaming changes every file that names the variable, as one undo
//! step (`studio::vars::rename_plan`).

use std::sync::Arc;

use egui::{ScrollArea, Ui};
use egui_macroquad::egui;
use razdor::i18n::tr;
use razdor::pack::load::{Located, Package};
use razdor::pack::triggers::{Var, VarKind, VarValue};
use razdor::studio::vars::{self as vars, Role, VarUse};

use super::forms;
use super::story::{holder_sel, StorySel};
use super::{Tab, Viewer, ERROR_RED, WARNING_AMBER};

impl Viewer<'_> {
    /// The Variables dock.
    pub(super) fn vars_dock(&mut self, ui: &mut Ui) {
        let Some(pkg) = self.pkg.clone() else {
            ui.colored_label(ERROR_RED, tr("The package could not be read: see Problems."));
            return;
        };
        if pkg.vars.is_empty() {
            ui.weak(tr("No variables: declare them in the [vars] table of a triggers file."));
            return;
        }
        ui.weak(tr("Where each variable is set and read. A click opens its trigger or dialog on the Story screen. Renaming changes every file that names it (Ctrl+Z undoes it)."));
        if let Some(e) = self.scen.story.error.clone() {
            ui.colored_label(ERROR_RED, e);
        }
        let mut sorted: Vec<&Located<Var>> = pkg.vars.iter().collect();
        sorted.sort_by(|a, b| a.value.name.cmp(&b.value.name));
        let mut goto: Option<StorySel> = None;
        let mut rename: Option<(String, String)> = None;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for v in sorted {
                let name = v.value.name.clone();
                let uses = vars::variable_uses(&pkg, &name);
                let sets: Vec<&VarUse> = uses.iter().filter(|u| u.role == Role::Write).collect();
                let reads: Vec<&VarUse> = uses.iter().filter(|u| u.role == Role::Read).collect();
                ui.group(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(name.as_str());
                        ui.weak(format!("{} · {}", kind_word(v.value.kind), default_word(&v.value.default)));
                        if let Some(new) = forms::text_field(ui, &format!("var-name-{name}"), &name, 120.0) {
                            rename = Some((name.clone(), new));
                        }
                    });
                    if sets.is_empty() {
                        ui.colored_label(WARNING_AMBER, tr("Never set: nothing writes it, so it stays at its default"));
                    }
                    if reads.is_empty() {
                        ui.colored_label(WARNING_AMBER, tr("Never read: no condition or change tests it"));
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.weak(tr("Set by"));
                        for u in &sets {
                            if ui.link(u.what.as_str()).clicked() {
                                goto = holder_sel(&u.holder);
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.weak(tr("Read by"));
                        for u in &reads {
                            if ui.link(u.what.as_str()).clicked() {
                                goto = holder_sel(&u.holder);
                            }
                        }
                    });
                });
            }
        });
        if let Some(sel) = goto {
            self.story_goto(sel);
            self.scen.want_tab = Some(Tab::Story);
        }
        if let Some((from, to)) = rename {
            self.var_rename(&pkg, &from, &to);
        }
    }

    /// Renames a variable in every file that names it, as one undo step.
    fn var_rename(&mut self, pkg: &Arc<Package>, from: &str, to: &str) {
        if from == to {
            return;
        }
        let plan = vars::rename_plan(pkg, from, to, &|f| self.ws.text_of(f).ok());
        match plan {
            Ok(files) => match self.ws.set_texts(&format!("rename {from} to {to}"), files) {
                Ok(()) => {
                    self.scen.story.error = None;
                    self.changed = true;
                }
                Err(e) => {
                    self.log.push(e.clone());
                    self.scen.story.error = Some(e);
                }
            },
            Err(e) => {
                self.log.push(e.clone());
                self.scen.story.error = Some(e);
            }
        }
    }
}

/// A variable's type, as the forms word it.
fn kind_word(k: VarKind) -> &'static str {
    match k {
        VarKind::Int => "number",
        VarKind::Bool => "yes/no",
    }
}

/// A variable's declared default.
fn default_word(v: &VarValue) -> String {
    match v {
        VarValue::Int(n) => format!("{} {n}", tr("default")),
        VarValue::Bool(b) => format!("{} {b}", tr("default")),
    }
}
