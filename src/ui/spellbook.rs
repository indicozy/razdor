//! The spell book on the world map (key B): the hero's learned spells in the original's
//! 3 × 5 cells, what each costs this hero (mana and casting time), how long it lasts and what
//! it does; cast on the own army or on an enemy army within reach; the spells on the army
//! now, with the time they have left. Razdor's choice (the owner's, 2026-10-22): the book
//! pages its spells 15 at a time (the original shows only the first 15); the pages turn by
//! the wheel, PageUp/PageDown, Left/Right and the two arrow buttons. A filter button cycles
//! the book through all its spells, the spells on the own army and the enemy spells.


use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::trf;

use razdor::rules::clock::duration_label;
use razdor::rules::content::SpellDef;
use razdor::rules::game::Game;
use razdor::rules::magic::{self, CastError, CastOutcome, CastTarget, Duration, BOOK_PAGE};

use super::audio::{cue, Cue};
use super::widgets::*;
use super::world_view;
use super::Screen;

/// Which spells the book lists (Razdor's choice, the owner's 2026-10-22; the original lists all
/// the hero's spells, 0x4d261c). One button cycles it: All → Own → Enemy → All.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BookFilter {
    #[default]
    All,
    /// The spells cast on the hero's own army («Волшебство»).
    Own,
    /// The spells cast on an enemy army («Проклятия»).
    Enemy,
}

impl BookFilter {
    /// The filter the button turns to.
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Own,
            Self::Own => Self::Enemy,
            Self::Enemy => Self::All,
        }
    }

    /// The button's label, from the catalog.
    fn label(self) -> &'static str {
        match self {
            Self::All => tr("All spells"),
            Self::Own => tr("Spells on your army"),
            Self::Enemy => tr("Enemy spells"),
        }
    }
}

/// The book indices the filter lists, in book order. `enemy[i]` says whether book spell `i`
/// is cast on an enemy army (`magic::targets_enemy`).
pub fn shown(enemy: &[bool], filter: BookFilter) -> Vec<usize> {
    (0..enemy.len())
        .filter(|&i| match filter {
            BookFilter::All => true,
            BookFilter::Own => !enemy[i],
            BookFilter::Enemy => enemy[i],
        })
        .collect()
}

/// The book's 3 columns of 5 cells (0x4dad08: cell `n` is column `n / 5`, row `n % 5`).
const ROWS: usize = 5;

/// A card's background (0x49bde4): the paper less a colour, by the card's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardFill {
    /// Affordable, not under the pointer (0x4dc118 colour 0x94,0x84,0x94: 265c).
    Normal,
    /// Affordable and under the pointer: the book's hover look (0x4c2cf4 draws the card a second
    /// time with `selected` = 1, background 0x80,0x70,0x80: 2660). The picture is brightened by
    /// 1400/1024 there too (`spell_badges::draw_picture_lit`).
    Hovered,
    /// Over the hero's mana (0x49bde4 switches to 2664 whatever the pointer does): the paper less
    /// 0xd0,0xd0,0xfa, then scaled by 2700/1024 (0x4dc118, FUN_0048da5c), so it reads dark red.
    Unaffordable,
}

/// The book cell `n` sits in: (column, row), the columns filled top to bottom (0x4dad08).
pub fn book_cell(n: usize) -> (usize, usize) {
    (n / ROWS, n % ROWS)
}

/// Pages a book of `spells` takes, 15 spells each; an empty book still shows one page.
pub fn page_count(spells: usize) -> usize {
    spells.div_ceil(BOOK_PAGE).max(1)
}

/// The page `delta` pages from `page`, kept within the `pages` there are.
pub fn page_turn(page: usize, pages: usize, delta: isize) -> usize {
    (page as isize + delta).clamp(0, pages.max(1) as isize - 1) as usize
}

/// The background a card has: [`CardFill::Unaffordable`] when `affordable` is false (the
/// sanctuary never asks, so it passes `true`), else [`CardFill::Hovered`] under the pointer.
pub fn card_fill(affordable: bool, hovered: bool) -> CardFill {
    if !affordable {
        CardFill::Unaffordable
    } else if hovered {
        CardFill::Hovered
    } else {
        CardFill::Normal
    }
}

