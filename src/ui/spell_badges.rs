//! The spell badges on the unit cards of the army, building and battle grids, and their
//! hint (original-mechanics/interface.md §9.4, 493a64, 49ece8): up to four round 22 px
//! pictures, 23 px apart, along the bottom of the portrait, one per running spell that costs
//! mana; hovering one shows a 420 px box with the spell's picture, name, effect, the unit's
//! life loss and the time left. The rules of what shows are in [`razdor::rules::spell_hint`].

use std::cell::RefCell;
use std::collections::HashMap;

use macroquad::prelude::*;

use razdor::dt::gfx::Image;
use razdor::i18n::{lang, n_, tr, Lang};
use razdor::rules::content::{Content, SpellDef};
use razdor::rules::spell_hint::{self, BADGE_PITCH, BADGE_SIZE, BADGE_TOP, HINT_WIDTH};
use razdor::rules::units::SpellSlot;

use super::chrome;
use super::widgets::{mouse_in, pointer, shadow_coloured, wrap_coloured};

/// What a hovered badge's hint shows.
struct Hint {
    spell: SpellDef,
    minutes_left: u64,
    drain: i32,
}

thread_local! {
    /// The hint of the badge under the pointer this frame, drawn on top by [`flush`].
    static PENDING: RefCell<Option<Hint>> = const { RefCell::new(None) };
    /// Composed pictures by spell id: (22 px badge, 100 px picture), `None` without art.
    static PICTURES: RefCell<HashMap<u32, Option<(Texture2D, Texture2D)>>> = RefCell::new(HashMap::new());
}

/// The badges of a unit whose portrait is `sq` (the original's 92 px square, scaled): its
/// spell `slots` at game minute `now`, `drain` its life loss (for the hint).
pub fn draw(sq: Rect, slots: &[Option<SpellSlot>], drain: i32, now: u64, content: &Content) {
    let s = sq.w / 92.0;
    for (i, (slot, spell)) in spell_hint::badge_spells(slots, now, |id| content.spell(id)).into_iter().enumerate() {
        let r = Rect::new(sq.x + i as f32 * BADGE_PITCH * s, sq.y + BADGE_TOP * s, BADGE_SIZE * s, BADGE_SIZE * s).round();
        // The original tests the pointer on 23 × 23 pixels (both ends included).
        let over = mouse_in(r.x, r.y, r.w + s, r.h + s);
        match pictures(spell) {
            // Hovered: added onto the card (476790 mode 0); else laid in through the
            // round `si-alpha` (476a3c).
            Some((badge, _)) if over => chrome::additive(|| chrome::tex(&badge, r, WHITE)),
            Some((badge, _)) => chrome::tex(&badge, r, WHITE),
            None => {
                let c = if spell_hint::on_own_army(spell) { chrome::BLUE_TEXT } else { chrome::RED_TEXT };
                let (cx, cy, rad) = (r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0);
                draw_circle(cx, cy, rad, Color::new(0.0, 0.0, 0.0, 0.8));
                draw_circle(cx, cy, rad * 0.7, if over { WHITE } else { c });
                draw_circle_lines(cx, cy, rad, 1.0, chrome::SILVER);
            }
        }
        if over {
            PENDING.with(|p| *p.borrow_mut() = Some(Hint { spell: spell.clone(), minutes_left: slot.until.saturating_sub(now), drain }));
        }
    }
}

/// [`RoundExt::round`] for rects: whole pixels, so the badges sit as crisp as the cards.
trait RoundExt {
    fn round(self) -> Self;
}

impl RoundExt for Rect {
    fn round(self) -> Rect {
        Rect::new(self.x.round(), self.y.round(), self.w.round(), self.h.round())
    }
}

/// A badge is under the pointer this frame (its hint will show).
pub fn hovered() -> bool {
    PENDING.with(|p| p.borrow().is_some())
}

