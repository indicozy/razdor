//! The Units and Items screens (P5): the package's records as lists with a filter and a new record
//! on a base, and each record's form in the Inspector: its fields with the base value each one
//! overrides (a reset per field), the suggested price from DT's own formula with its breakdown and
//! «Apply» (`studio::cost`, on the game's install), the level table, what the package format cannot
//! hold, the places that name it, rename and delete. Every change is one undo step: a field, a price
//! (`Workspace::apply_doc`), a rename across files (`set_texts`), a new record (a group).

use std::sync::Arc;

use egui::{CollapsingHeader, ComboBox, Grid, ScrollArea, TextEdit, Ui};
use egui_macroquad::egui;
use egui_phosphor::regular as ic;
use razdor::dt::data::Stat;
use razdor::i18n::tr;
use razdor::pack::aliases;
use razdor::pack::load::Package;
use razdor::pack::refs::Ref;
use razdor::studio::cost::{ItemPrice, UnitPrice};
use razdor::studio::records::{self as rec, ItemFail, Kind, Priced, Refused};
use razdor::trf;

use super::forms;
use super::inspector_kit as kit;
use super::scenarios::Focus;
use super::{Jump, Viewer, ERROR_RED, WARNING_AMBER};

/// The label of an override field (the Russian names of the original's windows, records.md §5.1).
fn field_label(key: &str) -> &str {
    match key {
        "cost" => tr("Price, gold"),
        "hits" => tr("Hits"),
        "attack_blow" => tr("Attack, blows"),
        "attack_shot" => tr("Attack, shots"),
        "magic_power" => tr("Magic power"),
        "defence_blow" => tr("Defence, blows"),
        "defence_shot" => tr("Defence, shots"),
        "protect_life" => tr("Protection, life"),
        "protect_death" => tr("Protection, death"),
        "protect_elemental" => tr("Protection, elements"),
        "initiative" => tr("Initiative"),
        "manevres" => tr("Manoeuvres"),
        "regen" => tr("Regeneration"),
        "vampirism" => tr("Vampirism"),
        "evasion" => tr("Evasion"),
        _ => "",
    }
}

/// What a record's Inspector form changes this frame; applied after the form is drawn.
#[derive(Default)]
struct Asks {
    sets: Vec<(String, Option<i64>)>,
    rename: Option<String>,
    delete: bool,
    apply: bool,
    keep: bool,
    jump: Option<(String, u32)>,
}

/// The unit's price as the form shows it: the formula's steps, or why there is none.
enum UnitQuote {
    Price(UnitPrice),
    Unresolved(String),
}

/// The item's price as the form shows it.
enum ItemQuote {
    Price(ItemPrice),
    Skip(razdor::studio::cost::PriceSkip),
    Unresolved(String),
}

