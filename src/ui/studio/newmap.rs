//! Package › «Новая карта (генератор)…»: the map generator's window, with the options of the
//! original's new-map dialog (size, map type, orientation, the ratio bars, blur, seed) and of its
//! world generator (the three steps, the chances and the budgets). «Создать» runs the generator
//! on a worker thread; the map it made is added to the package as one undo step (`gen::apply`).

use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

use egui::{Button, Color32, ComboBox, CornerRadius, DragValue, Pos2, Rect, Sense, Slider, Vec2};
use egui_macroquad::egui;
use razdor::dt::install::DtInstall;
use razdor::i18n::{n_, tr};
use razdor::studio::gen::{self, apply, newmap, to_package, worldgen};
use razdor::studio::Workspace;

/// The chance drop-downs' labels, by [`worldgen::Chance`].
const CHANCES: [&str; 10] = [
    n_("Towns"),
    n_("Castles"),
    n_("Villages"),
    n_("Ruins"),
    n_("Other"),
    n_("Armies in towns"),
    n_("Armies in castles"),
    n_("Armies in villages"),
    n_("Armies in ruins"),
    n_("Armies in other buildings"),
];

/// The budget spin boxes' labels, by [`worldgen::budget`].
const BUDGETS: [&str; 16] = ["T1", "T2", "C1", "C2", "V1", "V2", "R1", "R2", "O1", "O2", "TG1", "TG2", "CG1", "CG2", "RG1", "RG2"];

/// The panels' labels, by [`newmap::Field`]: the editor's names of them.
fn field_name(f: newmap::Field) -> &'static str {
    match f {
        newmap::Field::W0 => "W0",
        newmap::Field::W1 => "W1",
        newmap::Field::W3 => "W3",
        newmap::Field::Sl => "SL",
        newmap::Field::Ll => "LL",
        newmap::Field::Lh => "LH",
        newmap::Field::Lr => "LR",
        newmap::Field::Sl1 => "SL1",
        newmap::Field::Sl3 => "SL3",
        newmap::Field::Sw1 => "Sw1",
        newmap::Field::F0 => "F0",
        newmap::Field::F2 => "F2",
    }
}

/// The colours of the bars' panels, cycled.
const PANEL_COLOURS: [Color32; 4] = [Color32::from_rgb(0x3a, 0x6e, 0xa8), Color32::from_rgb(0x5f, 0x9e, 0x4a), Color32::from_rgb(0xb0, 0x9a, 0x4a), Color32::from_rgb(0x7a, 0x5a, 0x9a)];

/// A map on its way from the worker: the answer, the id it is for and whether it joins the campaign.
type Pending = (Receiver<Result<to_package::PackageMap, String>>, String, bool);

/// The window's state between frames.
pub struct NewMap {
    pub open: bool,
    size: u32,
    kind: u8,
    orient: u8,
    blur: u8,
    shares: newmap::Shares,
    seed: i32,
    buildings: bool,
    economy: bool,
    armies: bool,
    options: worldgen::Options,
    id: String,
    title: String,
    in_campaign: bool,
    status: String,
    /// The worker's answer: the map, with the id and title it was made for.
    job: Option<Pending>,
}

impl Default for NewMap {
    fn default() -> Self {
        NewMap::new()
    }
}

impl NewMap {
    /// The dialog's defaults: 50 cells, land, north, blur 2, the install's ratios, seed 1, all
    /// three steps on.
    pub fn new() -> NewMap {
        NewMap {
            open: false,
            size: 50,
            kind: newmap::LAND,
            orient: 0,
            blur: 2,
            shares: newmap::Shares::default(),
            seed: 1,
            buildings: true,
            economy: true,
            armies: true,
            options: worldgen::Options::new(50),
            id: "generated".into(),
            title: "Generated map".into(),
            in_campaign: false,
            status: String::new(),
            job: None,
        }
    }

    fn request(&self) -> gen::Request {
        gen::Request {
            map: newmap::Options { size: self.size, kind: self.kind, orient: self.orient, shares: self.shares, blur: self.blur, keep: true, rebuild: true },
            seed: self.seed,
            clock: gen::clock_of(self.seed),
            world: Some(gen::World { buildings: self.buildings, economy: self.economy, armies: self.armies, options: self.options.clone() }),
        }
    }

