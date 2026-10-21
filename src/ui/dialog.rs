//! Modal message windows in the original's style: a title bar, a parchment text box, a row
//! of resource icons and an OK button (video notes §5: the noon report, the victory window,
//! story and quest dialogs). A scenario question has Yes and No instead of OK.

use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::rules::battle::Team;
use razdor::trf;
use razdor::rules::content::{ItemId, UnitId};
use razdor::rules::game::{BattleResult, DayReport, Game};

use super::assets::Assets;
use super::chrome;
use super::widgets::*;

const MARBLE_EDGE: Color = Color::new(0.38, 0.58, 0.46, 1.0);
pub const MANA: Color = Color::new(0.55, 0.72, 1.0, 1.0);
/// The captions of the event window's icon rows and resources: the original's green font
/// ae249c, Benguiat less (100, 0, 100).
const CAPTION: Color = Color::new(155.0 / 255.0, 1.0, 155.0 / 255.0, 1.0);

/// A resource icon with its label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Gold,
    Mana,
    Income,
    Wages,
    Experience,
}

/// A picture over the text: a unit's portrait, or an event's own image.
#[derive(Clone, Debug)]
pub enum Picture {
    Unit(UnitId),
    Image(Texture2D),
    /// The original's event picture (the record's +0xa7, 0x4a9d88 / 0x4d1679): at its own
    /// size left of the text box, framed by a 1 px line; the box moves right by its width
    /// plus 14 and is at least as tall as it, the text centred in it (the village offers).
    Side(Texture2D),
    /// The defeat or victory still (`Stnd-1`, `Stnd-2`): in the place of [`Picture::Side`],
    /// drawn with its own alpha and no frame line (0x4d1686 → 0x476a3c).
    Still(Texture2D),
}

/// How the player closed a dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Close {
    Ok,
    Yes,
    No,
}

#[derive(Clone, Debug)]
pub struct Dialog {
    pub title: String,
    pub text: Vec<String>,
    /// The event's own text, read as the original's markup (0x4a9d88 → 0x48e438): drawn
    /// instead of [`Dialog::text`], line by line with its fonts, centred or justified.
    pub marked: Option<String>,
    /// Resource icons with a caption such as "Gold + 30".
    pub resources: Vec<(Resource, String)>,
    pub items: Vec<ItemId>,
    /// Spells the event teaches (shown as their pictures under «Выучены новые заклинания:»).
    pub spells: Vec<u32>,
    /// The item or spell a press on its icon shows while the button is held (0x4c1fec,
    /// 0x4c2048): (a spell, its id, where the press was).
    pub preview: std::cell::Cell<Option<(bool, u32, Vec2)>>,
    /// An extra line in blue (system notices).
    pub notice: Option<String>,
    /// The scenario event this dialog shows, if any.
    pub event: Option<u16>,
    /// The event's yes/no question: the dialog has Yes and No buttons.
    pub question: bool,
    /// The question is a village's offer (0x4aca80): its answer goes to the village
    /// (`building_view::answer_offer`), not to the event engine.
    pub offer: bool,
    /// The village offer's result window: its OK pays the witch's mana (0x4ab53b).
    pub offer_result: bool,
    pub picture: Option<Picture>,
    /// Units that joined / left the army.
    pub joined: Vec<UnitId>,
    pub left: Vec<UnitId>,
    /// Artifacts the event took from the player, as the window's lost row shows them (only
    /// the ones really held: `EventOutcome::ItemsLost`, 0x4a8fe5).
    pub lost: Vec<ItemId>,
    /// It is shown in the original's event window, which opens with a chord (a draw of the
    /// game's generator, 0x4d15d0): the events' dialogs, the victory box and the noon report.
    pub chord: bool,
    /// Its opening sound has played.
    pub cued: bool,
    /// Not shown before this clock time (seconds, `get_time`): the won battle's report comes
    /// 250 ms after the battle screen closes (the chained step 0x4af658).
    pub not_before: Option<f64>,
    /// Drawn in the original's defeat look (the event window's mode 2, 0x4a8ae8 / 0x4ed458):
    /// the red marble; its OK ends the game for the main menu (0x4c206c → 0x4bfb34).
    pub defeat: bool,
    /// A key that closes it went down while it was shown: its release presses OK (0x4cd558).
    /// A key already held as it opened does nothing (0x47f358).
    pub key_down: std::cell::Cell<bool>,
}

impl Dialog {
    pub fn new(title: impl Into<String>) -> Dialog {
        Dialog {
            title: title.into(),
            text: Vec::new(),
            marked: None,
            resources: Vec::new(),
            items: Vec::new(),
            spells: Vec::new(),
            preview: std::cell::Cell::new(None),
            notice: None,
            event: None,
            question: false,
            offer: false,
            offer_result: false,
            picture: None,
            joined: Vec::new(),
            left: Vec::new(),
            lost: Vec::new(),
            chord: false,
            cued: false,
            not_before: None,
            defeat: false,
            key_down: std::cell::Cell::new(false),
        }
    }

    /// It still waits for its time ([`Dialog::not_before`]) at clock `now`.
    pub fn waiting(&self, now: f64) -> bool {
        self.not_before.is_some_and(|t| now < t)
    }

    /// Adds a line to the blue notice.
    pub fn add_notice(&mut self, line: &str) {
        self.notice = Some(match self.notice.take() {
            Some(n) => format!("{n}   {line}"),
            None => line.to_string(),
        });
    }