/// Paints the card background `fill` into `r`: `Win-paper` (the empty cell's base, 0x4dc118)
/// less the colour of the state, scaled as the original's 2664 is. Without the art, the brown
/// surface; with the 1 px steel line round it that 0x4dc118 draws into every card background
/// (0x48d1d4).
fn card_paper(r: Rect, fill: CardFill) {
    let paper = match fill {
        CardFill::Normal => super::chrome::win_scaled("Win-paper", [1024; 3], [-148, -132, -148]),
        CardFill::Hovered => super::chrome::win_scaled("Win-paper", [1024; 3], [-128, -112, -128]),
        CardFill::Unaffordable => super::chrome::win_scaled("Win-paper", [2700; 3], [-548, -548, -659]),
    };
    match paper {
        Some(t) => super::chrome::tex_src(&t, Rect::new(20.0, 20.0, 286.0, 112.0), r, WHITE),
        None => super::chrome::surface(r, super::chrome::Skin::Brown),
    }
    let k = super::chrome::k();
    draw_rectangle_lines(r.x, r.y, r.w, r.h, k.max(1.0), super::chrome::SILVER);
}

pub fn duration_text(s: &SpellDef) -> String {
    match Duration::of(s) {
        Duration::Instant => tr("instant").into(),
        Duration::Minutes(m) => trf!("lasts {time}", time = duration_label(m as f64)),
    }
}

/// The spell `s` read on `t` landed with `outcome`: its sound, and the line telling it.
pub fn landed(s: &SpellDef, t: CastTarget, outcome: CastOutcome) -> String {
    if matches!(outcome, CastOutcome::Done { .. }) {
        cue(if matches!(t, CastTarget::Own) { Cue::SpellGood } else { Cue::SpellEvil });
    }
    match outcome {
        CastOutcome::Done { hits, killed, destroyed } => {
            let mut m = s.name.clone();
            if hits != 0 {
                m += &trf!(": {hits} hits", hits = format!("{hits:+}"));
            }
            if killed > 0 {
                m += &trf!(", {killed} fell", killed);
            }
            if destroyed {
                m += tr(". The army is no more.");
            } else if magic::is_lasting(s) {
                m += &format!(" ({})", duration_text(s));
            }
            m
        }
        CastOutcome::Interrupted => tr("An enemy fell on you while you were casting: the spell is lost.").into(),
        CastOutcome::TargetLost => tr("The target left the map before the spell was ready.").into(),
    }
}

fn cast_error(e: CastError) -> &'static str {
    match e {
        CastError::NotInBook | CastError::NoSuchSpell => tr("That spell is not in your book."),
        CastError::NotEnoughMana => tr("Not enough mana."),
        CastError::WrongTarget => tr("That spell does not work on that target."),
        CastError::NotATarget => tr("That army cannot be targeted."),
        CastError::Busy => tr("Not now: a battle is coming."),
    }
}

/// A text of the install (`[Magic] <key>`) in Russian, else ours.
fn own(key: &str, ours: &'static str) -> String {
    let t = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| super::chrome::ui_text("Magic", key)).flatten();
    t.unwrap_or_else(|| tr(ours).to_string())
}

/// Whole hours of `minutes`, as the book writes them ("2 час", "6 часов" in the original).
fn hours(minutes: u64, many: bool) -> String {
    let h = (minutes as f64 / 60.0).round() as u64;
    let word = if many { own("Hours", n_("hours")) } else { own("Hour", n_("h")) };
    format!("{h} {word}")
}