/// Draws the hint of the badge hovered this frame, if any, over everything the screen drew.
pub fn flush() {
    let Some(h) = PENDING.with(|p| p.borrow_mut().take()) else { return };
    let k = chrome::k() * 0.9375;
    let (title, body) = ((20.0 * k).round(), (15.0 * k).round());
    let w = (HINT_WIDTH * k).round();
    let pic = 50.0 * k;
    let pad = 8.0 * k;
    let tx = pad * 2.0 + pic;
    let room = w - tx - pad;
    // Deviation (the owner's decision, 2026-10-18, a player's request): each effect entry is
    // blue when good for the unit holding the spell and red when bad
    // (`spell_hint::good_for_holder`), as the spell cards; the original colours the whole
    // effect text blue for a spell on the own army and red otherwise (49ece8).
    let parts: Vec<(String, Color)> = spell_hint::effect_entries(&h.spell, &label)
        .into_iter()
        .map(|e| (e.text, if e.good { chrome::BLUE_TEXT } else { chrome::RED_TEXT }))
        .collect();
    let mut lines: Vec<Vec<(String, Color)>> = wrap_coloured(&parts, room, body);
    if let Some(l) = spell_hint::life_lost_line(&h.spell, h.drain, &label) {
        lines.push(vec![(l, chrome::RED_TEXT)]);
    }
    lines.push(vec![(spell_hint::time_left_line(h.minutes_left, &label), TIME_TEXT)]);
    let text_h = title + 6.0 * k + lines.len() as f32 * (body + 3.0 * k);
    let hh = (text_h.max(pic) + 2.0 * pad).round();
    // 21 px right of and 37 px below the pointer, flipped to the other side when it would
    // leave the screen, then kept inside (interface.md §10).
    let (mx, my) = pointer();
    let mut x = mx + 21.0 * k;
    if x + w > screen_width() {
        x = mx - 21.0 * k - w;
    }
    let mut y = my + 37.0 * k;
    if y + hh > screen_height() {
        y = my - 37.0 * k - hh;
    }
    let r = Rect::new(x.clamp(0.0, (screen_width() - w).max(0.0)).round(), y.clamp(0.0, (screen_height() - hh).max(0.0)).round(), w, hh);
    draw_rectangle(r.x + 6.0 * k, r.y + 6.0 * k, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.4));
    chrome::surface_alpha(r, chrome::Skin::Strip, 0.95);
    chrome::silver_frame(r, 1.0);
    let pr = Rect::new(r.x + pad, r.y + (r.h - pic) / 2.0, pic, pic);
    match pictures(&h.spell) {
        Some((_, picture)) => chrome::tex(&picture, pr, WHITE),
        None => chrome::spell_icon(&h.spell, pr),
    }
    let mut ty = r.y + (r.h - text_h) / 2.0 + title * 0.85;
    chrome::shadow_text(&h.spell.name, r.x + tx, ty, title, chrome::CREAM);
    ty += 6.0 * k + body;
    for l in lines {
        shadow_coloured(&l, r.x + tx, ty, body, false);
        ty += body + 3.0 * k;
    }
}

/// The time line's pale yellow (the original's font 0xae24c0).
const TIME_TEXT: Color = Color::new(0.96, 0.88, 0.55, 1.0);

/// A label or word of the hint by its key in the install's interface ini (`[Skills]`,
/// `[Time]`) in Russian, else ours. The item text (`ui::item_text`) takes its labels here too.
pub(super) fn label(key: &'static str) -> String {
    let section = if key.starts_with('c') { "Time" } else { "Skills" };
    let own = (lang() == Lang::Ru).then(|| chrome::ui_text(section, key)).flatten();
    own.unwrap_or_else(|| tr(english(key)).to_string())
}

/// Our own text for an ini key of the hint.
fn english(key: &str) -> &'static str {
    match key {
        "CureHit" => n_("Heals (+ hits)"),
        "CurseHit" => n_("Lowers hits (- hits)"),
        "SHit" => n_("Hits"),
        "SAttackBlow" => n_("Melee attack"),
        "SAttackShot" => n_("Ranged attack"),
        "SDefenceBlow" => n_("Melee defence"),
        "SDefenceShot" => n_("Ranged defence"),
        "SPhysicalAttack" => n_("Physical attack"),
        "SPhysicalDefence" => n_("Physical defence"),
        "SMagicPower" => n_("Magic power"),
        "SProtectAllMagic" => n_("Magic immunity"),
        "SProtectLife" => n_("Life magic protection"),
        "SProtectElemental" => n_("Elemental magic protection"),
        "SProtectDeath" => n_("Death magic protection"),
        "SRegen" => n_("Regeneration"),
        "SPoison" => n_("Poison (- hits)"),
        "SVampirizm" => n_("Vampirism"),
        "SInitiative" => n_("Initiative"),
        "SManevres" => n_("Actions"),
        "LifeLost" => n_("Drains life"),
        // Razdor's deviation for p-LifeLose (rules::spell_hint::effect_entries).
        "LeaderMaxLife" => n_("Leader's max life"),
        "SCurrentHit" => n_("Current hits"),
        "Resurrect" => n_("Raises from the dead"),
        "GetBonus" => n_("Gives the bonus"),
        "OnlyForWarriors" => n_("warriors only"),
        "OnlyForBowmans" => n_("archers only"),
        "OnlyForMages" => n_("mages only"),
        "NotForUse" => n_("dark forces only"),
        // A staff's school (Razdor's deviation, see `rules::item_text::usage_key`).
        "OnlyForLifeMages" => n_("life mages only"),
        "OnlyForElementalMages" => n_("elemental mages only"),
        "OnlyForDeathMages" => n_("death mages only"),
        "RemainedTimeOfEffect" => n_("Time left:"),
        "RemainedTimeOfEffectAll" => n_("Unknown"),
        "cMounth" => n_("month"),
        "cDay" => n_("day"),
        "cHour" => n_("hour"),
        "cLessAtHour" => n_("less than an hour"),
        _ => "?",
    }
}