    /// The 12:00 report, the event window's slot after the victory's (0x4a9d1e..0x4a9d74):
    /// titled `[Event] ResourceTitle` (0x4ab12c), the text `[Event] ResourceReport`
    /// (0x4ab0d4) in the plain tint (font ae24a0) and, when the gold and the income do not
    /// cover the wages, an empty row and `[Event] ResourceFail` (0x4ab0ec) in the orange one
    /// (ae24a4); then the gold, mana, income and wages, the ones that are 0 left out
    /// ([`report_resources`]).
    /// Razdor's own lines (the mana paid to elementals, who left) follow in the plain tint.
    pub fn day_report(game: &Game, r: &DayReport) -> Dialog {
        use razdor::i18n::n_;
        let ini = |key: &str, ours: &'static str| {
            super::chrome::ui_text("Event", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string())
        };
        let plain = |s: String| s.chars().filter(|c| !matches!(c, '*' | '|' | '@' | '^')).collect::<String>();
        let mut d = Dialog::new(ini("ResourceTitle", n_("Report on resources, income and expenses")));
        // The original shows it in the event window, which opens with its chord.
        d.chord = true;
        let mut text = format!("^{}", plain(ini("ResourceReport", n_("The report shows your gold, the daily income of your castles and the wages paid to your army."))));
        if r.gold + r.income < r.wages {
            text.push_str(&format!("\r\n\r\n^@{}", plain(ini("ResourceFail", n_("Your gold and income do not cover the wages.")))));
        }
        if r.mana_wages > 0 {
            text.push_str(&format!("\r\n^{}", trf!("Mana today: - {paid} paid to elementals.", paid = r.mana_wages)));
        }
        if !r.deserted.is_empty() {
            let names: Vec<&str> = r.deserted.iter().map(|&u| game.content.unit(u).name.as_str()).collect();
            text.push_str(&format!("\r\n^{}", trf!("Left the army unpaid: {names}.", names = names.join(", "))));
        }
        d.marked = Some(text);
        d.resources = report_resources(r.gold, r.mana_total, r.income, r.wages);
        if r.unpaid > 0 {
            d.add_notice(&trf!("{n} unpaid units refuse to fight until they are paid.", n = r.unpaid));
        }
        d
    }

    /// The report after the army was lost (events.md §11, 0x4c527d / 0x4906eb set the defeat
    /// mark 0xae6d23, the chained step 0x4af658 opens the battle-report slot): the event window
    /// in its defeat look, titled `[Event] DefeatTitle`, the `Stnd-1` picture at its side
    /// (0x674958, loaded at 0x4dc871; 0x4aa1ac), and two centred texts: `[Event] Defeat` in
    /// the plain tint (font ae24a0), an empty row, `[Event] DefeatInfo` in the orange one
    /// (ae24a4; 0x4aa76c). The texts are drawn as typed, their marks taken out. OK goes to the
    /// main menu (0x4c206c with the defeat mark → 0x4bfb34).
    pub fn defeat() -> Dialog {
        let ini = |key: &str, ours: &'static str| {
            super::chrome::ui_text("Event", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string())
        };
        let plain = |s: String| s.chars().filter(|c| !matches!(c, '*' | '|' | '@' | '^')).collect::<String>();
        let mut d = Dialog::new(ini("DefeatTitle", razdor::i18n::n_("Defeat in battle!")));
        let lost = plain(ini("Defeat", razdor::i18n::n_("Your enemy proved stronger than your army, and you lost this battle.")));
        let info = plain(ini("DefeatInfo", razdor::i18n::n_("You let your hero perish, and the game on this scenario cannot go on.")));
        d.marked = Some(format!("^{lost}\r\n\r\n^@{info}"));
        d.picture = super::chrome::win_ugs("Stnd-1").map(Picture::Still);
        d.chord = true;
        d.defeat = true;
        d
    }

    /// The window after a won battle: gold and mana taken, the loot.
    pub fn victory(result: &BattleResult) -> Option<Dialog> {
        let BattleResult::Victory { reward, mana, loot, left_behind, over, .. } = result else {
            return None;
        };
        let ini = |key: &str, ours: &'static str| {
            super::chrome::ui_text("Event", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string())
        };
        let mut d = Dialog::new(ini("VictoryTitle", razdor::i18n::n_("Victory over the enemy!")));
        d.chord = true;
        d.marked = Some(victory_markup(over, *reward, loot.len(), *mana, &ini));
        d.resources = victory_resources(*reward, *mana);
        d.items = loot.clone();
        if *left_behind > 0 {
            d.notice = Some(trf!("{left_behind} items were left behind: the pack is full.", left_behind));
        }
        Some(d)
    }
}

/// A resource's word in the event window's captions, from the install's `[Event]
/// Resource0..3` (Деньги, Магия, Доход, Выплата), as the original labels «Деньги + 160»;
/// Razdor's own word in English or without the install's text.
pub fn resource_word(r: Resource) -> String {
    let (key, ours) = match r {
        Resource::Gold => ("Resource0", razdor::i18n::n_("Gold")),
        Resource::Mana => ("Resource1", razdor::i18n::n_("Mana")),
        Resource::Income => ("Resource2", razdor::i18n::n_("Income")),
        Resource::Wages => ("Resource3", razdor::i18n::n_("Wages")),
        Resource::Experience => return tr("Experience").to_string(),
    };
    super::chrome::ui_text("Event", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string())
}