/// The spell book window as the original's (the video, 36:59): 3 × 5 brown cells, each
/// with the spell's round picture in its silver frame, its name, what it does, its mana and
/// reading time and how long it lasts; empty cells are dark with the knot and "Нет изученных
/// заклинаний". A click casts the spell; an enemy spell's target is then picked on the map
/// (`world_view::begin_targeting`). `selected` is the clicked spell's book index; `page` the
/// page shown (15 spells each, Razdor's choice); `filter` the spells listed (see [`BookFilter`]).
pub fn frame(
    game: &mut Game,
    assets: &super::assets::Assets,
    selected: &mut usize,
    page: &mut usize,
    filter: &mut BookFilter,
    message: &mut Option<String>,
) -> Option<Screen> {
    use super::chrome;
    let bar_pick = world_view::window_backdrop(game, assets, Some(super::game_bar::BarButton::Spells));
    let k = chrome::k();
    // The army screen's window: 836 × 600 video pixels over the map.
    let r = super::window_layout::window();
    let title = own("Title", n_("The hero's spell book"));
    let (inner, closed) = chrome::window(r, &title, chrome::Skin::Marble, true);
    let book: Vec<SpellDef> = game.book().into_iter().cloned().collect();
    let enemy: Vec<bool> = book.iter().map(magic::targets_enemy).collect();
    let gap = 8.0 * k;
    // The bottom row (the filter button, and with more than one page the arrows and "N / M")
    // always leaves its 41 px under the grid, above the message line.
    let grid = Rect::new(inner.x, inner.y, inner.w, inner.h - 41.0 * k);
    let (cw, ch) = ((grid.w - 4.0 * gap) / 3.0, (grid.h - 6.0 * gap) / 5.0 + gap);
    // The filter's list: its book indices, paged; a new filter starts on its first page.
    let listed = shown(&enemy, *filter);
    let pages = page_count(listed.len());
    if bottom_row(r, inner, gap, k, pages, page, filter) {
        *page = 0;
    }
    *page = (*page).min(pages - 1);
    let first = *page * BOOK_PAGE;
    let mut cast = None;
    for cell in 0..BOOK_PAGE {
        // The 15 cells fill each column top to bottom, then the next column (0x4dad08).
        let (c, row) = book_cell(cell);
        let cr = Rect::new(grid.x + gap + c as f32 * (cw + gap), grid.y + gap + row as f32 * ch, cw, ch - gap);
        let book_index = listed.get(first + cell).copied();
        let spell = book_index.map(|i| &book[i]);
        let hover = !input_blocked() && cr.contains(crate::ui::widgets::pointer().into());
        // The cell: an empty one is the original's (0x4dc118 makes it, 0x49bde4 draws it):
        // `Win-paper` from (20, 20) less (180, 140, 113), an olive, inside a 1 px steel line
        // (0x48d1d4). Every card has the same 1 px line as the empty one ([`card_paper`]); its
        // background tells the state (§9.3), and the hover adds no frame of its own, only its
        // colour (0x4c2cf4).
        match (spell, chrome::win_scaled("Win-paper", [1024; 3], [-180, -140, -113])) {
            (None, Some(paper)) => {
                chrome::tex_src(&paper, Rect::new(20.0, 20.0, 286.0, 112.0), cr, WHITE);
                draw_rectangle_lines(cr.x, cr.y, cr.w, cr.h, k.max(1.0), chrome::SILVER);
            }
            (None, None) => {
                chrome::text_box(cr);
                chrome::multiply(|| draw_rectangle(cr.x, cr.y, cr.w, cr.h, Color::new(0.62, 0.78, 0.5, 1.0)));
            }
            (Some(s), _) => {
                let affordable = game.mana >= game.cast_cost(s).mana;
                card_paper(cr, card_fill(affordable, hover));
            }
        }
        let icon = Rect::new(cr.x + 4.0 * k, cr.y + 4.0 * k, cr.h - 8.0 * k, cr.h - 8.0 * k);
        let tx = icon.x + icon.w + (cr.x + cr.w - icon.x - icon.w) / 2.0;
        let Some(s) = spell else {
            // The knot stamped in black: `MSign` less 192 taken off the paper (0x4dc118: 4768f8
            // then 476790 in mode 1 at (6, 6)).
            match chrome::win_scaled("MSign", [1024; 3], [-192; 3]) {
                Some(t) => chrome::subtractive(|| chrome::tex(&t, Rect::new(cr.x + 6.0 * k, cr.y + 6.0 * k, icon.w, icon.h), WHITE)),
                None => {
                    if let Some(t) = chrome::win_fx("MSign", chrome::Fx::Shade) {
                        chrome::tex(&t, icon, WHITE);
                    }
                }
            }
            let none = own("NoSpell", n_("No\nspells\nlearnt"));
            let lines: Vec<&str> = none.split('\n').collect();
            let size = 15.0 * k;
            let top = cr.y + cr.h / 2.0 - (lines.len() as f32 - 1.0) * 8.0 * k + size * 0.36;
            // Pressed into the paper as the original's (0x49bde4): white Benguiat taken off the
            // paper (the text box's mode 5, which leaves black) 2 px down and right and 1 px up
            // and left, then its font 0xae24b4 (Benguiat less 125, 140, 170) drawn over them.
            super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
                for (i, l) in lines.iter().enumerate() {
                    let y = top + i as f32 * 16.0 * k;
                    text_centered(l, tx + 2.0 * k, y + 2.0 * k, size, BLACK);
                    text_centered(l, tx - 1.0 * k, y - 1.0 * k, size, BLACK);
                    text_centered(l, tx, y, size, Color::from_rgba(130, 115, 85, 255));
                }
            });
            continue;
        };
        // The card under the pointer shows its picture brightened, when the hero can afford it
        // (4c2cf4 draws it through FUN_0048da5c with `selected` = 1; 49bde4 keeps it plain otherwise).
        let cost = game.cast_cost(s);
        let lit = hover && game.mana >= cost.mana && super::spell_badges::draw_picture_lit(s, icon);
        if !lit {
            chrome::spell_icon(s, icon);
        }
        // The book's cells are the original's spell cards (0x4d261c → 0x49bde4): the card
        // text of 0x49ba84 in the room right of the picture, red when unaffordable.
        let text = Rect::new(icon.x + icon.w, cr.y, cr.x + cr.w - icon.x - icon.w, cr.h);
        super::typography::Role::Body.with(|| card_text(game, s, text, true));
        if hover {
            if game.mana < cost.mana {
                tooltip(&[(own("NoManaForSpell", n_("Not enough mana\nto cast this spell!")).replace('\n', " "), chrome::RED_TEXT)]);
            }
            if clicked() {
                *selected = book_index.unwrap_or(0);
                cast = Some(s.clone());
            }
        }
    }
    // A spell on the hero's army is cast at once; an enemy spell closes the book and the map
    // asks for its target (0x4c2e34 → 0x4c2f39). Without the mana the click does nothing.
    let mut next = None;
    if let Some(s) = cast.filter(|s| game.mana >= game.cast_cost(s).mana) {
        if magic::targets_enemy(&s) {
            if game.foe.is_none() {
                world_view::begin_targeting(s.id);
                next = Some(Screen::WorldMap);
            }
        } else {
            next = self::cast(game, s.id, CastTarget::Own, message);
        }
    }
    if closed || key(KeyCode::Escape) || key(KeyCode::B) {
        next = next.or(Some(Screen::WorldMap));
    }
    if let Some(m) = message.as_ref().filter(|_| next.is_none()) {
        chrome::shadow_centered(m, r.center().x, r.y + r.h - 8.0 * k, 14.0 * k, ACCENT);
    }
    next.or(bar_pick)
}

