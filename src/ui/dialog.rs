//! Modal message windows in the original's style: a title bar, a parchment text box, a row
//! of resource icons and an OK button (video notes §5: the noon report, the victory window,
//! story and quest dialogs). A scenario question has Yes and No instead of OK.

use crate::ui::input::{is_mouse_button_down, is_mouse_button_pressed};
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

/// The noon report's resource labels. Deviation (the owner's decision, 2026-10-19, a
/// player's request): the gold in yellow, the magic in blue, the income in green and the wages
/// in red, all four of about the same brightness; the original draws all four in the green
/// ae249c ([`CAPTION`]).
fn report_label_ink(r: Resource) -> Color {
    match r {
        Resource::Gold => Color::from_rgba(250, 222, 90, 255),
        Resource::Mana => Color::from_rgba(150, 215, 255, 255),
        Resource::Wages => Color::from_rgba(255, 170, 150, 255),
        _ => CAPTION,
    }
}

/// The height of one row of the noon report's labels, in the original's pixels: the label's
/// baseline 18 under the row's top and its icon 62 (70 across), the next row 104 lower.
const REPORT_ROW: f32 = 104.0;

/// Where the noon report's `n` labels stand in the box from `x` over `w`, at `cy` and scale `o`,
/// in rows of `cols` (one row: `cols == n`): each label's centre x and its baseline y (whole
/// pixels). A row's labels share its baseline, and no label sits lower than the one before it
/// in its row; the next row is [`REPORT_ROW`] lower. A short last row is centred in the box.
fn report_label_grid(n: usize, cols: usize, x: f32, w: f32, o: f32, cy: f32) -> Vec<(f32, f32)> {
    (0..n)
        .map(|i| {
            let (row, col) = (i / cols, i % cols);
            let k = (n - row * cols).min(cols);
            let step = (w - 60.0 * o) / k as f32;
            let cx = x + 30.0 * o + step * (col as f32 + 0.5);
            (cx, (cy + row as f32 * REPORT_ROW * o + 18.0 * o).round())
        })
        .collect()
}

/// The noon report's labels per row: Razdor's phone choice (the owner's request), a phone held
/// upright (`window_layout::portrait`) stacks more than two of them in rows of two, the box too
/// narrow for four in one row. Every other window, and a report of at most two, is one row as
/// the original's (0x4a9d29..0x4a9d74).
fn report_label_cols(report: bool, portrait: bool, n: usize) -> usize {
    if report && portrait && n > 2 { 2 } else { n.max(1) }
}

/// The rows the resource labels take: [`report_label_cols`] labels to a row.
fn report_label_rows(report: bool, portrait: bool, n: usize) -> usize {
    if n == 0 { 0 } else { n.div_ceil(report_label_cols(report, portrait, n)) }
}

/// A resource icon with its label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Gold,
    Mana,
    Income,
    Wages,
    Experience,
}

/// A picture beside the text: a unit's portrait, or an event's own image.
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
    pub resource_report: bool,
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
    /// Both the village's offer and its result use the same scaled picture/text layout.
    pub village_event: bool,
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
    /// The first text line shown when the text is taller than the screen allows (Razdor's
    /// scrolling: the wheel over the text, the arrow and page keys).
    pub scroll: std::cell::Cell<usize>,
    /// A key that closes it went down while it was shown: its release presses OK (0x4cd558).
    /// A key already held as it opened does nothing (0x47f358).
    pub key_down: std::cell::Cell<bool>,
}

