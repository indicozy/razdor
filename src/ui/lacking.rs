//! A refused save and the hub mod it needs (1552): the refusal window's mod button, the switch to
//! an installed mod (then the save loads), the download of a hub mod (confirmed first, then the
//! same switch), and the check of the installed mods for an old save that records no mod.
//!
//! The decision is `hub::save_mod` (pure); this module runs it, with the workers and the window.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

use razdor::hub::save_mod::{self, Need, Offer};
use razdor::hub::{self, Hub, Installed, Kind, Source};
use razdor::i18n::{lang, tr, Lang};
use razdor::rules::map_refs::Unknown;
use razdor::rules::save::SaveMod;
use razdor::trf;

use super::saves::LackingBox;
use super::{finished, App};

/// A refused save waiting for its mod.
pub struct Lacking {
    /// The save, to load once its mod is in place.
    pub path: PathBuf,
    /// The save's name and the ids it lacks, as the refusal's words name them.
    name: String,
    ids: String,
    /// The box's message when a mod is offered: it ends with what the button does.
    message: Option<String>,
    /// The mod it needs; None while an old save's installed mods are checked.
    need: Option<Need>,
    checking: Option<Receiver<Vec<String>>>,
    /// What the window offers.
    offer: Offer,
    /// The offered mod's title (its id when the catalog has none).
    title: String,
    /// The download is being asked for (the Yes/No question is up).
    confirm: bool,
    download: Option<Receiver<Result<(), String>>>,
    /// The mod's switch was asked for: the save loads when it lands.
    switching: bool,
    /// A failure of the download or the switch, shown under the button.
    note: Option<String>,
}

impl Lacking {
    fn new(path: PathBuf) -> Lacking {
        Lacking { path, name: String::new(), ids: String::new(), message: None, need: None, checking: None, offer: Offer::Unknown, title: String::new(), confirm: false, download: None, switching: false, note: None }
    }

    /// The window of a refused save with its offer (the snapshot scenes; no hub is read).
    pub(super) fn scene(path: PathBuf, offer: Offer, title: &str, confirm: bool) -> Lacking {
        let mut l = Lacking::new(path);
        l.name = "Осенний поход".into();
        l.ids = Unknown { units: vec![104, 105, 106], items: vec![35] }.text();
        l.offer = offer;
        l.title = title.into();
        l.confirm = confirm;
        // The need is known (a scene reads no hub): the offer is shown as it is.
        l.need = Some(Need::Unknown);
        l.refresh_message();
        l
    }

    /// The box's message for the offer: when a mod is offered, the text ends with what the button
    /// does; otherwise the save's refusal stands (`None`).
    fn refresh_message(&mut self) {
        let (name, ids, title) = (self.name.clone(), self.ids.clone(), self.title.clone());
        self.message = match &self.offer {
            Offer::Installed { .. } => Some(trf!("The save «{name}» uses units or items that are not in your game ({ids}). It was made with the mod «{title}».", name = name, ids = ids, title = title)),
            Offer::Download { .. } => Some(trf!("The save «{name}» uses units or items that are not in your game ({ids}). It was made with the mod «{title}», which you do not have.", name = name, ids = ids, title = title)),
            Offer::NotInCatalog { .. } | Offer::Unknown => None,
        };
    }
}

fn lang_code() -> &'static str {
    match lang() {
        Lang::En => "en",
        Lang::Ru => "ru",
    }
}

/// The installed mods whose own tables have every id the save lacks, by id (an old save's check, on
/// a worker). It reads the tables only (`save_mod::installed_matching`): no mod's folder is built.
fn check_installed(local: &std::path::Path, base: &std::path::Path, missing: &Unknown) -> Vec<String> {
    save_mod::installed_matching(local, base, missing)
}

/// The download of mod `id` from the catalog (its cached copy), on a thread.
fn start_download(local: PathBuf, id: String) -> Receiver<Result<(), String>> {
    let (tx, rx) = channel();
    let lg = lang();
    let spawned = std::thread::Builder::new().name("lacking-download".into()).spawn(move || {
        let hub = Hub { source: Source::from_env(), local };
        let result = match hub.cached_catalog() {
            Some(catalog) => hub.install(&catalog, &id, lg).map(|_| ()),
            None => Err(tr("The mod catalog is not loaded.").into()),
        };
        let _ = tx.send(result);
    });
    // A thread that could not start drops its sender: the window says the download stopped.
    let _ = spawned;
    rx
}