/// The bottom row under the grid: the filter button at its left end (Razdor's choice, the
/// owner's 2026-10-22), and with `pages` > 1 the two arrows at the right and "N / M" between
/// them. The arrows, the wheel over the window, PageUp/PageDown and Left/Right turn `page` (the
/// wheel up is the previous page); the filter button cycles `filter`. Returns true when the
/// filter changed, so the caller starts its first page.
fn bottom_row(r: Rect, inner: Rect, gap: f32, k: f32, pages: usize, page: &mut usize, filter: &mut BookFilter) -> bool {
    use super::chrome;
    let row_h = 18.0 * k;
    let row_y = r.y + r.h - 44.0 * k;
    let fw = 150.0 * k;
    let mut changed = false;
    if button(inner.x + gap, row_y, fw, row_h, filter.label(), true) {
        *filter = filter.next();
        changed = true;
    }
    if pages <= 1 {
        return changed;
    }
    let aw = 26.0 * k;
    let next = Rect::new(inner.right() - gap - aw, row_y, aw, row_h);
    let prev = Rect::new(next.x - 6.0 * k - aw, row_y, aw, row_h);
    let mut delta: isize = 0;
    if button(prev.x, prev.y, prev.w, prev.h, "<", *page > 0) {
        delta -= 1;
    }
    if button(next.x, next.y, next.w, next.h, ">", *page + 1 < pages) {
        delta += 1;
    }
    if mouse_in(r.x, r.y, r.w, r.h) {
        let w = wheel();
        if w > 0.0 {
            delta -= 1;
        } else if w < 0.0 {
            delta += 1;
        }
    }
    if key(KeyCode::PageUp) || key(KeyCode::Left) {
        delta -= 1;
    }
    if key(KeyCode::PageDown) || key(KeyCode::Right) {
        delta += 1;
    }
    *page = page_turn(*page, pages, delta);
    let label = format!("{} / {}", *page + 1, pages);
    chrome::shadow_centered(&label, inner.x + inner.w / 2.0, row_y + row_h * 0.72, 13.0 * k, chrome::CREAM);
    changed
}