/// The victory report's resource row (0x4a9ce1..0x4a9d19): the gold won in the first place,
/// the mana in the second, the income and wages places empty; the row then shows only the
/// places that are not 0 (0x4a9db6, 0x4a9deb), and none at all when every place is 0. So a
/// battle that brought nothing has no «Деньги + 0», and a captured building no income.
pub fn victory_resources(gold: i32, mana: i32) -> Vec<(Resource, String)> {
    let mut rows = Vec::new();
    if gold != 0 {
        rows.push((Resource::Gold, format!("{} + {gold}", resource_word(Resource::Gold))));
    }
    if mana != 0 {
        rows.push((Resource::Mana, format!("{} + {mana}", resource_word(Resource::Mana))));
    }
    rows
}

/// The noon report's resource row (0x4a9d29..0x4a9d74): the gold, the mana, the income and
/// the wages negated in its four places; only the places that are not 0 are shown
/// (0x4a9deb), the gold and the mana with " = " (0x4aaf7c), the income with " + "
/// (0x4aaf70), the wages with " - " (0x4aaf64) and their value without the sign.
pub fn report_resources(gold: i32, mana: i32, income: i32, wages: i32) -> Vec<(Resource, String)> {
    [(Resource::Gold, gold, "="), (Resource::Mana, mana, "="), (Resource::Income, income, "+"), (Resource::Wages, wages, "-")]
        .into_iter()
        .filter(|&(_, v, _)| v != 0)
        .map(|(r, v, sign)| (r, format!("{} {sign} {v}", resource_word(r))))
        .collect()
}