impl App {
    /// A save was refused (`SaveError::Lacking`): its mod is the one it records, else the one the
    /// installed mods are checked for on a worker (`check_installed`).
    pub(super) fn lacking_start(&mut self, path: PathBuf, name: String, recorded: Option<SaveMod>, missing: Unknown) {
        let mut l = Lacking::new(path);
        l.name = name;
        l.ids = missing.text();
        match recorded {
            Some(m) => l.need = Some(Need::Recorded(m)),
            None => match (razdor::hub::local_dir(), razdor::dt::install::base_dir()) {
                (Some(local), Some(base)) => {
                    let (tx, rx) = channel();
                    let spawned = std::thread::Builder::new().name("lacking-check".into()).spawn(move || {
                        let _ = tx.send(check_installed(&local, &base, &missing));
                    });
                    if spawned.is_ok() {
                        l.checking = Some(rx);
                    } else {
                        l.need = Some(Need::Unknown);
                    }
                }
                _ => l.need = Some(Need::Unknown),
            },
        }
        self.lacking = Some(l);
        self.lacking_refresh();
    }

    /// The offer, once the need is known: what is installed, and what the cached catalog offers.
    fn lacking_refresh(&mut self) {
        let Some(l) = self.lacking.as_mut() else { return };
        let Some(need) = l.need.clone() else { return };
        let (installed, catalog) = match razdor::hub::local_dir() {
            Some(local) => {
                let installed = hub::read_installed(&local);
                let catalog = Hub { source: Source::from_env(), local }.cached_catalog();
                (installed, catalog)
            }
            None => (Installed::default(), None),
        };
        let (is_installed, download) = match &need {
            Need::Recorded(m) => (
                installed.entries.get(&m.id).is_some_and(|e| e.kind == Kind::Mod),
                catalog.as_ref().and_then(|c| save_mod::download_size(c, &installed, &m.id, lang_code())),
            ),
            Need::Inferred(_) | Need::Unknown => (true, None),
        };
        let offer = save_mod::offer(&need, is_installed, download);
        let id = match &offer {
            Offer::Installed { id } | Offer::Download { id, .. } | Offer::NotInCatalog { id } => Some(id.clone()),
            Offer::Unknown => None,
        };
        l.title = match id {
            Some(id) => installed
                .entries
                .get(&id)
                .map(|e| e.title.get(lang()).to_string())
                .or_else(|| catalog.as_ref().and_then(|c| c.entry(&id)).map(|e| e.title.get(lang()).to_string()))
                .unwrap_or(id),
            None => String::new(),
        };
        l.offer = offer;
        l.refresh_message();
    }

    /// Each frame: the box's answers (the mod button), the workers' results, and the box's words for
    /// the frame that follows. A refused save whose error is gone (OK, or a load that worked) ends.
    pub(super) fn poll_lacking(&mut self) {
        let pressed = std::mem::take(&mut self.lacking_box.pressed);
        // The download's question (the box asks Yes or No): Yes starts the download, No drops it.
        if let Some(answer) = self.lacking_box.answer.take() {
            if let Some(l) = self.lacking.as_mut() {
                l.confirm = false;
                if answer {
                    if let Offer::Download { id, .. } = l.offer.clone() {
                        l.note = None;
                        l.download = razdor::hub::local_dir().map(|local| start_download(local, id));
                    }
                }
            }
        }
        if self.load_error.is_none() {
            self.lacking = None;
        }
        if pressed {
            self.lacking_press();
        }
        let checked = match self.lacking.as_ref().and_then(|l| l.checking.as_ref()) {
            Some(rx) => finished(rx),
            None => None,
        };
        if let Some(found) = checked {
            if let Some(l) = self.lacking.as_mut() {
                l.checking = None;
                l.need = Some(save_mod::need(None, &found.unwrap_or_default()));
            }
            self.lacking_refresh();
        }
        let downloaded = match self.lacking.as_ref().and_then(|l| l.download.as_ref()) {
            Some(rx) => finished(rx),
            None => None,
        };
        if let Some(done) = downloaded {
            let id = self.lacking.as_ref().and_then(|l| match &l.offer {
                Offer::Download { id, .. } => Some(id.clone()),
                _ => None,
            });
            let result = done.unwrap_or_else(|()| Err(tr("The download stopped unexpectedly.").into()));
            match (result, id) {
                (Ok(()), Some(id)) => {
                    if let Some(l) = self.lacking.as_mut() {
                        l.download = None;
                        l.switching = true;
                    }
                    // The mod is installed: its switch is asked for, and the save loads when it lands.
                    self.choose_mod(Some(id));
                }
                (Err(e), _) => {
                    if let Some(l) = self.lacking.as_mut() {
                        l.download = None;
                        l.note = Some(trf!("The mod could not be downloaded: {e}.", e));
                    }
                }
                (Ok(()), None) => {
                    if let Some(l) = self.lacking.as_mut() {
                        l.download = None;
                    }
                }
            }
        }
        self.lacking_box = self.lacking_box_now();
    }