/// Casts `spell` on `target`: the hero reads it on the map, and it lands when the reading is
/// done. The map is next, or nothing when it cannot be cast (the message says why).
pub(super) fn cast(game: &mut Game, spell: u32, target: CastTarget, message: &mut Option<String>) -> Option<Screen> {
    let s = game.spell(spell)?.clone();
    match game.begin_cast(s.id, target) {
        Ok(outcome) => {
            cue(Cue::CastSpell);
            // The camera goes to the army the spell is for: the hero's own before the
            // reading, an enemy's after it (0x4c2e34, 0x4cc148); then the effect on it.
            let own = matches!(target, CastTarget::Own);
            if own {
                world_view::look_at(target);
            }
            if let Some(CastOutcome::Done { .. }) = outcome {
                if !own {
                    world_view::look_at(target);
                }
                world_view::spell_effect(&s, target);
            }
            razdor::diag::play(&game.clock.label(), &format!("CAST «{}» on {target:?}: {:.0} min", s.name, game.reading().map_or(0.0, |r| r.1)));
            *message = Some(match outcome {
                Some(o) => landed(&s, target, o),
                None => trf!("Reading «{name}»…", name = s.name),
            });
            Some(Screen::WorldMap)
        }
        Err(e) => {
            *message = Some(cast_error(e).into());
            None
        }
    }
}


/// A spell's card as the sanctuary and the event window show it (0x49bde4): the card paper
/// ([`card_paper`]), the spell's picture on the left (100 px, 6 px in; its own silver frame), and
/// to its right the card text ([`card_text`]). `tint_mana`: a spell over the hero's mana turns red, as
/// everywhere but the sanctuary's tab (0x49be2c skips the mana test on tab 4, 0x68dc88).
pub fn spell_card(game: &Game, s: &SpellDef, r: Rect, tint_mana: bool) {
    use super::chrome;
    super::typography::Role::Body.with(|| {
        let k = chrome::k();
        // The card's background (0x49bde4): red-brown over the hero's mana (49be19-49be42), the
        // normal paper otherwise; the sanctuary's tab never tints (`tint_mana` false).
        let red = tint_mana && game.mana < game.cast_cost(s).mana;
        card_paper(r, card_fill(!red, false));
        let side = (r.h - 12.0 * k).min(100.0 * k);
        let icon = Rect::new(r.x + 6.0 * k, r.y + (r.h - side) / 2.0, side, side);
        chrome::spell_icon(s, icon);
        let left = icon.x + icon.w + 2.0 * k;
        card_text(game, s, Rect::new(left, r.y, r.x + r.w - left, r.h), tint_mana);
    });
}