impl Viewer<'_> {
    /// The package's prices, built on the install for this package (rebuilt when the package is).
    /// `None` without the install.
    fn priced(&mut self, pkg: &Arc<Package>) -> Option<&mut Priced> {
        let install = self.install.clone()?;
        let fresh = matches!(&self.scen.priced, Some((p, _)) if Arc::ptr_eq(p, pkg));
        if !fresh {
            let lang = razdor::pack::open::lang();
            self.scen.priced = Some((pkg.clone(), Priced::new(&install, pkg.as_ref(), lang)));
        }
        self.scen.priced.as_mut().map(|(_, p)| p)
    }

    /// The selected record's id of `kind`.
    fn record_sel(&mut self, kind: Kind) -> &mut Option<String> {
        match kind {
            Kind::Unit => &mut self.scen.unit,
            Kind::Item => &mut self.scen.item,
        }
    }

    /// Selects a record: its form shows in the Inspector.
    fn record_pick(&mut self, kind: Kind, id: String) {
        *self.record_sel(kind) = Some(id);
        self.scen.focus = match kind {
            Kind::Unit => Focus::Units,
            Kind::Item => Focus::Items,
        };
        self.scen.error = None;
    }

    /// Shows an error under the form and in the log.
    fn record_fail(&mut self, e: String) {
        self.log.push(e.clone());
        self.scen.error = Some(e);
    }

    /// The Units screen.
    pub(super) fn units_screen(&mut self, ui: &mut Ui) {
        self.records_screen(ui, Kind::Unit);
    }

    /// The Items screen.
    pub(super) fn items_screen(&mut self, ui: &mut Ui) {
        self.records_screen(ui, Kind::Item);
    }

    /// The Inspector's form of the selected unit.
    pub(super) fn unit_inspector(&mut self, ui: &mut Ui) {
        self.record_inspector(ui, Kind::Unit);
    }

    /// The Inspector's form of the selected item.
    pub(super) fn item_inspector(&mut self, ui: &mut Ui) {
        self.record_inspector(ui, Kind::Item);
    }

    fn records_screen(&mut self, ui: &mut Ui, kind: Kind) {
        let Some(pkg) = self.pkg.clone() else {
            ui.colored_label(ERROR_RED, tr("The package could not be read: see Problems."));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(tr("Filter"));
            ui.add(TextEdit::singleline(&mut self.scen.record_filter).desired_width(160.0));
        });
        // The new record: its id, its base (a base of the game) and the button.
        let base_list: Vec<&str> = aliases::names(kind.dt_kind()).collect();
        if self.scen.new_base.is_empty() {
            if let Some(first) = base_list.first() {
                self.scen.new_base = first.to_string();
            }
        }
        let mut create = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(if kind == Kind::Unit { tr("New unit") } else { tr("New item") });
            ui.add(TextEdit::singleline(&mut self.scen.new_id).hint_text(tr("id")).desired_width(110.0));
            ComboBox::from_id_salt(("record-new-base", kind.key())).selected_text(self.scen.new_base.clone()).height(300.0).show_ui(ui, |ui| {
                for a in &base_list {
                    if ui.selectable_label(self.scen.new_base == *a, *a).clicked() {
                        self.scen.new_base = a.to_string();
                    }
                }
            });
            if ui.button(tr("Create")).on_hover_text(tr("Makes the record's file and its name text in every language, on the chosen base")).clicked() {
                create = true;
            }
        });
        if let Some(e) = self.scen.error.clone() {
            ui.colored_label(ERROR_RED, e);
        }
        if create {
            self.record_create(&pkg, kind);
        }
        ui.separator();
        let filter = self.scen.record_filter.trim().to_lowercase();
        let selected = self.record_sel(kind).clone();
        let mut pick: Option<String> = None;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for r in rec::rows(&pkg, kind) {
                if !filter.is_empty() && !format!("{} {} {}", r.id, r.base, r.name).to_lowercase().contains(&filter) {
                    continue;
                }
                ui.horizontal_wrapped(|ui| {
                    let text = format!("{} · {}", r.id, r.base);
                    if ui.selectable_label(selected.as_deref() == Some(r.id.as_str()), text).clicked() {
                        pick = Some(r.id.clone());
                    }
                    ui.weak(format!("{}:{}", r.file, r.line));
                });
            }
        });
        if let Some(id) = pick {
            self.record_pick(kind, id);
        }
    }

    /// Makes the new record of the form's id and base: its file and its name text in every language,
    /// as one undo step.
    fn record_create(&mut self, pkg: &Arc<Package>, kind: Kind) {
        let id = self.scen.new_id.trim().to_string();
        let alias = self.scen.new_base.clone();
        let name = self.base_name(pkg, kind, &alias);
        let n = match rec::new_record(pkg, kind, &id, &alias, &name) {
            Ok(n) => n,
            Err(e) => return self.record_fail(e),
        };
        let label = format!("add {} {id}", kind.key());
        self.ws.begin_group();
        let mut result = self.ws.create_text_file(&n.file, &n.text, &label);
        for (file, edit) in n.texts {
            if result.is_ok() {
                result = self.ws.apply_doc(&file, &label, &[edit]);
            }
        }
        match result {
            Ok(()) => {
                self.ws.end_group(&label);
                self.scen.new_id.clear();
                self.scen.error = None;
                self.changed = true;
                self.record_pick(kind, id);
            }
            Err(e) => {
                self.ws.cancel_group();
                self.record_fail(e);
            }
        }
    }

    /// The name of the base record, as the game names it (the alias when the install is not there).
    fn base_name(&mut self, pkg: &Arc<Package>, kind: Kind, alias: &str) -> String {
        let r = match kind {
            Kind::Unit => Ref::parse(&format!("dt:{alias}")),
            Kind::Item => Ref::parse(&format!("dt:item/{alias}")),
        };
        let name = match (self.priced(pkg), r) {
            (Some(p), Ok(r)) => match kind {
                Kind::Unit => p.base_unit(&r).map(|d| d.name),
                Kind::Item => p.base_item(&r).map(|a| a.name),
            },
            _ => None,
        };
        name.filter(|n| !n.is_empty()).unwrap_or_else(|| alias.to_string())
    }

    /// Sets a record's field `key` to `value` (`None`: the override is removed), one undo step.
    fn record_set(&mut self, pkg: &Arc<Package>, kind: Kind, id: &str, key: &str, value: Option<i64>) {
        let label = match value {
            Some(_) => format!("set {key} of {id}"),
            None => format!("reset {key} of {id}"),
        };
        match rec::set_edit(pkg, kind, id, key, value) {
            Ok((file, edit)) => match self.ws.apply_doc(&file, &label, &[edit]) {
                Ok(()) => {
                    self.scen.error = None;
                    self.changed = true;
                }
                Err(e) => self.record_fail(e),
            },
            Err(e) => self.record_fail(e),
        }
    }

    /// Writes the suggested price into the record's cost field, one undo step. A unit whose formula
    /// gives no price (a tactical cost below 1, or gold over 65,535) and an item the formula leaves
    /// alone are refused with the reason.
    fn record_apply_price(&mut self, pkg: &Arc<Package>, kind: Kind, id: &str) {
        let price: Result<i64, String> = match self.priced(pkg) {
            None => Err(tr("The price needs the game's install: open the studio with the game installed.").to_string()),
            Some(p) => match kind {
                Kind::Unit => match p.unit_price(id) {
                    Err(e) => Err(e),
                    Ok(q) if q.tactical < 1 => Err(tr("The tactical cost is below 1: the formula gives no price for this unit.").to_string()),
                    Ok(q) => q.price.ok_or_else(|| tr("The gold is over 65,535: the formula gives no price for this unit.").to_string()),
                },
                Kind::Item => match p.item_price(id) {
                    Ok(q) => Ok(q.banded),
                    Err(ItemFail::Skip(_)) => Err(tr("The formula leaves this artefact unpriced: see the note above the price.").to_string()),
                    Err(ItemFail::Unresolved(e)) => Err(e),
                },
            },
        };
        match price {
            Ok(v) => self.record_set(pkg, kind, id, "cost", Some(v)),
            Err(e) => self.record_fail(e),
        }
    }

    /// Deletes the record, refused (with its first use) while the package names it.
    fn record_delete(&mut self, pkg: &Arc<Package>, kind: Kind, id: &str) {
        match rec::delete_edit(pkg, kind, id) {
            Ok((file, edit)) => match self.ws.apply_doc(&file, &format!("delete {} {id}", kind.key()), &[edit]) {
                Ok(()) => {
                    *self.record_sel(kind) = None;
                    self.scen.error = None;
                    self.changed = true;
                }
                Err(e) => self.record_fail(e),
            },
            Err(Refused::NotFound) => self.record_fail(tr("No such record in the package.").to_string()),
            Err(Refused::Used(uses)) => {
                let first = format!("{}:{}, {}", uses[0].file, uses[0].line, uses[0].what);
                let count = uses.len();
                self.record_fail(trf!("Cannot delete: named by {count} places, the first at {first}", count = count, first = first));
            }
        }
    }

    /// Renames the record in every file that names it, one undo step.
    fn record_rename(&mut self, pkg: &Arc<Package>, kind: Kind, old: &str, new: &str) {
        let plan = {
            let ws = &*self.ws;
            rec::rename_plan(pkg, kind, old, new, &|f: &str| ws.text_of(f).ok())
        };
        match plan {
            Ok(files) => match self.ws.set_texts(&format!("rename {old} to {new}"), files) {
                Ok(()) => {
                    self.record_pick(kind, new.to_string());
                    self.scen.record_rename.clear();
                    self.changed = true;
                }
                Err(e) => self.record_fail(e),
            },
            Err(e) => self.record_fail(e),
        }
    }

    /// The Inspector's form of a record of `kind`.
    fn record_inspector(&mut self, ui: &mut Ui, kind: Kind) {
        let Some(pkg) = self.pkg.clone() else {
            ui.colored_label(ERROR_RED, tr("The package could not be read: see Problems."));
            return;
        };
        let Some(id) = self.record_sel(kind).clone() else {
            ui.weak(if kind == Kind::Unit { tr("Nothing selected: pick a unit in the Units tab.") } else { tr("Nothing selected: pick an item in the Items tab.") });
            return;
        };
        let Some(row) = rec::rows(&pkg, kind).into_iter().find(|r| r.id == id) else {
            *self.record_sel(kind) = None;
            ui.weak(tr("This record is gone: its file no longer names it."));
            return;
        };
        let fields: &[&str] = if kind == Kind::Unit { &rec::UNIT_FIELDS } else { &rec::ITEM_FIELDS };
        let price_open = self.scen.price_open;
        let has_install = self.install.is_some();

        // The values the form shows, read from the prices (the game's content) before the form is drawn.
        let (bases, unit_quote, item_quote, levels) = match self.priced(&pkg) {
            None => (vec![None; fields.len()], None, None, Vec::new()),
            Some(p) => {
                let bases: Vec<Option<i64>> = fields.iter().map(|k| rec::base_value(&pkg, p, kind, &id, k)).collect();
                match kind {
                    Kind::Unit => {
                        let quote = match p.unit_price(&id) {
                            Ok(q) => UnitQuote::Price(q),
                            Err(e) => UnitQuote::Unresolved(e),
                        };
                        let levels: Vec<[i64; 4]> = (1..=5)
                            .map(|l| {
                                let pct = p.level_percent(&id).map_or(0, |t| t[(l - 1) as usize]);
                                match p.level_stats(&id, l) {
                                    Some(s) => [s[Stat::Hits] as i64, s[Stat::AttackBlow] as i64, s[Stat::DefenceBlow] as i64, pct],
                                    None => [0, 0, 0, pct],
                                }
                            })
                            .collect();
                        (bases, Some(quote), None, levels)
                    }
                    Kind::Item => {
                        let quote = match p.item_price(&id) {
                            Ok(q) => ItemQuote::Price(q),
                            Err(ItemFail::Skip(s)) => ItemQuote::Skip(s),
                            Err(ItemFail::Unresolved(e)) => ItemQuote::Unresolved(e),
                        };
                        (bases, None, Some(quote), Vec::new())
                    }
                }
            }
        };

        let mut asks = Asks::default();
        let (icon, kind_word) = if kind == Kind::Unit { (ic::SHIELD_STAR, tr("Unit type")) } else { (ic::SCROLL, tr("Item")) };
        let q = super::inspector::head(ui, icon, &format!("{kind_word}: {id}"), &row.base);
        let kind_key = kind.key();

        // The price: the formula's number, «Apply» and «Keep» (the suggestion goes away for this record), and
        // its steps in a collapsible. The card is hidden when the author has kept the price.
        let price_kept = self.scen.price_kept.as_deref() == Some(id.as_str());
        kit::section(ui, kind_key, "price", tr("Price"), true, &q, |ui| {
            if price_kept {
                kit::note(ui, tr("The suggested price is kept as it is (the price field holds it)."));
                return;
            }
            kit::card(ui, |ui| {
                ui.strong(tr("Suggested price"));
                if !has_install {
                    ui.colored_label(WARNING_AMBER, tr("The price needs the game's install: open the studio with the game installed."));
                }
                let priced = match (&unit_quote, &item_quote) {
                    (Some(UnitQuote::Price(p)), _) if p.tactical >= 1 && p.price.is_some() => Some(p.price.unwrap_or(0)),
                    (_, Some(ItemQuote::Price(p))) => Some(p.banded),
                    _ => None,
                };
                match (&unit_quote, &item_quote) {
                    (Some(UnitQuote::Price(p)), _) => {
                        if p.tactical < 1 || p.price.is_none() {
                            ui.colored_label(WARNING_AMBER, tr("The formula gives no price here (see the steps)."));
                        }
                    }
                    (Some(UnitQuote::Unresolved(e)), _) => {
                        ui.colored_label(ERROR_RED, trf!("Cannot price: {e}", e = e));
                    }
                    (_, Some(ItemQuote::Skip(razdor::studio::cost::PriceSkip::NotPriced))) => {
                        ui.weak(tr("Not priced: the price is 1, or the artefact is a potion or an item (the button leaves it alone)."));
                    }
                    (_, Some(ItemQuote::Skip(razdor::studio::cost::PriceSkip::NoGain))) => {
                        ui.colored_label(WARNING_AMBER, tr("No unit type gains from it: the original's price there is not known. Set it by hand."));
                    }
                    (_, Some(ItemQuote::Unresolved(e))) => {
                        ui.colored_label(ERROR_RED, trf!("Cannot price: {e}", e = e));
                    }
                    _ => {}
                }
                if let Some(gold) = priced {
                    ui.label(trf!("{gold} gold, by the game's formula", gold = gold));
                    ui.horizontal(|ui| {
                        if ui.button(tr("Apply the price")).on_hover_text(tr("Write this price into the price field (one undo step)")).clicked() {
                            asks.apply = true;
                        }
                        if ui.button(tr("Keep")).on_hover_text(tr("Leave the price field as it is")).clicked() {
                            asks.keep = true;
                        }
                    });
                }
                CollapsingHeader::new(tr("How the price is made")).id_salt(("studio-inspector", kind_key, "price-steps")).default_open(price_open).show(ui, |ui| {
                    if let Some(UnitQuote::Price(p)) = &unit_quote {
                        ui.label(trf!("Strength S, the battle core's evaluator on the base stats: {s}", s = p.strength));
                        ui.label(trf!("Tactical cost T: S times {mult} / 100 gives {t}", mult = p.multiplier, t = p.tactical));
                        if p.tactical >= 1 {
                            match p.gold_raw {
                                Some(g) => ui.label(trf!("Gold: (T / 2.2 + 21) / {div} gives {g}", div = p.gold_div, g = g)),
                                None => ui.colored_label(ERROR_RED, tr("Gold over 65,535: a range error in the original, no price.")),
                            };
                            if let Some(b) = p.banded {
                                ui.label(trf!("Rounded down in bands: {b}", b = b));
                            }
                            if p.hero {
                                ui.label(trf!("Hero class, doubled: {price}", price = p.price.unwrap_or(0)));
                            } else {
                                ui.weak(tr("Not a hero class: no doubling."));
                            }
                        }
                    }
                    if let Some(ItemQuote::Price(p)) = &item_quote {
                        ui.label(trf!("Gains summed over the units that gain: {sum} (wearers {n}, gainers {m})", sum = p.sum, n = p.wearers, m = p.gainers));
                        ui.label(trf!("Mean of the gains, plus 10, at least 20: {mean} plus 10 gives {raw}", mean = p.mean, raw = p.raw));
                        ui.label(trf!("Rounded down in bands: {b}", b = p.banded));
                    }
                    if item_quote.is_none() && unit_quote.is_none() {
                        ui.weak(tr("No price steps: the game's install is needed."));
                    }
                });
            });
        });

        // The fields: the override (or the base's value), the base next to it, the changed dot and the reset.
        kit::section(ui, kind_key, "fields", tr("Fields"), true, &q, |ui| {
            kit::note(ui, tr("A field with no override takes the base's value. Reset removes the override."));
            for (i, key) in fields.iter().enumerate() {
                let ov = rec::override_of(&pkg, kind, &id, key);
                let base = bases[i];
                let (lo, hi) = rec::field_range(key);
                let model = ov.or(base).unwrap_or(0);
                let shown = kit::row(ui, &q, field_label(key), &format!("{kind_key}.{key}"), Some(ov.is_some()), |ui| {
                    ui.horizontal(|ui| {
                        let v = forms::int_field(ui, &format!("{id}-{key}"), model, lo, hi);
                        match base {
                            Some(b) => ui.weak(format!("{} {b}", tr("base"))),
                            None => ui.weak(tr("base: no install")),
                        };
                        v
                    })
                    .inner
                });
                if let Some(Some(v)) = shown.value {
                    asks.sets.push((key.to_string(), Some(v)));
                }
                if shown.reset {
                    asks.sets.push((key.to_string(), None));
                }
            }
            kit::note(ui, if kind == Kind::Unit {
                tr("Not in the package format, so the base's values hold: per-level gains, nature, magic school and direction, bonus, icon, upgrade units, surrender, starting experience, level multiplier, cost multiplier and gold divisor.")
            } else {
                tr("Not in the package format, so the base's values hold: the stat changes, type, magic school and bonus. The package holds the price.")
            });
        });

        if kind == Kind::Unit && !levels.is_empty() {
            kit::section(ui, kind_key, "levels", tr("Levels 1 to 5"), false, &q, |ui| {
                Grid::new("record-levels").num_columns(5).spacing([10.0, 4.0]).striped(true).show(ui, |ui| {
                    ui.strong(tr("Level"));
                    ui.strong(tr("Hits"));
                    ui.strong(tr("Attack, blows"));
                    ui.strong(tr("Defence, blows"));
                    ui.strong(tr("Tactical cost, % of base"));
                    ui.end_row();
                    for (k, row) in levels.iter().enumerate() {
                        ui.label(format!("{}", k + 1));
                        ui.label(format!("{}", row[0]));
                        ui.label(format!("{}", row[1]));
                        ui.label(format!("{}", row[2]));
                        ui.label(format!("{}%", row[3]));
                        ui.end_row();
                    }
                });
                kit::note(ui, tr("The base's per-level gains: the package has none of its own."));
            });
        }

        kit::section(ui, kind_key, "record", tr("Record"), true, &q, |ui| {
            kit::row(ui, &q, tr("Base"), &format!("{kind_key}.base"), None, |ui| {
                ui.monospace(row.base.as_str());
                ui.weak(format!("{}:{}", row.file, row.line));
            });
            kit::row(ui, &q, tr("Name"), &format!("{kind_key}.name"), None, |ui| {
                ui.weak(egui::RichText::new(row.name.as_str()).monospace());
            });
            if let Some(text) = pkg.texts.get(razdor::pack::open::lang()).and_then(|t| t.get(&row.name)).or_else(|| pkg.texts.values().find_map(|t| t.get(&row.name))) {
                kit::row(ui, &q, tr("Text"), &format!("{kind_key}.text"), None, |ui| ui.label(text.as_str()));
            }
            let rename = kit::row(ui, &q, tr("Rename to"), &format!("{kind_key}.id"), None, |ui| {
                ui.add(TextEdit::singleline(&mut self.scen.record_rename).desired_width(120.0));
                ui.button(tr("Rename")).on_hover_text(tr("Renames the id in every file that names it, and its name keys (one undo step)")).clicked()
            });
            if rename.value == Some(true) {
                let new = self.scen.record_rename.trim().to_string();
                if !new.is_empty() {
                    asks.rename = Some(new);
                }
            }
        });

        kit::section(ui, kind_key, "uses", tr("Used by"), true, &q, |ui| {
            let refs = rec::uses(&pkg, kind, &id);
            if refs.is_empty() {
                kit::note(ui, tr("Nothing names this record."));
            } else {
                kit::note(ui, tr("Click a use to open it in Script."));
                for u in &refs {
                    ui.horizontal_wrapped(|ui| {
                        if ui.link(u.what.as_str()).clicked() {
                            asks.jump = Some((u.file.clone(), u.line));
                        }
                        ui.weak(format!("{}:{}", u.file, u.line));
                    });
                }
            }
        });
        ui.add_space(6.0);
        if ui.button(tr("Delete")).on_hover_text(tr("Removes the record from its file; refused while the package names it")).clicked() {
            asks.delete = true;
        }

        // Apply what the form asked, in order: a delete ends the record's form.
        if asks.delete {
            self.record_delete(&pkg, kind, &id);
            return;
        }
        if asks.keep {
            self.scen.price_kept = Some(id.clone());
        }
        for (key, value) in asks.sets {
            self.record_set(&pkg, kind, &id, &key, value);
        }
        if asks.apply {
            self.record_apply_price(&pkg, kind, &id);
        }
        if let Some(new) = asks.rename {
            self.record_rename(&pkg, kind, &id, &new);
        }
        if let Some((file, line)) = asks.jump {
            self.open = Some(file);
            *self.jump = Some(Jump { line, range: None });
        }
    }
}