    /// A new seed from the clock, never 0.
    fn random_seed() -> i32 {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos());
        ((nanos % 2_000_000_000) as i32).max(1)
    }

    /// Draws the window. Returns the id of a map that was added to the package this frame (the
    /// caller shows it).
    pub fn show(&mut self, ctx: &egui::Context, ws: &mut Workspace, install: Option<&Arc<DtInstall>>) -> Option<String> {
        let mut open = self.open;
        egui::Window::new(tr("New map (generator)")).open(&mut open).collapsible(false).resizable(true).default_width(560.0).default_height(660.0).show(ctx, |ui| {
            // The buttons and the status stay in view; the options scroll above them.
            egui::TopBottomPanel::bottom("newmap-buttons").show_separator_line(true).show_inside(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let busy = self.job.is_some();
                    if ui.add_enabled(install.is_some() && !busy, Button::new(tr("Create"))).clicked() {
                        self.start(install);
                    }
                    if ui.button(tr("Cancel")).clicked() {
                        self.open = false;
                    }
                    if busy {
                        ui.spinner();
                        ui.weak(tr("Creating…"));
                    }
                });
                if install.is_none() {
                    ui.colored_label(Color32::from_rgb(0xe0, 0xb0, 0x50), tr("The generator needs the game's files (RAZDOR_DT_DIR): open the install first."));
                }
                if !self.status.is_empty() {
                    ui.colored_label(Color32::from_rgb(0xe0, 0xb0, 0x50), self.status.as_str());
                }
                ui.add_space(4.0);
            });
            egui::CentralPanel::default().show_inside(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    self.terrain_form(ui);
                    ui.separator();
                    self.name_form(ui);
                    ui.separator();
                    self.world_form(ui);
                });
            });
        });
        self.open = open && self.open;
        let added = self.finish(ws);
        if self.job.is_some() {
            ctx.request_repaint();
        }
        added
    }

    fn terrain_form(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr("Terrain"));
        egui::Grid::new("newmap-terrain").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(tr("Size"));
            ComboBox::from_id_salt("newmap-size").selected_text(format!("{0} × {0}", self.size)).show_ui(ui, |ui| {
                for s in [50u32, 100, 200] {
                    ui.selectable_value(&mut self.size, s, format!("{s} × {s}"));
                }
            });
            ui.end_row();
            ui.label(tr("Map type"));
            ComboBox::from_id_salt("newmap-kind").selected_text(tr(newmap::KINDS[self.kind as usize])).show_ui(ui, |ui| {
                for (i, k) in newmap::KINDS.iter().enumerate() {
                    ui.selectable_value(&mut self.kind, i as u8, tr(k));
                }
            });
            ui.end_row();
            ui.label(tr("Orientation"));
            ComboBox::from_id_salt("newmap-orient").selected_text(tr(newmap::ORIENTATIONS[self.orient as usize])).show_ui(ui, |ui| {
                for (i, o) in newmap::ORIENTATIONS.iter().enumerate() {
                    ui.selectable_value(&mut self.orient, i as u8, tr(o));
                }
            });
            ui.end_row();
            ui.label(tr("Blur"));
            ui.add(Slider::new(&mut self.blur, 0..=newmap::MAX_BLUR));
            ui.end_row();
            ui.label(tr("Seed"));
            ui.horizontal(|ui| {
                ui.add(DragValue::new(&mut self.seed).range(1..=i32::MAX));
                if ui.button(tr("Random")).clicked() {
                    self.seed = NewMap::random_seed();
                }
            });
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.label(tr("Ratios"));
        for bar in 0..newmap::BARS.len() {
            self.ratio_bar(ui, bar);
        }
    }

    /// One ratio bar: its panels drawn to scale, and the sized panels' widths as boxes.
    fn ratio_bar(&mut self, ui: &mut egui::Ui, bar: usize) {
        let panels = self.shares.panels(bar);
        let total: i32 = panels.iter().sum::<i32>().max(1);
        let width = ui.available_width().min(420.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 14.0), Sense::hover());
        let mut x = rect.left();
        for (k, p) in panels.iter().enumerate() {
            let w = rect.width() * *p as f32 / total as f32;
            let r = Rect::from_min_max(Pos2::new(x, rect.top()), Pos2::new(x + w, rect.bottom()));
            ui.painter().rect_filled(r, CornerRadius::same(2), PANEL_COLOURS[k % PANEL_COLOURS.len()]);
            x += w;
        }
        ui.horizontal(|ui| {
            for (k, s) in newmap::BARS[bar].iter().enumerate() {
                let mut v = self.shares.get(s.field);
                ui.label(field_name(s.field));
                if ui.add(DragValue::new(&mut v).range(0..=106)).changed() {
                    self.shares.drag(bar, k, v);
                }
            }
            let stretched = self.shares.stretched(bar);
            ui.weak(format!("{} ", stretched));
        });
    }

    fn world_form(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr("World"));
        ui.checkbox(&mut self.buildings, tr("Buildings and roads"));
        ui.checkbox(&mut self.economy, tr("Economy"));
        ui.checkbox(&mut self.armies, tr("Armies and garrisons"));
        egui::CollapsingHeader::new(tr("Chances")).default_open(false).show(ui, |ui| {
            for (i, label) in CHANCES.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(tr(label));
                    ComboBox::from_id_salt(("newmap-chance", i)).selected_text(format!("{} %", worldgen::chance(self.options.chances[i]))).show_ui(ui, |ui| {
                        for k in 0..worldgen::CHANCE_ENTRIES {
                            ui.selectable_value(&mut self.options.chances[i], k, format!("{} %", worldgen::chance(k)));
                        }
                    });
                });
            }
        });
        egui::CollapsingHeader::new(tr("Budgets")).default_open(false).show(ui, |ui| {
            egui::Grid::new("newmap-budgets").num_columns(4).spacing([10.0, 4.0]).show(ui, |ui| {
                for (i, label) in BUDGETS.iter().enumerate() {
                    ui.label(*label);
                    ui.add(DragValue::new(&mut self.options.budgets[i]).range(0..=worldgen::budget::MAX).speed(worldgen::budget::STEP as f64));
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        });
    }

    fn name_form(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr("Map"));
        egui::Grid::new("newmap-name").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(tr("Map id"));
            ui.add(egui::TextEdit::singleline(&mut self.id).desired_width(240.0));
            ui.end_row();
            ui.label(tr("Title"));
            ui.add(egui::TextEdit::singleline(&mut self.title).desired_width(320.0));
            ui.end_row();
        });
        ui.checkbox(&mut self.in_campaign, tr("Add to the campaign"));
    }

    /// Checks the form and starts the worker.
    fn start(&mut self, install: Option<&Arc<DtInstall>>) {
        self.status.clear();
        if !apply::valid_id(&self.id) {
            self.status = tr("The map id is lowercase letters, digits and dashes.").into();
            return;
        }
        if self.seed == 0 {
            self.status = tr("Seed 0 cannot be replayed: type another seed, or press Random.").into();
            return;
        }
        let Some(dt) = install.cloned() else { return };
        let req = self.request();
        let id = self.id.clone();
        let title = self.title.trim().to_string();
        let title = if title.is_empty() { id.clone() } else { title };
        let (tx, rx) = channel();
        let worker_id = id.clone();
        std::thread::spawn(move || {
            let _ = tx.send(build(&dt, &req, &worker_id, &title));
        });
        self.job = Some((rx, id, self.in_campaign));
    }

    /// Takes the worker's map when it is ready and adds it to the package. Returns its id.
    fn finish(&mut self, ws: &mut Workspace) -> Option<String> {
        let (rx, id, in_campaign) = self.job.take()?;
        match rx.try_recv() {
            Ok(Ok(map)) => match apply::add_map(ws, &id, &map, in_campaign) {
                Ok(()) => {
                    self.open = false;
                    self.status.clear();
                    Some(id)
                }
                Err(e) => {
                    self.status = e;
                    None
                }
            },
            Ok(Err(e)) => {
                self.status = e;
                None
            }
            Err(TryRecvError::Empty) => {
                self.job = Some((rx, id, in_campaign));
                None
            }
            Err(TryRecvError::Disconnected) => {
                self.status = tr("The generator stopped without a map.").into();
                None
            }
        }
    }

    /// Makes the map from the install's data (the worker's job, also the snapshot's).
    pub fn make(dt: &DtInstall, req: &gen::Request, id: &str, title: &str) -> Result<to_package::PackageMap, String> {
        build(dt, req, id, title)
    }
}

/// The map of a request, from the install (the worker's work): the terrain, the world steps and
/// the package's form of it.
fn build(dt: &DtInstall, req: &gen::Request, id: &str, title: &str) -> Result<to_package::PackageMap, String> {
    let data = gen::InstallData::from_install(dt)?;
    let made = gen::generate(req, &data.sources(), &std::sync::atomic::AtomicBool::new(false)).map_err(|f| gen::describe(&f))?;
    to_package::to_package(&made, id, title, &to_package::unit_aliases()).map_err(|r| gen::describe_refusal(&r))
}