/// The spell's badge and its 100 px picture, composed once from the install's art.
fn pictures(spell: &SpellDef) -> Option<(Texture2D, Texture2D)> {
    if let Some(p) = PICTURES.with(|m| m.borrow().get(&spell.id).cloned()) {
        return p;
    }
    let made = compose_from_install(spell).and_then(|(badge, picture)| Some((texture(&badge)?, texture(&picture)?)));
    PICTURES.with(|m| m.borrow_mut().insert(spell.id, made.clone()));
    made
}

/// Draw the original framed 100×100 spell picture in a menu or event window.
pub fn draw_picture(spell: &SpellDef, r: Rect) -> bool {
    let Some((_, picture)) = pictures(spell) else { return false };
    chrome::tex(&picture, r, WHITE);
    true
}

fn texture(img: &Image) -> Option<Texture2D> {
    let (w, h) = (u16::try_from(img.width).ok()?, u16::try_from(img.height).ok()?);
    let t = Texture2D::from_rgba8(w, h, &img.rgba);
    t.set_filter(FilterMode::Linear);
    Some(t)
}

fn compose_from_install(spell: &SpellDef) -> Option<(Image, Image)> {
    let mut layers = Vec::new();
    for icon in &spell.icons {
        let Some(name) = icon.image.as_deref() else { continue };
        let mut img = chrome::image(&format!("Spells/{name}.lit"))?;
        chrome::subtract(&mut img, icon.tint.unwrap_or([0; 3]));
        layers.push(img);
    }
    let w = |n: &str| chrome::image(&format!("Windows/{n}.lit"));
    let art = BadgeArt {
        icon_mask: w("Spell-IconMask")?,
        icon_alpha: w("Spell-IconAlpha")?,
        frame: w("Spell-Frame")?,
        frame_alpha: w("Spell-FrameAlpha")?,
        si_mask: w("si-mask")?,
        si_border: w("si-border")?,
        si_alpha: w("si-alpha")?,
    };
    Some(compose(&layers, &art))
}

/// The windows art a spell picture and its badge are made with (0x4e36cc's loads).
pub struct BadgeArt {
    pub icon_mask: Image,
    pub icon_alpha: Image,
    pub frame: Image,
    pub frame_alpha: Image,
    pub si_mask: Image,
    pub si_border: Image,
    pub si_alpha: Image,
}

/// A spell's 22 px badge and its 100 px picture as 49ac64 makes them:
/// - the 100 px picture: the `Icon1..3` layers (each already less its `ColorC`) added
///   together, `Spell-IconMask` taken off (subtractive: the corners go black), the silver
///   `Spell-Frame` laid in through `Spell-FrameAlpha`;
/// - the badge: that picture shrunk to 20 px at (1, 1) of a black 22 px square, the black
///   corners of `si-mask` (its white is the colour key) and the `si-border` ring over it;
///   its alpha is `si-alpha`, the round mask the card draw (493a64) lays it in with.
pub fn compose(layers: &[Image], art: &BadgeArt) -> (Image, Image) {
    let mut pic = Image { width: 100, height: 100, rgba: vec![0; 100 * 100 * 4] };
    for layer in layers {
        each(&mut pic, |x, y, p| {
            let s = px(layer, x, y);
            for c in 0..3 {
                p[c] = p[c].saturating_add(s[c]);
            }
        });
    }
    each(&mut pic, |x, y, p| {
        let m = px(&art.icon_mask, x, y);
        for c in 0..3 {
            p[c] = p[c].saturating_sub(m[c]);
        }
    });
    each(&mut pic, |x, y, p| {
        let a = (px(&art.frame_alpha, x, y)[0] >> 3) as u32;
        let f = px(&art.frame, x, y);
        for c in 0..3 {
            p[c] = (p[c] as u32 * (31 - a) / 31 + f[c] as u32).min(255) as u8;
        }
        // Type-8 spell layers can have an opaque purple matte in their decoded RGB.
        // Spell-IconAlpha is the separate cutout; combine it with frame coverage so the
        // silver frame remains visible around the icon.
        let icon_a = px(&art.icon_alpha, x, y)[0] as u32;
        let frame_a = a * 255 / 31;
        p[3] = (frame_a + icon_a * (255 - frame_a) / 255) as u8;
    });
    let mut badge = Image { width: 22, height: 22, rgba: vec![0; 22 * 22 * 4] };
    each(&mut badge, |x, y, p| {
        if (1..21).contains(&x) && (1..21).contains(&y) {
            // A 5 × 5 average of the picture.
            let mut sum = [0u32; 3];
            for dy in 0..5 {
                for dx in 0..5 {
                    let s = px(&pic, (x - 1) * 5 + dx, (y - 1) * 5 + dy);
                    for c in 0..3 {
                        sum[c] += s[c] as u32;
                    }
                }
            }
            for c in 0..3 {
                p[c] = (sum[c] / 25) as u8;
            }
        }
        let m = px(&art.si_mask, x, y);
        if m[..3].iter().any(|&v| v < 248) {
            p[..3].copy_from_slice(&m[..3]);
        }
        let b = px(&art.si_border, x, y);
        let a = b[3] as u32;
        for c in 0..3 {
            p[c] = ((p[c] as u32 * (255 - a) + b[c] as u32 * a) / 255) as u8;
        }
        p[3] = px(&art.si_alpha, x, y)[0];
    });
    (badge, pic)
}