    /// The mod button was pressed: an installed mod is switched to, a hub mod is asked for first.
    fn lacking_press(&mut self) {
        let Some(l) = self.lacking.as_mut() else { return };
        match l.offer.clone() {
            Offer::Installed { id } => {
                l.switching = true;
                l.note = None;
                self.choose_mod(Some(id));
            }
            Offer::Download { .. } => {
                l.confirm = true;
                l.note = None;
            }
            Offer::NotInCatalog { .. } | Offer::Unknown => {}
        }
    }

    /// The mod switch landed (`poll_mod_switch`): the save loads when the mod is the one now played;
    /// otherwise the switch's message is the window's line under the button.
    pub(super) fn lacking_switched(&mut self) {
        if self.mod_switch.is_some() {
            return;
        }
        let Some(l) = self.lacking.as_mut() else { return };
        if !l.switching {
            return;
        }
        l.switching = false;
        let id = match &l.offer {
            Offer::Installed { id } | Offer::Download { id, .. } => id.clone(),
            Offer::NotInCatalog { .. } | Offer::Unknown => return,
        };
        let path = l.path.clone();
        if self.active_mod.as_deref() == Some(id.as_str()) {
            self.load(&path);
        } else if let Some(l) = self.lacking.as_mut() {
            l.note = self.message.take();
        }
    }

    /// The box's words for this frame (`LackingBox`): the mod button, the line under it, and whether
    /// something is being done (the button and OK wait).
    fn lacking_box_now(&self) -> LackingBox {
        let Some(l) = self.lacking.as_ref() else { return LackingBox::default() };
        let busy = if l.checking.is_some() {
            Some(tr("Checking the installed mods…"))
        } else if l.download.is_some() {
            Some(tr("Downloading the mod…"))
        } else if l.switching && self.mod_switch.is_some() {
            Some(tr("Preparing the mod…"))
        } else {
            None
        };
        let (button, why) = match &l.offer {
            // Short labels: the message names the mod (`refresh_message`), the reason names a missing one.
            Offer::Installed { .. } => (Some((tr("Start with the mod").into(), true)), None),
            Offer::Download { .. } => (Some((tr("Download and start").into(), true)), None),
            Offer::NotInCatalog { id } => (Some((tr("Start with the mod").into(), false)), Some(trf!("The mod «{id}» is not in the catalog", id = id.clone()))),
            Offer::Unknown => (Some((tr("Start with the mod").into(), false)), Some(tr("Unknown which mod made this save").into())),
        };
        let reason = match busy {
            Some(text) => Some(text.to_string()),
            None => why.or_else(|| l.note.clone()),
        };
        let asking = if l.confirm { Some(self.download_question(l)) } else { None };
        LackingBox { active: true, button, message: l.message.clone(), reason, busy: busy.is_some() || l.need.is_none(), pressed: false, asking, answer: None }
    }

    /// The question of a download: its size in MB (the mod's download, `Offer::Download`).
    fn download_question(&self, l: &Lacking) -> String {
        let size = match &l.offer {
            Offer::Download { size, .. } => *size,
            _ => 0,
        };
        let mb = format!("{:.1}", size as f64 / 1_000_000.0);
        trf!("Download «{title}» ({mb} MB) and load the save with it?", title = l.title.clone(), mb = mb)
    }

    /// The scene's state of a refused save (the snapshot), as the App shows it.
    pub(super) fn lacking_scene(&mut self, l: Lacking) {
        self.lacking = Some(l);
        self.lacking_box = self.lacking_box_now();
    }

    /// The App's installed-mods record for a save (`save::set_played_mod`).
    pub(super) fn record_played_mod(&self) {
        let played = self.active_mod.as_ref().and_then(|id| {
            let local = razdor::hub::local_dir()?;
            let entry = hub::read_installed(&local).entries.get(id).cloned()?;
            Some(SaveMod { id: id.clone(), version: entry.version })
        });
        razdor::rules::save::set_played_mod(played);
    }
}