impl Dialog {
    pub fn new(title: impl Into<String>) -> Dialog {
        Dialog {
            resource_report: false,
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
            village_event: false,
            picture: None,
            joined: Vec::new(),
            left: Vec::new(),
            lost: Vec::new(),
            chord: false,
            cued: false,
            not_before: None,
            defeat: false,
            scroll: std::cell::Cell::new(0),
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
        d.resource_report = true;
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

/// The left edges of a question's Yes and No buttons `bw` wide in a window at `x`, `w` wide:
/// centred at 0.24 and 0.76 of its width, as the original's.
pub fn question_buttons_x(x: f32, w: f32, bw: f32) -> (f32, f32) {
    (x + w * 0.24 - bw / 2.0, x + w * 0.76 - bw / 2.0)
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
    [(Resource::Gold, gold), (Resource::Mana, mana)]
        .into_iter()
        .filter(|&(_, v)| v != 0)
        .map(|(r, v)| (r, format!("{} + {v}", resource_word(r))))
        .collect()
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
/// The side picture of a unit: its bust (type record +0x14, 92 × 92: 0x48efd0).
const UNIT_BUST: f32 = 92.0;

/// The left edges, from the window's left in the original's pixels, of a units row's `n`
/// cells: centred at W·(i+1)/(n+1) in whole pixels, less 23 (0x4a95a9 and 0x4a9a84: `IMUL`,
/// `IDIV`, `SUB 0x17`). Each event names at most four units to add or take (+0x61, +0x69).
/// `win_w` is the window's width in its own design pixels: the original's window is always
/// 634 wide (`EVENT_W`), and Razdor's phone and large windows are not, so the row follows
/// the window it is in.
fn unit_cells(n: usize, win_w: i32) -> Vec<f32> {
    let n = n.min(4) as i32;
    (0..n).map(|i| ((win_w * (i + 1)) / (n + 1) - 23) as f32).collect()
}

/// Razdor's scrollbar beside a scrolled event text (not in the original): 12 of the window's
/// pixels wide, 4 in from the box's right edge and 6 from its top and bottom, at the window's
/// scale `o`, so it grows with the window (on a phone it was 12 screen px: a player's report).
fn text_scrollbar_rect(tb: Rect, o: f32) -> Rect {
    let o = o.max(1.0);
    Rect::new(tb.x + tb.w - 16.0 * o, tb.y + 6.0 * o, 12.0 * o, tb.h - 12.0 * o)
}

/// The first line shown of a text of `total` lines with `shown` in sight in box `tb` of a
/// window at the scale `o`: the wheel over the box, Up/Down, Page Up/Page Down, Home/End move
/// it; the scrollbar on the right too.
fn scrolled(d: &Dialog, total: usize, shown: usize, tb: Rect, o: f32) -> usize {
    let last = total.saturating_sub(shown);
    let mut first = d.scroll.get().min(last);
    if last > 0 {
        swipe_rows(tb, tb.h / shown.max(1) as f32);
    }
    if last > 0 && !input_blocked() {
        let wh = if tb.contains(pointer().into()) { wheel() } else { 0.0 };
        let page = shown.saturating_sub(1).max(1);
        if wh < 0.0 || key(KeyCode::Down) {
            first = (first + 1).min(last);
        } else if wh > 0.0 || key(KeyCode::Up) {
            first = first.saturating_sub(1);
        } else if key(KeyCode::PageDown) {
            first = (first + page).min(last);
        } else if key(KeyCode::PageUp) {
            first = first.saturating_sub(page);
        } else if key(KeyCode::Home) {
            first = 0;
        } else if key(KeyCode::End) {
            first = last;
        }
    }
    // Razdor's scrollbar (a touch screen has neither wheel nor keys).
    scrollbar(text_scrollbar_rect(tb, o), &mut first, shown, total);
    d.scroll.set(first);
    first
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
/// On a phone the window's width range (Razdor's): narrower than the original's so that a
/// portrait phone shows its text larger (8 item icons of 55 still fit), and no wider than
/// this on tablets and landscape phones, where the lines would run on too long.
const EVENT_MIN_W: f32 = 480.0;
const EVENT_MAX_W: f32 = 860.0;
const EVENT_BOX_TOP: f32 = 46.0;
const EVENT_PAD: f32 = 16.0;
const EVENT_MIN_BOX: f32 = 92.0;
/// The box's tallest before the text scrolls (0x4aa88a: 499).
const EVENT_MAX_BOX: f32 = 499.0;
/// The window's height besides the box and the rows under it (0x4aa929: + 0x77).
const EVENT_REST: f32 = 119.0;

/// The message the phone's text base is read for (`display::text_base`): the smallest large
/// window, its box at its minimum and no picture. Tooltips and titles read it too, so that a
/// phone's text is one scale everywhere.
pub const TEXT_REF: (f32, f32) = (EVENT_MIN_W, EVENT_MIN_BOX + EVENT_REST);
/// Rows of the original's event text (Benguiat), and the defeat report's gap row (0x47e46c
/// with font ae24bc).
const EVENT_LINE: f32 = 19.0;
const EVENT_GAP: f32 = 12.0;
/// From a row's top to its baseline.
const EVENT_BASELINE: f32 = 13.0;

fn event_frame(side: Option<(f32, f32)>, text_h: f32, below: f32, max_box: f32, w: f32) -> EventFrame {
    let side_w = side.map_or(0.0, |(pw, _)| pw + 14.0);
    let min_box = side.map_or(EVENT_MIN_BOX, |(_, ph)| ph.max(EVENT_MIN_BOX));
    let box_h = (text_h + 2.0 * EVENT_PAD).max(min_box).min(max_box.max(min_box));
    EventFrame { w, h: box_h + below + EVENT_REST, box_x: 19.0 + side_w, box_w: w - 38.0 - side_w, box_h }
}

/// Where the window stands: centred over the map view above the bottom bar (the original's
/// 1024×682: 0x4aa9ff), in screen pixels for a window `w` × `h` of them.
fn event_origin(sw: f32, sh: f32, bar: f32, w: f32, h: f32) -> (f32, f32) {
    (((sw - w) / 2.0).floor(), ((sh - bar - h) / 2.0).floor().max(2.0))
}

/// The event window measured at the scale `o` (screen px per original px): its text at the
/// text scale `t` ([`super::display::text_o`], Razdor's), everything else in `o`.
struct EventLayout {
    o: f32,
    t: f32,
    size: f32,
    line: f32,
    side: Option<(f32, f32)>,
    room: f32,
    lines: Vec<String>,
    rows: Vec<MarkupRow>,
    total: usize,
    pic_h: f32,
    res_h: f32,
    items_h: f32,
    lost_h: f32,
    spells_h: f32,
    below: f32,
    f: EventFrame,
    shown: usize,
}

/// A text row's height: the defeat report's empty row is the gap font's (0x47e46c), shorter
/// than a line.
fn event_row_h(defeat: bool, line: f32, t: f32, r: &MarkupRow) -> f32 {
    if defeat && r.text.is_empty() { EVENT_GAP * t } else { line }
}

/// The event window's widest width in the original's pixels at the scale `o` on a `sw`-wide
/// screen: 634 on the desktop; on a phone (Razdor's, the owner's request: a portrait phone's
/// window was 60% of the width) the screen's width less 12 px, up to [`EVENT_MAX_W`]. A large
/// window is no wider than this; the text may need less ([`text_fit_width`]).
fn event_width(o: f32, sw: f32, phone: bool) -> f32 {
    let cap = if phone { EVENT_MAX_W } else { EVENT_W };
    cap.min((sw - 12.0) / o)
}

/// The frame's sides beside the text, in the original's pixels: the box's 19 + 19 and the
/// text's 40 (`room` in [`event_layout`]).
const EVENT_SIDES: f32 = 78.0;

/// The narrowest a large window gets for its text, in the original's pixels (a short line).
const EVENT_TEXT_MIN_W: f32 = 260.0;

/// The width of a large window that takes what its text needs (Razdor's, the owner's request
/// after a player's report: a one-line message filled the screen's width). `widest` is the
/// widest text row in screen px at the scale `o`; the window is that plus the frame and the
/// picture beside it, at least [`EVENT_TEXT_MIN_W`] of text, at most `max_w`.
fn text_fit_width(widest: f32, o: f32, side_w: f32, max_w: f32) -> f32 {
    // One original px over the greedy wrap's exact fit, so the lines break as they did.
    let want = widest / o + EVENT_SIDES + side_w + 1.0;
    want.max(EVENT_TEXT_MIN_W + EVENT_SIDES + side_w).min(max_w)
}

/// The width a window needs for its widest caption, in original px at the scale `o`: the caption
/// is centred over the window, so it takes the frame's side margins ([`EVENT_SIDES`]) as the
/// text does. A player's report: «Присоединившиеся персонажи:» ran past the frame of a phone's
/// narrow large window.
fn caption_fit_width(widest_caption: f32, o: f32) -> f32 {
    widest_caption / o + EVENT_SIDES
}

/// The widest of the captions shown in `d` (the icon rows' and the notice line), in screen px,
/// in their face and at the text size `size`.
fn widest_caption(d: &Dialog, size: f32) -> f32 {
    let mut captions: Vec<String> = Vec::new();
    if !d.spells.is_empty() {
        captions.push(caption_text("NewSpell", "New spells learnt:"));
    }
    if !d.left.is_empty() {
        captions.push(caption_text("DelUnit", "Characters departed:"));
    }
    if !d.joined.is_empty() {
        captions.push(caption_text("AddUnit", "Characters joined:"));
    }
    if !d.lost.is_empty() {
        captions.push(caption_text("DelItem", "Items lost:"));
    }
    if !d.items.is_empty() {
        captions.push(caption_text("AddItem", "Items received:"));
    }
    captions.extend(d.notice.clone());
    super::dt_font::with_original_face(super::dt_font::Face::Subtitle, || {
        captions.iter().map(|c| measure(c, size).width).fold(0.0, f32::max)
    })
}

/// A caption's text: the Russian catalog's `[Event]` line in Russian, else the English `ours`.
fn caption_text(key: &str, ours: &'static str) -> String {
    chrome::ui_text("Event", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string())
}

/// The widest of a laid-out text's rows, in screen px.
fn widest_text(lay: &EventLayout) -> f32 {
    let lines = lay.lines.iter().map(|l| measure(l, lay.size).width);
    let rows = lay.rows.iter().map(|r| r.text.chars().map(|c| glyph_width(c, lay.size)).sum::<f32>());
    lines.chain(rows).fold(0.0, f32::max)
}

/// Measures the event window `d` at the scale `o` and the text scale `t` (`display::text_o_at`,
/// its size on its own): the text wrapped at the text scale, the rows and the frame around
/// them, in the window `w` original px wide.
fn event_layout(d: &Dialog, o: f32, t: f32, w: f32) -> EventLayout {
    let sh = screen_height();
    let size = super::dt_font::sheet_size(super::dt_font::Face::Subtitle, t);
    let line = EVENT_LINE * t;
    // A picture at the side takes its width and 14 more from the text box.
    let side = match &d.picture {
        // A unit's 92 × 92 bust (type record +0x14, 0x4a8ae8; Icons.ugs, 0x48efd0).
        Some(Picture::Unit(_)) => Some((UNIT_BUST, UNIT_BUST)),
        Some(Picture::Image(t)) => {
            let scale = (128.0 / t.height()).min(220.0 / t.width());
            Some((t.width() * scale, t.height() * scale))
        }
        Some(Picture::Side(t)) => Some((t.width(), t.height())),
        Some(Picture::Still(t)) => Some((t.width(), t.height())),
        _ => None,
    };
    let side_w = side.map_or(0.0, |(pw, _)| pw + 14.0);
    let room = (w - 38.0 - side_w - 40.0) * o;
    let (lines, rows): (Vec<String>, Vec<MarkupRow>) = match &d.marked {
        Some(t) => (Vec::new(), markup_rows(t, room, size)),
        // Laid out three times a frame with a large window (`draw_content`): the wraps are kept.
        None => (d.text.iter().flat_map(|t| wrap_cached(t, room, size)).collect(), Vec::new()),
    };
    let total = lines.len() + rows.len();
    let full_h = lines.len() as f32 * line + rows.iter().map(|r| event_row_h(d.defeat, line, t, r)).sum::<f32>();
    // One row of labels per [`report_label_rows`]: the box grows by a row's height for each.
    let res_h = report_label_rows(d.resource_report, super::window_layout::portrait(), d.resources.len()) as f32 * REPORT_ROW * o;
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
    // The box stops at 499 (and short of the screen): the text scrolls in it, the buttons
    // stay in sight.
    // The title strip's growth (`chrome::title_grow`) is part of the window's height too.
    let screen_box = (sh - chrome::bar_height() - 4.0) / o - below / o - EVENT_REST - chrome::title_grow() / o;
    let max_box = EVENT_MAX_BOX.min(screen_box);
    let f = event_frame(side, full_h / o, below / o, max_box, w);
    let shown = if full_h / o + 2.0 * EVENT_PAD <= f.box_h { total } else { (((f.box_h - 2.0 * EVENT_PAD) * o / line).floor().max(1.0) as usize).min(total) };
    EventLayout { o, t, size, line, side, room, lines, rows, total, pic_h, res_h, items_h, lost_h, spells_h, below, f, shown }
}

/// Draws `d` over the map; returns how it was closed: OK (or Enter, Escape), or for a
/// question Yes or No (Esc or N No, any other key Yes: `answer_key`). Laid out in the
/// original's pixels ([`event_frame`]) times the interface scale, its text in the original's
/// Benguiat at its own size ([`super::dt_font::sheet_size`]). Razdor's two settings: the text
/// size (`display::text_o`) scales the text only. The large windows (`display::popup_o`) scale
/// the whole window while it fits the screen: on the desktop up to twice the interface scale,
/// 634 px wide; on a phone one original px per dp, and the width follows the screen between 480
/// and 860 px (`event_width`). Off, the window is the original's, at the interface scale.
pub fn draw(d: &Dialog, assets: &Assets, game: Option<&Game>) -> Option<Close> {
    super::dt_font::with_legacy_metrics(|| {
        super::dt_font::with_original_face(super::typography::Role::Name.face(), || draw_content(d, assets, game))
    })
}

fn draw_content(d: &Dialog, assets: &Assets, game: Option<&Game>) -> Option<Close> {
    if d.village_event {
        return draw_village_event(d, assets, game);
    }
    let (sw, sh) = (screen_width(), screen_height());
    // The original's pixel on screen, snapped as the font sheets snap (×1.875 → ×2), so the
    // text and its box grow together and the lines break where the original's do. Razdor's
    // two settings (the owner's request): the text takes the text size alone, at the scale of
    // the window with the large windows off, so it is the same with them on. The large windows
    // only grow the window: on, its scale is the largest that fits its smallest size on the
    // screen (on a phone one px per dp), and its width is what its text needs.
    let defeat = d.defeat;
    let base = chrome::k() * 0.9375;
    let plain = super::dt_font::whole_scale(base).min((sw - 12.0) / EVENT_W);
    let t = super::display::text_for(plain, TEXT_REF);
    let large = super::display::large_windows();
    // The phone layout needs the large windows on: off, a phone takes the original's path.
    let phone = large && super::display::dp_px().is_some();
    let mut o = plain;
    if large {
        let min_w = if phone { EVENT_MIN_W } else { EVENT_W };
        // The smallest window: the box at its minimum, the rows under it as they are.
        let first = event_layout(d, plain, t, EVENT_W);
        let min_h = event_frame(first.side, 0.0, first.below / plain, EVENT_MAX_BOX, min_w).h + chrome::title_grow() / plain;
        o = super::dt_font::whole_scale(super::display::popup_o(base, min_w, min_h)).min((sw - 12.0) / min_w).max(plain);
    }
    let lay = if large {
        let max_w = event_width(o, sw, phone);
        let probe = event_layout(d, o, t, max_w);
        let side_w = probe.side.map_or(0.0, |(pw, _)| pw + 14.0);
        let width = text_fit_width(widest_text(&probe), o, side_w, max_w);
        // The captions centred over the window must fit its frame too (never past the cap).
        let size = super::dt_font::sheet_size(super::dt_font::Face::Subtitle, t);
        let width = width.max(caption_fit_width(widest_caption(d, size), o)).min(max_w);
        event_layout(d, o, t, width)
    } else {
        event_layout(d, o, t, EVENT_W)
    };
    chrome::under_message();
    let EventLayout { o, t, size, line, side, room, lines, rows, total, pic_h, res_h, items_h, lost_h, spells_h, f, shown, .. } = lay;
    let row_h = |r: &MarkupRow| event_row_h(defeat, line, t, r);
    // The title strip's growth: the window is taller by it, and its frame and content move down.
    let grow = chrome::title_grow();
    let (w, h) = (f.w * o, f.h * o + grow);
    let (x, y) = event_origin(sw, sh, chrome::bar_height(), w, h);
    // The defeat look is the red marble (the original's window after a lost battle).
    let skin = if d.defeat { chrome::Skin::Red } else { chrome::Skin::Marble };
    if d.defeat || d.resource_report {
        chrome::window_with_title_color_grown(Rect::new(x, y, w, h), &d.title, skin, false, Color::from_rgba(255, 255, 255, 255));
    } else {
        chrome::window_grown(Rect::new(x, y, w, h), &d.title, skin, false);
    }
    let edge = o.max(1.0);
    // The event window's inner frames (0x4aaa4b, 0x4aaa79), as 0x48d710's.
    chrome::window_inner_frames_grown(Rect::new(x, y, w, h), o, grow);
    let top = y + EVENT_BOX_TOP * o + grow;
    let mut cy = top;
    match &d.picture {
        Some(Picture::Unit(u)) => {
            // Left of the text box, as the original's portrait (the box starts at 19 +
            // picture + 14).
            let r = Rect::new(x + 20.0 * o, top + o, UNIT_BUST * o, UNIT_BUST * o);
            assets.draw_portrait(*u, Team::Player, r);
            chrome::silver_frame(r, 1.5 * o);
        }
        Some(Picture::Image(tex)) => {
            let (tw, th) = side.expect("picture dimensions");
            let r = Rect::new(x + 20.0 * o, top + o, tw * o, th * o);
            draw_texture_ex(tex, r.x, r.y, WHITE, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), ..Default::default() });
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0 * o, MARBLE_EDGE);
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
    let first = scrolled(d, total, shown, tb, o);
    let shown_h = lines.iter().skip(first).take(shown).map(|_| line).sum::<f32>() + rows.iter().skip(first).take(shown).map(row_h).sum::<f32>();
    let mut ty = tb.y + ((tb.h - shown_h) / 2.0).max(EVENT_PAD * o).floor() + EVENT_BASELINE * t;
    let text_x = tb.x + 20.0 * o;
    for l in lines.iter().skip(first).take(shown) {
        super::dt_font::with_original_face(super::dt_font::Face::Subtitle, || chrome::shadow_centered(l, tb.x + tb.w / 2.0, ty, size, Color::new(1.0, 0.9, 0.66, 1.0)));
        ty += line;
    }
    for row in rows.iter().skip(first).take(shown) {
        draw_markup_row(row, text_x, ty, room, size);
        ty += row_h(row);
    }
    cy += tb.h + 10.0 * o;
    if !d.resources.is_empty() {
        let cols = report_label_cols(d.resource_report, super::window_layout::portrait(), d.resources.len());
        let places = report_label_grid(d.resources.len(), cols, x, w, o, cy);
        for (i, ((r, label), &(cx, baseline))) in d.resources.iter().zip(&places).enumerate() {
            // The original's caption face, every label in its green font ae249c (0x4d6114 sets
            // it on all four places), the mana's too.
            let c = if d.resource_report { report_label_ink(*r) } else { CAPTION };
            super::dt_font::with_original_face(super::dt_font::Face::Subtitle, || chrome::shadow_centered(label, cx, baseline, size, c));
            let row = (i / cols) as f32;
            resource_icon(*r, cx, cy + 62.0 * o + row * REPORT_ROW * o, 70.0 * o);
        }
        cy += res_h;
    }
    // The original stacks its rows from the bottom up (0x4a8ae8): the items gained lowest,
    // the units above them, the spells above those; each row its green caption centred over
    // the window and its icons spread by their count, centre i at W·(i+1)/(n+1).
    let spread = |n: usize, i: usize| x + w * (i + 1) as f32 / (n + 1) as f32;
    let caption = |key: &str, ours: &'static str, cy: f32| {
        let t = caption_text(key, ours);
        super::dt_font::with_original_face(super::dt_font::Face::Subtitle, || chrome::shadow_centered(&t, x + w / 2.0, cy + 18.0 * o, size, CAPTION));
    };
    let mut icons: Vec<(bool, u32, Rect)> = Vec::new();
    if !d.spells.is_empty() {
        caption("NewSpell", razdor::i18n::n_("New spells learnt:"), cy);
        let n = d.spells.len().min(4);
        for (i, &s) in d.spells.iter().enumerate().take(4) {
            let r = Rect::new(spread(n, i) - 25.0 * o, cy + 26.0 * o, 50.0 * o, 50.0 * o);
            if let Some(def) = game.and_then(|g| g.content.spells.iter().find(|x| x.id == s)) {
                chrome::spell_icon(def, r);
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
        for (&u, left) in units.iter().zip(unit_cells(units.len(), (w / o).round() as i32)) {
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
        super::dt_font::with_original_face(super::dt_font::Face::Subtitle, || chrome::shadow_centered(n, x + w / 2.0, cy + 18.0 * o, size, Color::new(0.35, 0.9, 1.0, 1.0)));
    }
    // The buttons, 95 × 36, 45 from the window's bottom.
    let (bw, bh) = (95.0 * o, 36.0 * o);
    let by = y + h - 58.0 * o + (58.0 * o - bh) / 2.0;
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
        // The item tooltip is 272 wide and as tall as its text plus 32 (0x4cbab0). The spell card is
        // the book's cell size (286 × 112) at the window's scale: the original's full card at the
        // pointer, with its cost and duration lines (the player's frames 625-626: 402 × 157 px at 1920).
        let (pw, ph) = if spell { (286.0 * o, 112.0 * o) } else { (super::item_text::TOOLTIP_W * o, super::item_text::tooltip_height(game, ItemId(id), o)) };
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
        // Yes at a quarter of the window, No at three quarters (the original's question,
        // measured on its «Просьба торговца»: centres at 0.24 W and 0.76 W).
        let (yes_x, no_x) = question_buttons_x(x, w, bw);
        let yes = button(yes_x, by, bw, bh, tr("Yes"), true) || answer == Some(true);
        let no = button(no_x, by, bw, bh, tr("No"), true) || answer == Some(false);
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
        let (_, down, pressed) = button_state(r, true);
        chrome::defeat_button(r, &ok_label, true, down);
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

/// Village offers and results share the original's 634 px wide event window.
/// The illustration stays beside the text, with rewards below and a framed footer.
fn draw_village_event(d: &Dialog, assets: &Assets, game: Option<&Game>) -> Option<Close> {
    chrome::under_message();
    let picture = match &d.picture { Some(Picture::Side(t)) => Some(t), _ => None };
    // The window's height in the original's pixels, for Razdor's large windows (the owner's
    // request: phones' text too small): the footer and the frame (114), the rewards (110), the
    // illustration's height (210 without one).
    let rewards = !d.items.is_empty() || !d.resources.is_empty();
    let need = 114.0 + if rewards { 110.0 } else { 0.0 } + picture.map_or(210.0, |t| t.height());
    let base = chrome::k() * 0.9375;
    let o = super::display::popup_o(base, 634.0, need).min((screen_width() - 12.0) / 634.0);
    let w = 634.0 * o;
    // The text takes Razdor's text size (`display::text_o`) at the window's scale with the large
    // windows off, so it is the same with them on; the large windows grow the window only.
    let t = super::display::text_for(base.min((screen_width() - 12.0) / 634.0), TEXT_REF);
    let size = 17.0 * t;
    let line_h = 18.0 * t;
    let pw = picture.map_or(106.0, |t| t.width()) * o;
    let ph = picture.map_or(210.0, |t| t.height()) * o;
    let text_w = w - pw - 50.0 * o;
    let rows = markup_rows(d.marked.as_deref().unwrap_or(""), text_w - 40.0 * o, size);
    let reward_h = if rewards { 110.0 * o } else { 0.0 };
    let fixed = 114.0 * o + reward_h;
    // The title strip's growth is part of the window's height (`chrome::title_grow`).
    let grow = chrome::title_grow();
    let available = (screen_height() - chrome::bar_height() - fixed - grow - 8.0).max(60.0 * o);
    let panel_h = ph.max(rows.len() as f32 * line_h + 36.0 * o).min(available);
    let shown = ((panel_h - 36.0 * o) / line_h).floor().max(1.0) as usize;
    let h = fixed + panel_h + grow;
    let x = ((screen_width() - w) / 2.0).round();
    let y = ((screen_height() - chrome::bar_height() - h) / 2.0).max(2.0).round();
    chrome::window_grown(Rect::new(x, y, w, h), &d.title, chrome::Skin::Marble, false);
    let footer = Rect::new(x + 2.0 * o, y + h - 55.0 * o, w - 4.0 * o, 53.0 * o);
    // The village window is ae2520's kind, with 0x48d710's inner frames (moved down with the
    // taller title strip).
    chrome::window_inner_frames_grown(Rect::new(x, y, w, h), o, grow);
    let top = y + 45.0 * o + grow;
    if let Some(t) = picture {
        let r = Rect::new(x + 18.0 * o, top, pw, ph.min(panel_h));
        chrome::tex(t, r, WHITE);
        chrome::silver_frame(r, o.max(1.0));
    }
    let tb = Rect::new(x + 32.0 * o + pw, top, text_w, panel_h);
    chrome::text_box(tb);
    let first = scrolled(d, rows.len(), shown, tb, o);
    let count = rows.len().saturating_sub(first).min(shown);
    let text_top = tb.y + (tb.h - count as f32 * line_h) / 2.0;
    let room = tb.w - 40.0 * o - if rows.len() > shown { 16.0 * o } else { 0.0 };
    for (i, row) in rows.iter().skip(first).take(shown).enumerate() {
        draw_markup_row(row, tb.x + 20.0 * o, text_top + i as f32 * line_h + size, room, size);
    }
    let reward_y = top + panel_h + 14.0 * o;
    if !d.items.is_empty() {
        let caption = chrome::ui_text("Event", "AddItem").filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru)
            .unwrap_or_else(|| tr("Items received:").to_string());
        chrome::shadow_centered(&caption, x + w / 2.0, reward_y + 15.0 * o, size, CAPTION);
        for (i, &item) in d.items.iter().enumerate() {
            let cx = x + w * (i + 1) as f32 / (d.items.len() + 1) as f32;
            let r = Rect::new(cx - 25.0 * o, reward_y + 28.0 * o, 50.0 * o, 50.0 * o);
            assets.draw_item(item, r.x, r.y, r.w);
            if r.contains(pointer().into()) && (is_mouse_button_down(MouseButton::Left) || is_mouse_button_down(MouseButton::Right)) {
                if let Some(game) = game {
                    let (tw, th) = (super::item_text::TOOLTIP_W * o, super::item_text::tooltip_height(game, item, o));
                    super::item_text::draw(game, assets, item, Rect::new(x + (w - tw) / 2.0, y, tw, th), super::item_text::ItemBox::Tooltip, o);
                    return None;
                }
            }
        }
    }
    for (i, (resource, label)) in d.resources.iter().enumerate() {
        let cx = x + w * (i + 1) as f32 / (d.resources.len() + 1) as f32;
        chrome::shadow_centered(label, cx, reward_y + 15.0 * o, size, MANA);
        resource_icon(*resource, cx, reward_y + 56.0 * o, 65.0 * o);
    }
    let (bw, bh) = (96.0 * o, 35.0 * o);
    let by = footer.y + (footer.h - bh) / 2.0;
    if d.question {
        let answer = answer_key();
        let yes = button(x + w * 0.24 - bw / 2.0, by, bw, bh, tr("Yes"), true);
        let no = button(x + w * 0.76 - bw / 2.0, by, bw, bh, tr("No"), true);
        return if yes || answer == Some(true) { Some(Close::Yes) } else if no || answer == Some(false) { Some(Close::No) } else { None };
    }
    let ok = button(x + (w - bw) / 2.0, by, bw, bh, tr("OK"), true);
    if super::widgets::ok_key_pressed() { d.key_down.set(true); }
    (ok || (d.key_down.get() && super::widgets::ok_key_released())).then_some(Close::Ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The original's question at 1024×768 (window 634 wide at x 195): Да centred near 345,
    /// Нет near 677; the captions «Деньги»… come from `[Event] Resource0..3`.
    #[test]
    fn a_question_s_buttons_stand_at_a_quarter_and_three_quarters() {
        let (yes, no) = question_buttons_x(195.0, 634.0, 95.0);
        assert!(((yes + 47.5) - 347.0).abs() < 4.0, "{yes}");
        assert!(((no + 47.5) - 677.0).abs() < 4.0, "{no}");
        assert_eq!(resource_word(Resource::Experience), tr("Experience"));
    }

    #[test]
    fn the_text_scrollbar_grows_with_the_window() {
        let tb = Rect::new(100.0, 50.0, 600.0, 300.0);
        assert_eq!(text_scrollbar_rect(tb, 1.0), Rect::new(684.0, 56.0, 12.0, 288.0), "the original's scale: as before");
        let r = text_scrollbar_rect(tb, 3.0);
        assert_eq!((r.x, r.y, r.w, r.h), (652.0, 68.0, 36.0, 264.0), "a phone's window three times larger: the bar too");
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
    /// The defeat report as the original lays it out at 1024×768 (captured under Wine): 634 ×
    /// 247 at (195, 217), centred over the 1024×682 map view; Stnd-1 (128²) at the side, the
    /// text box 454 wide from x + 161 and as tall as the picture, two lines, a gap row, two
    /// lines (4 × 19 + 12 = 88, centred in it).
    #[test]
    fn the_event_window_stands_as_the_original_s() {
        let f = event_frame(Some((128.0, 128.0)), 88.0, 0.0, EVENT_MAX_BOX, EVENT_W);
        assert_eq!(f, EventFrame { w: 634.0, h: 247.0, box_x: 161.0, box_w: 454.0, box_h: 128.0 });
        assert_eq!(event_origin(1024.0, 768.0, 86.0, f.w, f.h), (195.0, 217.0));
        // No picture: the box at least 92, else the text and 16 above and below.
        assert_eq!(event_frame(None, 19.0, 0.0, EVENT_MAX_BOX, EVENT_W).box_h, 92.0);
        assert_eq!(event_frame(None, 190.0, 0.0, EVENT_MAX_BOX, EVENT_W), EventFrame { w: 634.0, h: 341.0, box_x: 19.0, box_w: 596.0, box_h: 222.0 });
        // A text taller than 499 scrolls in a box of 499 (0x4aa88a), or less on a short screen.
        assert_eq!(event_frame(None, 900.0, 0.0, EVENT_MAX_BOX, EVENT_W).box_h, 499.0);
        assert_eq!(event_frame(None, 900.0, 0.0, 300.0, EVENT_W).box_h, 300.0);
    }

    /// Razdor's phone table (the owner's request): how large the event text is on phones, in dp
    /// (one original px is one dp there), against the original's own size. Each device in both
    /// orientations; the window fits the screen, and its line is no smaller than before.
    #[test]
    fn a_phone_s_event_text_is_as_large_as_its_dp_allows() {
        use crate::ui::dt_font::whole_scale;
        // (device, screen width and height in landscape, px per dp)
        let devices: [(&str, f32, f32, f32); 10] = [
            ("iPhone 15", 852.0, 393.0, 1.0),
            ("iPhone SE", 667.0, 375.0, 1.0),
            ("iPhone 15 Pro Max", 932.0, 430.0, 1.0),
            ("6.5in 720p", 1600.0, 720.0, 1.75),
            ("6.5in 720p, 2.0", 1600.0, 720.0, 2.0),
            ("5.5in 720p", 1280.0, 720.0, 2.0),
            ("small budget", 854.0, 480.0, 1.5),
            ("6.4in 1080p", 2400.0, 1080.0, 2.625),
            ("2160x1080", 2160.0, 1080.0, 2.75),
            ("tablet 10in", 1920.0, 1200.0, 1.5),
        ];
        let text = 1.25;
        let min_h = EVENT_REST + EVENT_MIN_BOX;
        for (name, long, short, dp) in devices {
            for (orientation, (sw, sh)) in [("landscape", (long, short)), ("portrait", (short, long))] {
                let fit_k = (sw / 960.0).min(sh / 720.0).max(0.5);
                let base = fit_k * 0.9375;
                let bar = (80.0 * fit_k).round();
                let old_o = whole_scale(base).min((sw - 12.0) / EVENT_W);
                let raw = crate::ui::display::popup_scale(base, true, Some(dp), (EVENT_MIN_W, min_h), (sw, sh), bar);
                let o = whole_scale(raw).min((sw - 12.0) / EVENT_MIN_W);
                let width = event_width(o, sw, true);
                // The text is the window's scale with the large windows off, whatever the window's.
                let t = crate::ui::display::text_o_at(text, old_o);
                let line_dp = EVENT_LINE * t / dp;
                eprintln!("{name:<18} {orientation:<9} {sw:>5}x{sh:<5} dp {dp:<5} scale {o:.2} width {width:>5.0} line {line_dp:>5.1} dp");
                assert!(o >= base - 1e-4, "{name} {orientation}: the scale is below the base");
                assert!(width * o <= sw - 12.0 + 0.01, "{name} {orientation}: the window is wider than the screen");
                assert!(min_h * o <= sh - bar - 4.0 + 0.01, "{name} {orientation}: the window is taller than the screen");
                assert!(o >= old_o - 1e-4, "{name} {orientation}: the large window is smaller than the normal one");
            }
        }
    }

    /// A large window takes what its text needs (a player's report: a one-line message filled
    /// the phone's width): narrower than the screen for a short text, the cap for a long one.
    #[test]
    fn a_large_window_is_as_wide_as_its_text_needs() {
        let o = 2.0;
        // A one-line text 300 px wide at 2 px per original px: the frame's 78 and one px more.
        let one_line = text_fit_width(300.0, o, 0.0, 860.0);
        assert_eq!(one_line, EVENT_TEXT_MIN_W + EVENT_SIDES, "a short text takes the least width");
        assert!(one_line * o < 1080.0 - 12.0, "a one-line window is narrower than the screen");
        // A text as wide as the screen takes the cap; the picture's side is added.
        assert_eq!(text_fit_width(5000.0, o, 0.0, 860.0), 860.0);
        assert_eq!(text_fit_width(300.0, o, 142.0, 860.0), EVENT_TEXT_MIN_W + EVENT_SIDES + 142.0);
        // Wide enough for the widest line: that line wraps as it did, with no room lost.
        let widest = 600.0;
        let w = text_fit_width(widest, o, 0.0, 860.0);
        assert!((w - EVENT_SIDES - widest / o).abs() <= 1.0 + 1e-4, "{w}");
    }

    /// The minimum width for the captions: the widest caption plus the frame's side margins, in
    /// original px (a player's report: «Присоединившиеся персонажи:» ran past the window's frame).
    #[test]
    fn a_window_is_as_wide_as_its_widest_caption_needs() {
        // A 300 px caption at 2 px per original px: 150 original px of text and the frame's 78.
        assert_eq!(caption_fit_width(300.0, 2.0), 150.0 + EVENT_SIDES);
        // A caption on the desktop's 1 px scale is taken as it is, in original px.
        assert_eq!(caption_fit_width(400.0, 1.0), 400.0 + EVENT_SIDES);
        // The width it gives is what the text-width rule takes at most: the cap holds.
        let w = text_fit_width(0.0, 2.0, 0.0, 860.0).max(caption_fit_width(5000.0, 2.0)).min(860.0);
        assert_eq!(w, 860.0, "a caption as wide as the screen takes the cap");
    }

    /// The desktop's event window stays the original's 634 px wide: its width cap is `EVENT_W`
    /// on any screen the desktop can have, so the caption minimum never widens it past that.
    #[test]
    fn the_desktop_event_window_is_634_wide() {
        for o in [1.0_f32, 1.5, 2.0] {
            assert_eq!(event_width(o, 1920.0, false), EVENT_W, "scale {o}");
            assert_eq!(event_width(o, 1280.0, false), EVENT_W.min((1280.0 - 12.0) / o), "scale {o}");
        }
        assert_eq!(EVENT_W, 634.0);
        // The phone's cap is no wider than the desktop's rule allows (the caption rule is capped).
        assert!(event_width(2.0, 2400.0, true) <= EVENT_MAX_W);
    }

    /// The title strip grows with the phone's text, and the event window's inner frame and
    /// content move down with it (a player's report: the strip crossed the frame with the large
    /// windows off): the inner frame's top stays below the strip's bottom, large windows on or off.
    #[test]
    fn a_phone_s_event_content_is_below_its_title_strip() {
        let (sw, sh, bar) = (2400.0, 1080.0, 120.0);
        for k in [1.5_f32, 1.125] {
            let factor = crate::ui::display::text_o_at(1.25, crate::ui::display::phone_text_base(Some(2.625), k, TEXT_REF, (sw, sh), bar)) / k;
            let strip = chrome::strip_height(k, factor);
            let grow = strip - (26.0 * k).round();
            assert!(grow > 0.0, "k {k}: the strip grows at a phone's text size");
            for o in [k * 0.9375, 2.625] {
                let inner_top = 31.0 * o + grow;
                assert!(inner_top >= strip, "k {k}, o {o}: the inner frame at {inner_top} is under the strip's {strip}");
            }
        }
    }

    /// The units rows (0x4a93bc, 0x4a9711): 46 × 46 icons in 47 × 47 cells centred at
    /// W·(i+1)/(n+1) less 23, in whole pixels; a row is 79 tall. A unit at the side is its
    /// 92 × 92 bust, the text box after it and 14 more.
    #[test]
    fn the_units_rows_stand_as_the_original_s() {
        assert_eq!(unit_cells(1, 634), vec![294.0]);
        assert_eq!(unit_cells(2, 634), vec![188.0, 399.0]);
        assert_eq!(unit_cells(3, 634), vec![135.0, 294.0, 452.0]);
        assert_eq!(unit_cells(4, 634), vec![103.0, 230.0, 357.0, 484.0]);
        assert_eq!(unit_cells(6, 634).len(), 4);
        assert_eq!((UNIT_ICON, UNIT_ROW), (46.0, 79.0));
        let f = event_frame(Some((UNIT_BUST, UNIT_BUST)), 19.0, UNIT_ROW, EVENT_MAX_BOX, EVENT_W);
        assert_eq!(f, EventFrame { w: 634.0, h: 92.0 + 79.0 + 119.0, box_x: 125.0, box_w: 490.0, box_h: 92.0 });
    }

    /// The units row follows its window's width, not the original's 634: the middle of the
    /// first and last cell (left + 23) sits within 1 px of the window's middle at any width.
    #[test]
    fn the_units_row_is_centred_in_a_window_of_any_width() {
        for win_w in [480, 634, 860, 900] {
            for n in 1..=4 {
                let cells = unit_cells(n, win_w);
                assert_eq!(cells.len(), n);
                let first = cells[0] + 23.0;
                let last = cells[n - 1] + 23.0;
                let mid = (first + last) / 2.0;
                assert!((mid - win_w as f32 / 2.0).abs() <= 1.0, "width {win_w}, n {n}: the row's middle at {mid}");
            }
        }
        assert_eq!(unit_cells(2, 900), vec![277.0, 577.0]);
        assert_eq!(unit_cells(2, 480), vec![137.0, 297.0]);
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

    /// The noon report's four labels share one baseline and sit in order across the box (they
    /// drifted a pixel lower each, one after another).
    #[test]
    fn the_report_s_resource_labels_share_one_baseline() {
        for (o, cy) in [(1.0, 300.0), (1.5, 412.3), (0.75, 97.6)] {
            let places = report_label_grid(4, 4, 40.0, 500.0, o, cy);
            assert_eq!(places.len(), 4);
            assert!(places.iter().all(|p| p.1 == (cy + 18.0 * o).round()), "{places:?}");
            assert!(places.windows(2).all(|p| p[0].0 < p[1].0), "{places:?}");
        }
    }

    /// Razdor's phone choice (the owner's request): held upright, the noon report's four labels
    /// stand in two rows of two, each row on its own baseline one row height lower; the centres
    /// of a column match. Landscape and the desktop keep one row (the test above).
    #[test]
    fn the_report_s_four_labels_stack_two_by_two_upright() {
        assert_eq!(report_label_cols(true, true, 4), 2);
        assert_eq!(report_label_rows(true, true, 4), 2);
        for (o, cy) in [(1.0, 300.0), (1.5, 412.3), (2.625, 97.6)] {
            let places = report_label_grid(4, 2, 40.0, 500.0, o, cy);
            let (top, bottom) = (places[0].1, places[2].1);
            assert_eq!(places[1].1, top);
            assert_eq!(places[3].1, bottom);
            assert_eq!(top, (cy + 18.0 * o).round());
            assert_eq!(bottom - top, (REPORT_ROW * o).round());
            assert_eq!(places[0].0, places[2].0, "column 1 centre");
            assert_eq!(places[1].0, places[3].0, "column 2 centre");
            assert!(places[0].0 < places[1].0);
        }
    }

    /// Three labels upright: the first row holds two, the last one is centred in the box.
    #[test]
    fn the_report_s_three_labels_stack_two_then_one_centred() {
        assert_eq!(report_label_cols(true, true, 3), 2);
        assert_eq!(report_label_rows(true, true, 3), 2);
        let (x, w, o, cy) = (40.0, 500.0, 1.5, 300.0);
        let places = report_label_grid(3, 2, x, w, o, cy);
        assert_eq!(places[0].1, places[1].1);
        assert_eq!(places[2].1, places[0].1 + (REPORT_ROW * o).round());
        assert_eq!(places[2].0, x + w / 2.0, "the lone label is centred");
        assert!(places[0].0 < places[1].0);
    }

    /// Up to two labels, and every label outside the upright phone, stay one row as before.
    #[test]
    fn the_report_s_labels_stay_one_row_elsewhere() {
        assert_eq!(report_label_cols(true, true, 2), 2);
        assert_eq!(report_label_rows(true, true, 2), 1);
        assert_eq!(report_label_rows(true, true, 1), 1);
        assert_eq!(report_label_rows(true, true, 0), 0);
        for n in 1..=4 {
            assert_eq!(report_label_cols(true, false, n), n, "landscape and desktop");
            assert_eq!(report_label_rows(true, false, n), 1, "landscape and desktop");
        }
        // The victory's labels are not the noon report's: they keep their one row as well.
        assert_eq!(report_label_rows(false, true, 3), 1);
        let places = report_label_grid(4, 4, 40.0, 500.0, 1.0, 300.0);
        assert!(places.iter().all(|p| p.1 == 318.0));
    }

    /// Deviation (the owner's decision): the four report labels are of about one brightness,
    /// and the magic is blue, the wages still red, the gold yellow.
    #[test]
    fn the_report_s_label_inks_are_of_one_brightness() {
        let luma = |c: Color| 0.299 * c.r * 255.0 + 0.587 * c.g * 255.0 + 0.114 * c.b * 255.0;
        let inks = [Resource::Gold, Resource::Mana, Resource::Income, Resource::Wages].map(report_label_ink);
        let (lo, hi) = inks.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &c| (lo.min(luma(c)), hi.max(luma(c))));
        assert!(hi - lo <= 24.0, "{lo} .. {hi}");
        let blue = report_label_ink(Resource::Mana);
        assert!(blue.b > blue.r && blue.b > blue.g, "{blue:?}");
        let red = report_label_ink(Resource::Wages);
        assert!(red.r > red.g && red.r > red.b, "{red:?}");
    }
}