/// The pixel at (x, y), black outside the image (art of an unexpected size).
fn px(img: &Image, x: u32, y: u32) -> [u8; 4] {
    if x < img.width && y < img.height {
        img.pixel(x, y)
    } else {
        [0, 0, 0, 0]
    }
}

fn each(img: &mut Image, mut f: impl FnMut(u32, u32, &mut [u8])) {
    let w = img.width;
    for (i, p) in img.rgba.chunks_exact_mut(4).enumerate() {
        f(i as u32 % w, i as u32 / w, p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, p: [u8; 4]) -> Image {
        Image { width: w, height: h, rgba: p.repeat((w * h) as usize) }
    }

    fn art() -> BadgeArt {
        // A mask white inside, black at the corner pixel (0, 0); no frame; a ring-less border.
        let mut si_mask = solid(22, 22, [255, 255, 255, 255]);
        si_mask.rgba[..4].copy_from_slice(&[0, 0, 0, 255]);
        let mut si_alpha = solid(22, 22, [255, 255, 255, 255]);
        si_alpha.rgba[..4].copy_from_slice(&[0, 0, 0, 255]);
        BadgeArt {
            icon_mask: solid(100, 100, [0, 0, 0, 255]),
            icon_alpha: solid(100, 100, [255, 255, 255, 255]),
            frame: solid(100, 100, [0, 0, 0, 255]),
            frame_alpha: solid(100, 100, [0, 0, 0, 255]),
            si_mask,
            si_border: solid(22, 22, [0, 0, 0, 0]),
            si_alpha,
        }
    }

    #[test]
    fn layers_add_up_and_the_badge_is_the_picture_shrunk_inside_its_border() {
        let layers = [solid(100, 100, [100, 20, 0, 255]), solid(100, 100, [200, 30, 0, 255])];
        let (badge, pic) = compose(&layers, &art());
        assert_eq!(pic.pixel(50, 50), [255, 50, 0, 255], "added, saturating");
        assert_eq!(badge.pixel(10, 10), [255, 50, 0, 255]);
        assert_eq!(badge.pixel(21, 10), [0, 0, 0, 255], "the 1 px edge stays black");
        assert_eq!(badge.pixel(0, 0), [0, 0, 0, 0], "si-mask's corner, si-alpha's 0");
    }

    #[test]
    fn the_icon_mask_takes_off_and_the_frame_lays_in() {
        let mut a = art();
        a.icon_mask = solid(100, 100, [255, 255, 255, 255]);
        let (_, pic) = compose(&[solid(100, 100, [200, 200, 200, 255])], &a);
        assert_eq!(pic.pixel(5, 5), [0, 0, 0, 255], "subtractive mask");
        let mut a = art();
        a.frame = solid(100, 100, [90, 90, 90, 255]);
        a.frame_alpha = solid(100, 100, [255, 255, 255, 255]);
        let (_, pic) = compose(&[solid(100, 100, [200, 200, 200, 255])], &a);
        assert_eq!(pic.pixel(5, 5), [90, 90, 90, 255], "full alpha: the frame alone");
    }
}