/// The card's fonts (0x49bde4 → 0x49ba84; the SanSerif tints of 0x4dbc26, white less a delta).
/// 0xae24c0, less (25, 25, 125): the effect text of a spell on the enemy; Razdor's bad entries.
const EFFECT_YELLOW: Color = Color::new(230.0 / 255.0, 230.0 / 255.0, 130.0 / 255.0, 1.0);
/// 0xae24cc, less (180, 70, 0): the effect text of a spell on the own army (Target 1); Razdor's good entries.
const EFFECT_BLUE: Color = Color::new(75.0 / 255.0, 185.0 / 255.0, 1.0, 1.0);
/// 0xae24c4, less (20, 90, 180): the cost and duration lines.
const GOLD: Color = Color::new(235.0 / 255.0, 165.0 / 255.0, 75.0 / 255.0, 1.0);
/// 0xae24c8, less (0, 152, 159): every line of a spell the hero cannot afford.
const UNAFFORDABLE: Color = Color::new(1.0, 103.0 / 255.0, 96.0 / 255.0, 1.0);
/// 0xae24a4, Benguiat less (0, 80, 170): the name of a spell the hero cannot afford.
const UNAFFORDABLE_NAME: Color = Color::new(1.0, 175.0 / 255.0, 85.0 / 255.0, 1.0);