/// The victory report's text as the original builds it (0x4a9d88), each line centred: a
/// beaten garrison's line (`VictoryBuilding` / `VictoryRuines` with `#NAME2` its owner, the
/// `NoName` ones without an owner) or `Victory` with `#NAME1` the army's name
/// (`NoNameArmy` when it has none), in white (font 0xae2498, `*`); then, each after an empty
/// row (`#\`), `VictoryEmpty` when nothing was won, else `VictoryGold`, `VictoryItems`
/// (0xae24a0) and `VictoryMana` (0xae24a8, `|`).
pub fn victory_markup(over: &razdor::rules::game::Beat, gold: i32, items: usize, mana: i32, ini: &dyn Fn(&str, &'static str) -> String) -> String {
    use razdor::i18n::n_;
    use razdor::rules::game::Beat;
    let plain = |s: String| s.chars().filter(|c| !matches!(c, '*' | '|' | '@' | '^')).collect::<String>();
    let head = match over {
        Beat::Place { ruins: true, owner } if !owner.trim().is_empty() => ini("VictoryRuines", n_("You have plundered the ruins where #NAME2 ruled.")).replace("#NAME2", owner),
        Beat::Place { ruins: true, .. } => ini("VictoryNoNameRuines", n_("You have plundered abandoned ruins. Now people can pass these grim walls without fear!")),
        Beat::Place { owner, .. } if !owner.trim().is_empty() => ini("VictoryBuilding", n_("You have taken the fortress that belonged to #NAME2.")).replace("#NAME2", owner),
        Beat::Place { .. } => ini("VictoryNoNameBuilding", n_("You have taken a fortress that had no owner. Now you will put it in order!")),
        Beat::Army(name) => {
            let name = if name.trim().is_empty() { ini("NoNameArmy", n_("Unknown army")) } else { name.clone() };
            ini("Victory", n_("The battle is over, and your enemy, #NAME1, is beaten!")).replace("#NAME1", &name)
        }
        Beat::Nobody => ini("Victory", n_("The battle is over, and your enemy, #NAME1, is beaten!")).replace("#NAME1", &ini("NoNameArmy", n_("Unknown army"))),
    };
    let mut text = format!("^*{}", plain(head));
    let mut add = |mark: &str, line: String| text.push_str(&format!("\r\n\r\n^{mark}{}", plain(line)));
    if gold <= 0 && items == 0 && mana <= 0 {
        add("", ini("VictoryEmpty", n_("This battle brought you nothing but new battle experience.")));
    }
    if gold > 0 {
        add("", ini("VictoryGold", n_("In this battle you won gold from the enemy.")));
    }
    if items > 0 {
        add("", ini("VictoryItems", n_("As trophies you get the items that belonged to your enemy.")));
    }
    if mana > 0 {
        add("|", ini("VictoryMana", n_("Grateful for your mercy, the surrendered troops pray for you.")));
    }
    text
}

/// Small drawn icons for the resources (our own shapes).
pub fn resource_icon(r: Resource, cx: f32, cy: f32, s: f32) {
    // The original's pictures: coins, the magic book, the house, the paid knight.
    let art = match r {
        Resource::Gold => Some(0),
        Resource::Mana => Some(1),
        Resource::Income => Some(2),
        Resource::Wages => Some(3),
        Resource::Experience => None,
    };
    if let Some(n) = art {
        let name = if s <= 48.0 { format!("res{n}-44") } else { format!("res{n}") };
        if let Some(t) = super::chrome::win(&name) {
            super::chrome::tex(&t, Rect::new(cx - s / 2.0, cy - s / 2.0, s, s), WHITE);
            return;
        }
    }
    match r {
        Resource::Gold => {
            for k in 0..3 {
                let y = cy + s * 0.25 - k as f32 * s * 0.18;
                draw_ellipse(cx, y, s * 0.36, s * 0.14, 0.0, Color::new(0.85, 0.66, 0.2, 1.0));
                draw_ellipse_lines(cx, y, s * 0.36, s * 0.14, 0.0, 1.5, Color::new(0.5, 0.35, 0.1, 1.0));
            }
        }
        Resource::Mana => {
            let (t, b) = (vec2(cx, cy - s * 0.45), vec2(cx, cy + s * 0.45));
            let (l, r) = (vec2(cx - s * 0.28, cy), vec2(cx + s * 0.28, cy));
            draw_triangle(t, l, r, MANA);
            draw_triangle(b, l, r, Color::new(0.3, 0.5, 0.9, 1.0));
        }
        Resource::Income => {
            draw_rectangle(cx - s * 0.3, cy - s * 0.05, s * 0.6, s * 0.4, Color::new(0.8, 0.7, 0.55, 1.0));
            draw_triangle(vec2(cx, cy - s * 0.42), vec2(cx - s * 0.4, cy - s * 0.05), vec2(cx + s * 0.4, cy - s * 0.05), Color::new(0.75, 0.3, 0.2, 1.0));
        }
        Resource::Wages => {
            draw_circle(cx, cy - s * 0.2, s * 0.16, Color::new(0.75, 0.75, 0.8, 1.0));
            draw_rectangle(cx - s * 0.22, cy - s * 0.04, s * 0.44, s * 0.46, Color::new(0.6, 0.62, 0.7, 1.0));
        }
        Resource::Experience => {
            let star = Color::new(0.95, 0.85, 0.35, 1.0);
            let (r, k) = (s * 0.42, s * 0.18);
            for i in 0..5 {
                let a = std::f32::consts::TAU * i as f32 / 5.0 - std::f32::consts::FRAC_PI_2;
                let (b, c) = (a - 0.63, a + 0.63);
                draw_triangle(
                    vec2(cx + r * a.cos(), cy + r * a.sin()),
                    vec2(cx + k * b.cos(), cy + k * b.sin()),
                    vec2(cx + k * c.cos(), cy + k * c.sin()),
                    star,
                );
            }
            draw_circle(cx, cy, k, star);
        }
    }
}

/// The units rows (characters joined, characters departed: 0x4a93bc, 0x4a9711): the
/// original's picture cells of 47 × 47 (set up at 0x4c612c's event window), each showing the
/// unit type's 46 × 46 icon (type record +0x18, the 92 × 92 bust box-averaged to half by
/// 0x48efd0 / 0x48d830), with no ring or frame. A row grows the window by its icons, 8, the
/// 16 of its caption and 8 more (0x4a9650: 0x2f + 8 + 0x10 + 8).
const UNIT_ICON: f32 = 46.0;
const UNIT_ROW: f32 = 79.0;

/// The left edges, from the window's left in the original's pixels, of a units row's `n`
/// cells: centred at W·(i+1)/(n+1) in whole pixels, less 23 (0x4a95a9 and 0x4a9a84: `IMUL`,
/// `IDIV`, `SUB 0x17`). Each event names at most four units to add or take (+0x61, +0x69).
fn unit_cells(n: usize) -> Vec<f32> {
    let n = n.min(4) as i32;
    (0..n).map(|i| ((EVENT_W as i32 * (i + 1)) / (n + 1) - 23) as f32).collect()
}

/// The event window's frame in the original's pixels (0x4a8ae8), before the interface scale:
/// 634 wide; the text box at (19 + side, 46), `side` being a side picture's width and 14, as
/// wide as the rest of 634 − 38; the side picture at (20, 47); the box at least 92 tall or
/// as tall as the picture, the text centred in it with 16 above and below; 119 more than
/// the box (and Razdor's rows under it) in all, the buttons 45 from the bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
struct EventFrame {
    w: f32,
    h: f32,
    box_x: f32,
    box_w: f32,
    box_h: f32,
}

const EVENT_W: f32 = 634.0;
const EVENT_BOX_TOP: f32 = 46.0;
const EVENT_PAD: f32 = 16.0;
const EVENT_MIN_BOX: f32 = 92.0;
/// The box's tallest before the text scrolls (0x4aa88a: 499).
const EVENT_MAX_BOX: f32 = 499.0;
/// The window's height besides the box and the rows under it (0x4aa929: + 0x77).
const EVENT_REST: f32 = 119.0;
/// Rows of the original's event text (Benguiat), and the defeat report's gap row (0x47e46c
/// with font ae24bc).
const EVENT_LINE: f32 = 19.0;
const EVENT_GAP: f32 = 12.0;
/// From a row's top to its baseline.
const EVENT_BASELINE: f32 = 13.0;

fn event_frame(side: Option<(f32, f32)>, text_h: f32, below: f32, max_box: f32) -> EventFrame {
    let side_w = side.map_or(0.0, |(pw, _)| pw + 14.0);
    let min_box = side.map_or(EVENT_MIN_BOX, |(_, ph)| ph.max(EVENT_MIN_BOX));
    let box_h = (text_h + 2.0 * EVENT_PAD).max(min_box).min(max_box.max(min_box));
    EventFrame { w: EVENT_W, h: box_h + below + EVENT_REST, box_x: 19.0 + side_w, box_w: EVENT_W - 38.0 - side_w, box_h }
}

/// Where the window stands: centred over the map view above the bottom bar (the original's
/// 1024×682: 0x4aa9ff), in screen pixels for a window `w` × `h` of them.
fn event_origin(sw: f32, sh: f32, bar: f32, w: f32, h: f32) -> (f32, f32) {
    (((sw - w) / 2.0).floor(), ((sh - bar - h) / 2.0).floor().max(2.0))
}

/// Draws `d` over the map; returns how it was closed: OK (or Enter, Escape), or for a
/// question Yes or No (Esc or N No, any other key Yes: `answer_key`). Laid out in the
/// original's pixels ([`event_frame`]) times the interface scale, its text in the original's
/// Benguiat at its own size ([`super::dt_font::sheet_size`]).
pub fn draw(d: &Dialog, assets: &Assets, game: Option<&Game>) -> Option<Close> {
    let (sw, sh) = (screen_width(), screen_height());
    // The original's pixel on screen, snapped as the font sheets snap (×1.875 → ×2), so the
    // text and its box grow together and the lines break where the original's do.
    let o = super::dt_font::whole_scale(chrome::k() * 0.9375).min((sw - 12.0) / EVENT_W);
    chrome::under_message();
    let size = super::dt_font::sheet_size(super::dt_font::Face::Title, o);
    let line = EVENT_LINE * o;
    // A picture at the side takes its width and 14 more from the text box.
    let side = match &d.picture {
        Some(Picture::Side(t) | Picture::Still(t)) => Some((t.width(), t.height())),
        _ => None,
    };
    let side_w = side.map_or(0.0, |(pw, _)| pw + 14.0);
    let room = (EVENT_W - 38.0 - side_w - 40.0) * o;
    let (lines, rows): (Vec<String>, Vec<MarkupRow>) = match &d.marked {
        Some(t) => (Vec::new(), markup_rows(t, room, size)),
        None => (d.text.iter().flat_map(|t| wrap(t, room, size)).collect(), Vec::new()),
    };
    // The defeat report's empty row is the gap font's (0x47e46c), shorter than a line.
    let row_h = |r: &MarkupRow| if d.defeat && r.text.is_empty() { EVENT_GAP * o } else { line };
    let total = lines.len() + rows.len();
    let full_h = lines.len() as f32 * line + rows.iter().map(row_h).sum::<f32>();
    let res_h = if d.resources.is_empty() { 0.0 } else { 104.0 * o };
    // An icon row grows the window by its caption and icons (0x4a8e66: 87 for items, 82
    // for spells).
    let items_h = if d.items.is_empty() { 0.0 } else { 87.0 * o };
    // The lost row is laid out as the gained one (0x4a8fe5: the same 0x37 / 0x18 / 8 steps).
    let lost_h = if d.lost.is_empty() { 0.0 } else { 87.0 * o };
    let spells_h = if d.spells.is_empty() { 0.0 } else { 82.0 * o };
    // The quest line's row (0x4a8dbf: 16 above the line, 8 under it), measured against the
    // original's opening event as 30 of Razdor's layout.
    let notice_h = if d.notice.is_some() { 30.0 * o } else { 0.0 };
    let pic_h = if d.picture.is_some() && side.is_none() { 140.0 * o } else { 0.0 };
    let units_h = [&d.joined, &d.left].iter().filter(|u| !u.is_empty()).count() as f32 * UNIT_ROW * o;
    let below = pic_h + res_h + items_h + lost_h + spells_h + units_h + notice_h;
    // The box stops at 499 (and short of the screen), the buttons in sight.
    let screen_box = (sh - chrome::bar_height() - 4.0) / o - below / o - EVENT_REST;
    let max_box = EVENT_MAX_BOX.min(screen_box);
    let f = event_frame(side, full_h / o, below / o, max_box);
    let shown = if full_h / o + 2.0 * EVENT_PAD <= f.box_h { total } else { (((f.box_h - 2.0 * EVENT_PAD) * o / line).floor().max(1.0) as usize).min(total) };
    let (w, h) = (f.w * o, f.h * o);
    let (x, y) = event_origin(sw, sh, chrome::bar_height(), w, h);
    // The defeat look is the red marble (the original's window after a lost battle).
    let skin = if d.defeat { chrome::Skin::Red } else { chrome::Skin::Marble };
    chrome::window(Rect::new(x, y, w, h), &d.title, skin, false);
    let edge = o.max(1.0);
    let title_h = (26.0 * chrome::k()).round();
    chrome::silver_frame(Rect::new(x + 3.0 * o, y + title_h + 3.0 * o, w - 6.0 * o, h - title_h - 64.0 * o), edge);
    chrome::silver_frame(Rect::new(x + 2.0 * o, y + h - 58.0 * o, w - 4.0 * o, 56.0 * o), edge);
    let top = y + EVENT_BOX_TOP * o;
    let mut cy = top;
    match &d.picture {
        Some(Picture::Unit(u)) => {
            let r = Rect::new(x + w / 2.0 - 64.0 * o, cy, 128.0 * o, 128.0 * o);
            assets.draw_portrait(*u, Team::Player, r);
            chrome::silver_frame(r, 1.5 * o);
        }
        Some(Picture::Image(tex)) => {
            let k = (128.0 * o / tex.height()).min((w - 60.0 * o) / tex.width());
            let (tw, th) = (tex.width() * k, tex.height() * k);
            draw_texture_ex(tex, x + (w - tw) / 2.0, cy, WHITE, DrawTextureParams { dest_size: Some(vec2(tw, th)), ..Default::default() });
            draw_rectangle_lines(x + (w - tw) / 2.0, cy, tw, th, 2.0 * o, MARBLE_EDGE);
        }
        Some(Picture::Still(tex)) => chrome::tex(tex, Rect::new(x + 20.0 * o, top + o, tex.width() * o, tex.height() * o), WHITE),
        Some(Picture::Side(tex)) => {
            let r = Rect::new(x + 20.0 * o, top + o, tex.width() * o, tex.height() * o);
            chrome::tex(tex, r, WHITE);
            draw_rectangle_lines(r.x - 1.0, r.y - 1.0, r.w + 2.0, r.h + 2.0, 1.0, WHITE);
        }
        None => {}
    }
    cy += pic_h;
    let tb = Rect::new(x + f.box_x * o, cy, f.box_w * o, f.box_h * o);
    chrome::text_box(tb);
    // No scrolling on this branch: a text taller than the box shows its first rows.
    let first = 0;
    let shown_h = lines.iter().skip(first).take(shown).map(|_| line).sum::<f32>() + rows.iter().skip(first).take(shown).map(row_h).sum::<f32>();
    let mut ty = tb.y + ((tb.h - shown_h) / 2.0).max(EVENT_PAD * o).floor() + EVENT_BASELINE * o;
    let text_x = tb.x + 20.0 * o;
    for l in lines.iter().skip(first).take(shown) {
        super::dt_font::with_face(super::dt_font::Face::Title, || chrome::shadow_centered(l, tb.x + tb.w / 2.0, ty, size, Color::new(1.0, 0.9, 0.66, 1.0)));
        ty += line;
    }
    for row in rows.iter().skip(first).take(shown) {
        draw_markup_row(row, text_x, ty, room, size);
        ty += row_h(row);
    }
    cy += tb.h + 10.0 * o;
    if !d.resources.is_empty() {
        let n = d.resources.len() as f32;
        let step = (w - 60.0 * o) / n;
        for (k, (r, label)) in d.resources.iter().enumerate() {
            let cx = x + 30.0 * o + step * (k as f32 + 0.5);
            // Every resource label in the caption green ae249c (0x4d6114 sets it on all four
            // places), the mana's too.
            chrome::shadow_centered(label, cx, cy + 18.0 * o, size, CAPTION);
            resource_icon(*r, cx, cy + 62.0 * o, 70.0 * o);
        }
        cy += res_h;
    }
    // The original stacks its rows from the bottom up (0x4a8ae8): the items gained lowest,
    // the units above them, the spells above those; each row its green caption centred over
    // the window and its icons spread by their count, centre i at W·(i+1)/(n+1).
    let spread = |n: usize, i: usize| x + w * (i + 1) as f32 / (n + 1) as f32;
    let caption = |key: &str, ours: &'static str, cy: f32| {
        let t = chrome::ui_text("Event", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string());
        chrome::shadow_centered(&t, x + w / 2.0, cy + 18.0 * o, size, CAPTION);
    };
    let mut icons: Vec<(bool, u32, Rect)> = Vec::new();
    if !d.spells.is_empty() {
        caption("NewSpell", razdor::i18n::n_("New spells learnt:"), cy);
        let n = d.spells.len().min(4);
        for (i, &s) in d.spells.iter().enumerate().take(4) {
            let r = Rect::new(spread(n, i) - 25.0 * o, cy + 26.0 * o, 50.0 * o, 50.0 * o);
            if let Some(def) = game.and_then(|g| g.content.spells.iter().find(|x| x.id == s)) {
                chrome::spell_icon(&def.icons, r);
                chrome::silver_frame(r, edge);
            }
            icons.push((true, s, r));
        }
        cy += spells_h;
    }
    // The units rows: the departed above the joined (0x4a8ae8 stacks them from the bottom
    // up), each its green caption centred (`[Event] DelUnit` / `AddUnit`) over its icons.
    for (units, key, ours) in [(&d.left, "DelUnit", razdor::i18n::n_("Characters departed:")), (&d.joined, "AddUnit", razdor::i18n::n_("Characters joined:"))] {
        if units.is_empty() {
            continue;
        }
        caption(key, ours, cy);
        for (&u, left) in units.iter().zip(unit_cells(units.len())) {
            assets.draw_portrait(u, Team::Player, Rect::new(x + left * o, cy + 26.0 * o, UNIT_ICON * o, UNIT_ICON * o));
        }
        cy += UNIT_ROW * o;
    }
    // The lost row sits above the gained one (the original stacks its rows from the bottom
    // up: 0x4a8fe5 builds the gained row first, then the lost one, the units above them).
    if !d.lost.is_empty() {
        caption("DelItem", razdor::i18n::n_("Items lost:"), cy);
        // Up to four slots name an artifact (117–120), each 55 × 55 at W·(i+1)/(n+1) − 27
        // (0x4a8fe5, 0x4d5884's 0x37 picture cells).
        let n = d.lost.len().min(4);
        for (i, &item) in d.lost.iter().enumerate().take(4) {
            let r = Rect::new(spread(n, i) - 27.0 * o, cy + 26.0 * o, 55.0 * o, 55.0 * o);
            assets.draw_item(item, r.x, r.y, r.w);
            icons.push((false, item.0, r));
        }
        cy += lost_h;
    }
    if !d.items.is_empty() {
        caption("AddItem", razdor::i18n::n_("Items received:"), cy);
        let n = d.items.len().min(8);
        for (i, &item) in d.items.iter().enumerate().take(8) {
            let r = Rect::new(spread(n, i) - 27.0 * o, cy + 26.0 * o, 55.0 * o, 55.0 * o);
            assets.draw_item(item, r.x, r.y, r.w);
            icons.push((false, item.0, r));
        }
        cy += items_h;
    }
    if let Some(n) = &d.notice {
        // In the event text's own face and size, the original's cyan (90, 231, 255).
        super::dt_font::with_face(super::dt_font::Face::Title, || chrome::shadow_centered(n, x + w / 2.0, cy + 18.0 * o, size, Color::new(0.35, 0.9, 1.0, 1.0)));
    }
    // The buttons, 95 × 36, 45 from the window's bottom.
    let (bw, bh) = (95.0 * o, 36.0 * o);
    let by = y + h - 45.0 * o;
    // A left or right press on an icon shows its item or spell centred on the press, kept
    // inside the window, until the button is let go (0x4c1fec / 0x4c2048, the frame handler
    // 0x4cd558); the window's buttons wait meanwhile.
    let held = is_mouse_button_down(MouseButton::Left) || is_mouse_button_down(MouseButton::Right);
    if d.preview.get().is_some() && !held {
        d.preview.set(None);
    } else if d.preview.get().is_none() && (is_mouse_button_pressed(MouseButton::Left) || is_mouse_button_pressed(MouseButton::Right)) {
        let p: Vec2 = pointer().into();
        if let Some(&(spell, id, _)) = icons.iter().find(|(_, _, r)| r.contains(p)) {
            d.preview.set(Some((spell, id, p)));
        }
    }
    if let (Some((spell, id, at)), Some(game)) = (d.preview.get(), game) {
        // The item tooltip is 272 wide and as tall as its text plus 32 (0x4cbab0).
        let (pw, ph) = if spell { (286.0, 112.0) } else { (super::item_text::TOOLTIP_W * o, super::item_text::tooltip_height(game, ItemId(id), o)) };
        let px = (at.x - pw / 2.0).clamp(x + 10.0, (x + w - 10.0 - pw).max(x + 10.0));
        let py = (at.y - ph / 2.0).clamp(y + 10.0, (y + h - 10.0 - ph).max(y + 10.0));
        draw_rectangle(px + 8.0, py + 8.0, pw, ph, Color::new(0.0, 0.0, 0.0, 0.45));
        if spell {
            if let Some(s) = game.content.spells.iter().find(|s| s.id == id) {
                super::spellbook::spell_card(game, s, Rect::new(px, py, pw, ph), true);
            }
        } else {
            super::item_text::draw(game, assets, ItemId(id), Rect::new(px, py, pw, ph), super::item_text::ItemBox::Tooltip, o);
        }
        return None;
    }
    if d.question {
        let answer = answer_key();
        let yes = button(x + w / 2.0 - bw - 10.0 * o, by, bw, bh, tr("Yes"), true) || answer == Some(true);
        let no = button(x + w / 2.0 + 10.0 * o, by, bw, bh, tr("No"), true) || answer == Some(false);
        return if yes {
            Some(Close::Yes)
        } else if no {
            Some(Close::No)
        } else {
            None
        };
    }
    // The label is the language file's `[Buttons] Ok` («Ок»); in the defeat look the button
    // is the red one (0x4aa263).
    let ok_label = chrome::ui_text("Buttons", "Ok").filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| "OK".to_string());
    let ok = if d.defeat {
        let r = Rect::new(x + (w - bw) / 2.0, by, bw, bh);
        let hover = mouse_in(r.x, r.y, r.w, r.h);
        chrome::defeat_button(r, &ok_label, true, hover);
        let pressed = hover && clicked();
        if pressed {
            super::audio::cue(super::audio::Cue::Button);
        }
        pressed
    } else {
        button(x + (w - bw) / 2.0, by, bw, bh, &ok_label, true)
    };
    // Any key but Tab, Alt and the Up and Down arrows, on its release (0x4cd558): Space,
    // Enter and Esc alike. Closed on the release, the key is no longer held when the map
    // takes over, so the hero walks on.
    if super::widgets::ok_key_pressed() {
        d.key_down.set(true);
    }
    (ok || (d.key_down.get() && super::widgets::ok_key_released())).then_some(Close::Ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defeat report as the original lays it out at 1024×768 (captured under Wine): 634 ×
    /// 247 at (195, 217), centred over the 1024×682 map view; Stnd-1 (128²) at the side, the
    /// text box 454 wide from x + 161 and as tall as the picture, two lines, a gap row, two
    /// lines (4 × 19 + 12 = 88, centred in it).
    #[test]
    fn the_event_window_stands_as_the_original_s() {
        let f = event_frame(Some((128.0, 128.0)), 88.0, 0.0, EVENT_MAX_BOX);
        assert_eq!(f, EventFrame { w: 634.0, h: 247.0, box_x: 161.0, box_w: 454.0, box_h: 128.0 });
        assert_eq!(event_origin(1024.0, 768.0, 86.0, f.w, f.h), (195.0, 217.0));
        assert_eq!(event_frame(None, 19.0, 0.0, EVENT_MAX_BOX).box_h, 92.0);
        assert_eq!(event_frame(None, 190.0, 0.0, EVENT_MAX_BOX), EventFrame { w: 634.0, h: 341.0, box_x: 19.0, box_w: 596.0, box_h: 222.0 });
        assert_eq!(event_frame(None, 900.0, 0.0, EVENT_MAX_BOX).box_h, 499.0);
        assert_eq!(event_frame(None, 900.0, 0.0, 300.0).box_h, 300.0);
    }

    /// 0x4a9deb: a victory shows only the resources it brought, none when it brought none.
    #[test]
    fn a_victory_shows_only_the_resources_it_brought() {
        assert!(victory_resources(0, 0).is_empty(), "no «Деньги + 0»");
        let kinds = |v: Vec<(Resource, String)>| v.into_iter().map(|(r, _)| r).collect::<Vec<_>>();
        assert_eq!(kinds(victory_resources(160, 0)), vec![Resource::Gold]);
        assert_eq!(kinds(victory_resources(0, 25)), vec![Resource::Mana]);
        assert_eq!(kinds(victory_resources(160, 25)), vec![Resource::Gold, Resource::Mana]);
    }

    /// 0x4a9d29..0x4a9d74, 0x4a9deb: the noon report's places, signs, and no zero place.
    #[test]
    fn the_noon_report_shows_its_signs_and_leaves_out_what_is_zero() {
        let report = report_resources(19260, 2000, 500, 595);
        let labels: Vec<&str> = report.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(labels.len(), 4);
        assert!(labels[0].ends_with(" = 19260") && labels[1].ends_with(" = 2000"), "{labels:?}");
        assert!(labels[2].ends_with(" + 500") && labels[3].ends_with(" - 595"), "{labels:?}");
        let kinds: Vec<Resource> = report_resources(100, 0, 0, 40).into_iter().map(|(r, _)| r).collect();
        assert_eq!(kinds, vec![Resource::Gold, Resource::Wages]);
        assert!(report_resources(0, 0, 0, 0).is_empty());
    }

    /// The won battle's report (0x4a9d88): the beaten one's line in white, then each prize's
    /// line after an empty row, mana in the blue tint; "nothing" only when nothing was won.
    #[test]
    fn the_victory_report_is_built_as_the_original_s() {
        use razdor::rules::game::Beat;
        let ini = |key: &str, _: &'static str| format!("<{key}{}>", if key == "Victory" { " #NAME1" } else if key.ends_with("Building") || key.ends_with("Ruines") { " #NAME2" } else { "" });
        let army = victory_markup(&Beat::Army("Мостовые братья".into()), 160, 2, 0, &ini);
        assert_eq!(army, "^*<Victory Мостовые братья>\r\n\r\n^<VictoryGold>\r\n\r\n^<VictoryItems>");
        let nameless = victory_markup(&Beat::Army(String::new()), 0, 0, 0, &ini);
        assert_eq!(nameless, "^*<Victory <NoNameArmy>>\r\n\r\n^<VictoryEmpty>");
        let ruins = victory_markup(&Beat::Place { ruins: true, owner: "Колдун".into() }, 0, 0, 5, &ini);
        assert_eq!(ruins, "^*<VictoryRuines Колдун>\r\n\r\n^|<VictoryMana>");
        let fort = victory_markup(&Beat::Place { ruins: false, owner: " ".into() }, 50, 0, 0, &ini);
        assert!(fort.starts_with("^*<VictoryNoNameBuilding #NAME2>"), "{fort}");
    }

    /// The lost battle's report (0x4aa76c): the defeat look, no event behind it (so its OK
    /// ends the game), the defeat text centred in the plain tint, an empty row, the info
    /// centred in the orange one.
    #[test]
    fn the_defeat_report_is_the_event_window_in_its_defeat_look() {
        use razdor::dt::markup::{parse, Align, Ink};
        let d = Dialog::defeat();
        assert!(d.defeat && d.event.is_none() && !d.question && d.chord);
        let lines = parse(d.marked.as_deref().unwrap());
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0].ink, lines[0].align), (Ink::Plain, Align::Centre));
        assert!(lines[1].text.trim_matches('_').is_empty());
        assert_eq!((lines[2].ink, lines[2].align), (Ink::At, Align::Centre));
    }

    /// The won battle's report waits its 250 ms after the screen closes (0x4af658), then shows.
    /// The units rows (0x4a93bc, 0x4a9711): 46 × 46 icons in 47 × 47 cells centred at
    /// W·(i+1)/(n+1) less 23, in whole pixels; a row is 79 tall.
    #[test]
    fn the_units_rows_stand_as_the_original_s() {
        assert_eq!(unit_cells(1), vec![294.0]);
        assert_eq!(unit_cells(2), vec![188.0, 399.0]);
        assert_eq!(unit_cells(3), vec![135.0, 294.0, 452.0]);
        assert_eq!(unit_cells(4), vec![103.0, 230.0, 357.0, 484.0]);
        assert_eq!(unit_cells(6).len(), 4);
        assert_eq!((UNIT_ICON, UNIT_ROW), (46.0, 79.0));
    }

    #[test]
    fn a_dialog_with_a_time_waits_for_it() {
        let gap = razdor::av::BATTLE_REPORT_GAP_MS as f64 / 1000.0;
        assert_eq!(gap, 0.25);
        let d = Dialog { not_before: Some(10.0 + gap), ..Dialog::new("report") };
        assert!(d.waiting(10.0));
        assert!(d.waiting(10.2));
        assert!(!d.waiting(10.25));
        assert!(!Dialog::new("now").waiting(0.0));
    }
}