/// The card text (0x49ba84) in `area`, centred in it both ways and word-wrapped to it as one
/// paragraph per part (the text list 0x47e46c breaks at the last space that fits): the name;
/// then, only when the spell has an effect text, an empty line, the effect text (0x49b63c,
/// [`razdor::rules::spell_hint::effect_text`] with the install's `[Skills]` labels), the
/// cost line (`Mana`, `Reading`, `Hour`) and the duration line (`MomentaryEffect`, or
/// `TimeOfEffect` with the hours and `Hours`). The effect is yellow, blue for a spell on the
/// own army (Razdor: each entry by its sign, a deviation); cost and duration orange. With `tint_mana` a spell over the hero's mana has all
/// its lines red, its name in the red Benguiat, and its effect is not turned blue
/// (0x49be2c, 0x49bb36).
fn card_text(game: &Game, s: &SpellDef, area: Rect, tint_mana: bool) {
    use super::chrome::{self, CREAM};
    use super::typography::Role;
    let k = chrome::k();
    let room = area.w - 12.0 * k;
    let tx = area.x + area.w / 2.0;
    let cost = game.cast_cost(s);
    let red = tint_mana && game.mana < cost.mana;
    let effect = razdor::rules::spell_hint::effect_entries(s, &super::spell_badges::label);
    let name_size = Role::Name.size(17.0 * k);
    let names = Role::Name.with(|| wrap(&s.name, room, name_size));
    let name_lh = Role::Name.line_height(18.0 * k);
    let size = Role::Body.size(12.0 * k);
    let line_h = Role::Body.line_height(13.0 * k);
    let mut lines: Vec<Vec<(String, Color)>> = Vec::new();
    // Without an effect text the card has the name alone (0x49bb0b).
    if !effect.is_empty() {
        // Deviation (the owner's decision, 2026-10-18, a player's request): each entry is blue
        // when it is good for the unit holding the spell and yellow when bad
        // (`spell_hint::good_for_holder`). The original turns the whole effect text blue for a
        // spell on the own army and leaves it yellow on the enemy (0x49bb36). Unaffordable,
        // all red as the original's.
        let ink = |good: bool| if red { UNAFFORDABLE } else if good { EFFECT_BLUE } else { EFFECT_YELLOW };
        let parts: Vec<(String, Color)> = effect.into_iter().map(|e| (e.text, ink(e.good))).collect();
        lines.extend(wrap_coloured(&parts, room, size));
        // The card shows the cost function of `TimeCast` in hours, not the real wait (0x49bbe0).
        let card_hours = game.card_cast_hours(s).max(0) as u64;
        let mana_line = format!("{} {}, {} {}", own("Mana", n_("Mana:")), cost.mana, own("Reading", n_("Reading:")), hours(card_hours * 60, false));
        let last = match Duration::of(s) {
            Duration::Instant => own("MomentaryEffect", n_("Instant effect")),
            Duration::Minutes(m) => format!("{} {}", own("TimeOfEffect", n_("Lasts:")), hours(m, true)),
        };
        let ink = if red { UNAFFORDABLE } else { GOLD };
        lines.extend(wrap(&mana_line, room, size).into_iter().map(|l| vec![(l, ink)]));
        lines.extend(wrap(&last, room, size).into_iter().map(|l| vec![(l, ink)]));
    }
    let gap = if lines.is_empty() { 0.0 } else { line_h };
    let block = names.len() as f32 * name_lh + gap + lines.len() as f32 * line_h;
    let top = area.y + ((area.h - block) / 2.0).max(3.0 * k);
    let name_ink = if red { UNAFFORDABLE_NAME } else { CREAM };
    Role::Name.with(|| {
        for (n, name) in names.iter().enumerate() {
            chrome::shadow_centered(name, tx, top + n as f32 * name_lh + name_size * 0.72, name_size, name_ink);
        }
    });
    let mut y = top + names.len() as f32 * name_lh + gap + size * 0.72;
    for line in lines {
        if y > area.y + area.h - 2.0 * k {
            break;
        }
        shadow_coloured(&line, tx, y, size, true);
        y += line_h;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_book_fills_each_column_before_the_next() {
        // 0x4dad08: column = n / 5, row = n % 5, so cell 1 is under cell 0, not beside it.
        assert_eq!(book_cell(0), (0, 0));
        assert_eq!(book_cell(1), (0, 1));
        assert_eq!(book_cell(4), (0, 4));
        assert_eq!(book_cell(5), (1, 0));
        assert_eq!(book_cell(14), (2, 4));
        let mut seen: Vec<(usize, usize)> = (0..BOOK_PAGE).map(book_cell).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), BOOK_PAGE, "every cell is its own place");
    }

    #[test]
    fn the_filter_lists_the_own_or_enemy_spells_in_book_order() {
        // `enemy[i]`: book spell i is cast on an enemy army.
        let enemy = [false, true, false, true, true];
        assert_eq!(shown(&enemy, BookFilter::All), [0, 1, 2, 3, 4]);
        assert_eq!(shown(&enemy, BookFilter::Own), [0, 2]);
        assert_eq!(shown(&enemy, BookFilter::Enemy), [1, 3, 4]);
        assert!(shown(&[], BookFilter::Own).is_empty(), "an empty book shows no spells");
    }

    #[test]
    fn the_filter_button_cycles_all_own_enemy() {
        assert_eq!(BookFilter::default(), BookFilter::All);
        assert_eq!(BookFilter::All.next(), BookFilter::Own);
        assert_eq!(BookFilter::Own.next(), BookFilter::Enemy);
        assert_eq!(BookFilter::Enemy.next(), BookFilter::All);
    }

    #[test]
    fn the_book_pages_15_spells_each() {
        // Razdor's choice (the owner's, 2026-10-22): an empty book still has its one page.
        assert_eq!(page_count(0), 1);
        assert_eq!(page_count(15), 1);
        assert_eq!(page_count(16), 2);
        assert_eq!(page_count(30), 2);
        assert_eq!(page_count(31), 3);
        // Turning clamps at the first and the last page.
        assert_eq!(page_turn(0, 1, 1), 0, "one page never turns");
        assert_eq!(page_turn(0, 3, -1), 0);
        assert_eq!(page_turn(0, 3, 1), 1);
        assert_eq!(page_turn(1, 3, 5), 2);
        assert_eq!(page_turn(2, 3, -2), 0);
    }

    #[test]
    fn the_background_follows_the_mana_then_the_pointer() {
        // 49be19-49be42: a card the hero cannot afford is the cannot-afford one whatever the
        // pointer does; the hover look is the selected background (4c2cf4, 0x4dc118).
        assert_eq!(card_fill(true, false), CardFill::Normal);
        assert_eq!(card_fill(true, true), CardFill::Hovered);
        assert_eq!(card_fill(false, false), CardFill::Unaffordable);
        assert_eq!(card_fill(false, true), CardFill::Unaffordable);
    }
}
