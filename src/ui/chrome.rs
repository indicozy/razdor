//! The original's window art: marble and parchment surfaces, title bars, silver frames,
//! buttons, card frames, empty-cell icons, resource icons and the battle's spell
//! animations. Everything is decoded at runtime from the player's install
//! (`Graphics/Windows`, `Graphics/Spells`, `Graphics/Battle`) and cached as textures; nothing
//! decoded is written anywhere. Without an install each piece falls back to our own
//! placeholder style with the same layout.
//!
//! Layout numbers of the screens that follow the original closely (battle, bottom bar) are
//! in pixels of the 960×720 gameplay video (`docs/reference/video-notes.md`), scaled by [`k`].

use razdor::local;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use macroquad::prelude::*;

use razdor::dt::gfx::{self, Image};
use razdor::dt::ini::Ini;
use razdor::dt::preload;
use razdor::dt::install::{find_path, SETTINGS_FILE};

use super::widgets::{measure, text};

/// How a decoded picture is turned into a texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fx {
    /// As stored.
    Plain,
    /// Black is transparent (frames drawn over a black middle).
    KeyBlack,
    /// Additive art (spell animations, glyph icons): brightness becomes alpha.
    Glow,
    /// Greyed out (disabled buttons).
    Grey,
    /// A dark overlay whose opacity is the picture's brightness (ornaments on parchment).
    Shade,
    /// Green/blue swap and black transparency key for the barracks' green hire pill.
    KeyBlackSwapGreenBlue,
    /// The colour of the bottom-left pixel is transparent, compared at 16 bits (the ability
    /// icons, [`gfx::key_bottom_left_565`]).
    KeyCorner,
    /// Warm red button palette, preserving the silver outer edge of the original art.
    ButtonRed,
    /// Red takes green, green takes blue, blue takes red (0x48dd40 with 2, 3, 1), then
    /// (0, 64, 128) taken off inside the 1 px edge (0x4768f8 subtracting over (1,1)–(w−1,h−1)):
    /// the green buttons made the defeat window's dark red ones with their silver rim (0x4dba90).
    RotateRgb,
    /// The new-hero window's portraits ([`hero_portrait`]).
    HeroPicked,
    HeroOffered,
    HeroLocked,
}

struct Chrome {
    dir: PathBuf,
    /// By path, then by [`Fx`] (its index).
    textures: HashMap<String, [Option<Option<Texture2D>>; 12]>,
    /// By key: the tuning it was made with and the texture (`None`: its file is missing).
    visual_textures: HashMap<&'static str, ([f32; 4], Option<Texture2D>)>,
    /// The pictures of [`visual_texture`] as decoded, by path: the light and the brown paper
    /// are one file, and a new tuning makes its texture again.
    visual_images: HashMap<String, Option<Image>>,
    animations: HashMap<String, Option<Vec<Texture2D>>>,
    texts: Option<Option<Ini>>,
    /// The data language's `<Prefix>_DiscordTimes.ini` made by razdor-translate (for English
    /// the install's, else Razdor's copy), by the prefix it was read for
    /// (`razdor::dt::english`, `super::language::data_prefix`).
    english_texts: Option<(String, Option<Ini>)>,
    /// `Eng_DiscordTimes.ini` for what another language's file lacks.
    english_fallback: Option<Option<Ini>>,
}

local! {
    static CHROME: RefCell<Option<Chrome>> = const { RefCell::new(None) };
}

/// Use the art of the install in `dir` from now on.
pub fn set_install(dir: PathBuf) {
    let ahead: Vec<(PathBuf, &'static str)> =
        WINDOWS_AHEAD.iter().filter_map(|n| Some((find_path(&dir, &format!("Graphics/{n}")).ok()?, *n))).collect();
    // The decode store decodes them in order, in the background; the lit ones are uploaded
    // a few per frame ([`warm`]) so that their first use finds the texture made.
    preload::preload(ahead.iter().map(|(p, _)| p.clone()).collect());
    WARM.with(|w| *w.borrow_mut() = ahead.into_iter().filter(|(_, n)| n.ends_with(".lit")).map(|(p, n)| (p, n.to_string())).collect());
    CHROME.with(|c| {
        *c.borrow_mut() = Some(Chrome { dir, textures: HashMap::new(), visual_textures: HashMap::new(), visual_images: HashMap::new(), animations: HashMap::new(), texts: None, english_texts: None, english_fallback: None })
    });
}

/// The pictures the windows decode when they first open, the slow ones (over 2 ms each with
/// `RAZDOR_PROFILE`), decoded ahead at the start ([`preload::preload`]): the menus and the title
/// (Castle, the flames, the exit window), the window frames and buttons, the fonts and cursors,
/// the battle's art, the paper (army, building, journal), the army window's frames and its
/// portrait and icon sheets, the load window's book, the building window's tabs.
const WINDOWS_AHEAD: &[&str] = &[
    "Windows/Win-paper.lit",
    "Windows/Win-marble.lit",
    "Windows/Win-red.lit",
    "Windows/Win2a.lit",
    "Windows/Castle.lit",
    "Windows/TitleFlame_RUS.lit",
    "Windows/MenuFlame.lit",
    "Windows/S_Exit_RUS.lit",
    "Windows/Title_RUS.ugs",
    "Windows/SMText_RUS.lit",
    "Windows/Cursor-Normal.lit",
    "Windows/Cursor-Denied.lit",
    "Windows/Selection-0.lit",
    "Windows/Selection-1.lit",
    "Windows/SanSerif.lit",
    "Windows/SanSerif_Shadow.lit",
    "Windows/SanSerif_Bold.lit",
    "Windows/SanSerif_Bold_Shadow.lit",
    "Windows/Benguiat.lit",
    "Windows/Benguiat_Shadow.lit",
    "Windows/SteelLine.lit",
    "Windows/Corner-Left.lit",
    "Windows/Corner-Right.lit",
    "Windows/DownCorner.lit",
    "Windows/Corner_Frame-LU.lit",
    "Windows/Corner_Frame-RU.lit",
    "Windows/Corner_Frame-LD.lit",
    "Windows/Corner_Frame-RD.lit",
    "Windows/Big_Frame_1a.lit",
    "Windows/Big_Frame_2a.lit",
    "Windows/VSTexture.lit",
    "Windows/VSlider.lit",
    "Windows/VArrowUp.lit",
    "Windows/VArrowDown.lit",
    "Windows/LSign-Delete.lit",
    "Windows/LSign-Load.lit",
    "Windows/CloseButtonGreen-Up.lit",
    "Windows/CloseButtonRed-Up.lit",
    "Windows/load-button-1.lit",
    "Windows/load-button-2.lit",
    "Windows/Btn1Up.lit",
    "Windows/Btn1Down.lit",
    "Windows/Btn2Up.lit",
    "Windows/Btn2Down.lit",
    "Windows/Btn3Up.lit",
    "Windows/Btn3Down.lit",
    "Windows/MLBtn1Up.lit",
    "Windows/MLBtn2Up.lit",
    "Windows/MLBtn2Down.lit",
    "Windows/MRBtn1Up.lit",
    "Windows/MRBtn2Up.lit",
    "Windows/GP-ButtonLeft.lit",
    "Windows/GP-Button-Alpha.lit",
    "Windows/GP-ButtonCenter.lit",
    "Windows/GP-ButtonCenter-Alpha.lit",
    "Windows/GP-ButtonRight.lit",
    "Windows/button-icon-1.lit",
    "Windows/button-icon-2.lit",
    "Windows/button-icon-3.lit",
    "Windows/button-icon-4.lit",
    "Windows/button-icon-5.lit",
    "Windows/button-icon-6.lit",
    "Windows/button-icon-7.lit",
    "Windows/button-icon-8.lit",
    "Windows/Res-Magic.lit",
    "Windows/Res-Money.lit",
    "Windows/Res-Income.lit",
    "Windows/Res-Payment.lit",
    "Windows/res0.lit",
    "Windows/res1.lit",
    "Windows/Shadow-Empty.lit",
    "Windows/Shadow-Unit.lit",
    "Windows/wb1.lit",
    "Windows/wb2.lit",
    "Windows/wb3.lit",
    "Windows/Swords.ugs",
    "Windows/Way_Arrows.ugs",
    "Windows/Clock.ugs",
    "Windows/mb2.ugs",
    "Windows/Bonus4.lit",
    "Windows/Bonus5.lit",
    "Windows/Bonus7.lit",
    "Windows/Bonus12.lit",
    "Windows/Bonus-2Row.lit",
    "Windows/Bonus-InCastle.lit",
    "Windows/HeroBonus1.lit",
    "Windows/sign-curse.lit",
    "Windows/army-1.lit",
    "Windows/army-3.lit",
    "Windows/army-5.lit",
    "Windows/army-alpha.lit",
    "Windows/exp.lit",
    "Windows/exp-hole.lit",
    "Battle/--PAR.ugs",
    "Battle/--KUSKI.ugs",
    "Battle/--KUSKIBIG.ugs",
    "Objects/Persones.ugs",
    "Windows/UnitFrame.lit",
    "Windows/Inventory.lit",
    "Windows/UpgradeTree.lit",
    "Objects/Icons.ugs",
    "Objects/Items.ugs",
    "Windows/S_Save.lit",
    "Windows/HintFrame.lit",
    "Windows/TB-1_RUS.lit",
    "Windows/TB-2_RUS.lit",
    "Windows/TB-3_RUS.lit",
    "Windows/TB-4_RUS.lit",
    "Windows/TB-5_RUS.lit",
    "Windows/TB-6_RUS.lit",
    "Windows/TBd1_RUS.lit",
    "Windows/TBd2_RUS.lit",
    "Windows/TBd3_RUS.lit",
    "Windows/TBd4_RUS.lit",
    "Windows/TBd5_RUS.lit",
    "Windows/TBd6_RUS.lit",
];

// Lit pictures of [`WINDOWS_AHEAD`] whose textures [`warm`] makes ahead: the path and the name
// as [`art`] takes it.
local! {
    static WARM: RefCell<std::collections::VecDeque<(PathBuf, String)>> = RefCell::new(Default::default());
}

/// Uploads the textures of pictures the decode store has ready, a few per frame, within
/// `budget_ms`, so that the first use of each finds it made (see
/// [`preload::upload_within_budget`]). Pictures not decoded yet are kept for later frames.
pub fn warm(budget_ms: f64) {
    // Snapshot runs make their textures on demand, as before, so their frames stay the same.
    if std::env::var_os("RAZDOR_SNAPSHOT").is_some() {
        return;
    }
    let start = std::time::Instant::now();
    WARM.with(|w| {
        let mut queue = std::mem::take(&mut *w.borrow_mut());
        if queue.is_empty() {
            return;
        }
        preload::upload_within_budget(
            &mut queue,
            |(path, _)| preload::is_ready(path),
            |(_, rel)| {
                let _ = art(rel, Fx::Plain);
            },
            || start.elapsed().as_secs_f64() * 1000.0,
            budget_ms,
        );
        *w.borrow_mut() = queue;
    });
}

// ------------------------------------------------------------------------------------------
// Pixel transforms (pure)
// ------------------------------------------------------------------------------------------

fn lum(p: &[u8]) -> u32 {
    (p[0] as u32 * 30 + p[1] as u32 * 59 + p[2] as u32 * 11) / 100
}

/// Takes `minus` (r, g, b) off every pixel of `img`, down to black: a spell layer's
/// `ColorC`.
pub fn subtract(img: &mut Image, minus: [i32; 3]) {
    for p in img.rgba.chunks_exact_mut(4) {
        for (c, m) in p[..3].iter_mut().zip(minus) {
            *c = (*c as i32 - m).clamp(0, 255) as u8;
        }
    }
}

/// Scales and shifts every channel of `img` as the original's loaders and 48da5c do:
/// `c·k/1024 + offset`, clamped to a byte (r, g, b).
pub fn scale_channels(img: &mut Image, k: [i32; 3], offset: [i32; 3]) {
    for p in img.rgba.chunks_exact_mut(4) {
        for ((c, k), o) in p[..3].iter_mut().zip(k).zip(offset) {
            *c = (*c as i32 * k / 1024 + o).clamp(0, 255) as u8;
        }
    }
}

/// The promotion screen's locked portrait (494340), for a unit that cannot be promoted (level
/// 0 in the original, or no next type): grey weighted 100/200/100 (48db5c), each channel
/// then scaled by 1600/1024 and shifted by −48 red, −176 green, −256 blue (48da5c: a dark
/// brown), and the 92×92 vignette 0xae26b8 subtracted (476790 mode 1): squares inset by d
/// px, d = 0..18, filled with grey 96 − d·5334/1000, so the edge loses 96 and the middle
/// nothing. The original's 16-bit colour depth is left out.
pub fn lock_portrait(img: &mut Image) {
    let (w, h) = (img.width as i32, img.height as i32);
    for (n, p) in img.rgba.chunks_exact_mut(4).enumerate() {
        let (x, y) = (n as i32 % w.max(1), n as i32 / w.max(1));
        // The vignette is the original's 92 px square; a portrait of another size is mapped.
        let (vx, vy) = (x * 92 / w.max(1), y * 92 / h.max(1));
        let d = vx.min(vy).min(91 - vx).min(91 - vy).min(18);
        let vignette = 96 - d * 5334 / 1000;
        let grey = (p[0] as i32 * 100 + p[1] as i32 * 200 + p[2] as i32 * 100) / 400;
        let scaled = (grey * 1600) >> 10;
        for (c, off) in p[..3].iter_mut().zip([-48, -176, -256]) {
            let v = (scaled + off).clamp(0, 255);
            *c = (v - vignette).max(0) as u8;
        }
    }
}

/// Applies `fx` to `img` in place.
pub fn transform(img: &mut Image, fx: Fx) {
    let (w, h) = (img.width as usize, img.height as usize);
    for (i, p) in img.rgba.chunks_exact_mut(4).enumerate() {
        match fx {
            Fx::Plain => {}
            Fx::ButtonRed => {
                // A colour multiplication loses the green marble's bright channel and
                // stains its silver edge. Move that channel into red instead, and keep
                // the one-pixel outer rim in its original colour.
                let (x, y) = (i % w, i / w);
                if x > 0 && y > 0 && x + 1 < w && y + 1 < h {
                    let (r, g) = (p[0] as i32, p[1] as i32);
                    p[0] = (g + 5).min(255) as u8;
                    p[1] = (r * 88 / 100 - 36).clamp(0, 255) as u8;
                    p[2] = (r * 35 / 100 - 40).clamp(0, 255) as u8;
                } else {
                    let silver = p[1];
                    p[..3].fill(silver);
                }
            }
            Fx::KeyBlack | Fx::KeyBlackSwapGreenBlue => {
                let m = p[0].max(p[1]).max(p[2]) as u32;
                let a = (m.saturating_sub(10) * 16).min(255);
                p[3] = p[3].min(a as u8);
                if fx == Fx::KeyBlackSwapGreenBlue {
                    p.swap(1, 2);
                }
            }
            Fx::Glow => {
                let m = p[0].max(p[1]).max(p[2]);
                if m == 0 {
                    p[3] = 0;
                } else {
                    for c in &mut p[..3] {
                        *c = (*c as u32 * 255 / m as u32) as u8;
                    }
                    p[3] = m;
                }
            }
            Fx::Grey => {
                let l = (lum(p) * 9 / 10) as u8;
                p[0] = l;
                p[1] = l;
                p[2] = l;
            }
            Fx::Shade => {
                let a = lum(p) as u8;
                p[..3].copy_from_slice(&[20, 10, 4]);
                p[3] = a.min(p[3]);
            }
            Fx::RotateRgb => {
                let (r, g, b) = (p[0], p[1], p[2]);
                p[..3].copy_from_slice(&[g, b, r]);
            }
            Fx::KeyCorner | Fx::HeroPicked | Fx::HeroOffered | Fx::HeroLocked => {}
        }
    }
    match fx {
        Fx::RotateRgb => darken_inside_edge(img, [0, 64, 128]),
        Fx::KeyCorner => gfx::key_bottom_left_565(img),
        Fx::HeroPicked => hero_portrait(img, HeroPortrait::Picked),
        Fx::HeroOffered => hero_portrait(img, HeroPortrait::Offered),
        Fx::HeroLocked => hero_portrait(img, HeroPortrait::Locked),
        _ => {}
    }
}

/// Which of the new-hero window's three pictures of a class portrait (0x4d7d30).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeroPortrait {
    /// The picked class: the picture brightened (×1100/1024).
    Picked,
    /// Another class the map offers: the blue copy (`65bcd8`).
    Offered,
    /// A class the map leaves out: the dark red one (`65bce4`).
    Locked,
}

/// The new-hero window's class portraits as the original builds them once, at 16 bits per
/// pixel (0x4d7d30, for each of `Hero0`, `hero1`, `hero2`, loaded without their alpha by
/// 0x48d02c):
/// 1. the 160 × 160 vignette taken off the picture (0x476790 mode 1): squares inset by i px,
///    i = 0..32, filled with grey `(32 − i)·2`, so the edge loses 64 and from 32 px in nothing;
/// 2. two copies made of that (0x476594); the picture itself scaled by 1100/1024 (0x48da5c);
/// 3. each copy turned grey, `(100 r + 200 g + 100 b) / 400` (0x48db5c), then scaled and
///    shifted per channel (0x48da5c): the blue one `g·1300/1024 − (256, 80, 48)`, the dark one
///    `g·1600/1024 − (48, 176, 256)`, each clamped to a byte, so black stays black and the
///    light parts take the colour;
/// 4. the vignette taken off each copy once more (0x476790 mode 1).
///
/// Every step reads the 16-bit pixel widened back to bytes (0x4763b8) and writes it through
/// 0x476374's 5-6-5 tables.
pub fn hero_portrait(img: &mut Image, which: HeroPortrait) {
    let (w, h) = (img.width as i32, img.height as i32);
    // 0x4763b8: a 5-bit channel shifted up by 3 gains its top bit once more; a 6-bit one not.
    let widen = |p: &mut [u8]| {
        let (r, g, b) = (p[0] >> 3 << 3, p[1] >> 2 << 2, p[2] >> 3 << 3);
        p[0] = r + (r >> 7);
        p[1] = g;
        p[2] = b + (b >> 7);
    };
    let vignette = |x: i32, y: i32| {
        // The squares are the original's 160 px; a picture of another size keeps that border.
        let d = x.min(y).min(w - 1 - x).min(h - 1 - y).clamp(0, 32);
        let mut grey = [((32 - d) * 2) as u8; 3];
        widen(&mut grey);
        grey
    };
    let take_vignette = |p: &mut [u8], x: i32, y: i32| {
        let v = vignette(x, y);
        for (c, v) in p[..3].iter_mut().zip(v) {
            *c = c.saturating_sub(v);
        }
        widen(p);
    };
    for (n, p) in img.rgba.chunks_exact_mut(4).enumerate() {
        let (x, y) = (n as i32 % w.max(1), n as i32 / w.max(1));
        widen(p);
        take_vignette(p, x, y);
        let (k, shift) = match which {
            HeroPortrait::Picked => {
                for c in &mut p[..3] {
                    *c = ((*c as i32 * 1100) >> 10).min(255) as u8;
                }
                widen(p);
                p[3] = 255;
                continue;
            }
            HeroPortrait::Offered => (1300, [-256, -80, -48]),
            HeroPortrait::Locked => (1600, [-48, -176, -256]),
        };
        let grey = ((p[0] as i32 * 100 + p[1] as i32 * 200 + p[2] as i32 * 100) / 400).min(255) as u8;
        p[..3].fill(grey);
        widen(p);
        for (c, s) in p[..3].iter_mut().zip(shift) {
            *c = (((*c as i32 * k) >> 10) + s).clamp(0, 255) as u8;
        }
        widen(p);
        take_vignette(p, x, y);
        p[3] = 255;
    }
}

/// Takes `rgb` off every pixel but the outermost row and column on each side, down to 0
/// (0x4768f8 in its subtracting mode, from (1, 1) to (w − 1, h − 1)).
fn darken_inside_edge(img: &mut Image, rgb: [u8; 3]) {
    let (w, h) = (img.width as usize, img.height as usize);
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let p = &mut img.rgba[(y * w + x) * 4..][..3];
            for (c, d) in p.iter_mut().zip(rgb) {
                *c = c.saturating_sub(d);
            }
        }
    }
}

fn to_texture(img: &Image) -> Option<Texture2D> {
    let (w, h) = (u16::try_from(img.width).ok()?, u16::try_from(img.height).ok()?);
    let t = Texture2D::from_rgba8(w, h, &img.rgba);
    t.set_filter(FilterMode::Linear);
    Some(t)
}

/// The decoded first frame of `Graphics/<rel>`, without an install `None`.
pub fn image(rel: &str) -> Option<Image> {
    let dir = CHROME.with(|c| c.borrow().as_ref().map(|c| c.dir.clone()))?;
    find_path(&dir, &format!("Graphics/{rel}"))
        .and_then(|p| preload::decode_shared(&p))
        .map_err(|e| razdor::diag!("Discord Times art: {rel}: {e}"))
        .ok()
        .and_then(|frames| frames.first().cloned())
}

/// Picture `rel` under `Graphics/` (e.g. `"Windows/Win-red.lit"`), first frame, with `fx`.
/// `None` without an install or when the file is missing (logged once).
pub fn art(rel: &str, fx: Fx) -> Option<Texture2D> {
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if let Some(t) = c.textures.get(rel).and_then(|v| v[fx as usize].as_ref()) {
            return t.clone();
        }
        let t = find_path(&c.dir, &format!("Graphics/{rel}"))
            .and_then(|p| preload::decode_shared(&p))
            .map_err(|e| razdor::diag!("Discord Times art: {rel}: {e}"))
            .ok()
            .and_then(|frames| {
                let mut img = frames.first()?.clone();
                transform(&mut img, fx);
                to_texture(&img)
            });
        c.textures.entry(rel.to_string()).or_default()[fx as usize] = Some(t.clone());
        t
    })
}

/// A spell's picture, with the available layers as a fallback if its framed
/// composition cannot be made (for example, when a frame asset is missing).
pub fn spell_icon(spell: &razdor::rules::content::SpellDef, r: Rect) {
    if super::spell_badges::draw_picture(spell, r) {
        return;
    }
    let mut layers: Vec<(&str, [i32; 3])> = spell.icons.iter().filter_map(|i| Some((i.image.as_deref()?, i.tint.unwrap_or([0; 3])))).collect();
    layers.sort_by_key(|(name, _)| !name.starts_with('_'));
    let mut drawn = false;
    for (name, tint) in layers {
        if let Some(t) = spell_layer(name, tint) {
            tex(&t, r, WHITE);
            drawn = true;
        }
    }
    if !drawn {
        surface(r, Skin::Strip);
        shadow_centered(&spell.name.chars().next().unwrap_or('?').to_string(), r.x + r.w / 2.0, r.y + r.h * 0.65, (r.h * 0.5).max(10.0), CREAM);
    }
    silver_frame(r, 1.0);
}

/// A colour-corrected spell layer for the fallback picture.
fn spell_layer(name: &str, minus: [i32; 3]) -> Option<Texture2D> {
    let rel = format!("Spells/{name}.lit");
    let key = format!("{rel}#{},{},{}", minus[0], minus[1], minus[2]);
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if let Some(t) = c.textures.get(&key).and_then(|v| v[Fx::Glow as usize].as_ref()) {
            return t.clone();
        }
        let t = find_path(&c.dir, &format!("Graphics/{rel}"))
            .and_then(|p| preload::decode_shared(&p))
            .map_err(|e| razdor::diag!("Discord Times art: {rel}: {e}"))
            .ok()
            .and_then(|frames| {
                let mut img = frames.first()?.clone();
                subtract(&mut img, minus);
                transform(&mut img, Fx::Glow);
                to_texture(&img)
            });
        c.textures.entry(key).or_default()[Fx::Glow as usize] = Some(t.clone());
        t
    })
}

/// Windows art `name` with its channels scaled and shifted as loaded ([`scale_channels`]),
/// cached by both.
pub fn win_scaled(name: &str, k: [i32; 3], offset: [i32; 3]) -> Option<Texture2D> {
    win_adjusted(name, &format!("{k:?}{offset:?}"), |img| scale_channels(img, k, offset))
}

/// Windows art `name` changed by `adjust` once, as the original changes its copies at
/// start-up, cached under `tag` (which must name the change).
pub fn win_adjusted(name: &str, tag: &str, adjust: impl FnOnce(&mut Image)) -> Option<Texture2D> {
    let rel = format!("Windows/{name}.lit");
    let key = format!("{rel}#{tag}");
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if let Some(t) = c.textures.get(&key).and_then(|v| v[Fx::Plain as usize].as_ref()) {
            return t.clone();
        }
        let t = find_path(&c.dir, &format!("Graphics/{rel}"))
            .and_then(|p| preload::decode_shared(&p))
            .map_err(|e| razdor::diag!("Discord Times art: {rel}: {e}"))
            .ok()
            .and_then(|frames| {
                let mut img = frames.first()?.clone();
                adjust(&mut img);
                to_texture(&img)
            });
        c.textures.entry(key).or_default()[Fx::Plain as usize] = Some(t.clone());
        t
    })
}

/// Windows art (`Graphics/Windows/<name>.lit`).
pub fn win(name: &str) -> Option<Texture2D> {
    art(&format!("Windows/{name}.lit"), Fx::Plain)
}

fn visual_texture(key: &'static str, rel: &str, params: [f32; 4], adjust: impl FnOnce(&mut Image)) -> Option<Texture2D> {
    if let Some(t) = CHROME.with(|state| {
        let state = state.borrow();
        let (_, texture) = state.as_ref()?.visual_textures.get(key).filter(|(saved, _)| *saved == params)?;
        Some(texture.clone())
    }) {
        return t;
    }
    // A missing or bad file is remembered too, so it is looked for (and reported) once.
    let known = CHROME.with(|state| state.borrow().as_ref().and_then(|c| c.visual_images.get(rel).cloned()));
    let decoded = known.unwrap_or_else(|| {
        let img = image(rel);
        CHROME.with(|state| state.borrow_mut().as_mut().map(|c| c.visual_images.insert(rel.to_string(), img.clone())));
        img
    });
    let texture = decoded.and_then(|mut decoded| {
        adjust(&mut decoded);
        to_texture(&decoded)
    });
    CHROME.with(|state| {
        state.borrow_mut().as_mut()?.visual_textures.insert(key, (params, texture.clone()));
        texture
    })
}

/// The brown text box's paper: Win-paper with (148, 138, 156) taken off (0x48e4e8: 0x476b8c
/// copies the paper, 0x4768f8 mode 1 subtracts the colour), at the 16-bit screen's levels.
fn brown_paper_texture() -> Option<Texture2D> {
    visual_texture("brown-box-paper", "Windows/Win-paper.lit", [148.0, 138.0, 156.0, 565.0], |img| {
        subtract(img, BROWN_BOX_TAKEN);
        quantize_565(img);
    })
}

/// What the brown text box takes off its paper (0x48e4e8, 0x476374(0x94, 0x8a, 0x9c)).
const BROWN_BOX_TAKEN: [i32; 3] = [148, 138, 156];

pub use gfx::quantize_565;

/// The original's card strip picture: 94×39 (0x5e×0x27).
pub const STRIP_SIZE: (u32, u32) = (94, 39);

/// The picture under a card's stat strip, made from Win-paper at start-up as the original's
/// (0x4dc118, 0x4de0f0–0x4de1f7): a 94×39 piece of the paper (0x476b8c), at the 16-bit
/// screen's levels, then
/// - plain (ae2698): the piece at (20, 20) with (148, 132, 148) taken off (0x4768f8 mode 1):
///   a dark brown;
/// - the hero's (ae269c): the same, then red ×4222/1024 − 227, green ×768/1024, blue 0
///   (0x48da5c): a deep red-brown;
/// - a named character's (ae26a0): the piece at (220, 20), red and blue swapped (0x48dd40 with
///   3, 2, 1), then red 0, green ×1394/1024 − 196, blue ×2790/1024 − 517 (0x48da5c): a deep
///   blue.
pub fn strip_picture(paper: &Image, panel: super::unit_sheet::StripPanel) -> Image {
    use super::unit_sheet::StripPanel;
    let (w, h) = STRIP_SIZE;
    let x0 = if panel == StripPanel::Named { 220 } else { 20 };
    let mut img = Image { width: w, height: h, rgba: Vec::with_capacity((w * h * 4) as usize) };
    for y in 0..h {
        for x in 0..w {
            // A paper smaller than the original's is read round (never seen in an install).
            let px = paper.pixel((x0 + x) % paper.width.max(1), (20 + y) % paper.height.max(1));
            img.rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
    }
    quantize_565(&mut img);
    match panel {
        StripPanel::Plain | StripPanel::Hero => {
            subtract(&mut img, [148, 132, 148]);
            quantize_565(&mut img);
            if panel == StripPanel::Hero {
                scale_channels(&mut img, [4222, 768, 0], [-227, 0, 0]);
                quantize_565(&mut img);
            }
        }
        StripPanel::Named => {
            for p in img.rgba.chunks_exact_mut(4) {
                p.swap(0, 2);
            }
            scale_channels(&mut img, [0, 1394, 2790], [0, -196, -517]);
            quantize_565(&mut img);
        }
    }
    img
}

/// [`strip_picture`] as a texture (`None` without the install's paper).
pub fn strip_texture(panel: super::unit_sheet::StripPanel) -> Option<Texture2D> {
    use super::unit_sheet::StripPanel;
    let key = match panel {
        StripPanel::Plain => "card-strip",
        StripPanel::Hero => "card-strip-hero",
        StripPanel::Named => "card-strip-named",
    };
    visual_texture(key, "Windows/Win-paper.lit", [0.0; 4], |img| *img = strip_picture(img, panel))
}

fn paper_texture(brown: bool) -> Option<Texture2D> {
    let v = super::window_layout::visual();
    let (brightness, gamma, key) = if brown {
        (v.brown_brightness, v.brown_gamma, "brown-paper")
    } else {
        (v.white_brightness, v.white_gamma, "light-paper")
    };
    visual_texture(key, "Windows/Win-paper.lit", [brightness, gamma, 0.0, 0.0], |img| {
        let levels = std::array::from_fn::<_, 256, _>(|i| {
            ((i as f32 / 255.0).powf(1.0 / gamma) * brightness * 255.0).clamp(0.0, 255.0) as u8
        });
        for p in img.rgba.chunks_exact_mut(4) {
            for c in &mut p[..3] { *c = levels[*c as usize]; }
        }
    })
}

pub fn quest_border_texture() -> Option<Texture2D> {
    let v = super::window_layout::visual();
    let params = [v.quest_brightness, v.quest_gamma, v.quest_opacity, v.quest_cutoff];
    visual_texture("quest-border", "Windows/QuestBorder.lit", params, |img| {
        let levels = std::array::from_fn::<_, 256, _>(|i| {
            ((i as f32 / 255.0).powf(1.0 / v.quest_gamma) * v.quest_brightness * 255.0).clamp(0.0, 255.0) as u8
        });
        for p in img.rgba.chunks_exact_mut(4) {
            // The source alpha has opaque black scanlines. Derive alpha from the original
            // brightness, then adjust colour without normalising every pixel to white.
            let m = p[0].max(p[1]).max(p[2]) as f32;
            p[3] = (((m - v.quest_cutoff) / 22.0).clamp(0.0, 1.0) * v.quest_opacity * 255.0) as u8;
            for c in &mut p[..3] { *c = levels[*c as usize]; }
        }
    })
}

/// The decorative texture repeated across the resource strip below the game bar.
pub fn bottom_bar_ornament() -> Option<Texture2D> {
    let v = super::window_layout::visual();
    visual_texture(
        "bottom-bar-ornament",
        "Windows/DownCorner.lit",
        [v.bar_ornament_brightness, v.bar_ornament_gamma, 0.0, 0.0],
        |img| {
            for p in img.rgba.chunks_exact_mut(4) {
                let gamma = v.bar_ornament_gamma.max(0.01);
                for c in &mut p[..3] {
                    *c = ((*c as f32 / 255.0).powf(1.0 / gamma) * v.bar_ornament_brightness * 255.0)
                        .clamp(0.0, 255.0) as u8;
                }
                let mask = (p[0].max(p[1]).max(p[2]) as u32).saturating_sub(10).saturating_mul(16).min(255) as u8;
                p[3] = p[3].min(mask);
            }
        },
    )
}

pub fn win_fx(name: &str, fx: Fx) -> Option<Texture2D> {
    art(&format!("Windows/{name}.lit"), fx)
}

/// A still of `Graphics/Windows/<name>.ugs` (the logo, the menu buttons, the minimap symbols).
pub fn win_ugs(name: &str) -> Option<Texture2D> {
    art(&format!("Windows/{name}.ugs"), Fx::Plain)
}

/// `Graphics/Windows/<name>.lit` cut out by the brightness of `<mask>.lit` (the original's
/// separate alpha pictures, e.g. the battle's slide sprites over `army-alpha`, 0x4afbd8).
pub fn win_masked(name: &str, mask: &str) -> Option<Texture2D> {
    let key = format!("masked:{name}:{mask}");
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if let Some(t) = c.animations.get(&key) {
            return t.as_ref().and_then(|v| v.first().cloned());
        }
        let load = |n: &str| find_path(&c.dir, &format!("Graphics/Windows/{n}.lit")).and_then(|p| gfx::decode_file(&p)).ok().and_then(|f| f.into_iter().next());
        let t = match (load(name), load(mask)) {
            (Some(mut img), Some(m)) if (img.width, img.height) == (m.width, m.height) => {
                for (p, q) in img.rgba.chunks_exact_mut(4).zip(m.rgba.chunks_exact(4)) {
                    p[3] = q[0].max(q[1]).max(q[2]);
                }
                to_texture(&img).map(|t| vec![t])
            }
            _ => None,
        };
        c.animations.insert(key, t.clone());
        t.and_then(|v| v.first().cloned())
    })
}

/// The frames of a battle effect (`Graphics/Battle/<file>.ugs`) with its colour and alpha
/// offsets applied (0x4e1148), for [`premultiplied`] drawing; `None` without the file.
pub fn battle_effect(e: &razdor::dt::data::BattleEffect) -> Option<Vec<Texture2D>> {
    if e.file.trim().is_empty() {
        return None;
    }
    let key = format!("battle-effect:{}:{:?}:{}", e.file, e.rgb, e.alpha);
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if let Some(t) = c.animations.get(&key) {
            return t.clone();
        }
        let t = find_path(&c.dir, &format!("Graphics/Battle/{}.ugs", e.file.trim()))
            .and_then(|p| gfx::decode_file(&p))
            .map_err(|err| razdor::diag!("Discord Times art: {}: {err}", e.file))
            .ok()
            .map(|mut frames| {
                frames
                    .iter_mut()
                    .filter_map(|f| {
                        for p in f.rgba.chunks_exact_mut(4) {
                            let (rgb, a5) = e.pixel([p[0], p[1], p[2], p[3]]);
                            p[..3].copy_from_slice(&rgb);
                            p[3] = (a5 as u32 * 255 / 31) as u8;
                        }
                        to_texture(f)
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty());
        c.animations.insert(key, t.clone());
        t
    })
}

/// Every frame of an animation (`Graphics/<rel>`); additive ones are made to glow.
pub fn animation(rel: &str) -> Option<Vec<Texture2D>> {
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if let Some(t) = c.animations.get(rel) {
            return t.clone();
        }
        let t = find_path(&c.dir, &format!("Graphics/{rel}"))
            .and_then(|p| gfx::decode_file(&p))
            .map_err(|e| razdor::diag!("Discord Times art: {rel}: {e}"))
            .ok()
            .map(|mut frames| {
                let additive = gfx::is_additive(&frames);
                frames
                    .iter_mut()
                    .filter_map(|f| {
                        if additive {
                            transform(f, Fx::Glow);
                        }
                        to_texture(f)
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty());
        c.animations.insert(rel.to_string(), t.clone());
        t
    })
}

/// An interface text of the install (`Rus_DiscordTimes.ini`), e.g. `("Army", "Bonus2")`.
pub fn ui_text(section: &str, key: &str) -> Option<String> {
    if section == "Options" {
        if let Some(v) = option_override(key) {
            return Some(v);
        }
    }
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if let Some(en) = english_of(c, section, key) {
            return Some(en.replace("#\\", "\n"));
        }
        if c.texts.is_none() {
            c.texts = Some(read_texts(&c.dir));
        }
        text_of(c.texts.as_ref()?.as_ref()?, section, key)
    })
}

/// In a language other than Russian, the install's interface text translated into it by
/// razdor-translate, else into English (the interface is English then), if there is one
/// (Razdor's own: the original reads only the Russian file).
fn english_of(c: &mut Chrome, section: &str, key: &str) -> Option<String> {
    let prefix = super::language::data_prefix()?;
    if c.english_texts.as_ref().is_none_or(|(p, _)| *p != prefix) {
        let ini = razdor::dt::english::data_ini(&c.dir, &prefix, SETTINGS_FILE);
        c.english_texts = Some((prefix.clone(), ini));
    }
    let own = c.english_texts.as_ref()?.1.as_ref().and_then(|i| i.section(section)?.get_nonempty(key).map(str::to_string));
    if own.is_some() || prefix.eq_ignore_ascii_case(razdor::dt::english::ENGLISH) {
        return own;
    }
    if c.english_fallback.is_none() {
        c.english_fallback = Some(razdor::dt::english::english_ini(&c.dir, SETTINGS_FILE));
    }
    c.english_fallback.as_ref()?.as_ref()?.section(section)?.get_nonempty(key).map(str::to_string)
}

/// An interface text of the install as written, its marks kept (`#\` line breaks, `*` `|`
/// `@` `^`): for the windows that read it as markup ([`super::widgets::markup_rows`]).
pub fn ui_markup(section: &str, key: &str) -> Option<String> {
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if let Some(en) = english_of(c, section, key) {
            return Some(en);
        }
        if c.texts.is_none() {
            c.texts = Some(read_texts(&c.dir));
        }
        c.texts.as_ref()?.as_ref()?.section(section)?.get_nonempty(key).map(str::to_string)
    })
}

/// Line `n` (0-based) of the unnamed lines of a section of the install's interface texts,
/// e.g. `[Army]`'s «Кликните, что бы выделить #NAME1» (line 1).
pub fn ui_line(section: &str, n: usize) -> Option<String> {
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if c.texts.is_none() {
            c.texts = Some(read_texts(&c.dir));
        }
        c.texts.as_ref()?.as_ref()?.section(section)?.lines.get(n).cloned()
    })
}

/// A raw value of the install's `[Options]` (`Rus_DiscordTimes.ini`), e.g. the minimap colours.
pub fn options_value(key: &str) -> Option<String> {
    if let Some(v) = option_override(key) {
        return Some(v);
    }
    install_option(key)
}

local! {
    /// `[Options]` values the settings window set (`main_menu::apply_options`): they stand
    /// in for the install's, as the original's window writes them back into its ini.
    static OPTION_OVERRIDES: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

/// Sets (or with `None` drops) the settings window's value of `[Options] key`.
pub fn set_option_override(key: &str, value: Option<String>) {
    OPTION_OVERRIDES.with(|o| {
        let mut o = o.borrow_mut();
        o.retain(|(k, _)| k != key);
        if let Some(v) = value {
            o.push((key.to_string(), v));
        }
    });
}

fn option_override(key: &str) -> Option<String> {
    OPTION_OVERRIDES.with(|o| o.borrow().iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()))
}

/// The install's own `[Options] key`, whatever the settings window chose.
pub fn install_option(key: &str) -> Option<String> {
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if c.texts.is_none() {
            c.texts = Some(read_texts(&c.dir));
        }
        c.texts.as_ref()?.as_ref()?.section("Options")?.get_nonempty(key).map(str::to_string)
    })
}

/// The install's texts and options ini (`Rus_DiscordTimes.ini`) is there to read.
pub fn has_texts() -> bool {
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let Some(c) = c.as_mut() else { return false };
        if c.texts.is_none() {
            c.texts = Some(read_texts(&c.dir));
        }
        c.texts.as_ref().is_some_and(Option::is_some)
    })
}

fn read_texts(dir: &std::path::Path) -> Option<Ini> {
    find_path(dir, SETTINGS_FILE).ok().and_then(|p| std::fs::read(p).ok()).map(|b| Ini::from_cp1251(&b))
}

/// A text of the ini; the original's `#\` line breaks become newlines.
fn text_of(ini: &Ini, section: &str, key: &str) -> Option<String> {
    ini.section(section)?.get_nonempty(key).map(|s| s.replace("#\\", "\n"))
}

#[cfg(test)]
fn ui_text_from(dir: &std::path::Path, section: &str, key: &str) -> Option<String> {
    text_of(&read_texts(dir)?, section, key)
}

// ------------------------------------------------------------------------------------------
// Additive drawing (fire, glows)
// ------------------------------------------------------------------------------------------

const ADD_VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying lowp vec2 uv;
varying lowp vec4 color;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
}"#;

const ADD_FRAGMENT: &str = r#"#version 100
varying lowp vec4 color;
varying lowp vec2 uv;
uniform sampler2D Texture;
void main() {
    gl_FragColor = color * texture2D(Texture, uv);
}"#;

// Rotate the circular source art before the quad is vertically compressed into a
// ground-plane ellipse. Rotating the finished quad instead makes the oval itself spin.
const GROUND_RING_FRAGMENT: &str = r#"#version 100
precision mediump float;
varying lowp vec4 color;
varying lowp vec2 uv;
uniform sampler2D Texture;
uniform float Rotation;
void main() {
    vec2 p = uv - vec2(0.5);
    float c = cos(Rotation);
    float s = sin(Rotation);
    vec2 source_uv = mat2(c, -s, s, c) * p + vec2(0.5);
    if (source_uv.x < 0.0 || source_uv.x > 1.0 || source_uv.y < 0.0 || source_uv.y > 1.0) {
        discard;
    }
    gl_FragColor = color * texture2D(Texture, source_uv);
}"#;

local! {
    static ADDITIVE: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
    static MULTIPLY: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
    static PREMULTIPLIED: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
    static GROUND_RING: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
}

/// Draw a circular marker texture projected onto the ground. The source pattern rotates
/// in its own plane while the output remains a stable foreshortened ellipse.
pub fn ground_ring(texture: &Texture2D, rect: Rect, color: Color, angle: f32) {
    use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation, PipelineParams};
    let material = GROUND_RING.with(|cell| {
        cell.get_or_init(|| {
            load_material(
                ShaderSource::Glsl { vertex: ADD_VERTEX, fragment: GROUND_RING_FRAGMENT },
                MaterialParams {
                    uniforms: vec![UniformDesc::new("Rotation", UniformType::Float1)],
                    pipeline_params: PipelineParams {
                        color_blend: Some(BlendState::new(
                            Equation::Add,
                            BlendFactor::Value(BlendValue::SourceAlpha),
                            BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                        )),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .map_err(|e| razdor::diag!("ground ring material: {e}"))
            .ok()
        })
        .clone()
    });
    if let Some(material) = material {
        material.set_uniform("Rotation", angle);
        with_material(Some(material), || {
            draw_texture_ex(
                texture,
                rect.x,
                rect.y,
                color,
                DrawTextureParams { dest_size: Some(vec2(rect.w, rect.h)), ..Default::default() },
            );
        });
    } else {
        draw_texture_ex(
            texture,
            rect.x,
            rect.y,
            color,
            DrawTextureParams { dest_size: Some(vec2(rect.w, rect.h)), ..Default::default() },
        );
    }
}

/// Runs `draw` blending as the original's 5-bit alpha masks (engine.md §6): what is below
/// is kept by `1 − alpha` and the colour drawn is added as it is (premultiplied).
pub fn premultiplied(draw: impl FnOnce()) {
    use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation};
    let material = PREMULTIPLIED.with(|m| {
        m.get_or_init(|| {
            blend_material(BlendState::new(Equation::Add, BlendFactor::One, BlendFactor::OneMinusValue(BlendValue::SourceAlpha)), "premultiplied")
        })
        .clone()
    });
    with_material(material, draw);
}

fn blend_material(blend: macroquad::miniquad::BlendState, what: &str) -> Option<Material> {
    let pipeline_params = macroquad::miniquad::PipelineParams { color_blend: Some(blend), ..Default::default() };
    load_material(ShaderSource::Glsl { vertex: ADD_VERTEX, fragment: ADD_FRAGMENT }, MaterialParams { pipeline_params, ..Default::default() })
        .map_err(|e| razdor::diag!("{what} material: {e}"))
        .ok()
}

/// Runs `draw` with additive blending (light is added to what is below, as the original's
/// fire and glows), or with normal blending if the material cannot be made.
pub fn additive(draw: impl FnOnce()) {
    use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation};
    let material = ADDITIVE.with(|m| {
        m.get_or_init(|| blend_material(BlendState::new(Equation::Add, BlendFactor::Value(BlendValue::SourceAlpha), BlendFactor::One), "additive"))
            .clone()
    });
    with_material(material, draw);
}

const SILHOUETTE_FRAGMENT: &str = r#"#version 100
varying lowp vec4 color;
varying lowp vec2 uv;
uniform sampler2D Texture;
void main() {
    gl_FragColor = vec4(color.rgb, color.a * step(0.77, texture2D(Texture, uv).a));
}"#;

local! {
    static SILHOUETTE: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
}

/// Runs `draw` with every picture drawn as its solid shape in the tint colour, for outlines:
/// only alpha above 0.77, so a building's shadow (up to
/// [`razdor::dt::gfx::BUILDING_SHADOW_ALPHA`], 0.733) is left out and its edges (from 0.8)
/// kept. Without the material, nothing is drawn.
pub fn silhouette(draw: impl FnOnce()) {
    use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation, PipelineParams};
    let material = SILHOUETTE.with(|m| {
        m.get_or_init(|| {
            let blend = BlendState::new(Equation::Add, BlendFactor::Value(BlendValue::SourceAlpha), BlendFactor::OneMinusValue(BlendValue::SourceAlpha));
            let pipeline_params = PipelineParams { color_blend: Some(blend), ..Default::default() };
            load_material(ShaderSource::Glsl { vertex: ADD_VERTEX, fragment: SILHOUETTE_FRAGMENT }, MaterialParams { pipeline_params, ..Default::default() })
                .map_err(|e| razdor::diag!("silhouette material: {e}"))
                .ok()
        })
        .clone()
    });
    if let Some(m) = material {
        with_material(Some(m), draw);
    }
}

/// Runs `draw` with multiplying blending: what is below is multiplied by the colour drawn
/// (darkening and tinting parchment), or with normal blending if the material cannot be made.
pub fn multiply(draw: impl FnOnce()) {
    use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation};
    let material = MULTIPLY.with(|m| {
        m.get_or_init(|| blend_material(BlendState::new(Equation::Add, BlendFactor::Zero, BlendFactor::Value(BlendValue::SourceColor)), "multiply"))
            .clone()
    });
    with_material(material, draw);
}

/// Runs `draw` with subtractive blending: the colour drawn is taken off what is below, the
/// original's blend mode 1 (476790, 4768f8), the window's alpha kept. Without the material,
/// normal blending.
pub fn subtractive(draw: impl FnOnce()) {
    subtract_with(&SUBTRACT, ADD_FRAGMENT, draw);
}

/// Subtractive blending for text: like [`subtractive`], but the fragment is premultiplied by the
/// glyph's coverage, so only the glyph's ink is taken off the parchment, not its whole box.
pub fn subtractive_text(draw: impl FnOnce()) {
    subtract_with(&SUBTRACT_TEXT, SUBTRACT_TEXT_FRAGMENT, draw);
}

fn subtract_with(cell: &'static razdor::local::Local<std::cell::OnceCell<Option<Material>>>, fragment: &'static str, draw: impl FnOnce()) {
    use macroquad::miniquad::{BlendFactor, BlendState, Equation, PipelineParams};
    let material = cell.with(|m| {
        m.get_or_init(|| {
            let pipeline_params = PipelineParams {
                color_blend: Some(BlendState::new(Equation::ReverseSubtract, BlendFactor::One, BlendFactor::One)),
                alpha_blend: Some(BlendState::new(Equation::Add, BlendFactor::Zero, BlendFactor::One)),
                ..Default::default()
            };
            load_material(ShaderSource::Glsl { vertex: ADD_VERTEX, fragment }, MaterialParams { pipeline_params, ..Default::default() })
                .map_err(|e| razdor::diag!("subtractive material: {e}"))
                .ok()
        })
        .clone()
    });
    with_material(material, draw);
}

local! {
    static SUBTRACT: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
    static SUBTRACT_TEXT: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
}

const SUBTRACT_TEXT_FRAGMENT: &str = r#"#version 100
precision lowp float;
varying lowp vec4 color;
varying lowp vec2 uv;
uniform sampler2D Texture;
void main() {
    vec4 t = texture2D(Texture, uv);
    gl_FragColor = vec4(color.rgb * t.rgb * t.a, t.a);
}"#;

fn with_material(material: Option<Material>, draw: impl FnOnce()) {
    match material {
        Some(m) => {
            gl_use_material(&m);
            draw();
            gl_use_default_material();
        }
        None => draw(),
    }
}

const GREY_VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
varying lowp vec2 uv_screen;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    vec4 res = Projection * Model * vec4(position, 1);
    uv_screen = res.xy / 2.0 + vec2(0.5, 0.5);
    gl_Position = res;
}"#;

const GREY_FRAGMENT: &str = r#"#version 100
precision lowp float;
varying vec2 uv_screen;
uniform sampler2D _ScreenTexture;
void main() {
    vec3 c = texture2D(_ScreenTexture, uv_screen).rgb;
    float l = dot(c, vec3(0.30, 0.59, 0.11));
    gl_FragColor = vec4(vec3(l) * 0.85, 1.0);
}"#;

local! {
    static GREY: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
    /// The largest window drawn this frame (what a message greys out).
    static BIGGEST: std::cell::Cell<Option<Rect>> = const { std::cell::Cell::new(None) };
}

/// A new frame: no window drawn yet.
pub fn begin_frame() {
    BIGGEST.with(|b| b.set(None));
}

/// Under a message window, as in the original (the video: the town window goes black and
/// white under "Слухи в таверне" while the map around it keeps its colours): the window
/// below turns grey; on the map with no window, a dark veil *(guess)*.
pub fn under_message() {
    match BIGGEST.with(|b| b.get()) {
        Some(r) => grey_out(r),
        None => draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.35)),
    }
}

/// Whether the grey-out takes its colours from the screen copy (`_ScreenTexture`). Phones
/// cannot: macroquad fills that copy with `glCopyTexImage2D` from the default framebuffer,
/// which miniquad makes without alpha on Android (EGL_ALPHA_SIZE 0); OpenGL ES rejects an
/// RGBA copy from a framebuffer with no alpha (GL_INVALID_OPERATION), so the copy stays
/// empty and the shader paints black where the window was. Desktop GL allows the copy.
/// Phones take the dark veil instead. The alpha bits are not asked for: an alpha surface can
/// make the app see-through on Android.
pub fn grey_by_screen_texture() -> bool {
    !razdor::platform::MOBILE
}

/// Turns what is drawn so far in `r` grey and a little darker. Without the shader (or on a
/// phone, see [`grey_by_screen_texture`]), a dark veil.
pub fn grey_out(r: Rect) {
    let material = if grey_by_screen_texture() {
        GREY.with(|m| {
            m.get_or_init(|| {
                load_material(ShaderSource::Glsl { vertex: GREY_VERTEX, fragment: GREY_FRAGMENT }, MaterialParams::default())
                    .map_err(|e| razdor::diag!("grey material: {e}"))
                    .ok()
            })
            .clone()
        })
    } else {
        None
    };
    match material {
        Some(m) => {
            gl_use_material(&m);
            draw_rectangle(r.x, r.y, r.w, r.h, WHITE);
            gl_use_default_material();
        }
        None => draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.35)),
    }
}

// ------------------------------------------------------------------------------------------
// Geometry and plain drawing
// ------------------------------------------------------------------------------------------

/// Screen pixels per pixel of the 960×720 reference video (the original ran at 1024×768):
/// the settings' interface scale, kept to the screen's fit ([`fit_k`]): a scale above the fit
/// is kept to the fit (players got stuck with the settings off-screen in 1.0.0).
pub fn k() -> f32 {
    super::display::window_scale().unwrap_or_else(|| super::display::scale(fit_k()))
}

/// The largest interface scale the window fits (the settings' "Auto").
pub fn fit_k() -> f32 {
    ((screen_width() / 1024.0).min(screen_height() / 768.0) / 0.9375).max(0.5)
}

/// The scale of the bottom bar, the FPS counter and the keyboard strip: the chosen one kept to
/// the fit, so they stay fully visible whatever the windows' scale is.
pub fn bar_k() -> f32 {
    super::display::fitted_scale(fit_k())
}

/// Height of the bottom game bar.
pub fn bar_height() -> f32 {
    (80.0 * bar_k()).round()
}

/// Top of a `canvas_h`-tall canvas centred vertically on a `screen_w`×`screen_h` screen. Only
/// screens narrower than 4:3 get it: there the width sets the scale and leaves height free (a
/// phone in portrait). A 4:3 or wider screen, the desktop's, gets 0 and is never moved.
pub fn centred_top(screen_w: f32, screen_h: f32, canvas_h: f32) -> f32 {
    if screen_w * 3.0 >= screen_h * 4.0 || screen_h <= canvas_h {
        0.0
    } else {
        ((screen_h - canvas_h) / 2.0).round()
    }
}

/// Top of the title screens' 720-line canvas ([`centred_top`]); the menus, their logo and their
/// windows are placed from it.
pub fn canvas_top() -> f32 {
    centred_top(screen_width(), screen_height(), 720.0 * k())
}

/// The part of a `pic` picture (width, height) that covers a `screen` (width, height) at the
/// picture's own aspect: the centred crop, in picture pixels. None where the picture keeps
/// being stretched over the screen: a landscape or square screen (unchanged from before), and a
/// portrait one whose aspect is within 20% of the picture's.
pub fn cover_source(pic: (f32, f32), screen: (f32, f32)) -> Option<Rect> {
    let ((pw, ph), (sw, sh)) = (pic, screen);
    if pw <= 0.0 || ph <= 0.0 || sw <= 0.0 || sh <= sw {
        return None;
    }
    let (pic_aspect, screen_aspect) = (pw / ph, sw / sh);
    if (0.8..=1.2).contains(&(screen_aspect / pic_aspect)) {
        return None;
    }
    if screen_aspect < pic_aspect {
        let cut = ph * screen_aspect;
        Some(Rect::new(((pw - cut) / 2.0).round(), 0.0, cut.round(), ph))
    } else {
        let cut = pw / screen_aspect;
        Some(Rect::new(0.0, ((ph - cut) / 2.0).round(), pw, cut.round()))
    }
}

pub fn tex(t: &Texture2D, r: Rect, color: Color) {
    draw_texture_ex(t, r.x, r.y, color, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), ..Default::default() });
}

pub fn tex_src(t: &Texture2D, src: Rect, dst: Rect, color: Color) {
    draw_texture_ex(t, dst.x, dst.y, color, DrawTextureParams { dest_size: Some(vec2(dst.w, dst.h)), source: Some(src), ..Default::default() });
}

fn tex_src_flipped_x(t: &Texture2D, src: Rect, dst: Rect, color: Color) {
    draw_texture_ex(t, dst.x, dst.y, color, DrawTextureParams {
        dest_size: Some(vec2(dst.w, dst.h)),
        source: Some(src),
        flip_x: true,
        ..Default::default()
    });
}

/// Repeats `t` over `r` at `scale` screen pixels per texel, clipping the last row and column.
pub fn tile(t: &Texture2D, r: Rect, scale: f32, color: Color) {
    let (tw, th) = (t.width() * scale, t.height() * scale);
    if tw < 1.0 || th < 1.0 {
        return;
    }
    let mut y = r.y;
    while y < r.y + r.h - 0.5 {
        let h = th.min(r.y + r.h - y);
        let mut x = r.x;
        while x < r.x + r.w - 0.5 {
            let w = tw.min(r.x + r.w - x);
            tex_src(t, Rect::new(0.0, 0.0, w / scale, h / scale), Rect::new(x, y, w, h), color);
            x += tw;
        }
        y += th;
    }
}

/// Three-slice: the ends of `t` keep their aspect, the middle stretches to `r.w`.
pub fn three_slice(t: &Texture2D, r: Rect, end: f32, color: Color) {
    let s = r.h / t.height();
    let e = (end * s).min(r.w / 2.0);
    let src_e = e / s;
    tex_src(t, Rect::new(0.0, 0.0, src_e, t.height()), Rect::new(r.x, r.y, e, r.h), color);
    tex_src(t, Rect::new(src_e, 0.0, t.width() - 2.0 * src_e, t.height()), Rect::new(r.x + e, r.y, r.w - 2.0 * e, r.h), color);
    tex_src(t, Rect::new(t.width() - src_e, 0.0, src_e, t.height()), Rect::new(r.x + r.w - e, r.y, e, r.h), color);
}

/// Text with the original's dark drop shadow.
pub fn shadow_text(s: &str, x: f32, y: f32, size: f32, color: Color) {
    let size = super::dt_font::native_size(super::dt_font::face(), size);
    // From the original's sheets: their shadow sheet, 1 px down and right (4e5c9c, 4dbc26).
    if !super::dt_font::draw_shadow(s, x, y, size, color.a) {
        let o = (size / 14.0).clamp(1.0, 2.0);
        text(s, x + o, y + o, size, Color::new(0.0, 0.0, 0.0, 0.75 * color.a));
    }
    text(s, x, y, size, color);
}

/// The font size at which a word stands in for an engraved word picture: its capitals
/// `cap` px tall, shrunk so it is at most `max_w` wide. `cap_per_size` and `width_per_size`
/// are the word's capital height and width per unit of font size.
pub fn engraved_size(cap: f32, max_w: f32, cap_per_size: f32, width_per_size: f32) -> f32 {
    let tall = cap / cap_per_size.max(1e-3);
    if width_per_size > 0.0 { tall.min(max_w / width_per_size) } else { tall }
}

/// A Razdor word in place of the original's engraved word picture (the title menu's words
/// in English): Becker, the titles' TrueType face, whatever the font mode, so the size asked
/// for is the size drawn (the interface's text sizes follow the original's sheets, which
/// made the word as small as the body text), with a real space. Capitals `cap` px tall,
/// centred on `c` and at most `max_w` wide; a dark rim `rim` px wide like the pictures',
/// drawn twice `bold` px apart for their weight.
pub fn engraved_centered(s: &str, c: Vec2, (cap, max_w): (f32, f32), (rim, bold): (f32, f32), color: Color, edge: Color) {
    let font = super::widgets::face_font(super::dt_font::Face::Title.index(), super::dt_font::FontMode::Modern);
    let at = |size: u16| measure_text(s, font.as_ref(), size, 1.0);
    let probe = at(100);
    let cap_per_size = measure_text("H", font.as_ref(), 100, 1.0).offset_y / 100.0;
    let size = engraved_size(cap, max_w - bold, cap_per_size, probe.width / 100.0).round().max(1.0) as u16;
    let m = at(size);
    let x = (c.x - (m.width + bold) / 2.0).round();
    let y = (c.y + cap_per_size * size as f32 / 2.0).round();
    if super::text_probe::on() {
        super::text_probe::record(s, x, y, m.width, size as f32, cap_per_size * size as f32);
    }
    let draw = |x: f32, y: f32, color: Color| {
        for dx in [0.0, bold] {
            draw_text_ex(s, x + dx, y, TextParams { font: font.as_ref(), font_size: size, color, ..Default::default() });
        }
    };
    for (dx, dy) in [(-rim, 0.0), (rim, 0.0), (0.0, -rim), (0.0, rim), (rim, rim)] {
        draw(x + dx, y + dy, edge);
    }
    draw(x, y, color);
}


pub fn shadow_centered(s: &str, cx: f32, y: f32, size: f32, color: Color) {
    let w = measure(s, size).width;
    shadow_text(s, cx - w / 2.0, y, size, color);
}

pub fn shadow_right(s: &str, rx: f32, y: f32, size: f32, color: Color) {
    let w = measure(s, size).width;
    shadow_text(s, rx - w, y, size, color);
}

// ------------------------------------------------------------------------------------------
// Surfaces and frames
// ------------------------------------------------------------------------------------------

/// Palette of the placeholder style (and of text drawn over the original's art).
pub const CREAM: Color = Color::new(0.96, 0.91, 0.76, 1.0);
pub const GOLD: Color = Color::new(1.0, 0.82, 0.42, 1.0);
pub const SILVER: Color = Color::new(0.80, 0.80, 0.84, 1.0);
pub const SILVER_DARK: Color = Color::new(0.30, 0.31, 0.34, 1.0);
pub const BLUE_TEXT: Color = Color::new(0.45, 0.70, 1.0, 1.0);
pub const RED_TEXT: Color = Color::new(1.0, 0.33, 0.25, 1.0);
/// The warning line's ink (ae24f8, 4c280c / 4c2f54 / 4c3dcc): the bold SanSerif with its green
/// less 152 and blue less 159 (4dc051), so white ink becomes (255, 103, 96).
pub const WARNING_INK: Color = Color::new(1.0, 103.0 / 255.0, 96.0 / 255.0, 1.0);
pub const ORANGE_TEXT: Color = Color::new(1.0, 0.70, 0.25, 1.0);

/// A window's or panel's background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skin {
    /// Green marble: most windows, tooltips.
    Marble,
    /// Red marble: the battle.
    Red,
    /// Light parchment: building tabs, the unit panel.
    Paper,
    /// Red-brown parchment: story and description text boxes.
    Brown,
    /// Gold-brown: the stat strip under a card.
    Strip,
}

impl Skin {
    fn texture(self) -> Option<(Texture2D, Color)> {
        let paper = |brown: bool, c: Color| paper_texture(brown).map(|t| (t, c));
        match self {
            Skin::Marble => win("Win-marble").map(|t| (t, WHITE)),
            // Loaded with (−20, −40, −100) added (0x4dbc34 → 0x48d02c), into ae2530.
            Skin::Red => win_scaled("Win-red", [1024; 3], [-20, -40, -100]).map(|t| (t, WHITE)),
            Skin::Paper => paper(false, WHITE),
            Skin::Brown => brown_paper_texture().map(|t| (t, WHITE)),
            Skin::Strip => paper(true, Color::new(0.37, 0.265, 0.02, 1.0)),
        }
    }

    /// The placeholder colour.
    pub fn color(self) -> Color {
        match self {
            Skin::Marble => Color::new(0.10, 0.22, 0.18, 1.0),
            Skin::Red => Color::new(0.30, 0.06, 0.04, 1.0),
            Skin::Paper => Color::new(0.86, 0.68, 0.45, 1.0),
            Skin::Brown => Color::new(0.46, 0.25, 0.10, 1.0),
            Skin::Strip => Color::new(0.45, 0.30, 0.12, 1.0),
        }
    }
}

/// Brightness (0..255) of a tileable cloudy noise, `n`×`n`: our own stand-in for the
/// original's marble and parchment (pure, deterministic).
pub fn marble_noise(n: usize) -> Vec<u8> {
    let hash = |x: usize, y: usize, s: u32| -> f32 {
        let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263) ^ s.wrapping_mul(2_246_822_519);
        h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
        ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
    };
    let mut out = vec![0f32; n * n];
    let mut amp = 1.0;
    let mut total = 0.0;
    for (octave, cells) in [4usize, 8, 16, 32].into_iter().enumerate() {
        let step = n as f32 / cells as f32;
        for y in 0..n {
            for x in 0..n {
                let (fx, fy) = (x as f32 / step, y as f32 / step);
                let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
                let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
                let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
                let v = |i: usize, j: usize| hash(i % cells, j % cells, octave as u32 + 1);
                let top = v(x0, y0) + (v(x0 + 1, y0) - v(x0, y0)) * sx;
                let bottom = v(x0, y0 + 1) + (v(x0 + 1, y0 + 1) - v(x0, y0 + 1)) * sx;
                out[y * n + x] += amp * (top + (bottom - top) * sy);
            }
        }
        total += amp;
        amp *= 0.55;
    }
    out.iter().map(|v| (150.0 + 105.0 * (v / total).clamp(0.0, 1.0)) as u8).collect()
}

local! {
    static NOISE: RefCell<Option<Texture2D>> = const { RefCell::new(None) };
    static UNIT_PANEL_FADE: RefCell<Option<Texture2D>> = const { RefCell::new(None) };
}

/// A smoothly interpolated brown tint used above the character stats in the unit panel.
pub fn unit_panel_fade(r: Rect) {
    let texture = UNIT_PANEL_FADE.with(|slot| {
        slot.borrow_mut().get_or_insert_with(|| {
            let height = 128usize;
            let mut rgba = Vec::with_capacity(height * 4);
            for y in 0..height {
                let t = y as f32 / (height - 1) as f32;
                rgba.extend_from_slice(&[
                    ((1.0 - 0.5 * t) * 255.0).round() as u8,
                    ((1.0 - 0.65 * t) * 255.0).round() as u8,
                    ((1.0 - t) * 255.0).round() as u8,
                    255,
                ]);
            }
            let texture = Texture2D::from_rgba8(1, height as u16, &rgba);
            texture.set_filter(FilterMode::Linear);
            texture
        }).clone()
    });
    multiply(|| tex(&texture, r, WHITE));
}

fn noise_texture() -> Texture2D {
    NOISE.with(|t| {
        t.borrow_mut()
            .get_or_insert_with(|| {
                let n = 128;
                let rgba: Vec<u8> = marble_noise(n).into_iter().flat_map(|v| [v, v, v, 255]).collect();
                let tex = Texture2D::from_rgba8(n as u16, n as u16, &rgba);
                tex.set_filter(FilterMode::Linear);
                tex
            })
            .clone()
    })
}

/// Fills `r` with `skin`: the original's texture, or our cloudy placeholder in the skin's
/// colour.
pub fn surface(r: Rect, skin: Skin) {
    surface_alpha(r, skin, 1.0);
}

/// Keeps the paper grain at its normal scale. Alternating mirrored tiles meet at the
/// same source edge, avoiding the top-to-bottom seam without stretching the parchment.
pub fn paper_panel(r: Rect) {
    if let Some((t, c)) = Skin::Paper.texture() {
        let scale = k().min(1.4);
        let (tw, th) = (t.width() * scale, t.height() * scale);
        if tw < 1.0 || th < 1.0 { return; }
        let mut row = 0;
        let mut y = r.y;
        while y < r.bottom() {
            let h = th.min(r.bottom() - y);
            let mut column = 0;
            let mut x = r.x;
            while x < r.right() {
                let w = tw.min(r.right() - x);
                let (sw, sh) = (w / scale, h / scale);
                let (flip_x, flip_y) = (column % 2 != 0, row % 2 != 0);
                // A clipped mirrored tile starts at the far source edge, so the final
                // partial row/column keeps the same grain size and edge continuity.
                let source = Rect::new(
                    if flip_x { t.width() - sw } else { 0.0 },
                    if flip_y { t.height() - sh } else { 0.0 },
                    sw, sh,
                );
                draw_texture_ex(&t, x, y, c, DrawTextureParams {
                    dest_size: Some(vec2(w, h)), source: Some(source), flip_x, flip_y,
                    ..Default::default()
                });
                x += tw;
                column += 1;
            }
            y += th;
            row += 1;
        }
    } else {
        surface(r, Skin::Paper);
    }
}

/// A skin at opacity `alpha` (the fading hint boxes).
pub fn surface_alpha(r: Rect, skin: Skin, alpha: f32) {
    if let Some((t, c)) = skin.texture() {
        let scale = if matches!(skin, Skin::Brown | Skin::Red) { k() } else { k().min(1.4) };
        tile(&t, r, scale, Color::new(c.r, c.g, c.b, alpha));
        return;
    }
    let base = skin.color();
    let lift = |v: f32| (v * 1.25).min(1.0);
    tile(&noise_texture(), r, 2.0, Color::new(lift(base.r), lift(base.g), lift(base.b), alpha));
}

/// Screen pixels per pixel of the original's 1024×768 screen (`k` is per pixel of the 960×720
/// reference: the original's pixel is 0.9375 of it).
pub fn opx() -> f32 {
    0.9375 * k()
}

/// `SteelLine.lit` (0xae2538, loaded by 0x4dbb0c): the row texture and its transpose, for the
/// frame lines' runs along and down.
fn steel_textures() -> Option<(Texture2D, Texture2D)> {
    let row = win("SteelLine")?;
    let col = win_adjusted("SteelLine", "transposed", |img| {
        let (w, h) = (img.width as usize, img.height as usize);
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let (from, to) = ((y * w + x) * 4, (x * h + y) * 4);
                out[to..to + 4].copy_from_slice(&img.rgba[from..from + 4]);
            }
        }
        *img = Image { width: h as u32, height: w as u32, rgba: out };
    })?;
    row.set_filter(FilterMode::Nearest);
    col.set_filter(FilterMode::Nearest);
    Some((row, col))
}

/// Splits a run of `n` texels from the flat index `start` of a `w`×`h` picture read as one
/// long row (0x4764c0 reads it flat, so a run that passes a row's end goes on with the next
/// row) into `(index, offset in the run, length)` pieces that stay within one row. Past the
/// picture's end the original reads on into unrelated memory; Razdor starts the picture
/// again.
pub fn steel_pieces(start: usize, n: usize, w: usize, h: usize) -> Vec<(usize, usize, usize)> {
    let total = (w * h).max(1);
    let mut pieces = Vec::new();
    let mut done = 0;
    while done < n && w > 0 {
        let i = (start + done) % total;
        let len = (w - i % w).min(n - done);
        pieces.push((i, done, len));
        done += len;
    }
    pieces
}

/// The original's frame line (0x48d1d4 with colour −1, every window and panel outline): the
/// outline of `r`, `thick` (1 or 2) pixels wide, each pixel `px` screen pixels along the
/// line and `line` across it, its colours `SteelLine.lit`'s texels taken one by one by an
/// index that runs on round the frame: the top left to right, the right side down, the
/// bottom right to left, the left side up. A 2 px line's second pixel lies inside (y + 1,
/// x − 2, y − 2, x + 1) in the same colour; each side overwrites the corners the one before
/// it drew. Without the install a plain silver outline.
fn steel_outline(r: Rect, px: f32, thick: u8, line: f32) {
    let t = f32::from(thick.clamp(1, 2));
    let Some((row, col)) = steel_textures() else {
        draw_rectangle_lines(r.x, r.y, r.w, r.h, t * line, SILVER);
        return;
    };
    let (tw, th) = (row.width() as usize, row.height() as usize);
    let w = (r.w / px).round().max(0.0) as usize;
    let h = (r.h / px).round().max(0.0) as usize;
    if w == 0 || h == 0 {
        return;
    }
    let band = t * line;
    let along = |start: usize, n: usize, x: f32, y: f32, reversed: bool| {
        for (i, off, len) in steel_pieces(start, n, tw, th) {
            let at = if reversed { n - off - len } else { off };
            draw_texture_ex(&row, x + at as f32 * px, y, WHITE, DrawTextureParams {
                dest_size: Some(vec2(len as f32 * px, band)),
                source: Some(Rect::new((i % tw) as f32, (i / tw) as f32, len as f32, 1.0)),
                flip_x: reversed,
                ..Default::default()
            });
        }
    };
    let down = |start: usize, n: usize, x: f32, y: f32, reversed: bool| {
        for (i, off, len) in steel_pieces(start, n, tw, th) {
            let at = if reversed { n - off - len } else { off };
            draw_texture_ex(&col, x, y + at as f32 * px, WHITE, DrawTextureParams {
                dest_size: Some(vec2(band, len as f32 * px)),
                source: Some(Rect::new((i / tw) as f32, (i % tw) as f32, 1.0, len as f32)),
                flip_y: reversed,
                ..Default::default()
            });
        }
    };
    let (right, bottom) = (r.x + w as f32 * px - band, r.y + h as f32 * px - band);
    along(0, w, r.x, r.y, false);
    down(w, h, right, r.y, false);
    along(w + h, w, r.x, bottom, true);
    down(2 * w + h, h, r.x, r.y, true);
}

/// 0x48d1d4's outline of `r` in the original's pixels of `px` screen pixels.
pub fn steel_frame(r: Rect, px: f32, thick: u8) {
    steel_outline(r, px, thick, px);
}

/// A straight line of the original's: a rectangle `width` high (or wide) round the line,
/// drawn as a 2 px frame when the width is two of the original's pixels, else 1 px
/// (0x48d1d4's lines are such thin frames: the title line is (2, 27)–(w − 2, 29) at 2 px).
pub fn silver_line(from: Vec2, to: Vec2, width: f32) {
    let width = width.max(1.0);
    let thick = if width >= 1.5 * opx() { 2 } else { 1 };
    let band = if from.y == to.y {
        Rect::new(from.x.min(to.x), from.y - width / 2.0, (to.x - from.x).abs(), width)
    } else if from.x == to.x {
        Rect::new(from.x - width / 2.0, from.y.min(to.y), width, (to.y - from.y).abs())
    } else {
        draw_line(from.x, from.y, to.x, to.y, width, SILVER);
        return;
    };
    steel_outline(band, opx(), thick, width / f32::from(thick));
}

/// A frame of the original's line round `r`: 2 px when `width` is two of the original's
/// pixels or more, else 1 px (0x48d1d4).
pub fn silver_frame(r: Rect, width: f32) {
    let width = width.max(1.0);
    let thick = if width >= 1.5 * opx() { 2 } else { 1 };
    steel_outline(r, opx(), thick, width / f32::from(thick));
}

/// The frames 0x48d710 adds inside a window made with them (the 634×516 windows, the
/// village's, the leave-battle window's; the event window draws the same, 0x4aaa4b and
/// 0x4aaa79): a 2 px line from (2, h − 56) to (w − 2, h − 54) over the buttons and a 1 px
/// frame from (4, 31) to (w − 4, h − 58) round the content. `px`: screen pixels per pixel
/// of the window as the original builds it.
pub fn window_inner_frames(r: Rect, px: f32) {
    window_inner_frames_grown(r, px, 0.0);
}

/// [`window_inner_frames`] for a window whose title strip is taller by `grow` px: the top of
/// the inner frame moves down with it, the bottom stays where it is (`r` includes the growth).
pub fn window_inner_frames_grown(r: Rect, px: f32, grow: f32) {
    let (w, h) = (r.w / px, r.h / px);
    steel_frame(Rect::new(r.x + 2.0 * px, r.y + (h - 56.0) * px, (w - 4.0) * px, 2.0 * px), px, 2);
    steel_frame(Rect::new(r.x + 4.0 * px, r.y + (31.0 * px + grow), (w - 8.0) * px, (h - 89.0) * px - grow), px, 1);
}

/// A title strip's height in px at the interface scale `k` and the title factor `factor`
/// (26 px at the original's size; `display::title_factor`).
pub fn strip_height(k: f32, factor: f32) -> f32 {
    (26.0 * k * factor).round()
}

/// The title factor of a fixed-layout window (the hero, army, building and settings windows, the
/// save and the spell book): the original's title whatever «Размер текста» is, since their
/// layouts are fixed. Only the windows that can grow take the text's title (`growing_title`).
pub const FIXED_TITLE: f32 = 1.0;

/// The title factor of the windows that can grow with the text (the event and message windows):
/// the text's scale over the interface scale (`display::title_factor`).
pub fn growing_title() -> f32 {
    super::display::title_factor(k())
}

/// How much taller the growing windows' title strip is than the original's 26 px, in px (0 at
/// the original's size).
pub fn title_grow() -> f32 {
    strip_height(k(), growing_title()) - (26.0 * k()).round()
}

/// How much taller a phone's title strip is than the desktop's, as a factor (a phone choice, not in
/// the original: the close cross sits inside the strip, so the strip is taller there).
pub const PHONE_STRIP: f32 = 1.5;

/// A title strip `tb` px tall on the desktop, as it is on this screen: 1.5× on a phone (`dp` Some),
/// `tb` as it is otherwise.
pub fn phone_strip_height(tb: f32, dp: Option<f32>) -> f32 {
    match dp.filter(|d| *d > 0.0) {
        Some(_) => (tb * PHONE_STRIP).round(),
        None => tb,
    }
}

/// How far a phone's title strip grows above its window's rect for `factor` (0 on the desktop): the
/// strip is `phone_strip_height` tall and the window's frame starts that much higher, so the content
/// keeps its place (see [`window_styled`]).
pub fn phone_title_grow(k: f32, factor: f32, dp: Option<f32>) -> f32 {
    let tb = strip_height(k, factor);
    phone_strip_height(tb, dp) - tb
}

/// A window's rect with its title strip grown `grow` px upward (the frame starts that much higher and
/// is that much taller), when the screen's safe top `safe_top` leaves that room above it; else `r` as it is.
pub fn grown_upward(r: Rect, grow: f32, safe_top: f32) -> Rect {
    if grow > 0.0 && r.y - grow >= safe_top { Rect::new(r.x, r.y - grow, r.w, r.h + grow) } else { r }
}

/// The frames 0x48d3f8 draws on every window: a 2 px frame round it and a 2 px line from
/// (2, 27) to (w − 2, 29) under the title. `px` as in [`window_inner_frames`].
pub fn window_frames(r: Rect, px: f32) {
    window_frames_grown(r, px, 0.0);
}

/// [`window_frames`] with the title strip taller by `grow` px (Razdor's text size grows the
/// strip, see `window_styled`): the line under the title moves down with it.
fn window_frames_grown(r: Rect, px: f32, grow: f32) {
    steel_frame(r, px, 2);
    steel_frame(Rect::new(r.x + 2.0 * px, r.y + 27.0 * px + grow, r.w - 4.0 * px, 2.0 * px), px, 2);
}

/// The original's empty worn-item slot (0xae2860 with its alpha 0xae2864, built at start-up
/// near 0x4dbc26): a 55 px square, black inside at alpha 0x66 (40%, 0x47650c fills the alpha
/// 1..53 with grey 0x66), with a 1 px ring at full alpha whose colours run clockwise from the
/// top left along the first row of `SteelLine.lit` (0x48d1d4 with colour −1 reads 0xae2538's
/// row 0 at a running index: the top left to right, the right side down, the bottom right to
/// left, the left side up). Drawn in `r`, the 55 px scaled to its width.
pub fn empty_item_slot(r: Rect) {
    const SIDE: f32 = 55.0;
    let px = r.w / SIDE;
    draw_rectangle(r.x + px, r.y + px, r.w - 2.0 * px, r.h - 2.0 * px, Color::new(0.0, 0.0, 0.0, 0x66 as f32 / 255.0));
    steel_frame(r, px, 1);
}

/// The window's close box; true when clicked.
pub fn close_button(x: f32, y: f32, s: f32, red: bool) -> bool {
    // The cross is drawn as it always was; a phone's click area is finger-sized (`touch_hit`). The
    // title strip ends 3 px under the cross (`title_bar_styled`: the cross at 3 px, the strip's height less 6).
    let k = k();
    let cross = Rect::new(x, y, s, s);
    let hit = touch_hit(cross, y + s + 3.0 * k, super::display::dp_px(), (screen_width(), screen_height()));
    close_button_at(cross, hit, red)
}

/// A close cross drawn at `cross` whose click area is `hit` (a phone's title cross is its own click
/// area: [`title_bar_styled`]); true when clicked.
fn close_button_at(cross: Rect, hit: Rect, red: bool) -> bool {
    let (x, y, s) = (cross.x, cross.y, cross.w);
    let (hover, down, pressed) = super::widgets::button_state(hit, true);
    let name = match (red, down) {
        (true, false) => "CloseButtonRed-Up",
        (true, true) => "CloseButtonRed-Down2",
        (false, false) => "CloseButtonGreen-Up",
        (false, true) => "CloseButtonGreen-Down2",
    };
    if let Some(t) = win(name) {
        tex(&t, Rect::new(x, y, s, s), WHITE);
    } else {
        draw_rectangle(x, y, s, s, Color::new(0.05, 0.08, 0.06, 1.0));
        draw_rectangle_lines(x, y, s, s, 1.5, if hover { GOLD } else { SILVER });
        let c = if red { RED_TEXT } else { Color::new(0.4, 0.9, 0.5, 1.0) };
        let p = s * 0.25;
        draw_line(x + p, y + p, x + s - p, y + s - p, 2.0, c);
        draw_line(x + s - p, y + p, x + p, y + s - p, 2.0, c);
    }
    pressed
}

/// The touch target of a phone's close cross, in dp (Android's and iOS's minimum).
pub const CLOSE_DP: f32 = 44.0;

/// The click area of a close cross drawn at `cross`, whose strip ends at `strip_bottom` (px). On a
/// phone (`dp`, screen px per dp) it is a square of 44 dp centred on the cross, so it reaches up and
/// outside the window, but its bottom is never below the strip's, so it never takes a click from the
/// window's content below. Clamped to the screen (`screen` px). The desktop's is the cross itself.
pub fn touch_hit(cross: Rect, strip_bottom: f32, dp: Option<f32>, screen: (f32, f32)) -> Rect {
    let Some(dp) = dp.filter(|d| *d > 0.0) else { return cross };
    let side = (CLOSE_DP * dp).round();
    let x = (cross.center().x - side / 2.0).clamp(0.0, (screen.0 - side).max(0.0));
    // The square's bottom is the strip's; at the screen's top it is cut to the strip's height.
    let h = side.min(strip_bottom.max(0.0));
    Rect::new(x, strip_bottom - h, side, h)
}

/// The close cross of a title bar whose strip is `strip` (its top and its width): the strip's height
/// less 6 px, 3 px down, 4 px in from the right. On a phone the strip is taller (`phone_strip_height`),
/// so the cross is larger and still inside it (a phone choice, not in the original).
pub fn title_close_cross(strip: Rect, k: f32) -> Rect {
    let s = strip.h - 6.0 * k;
    Rect::new(strip.right() - s - 4.0 * k, strip.y + 3.0 * k, s, s)
}

/// The title's room (its width, centred) kept clear of a close cross that sits inside the strip:
/// `room` less the cross's side and gap on each side. A cross outside the strip leaves `room` as it is.
fn room_clear_of(room: f32, strip: Rect, cross: Rect, k: f32) -> f32 {
    if cross.x < strip.x || cross.right() > strip.right() {
        return room;
    }
    room.min(strip.w - 2.0 * (strip.right() - cross.x) - 8.0 * k)
}

/// Title bar of a window: the window's own skin, the title centred; `ornate` puts the
/// corner ornaments on a wide one, including town and building windows.
/// Returns true when its close box (if any) was clicked.
fn title_bar_styled(r: Rect, title: &str, skin: Skin, closable: bool, ornate: bool, title_color: Option<Color>, factor: f32) -> bool {
    let k = k();
    surface(r, skin);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.15));
    // Ornaments at both ends of a wide title: orange on the battle's red, silver-green on
    // marble (the load window, the army screen).
    // They give way to a title that would not fit between them.
    // Every window title in the titles' face (the original draws a plain one in its body font).
    let face = super::typography::Role::Title.face();
    // Razdor's text size scales the titles too (the owner's request: phones' titles are small);
    // at the original's size this is the title's own.
    let size = super::typography::Role::Title.size((16.0 * k).round()) * factor;
    let title_w = super::dt_font::with_face(face, || measure(title, size).width);
    let ornate = ornate && matches!(skin, Skin::Red | Skin::Marble) && r.w > 400.0 && title_w < r.w - 300.0 * k;
    if let Some(t) = win_fx("Corner-Left", Fx::KeyBlack).filter(|_| ornate) {
        let tint = if skin == Skin::Red { Color::new(1.0, 0.30, 0.0, 1.0) } else { Color::new(0.55, 0.8, 0.7, 0.8) };
        let h = r.h * 0.8;
        let w = t.width() * h / t.height();
        tex(&t, Rect::new(r.x + k, r.y + (r.h - h) / 2.0, w, h), tint);
        if let Some(t2) = win_fx("Corner-Right", Fx::KeyBlack) {
            // Let the right ornament continue under the close box; the button is drawn
            // later and stays visually on top of the pattern.
            let right = r.x + r.w - w - k;
            tex(&t2, Rect::new(right, r.y + (r.h - h) / 2.0, w, h), tint);
        }
    }
    let color = title_color.unwrap_or(if skin == Skin::Red { GOLD } else { CREAM });
    // A phone's cross sits inside its taller strip (a phone choice, not in the original): the title
    // keeps clear of it; the desktop's cross is narrower than the room left by the formula.
    let phone = super::display::dp_px();
    let cross = closable.then(|| title_close_cross(r, k));
    let room = if ornate { r.w - 300.0 * k } else { r.w - 2.0 * r.h - 8.0 * k };
    let room = match cross {
        Some(c) if phone.is_some() => room_clear_of(room, r, c, k),
        _ => room,
    };
    // The title's size is the one asked for (Razdor's text size), not the interface scale's native.
    let block = || super::dt_font::with_face(face, || {
        // A wide face shrinks to fit first (at most to 70%); what still does not fit is cut.
        let size = super::widgets::fit_size(title, room, size);
        let t = super::widgets::ellipsize(title, room, size);
        shadow_centered(&t, r.x + r.w / 2.0, r.y + r.h * 0.5 + size * 0.36, size, color);
    });
    // The original's size (factor 1) is drawn as it always was; a grown title takes its size.
    if factor == FIXED_TITLE { block() } else { super::dt_font::with_legacy_metrics(block) }
    // The cross's click area: the cross itself on the desktop; on a phone a finger-sized square over it,
    // never below the strip (`touch_hit`).
    match cross {
        Some(c) => {
            let hit = touch_hit(c, r.bottom(), phone, (screen_width(), screen_height()));
            close_button_at(c, hit, skin == Skin::Red)
        }
        None => false,
    }
}

/// A modal window: background, silver frame, title bar. Returns the area below the title
/// bar and whether the close box was clicked.
pub fn window(r: Rect, title: &str, skin: Skin, closable: bool) -> (Rect, bool) {
    window_styled(r, title, skin, closable, true, None, FIXED_TITLE)
}

pub fn window_with_title_color(r: Rect, title: &str, skin: Skin, closable: bool, color: Color) -> (Rect, bool) {
    window_styled(r, title, skin, closable, true, Some(color), FIXED_TITLE)
}

/// [`window`] for a window that can grow with the text (an event or message window): its title
/// takes «Размер текста» (`growing_title`), and its strip grows (`title_grow`).
pub fn window_grown(r: Rect, title: &str, skin: Skin, closable: bool) -> (Rect, bool) {
    window_styled(r, title, skin, closable, true, None, growing_title())
}

/// [`window_with_title_color`] for a window that can grow with the text (see [`window_grown`]).
pub fn window_with_title_color_grown(r: Rect, title: &str, skin: Skin, closable: bool, color: Color) -> (Rect, bool) {
    window_styled(r, title, skin, closable, true, Some(color), growing_title())
}

/// The battle exit window uses the artwork's own pixel scale for its title and nested frames.
pub fn battle_exit_frame(r: Rect, title: &str, px: f32) -> bool {
    BIGGEST.with(|b| {
        if b.get().is_none_or(|old| old.w * old.h < r.w * r.h) { b.set(Some(r)); }
    });
    if let Some(t) = win_scaled("Win-red", [1024; 3], [-20, -40, -100]) {
        tile(&t, r, px, WHITE);
    } else {
        surface(r, Skin::Red);
    }
    for (name, right) in [("Corner-Left", false), ("Corner-Right", true)] {
        if let Some(t) = win_fx(name, Fx::KeyBlack) {
            let (w, h) = (t.width() * px, t.height() * px);
            let x = if right { r.right() - w - px } else { r.x + px };
            tex(&t, Rect::new(x, r.y + 2.0 * px, w, h), Color::new(1.0, 0.30, 0.0, 1.0));
        }
    }
    let yellow = Color::from_rgba(255, 255, 155, 255);
    let face = super::dt_font::Face::Subtitle;
    let size = super::dt_font::sheet_size(face, px);
    super::dt_font::with_face(face, || shadow_centered(title, r.center().x, r.y + 19.0 * px, size, yellow));
    close_button(r.right() - 23.0 * px, r.y + 3.0 * px, 20.0 * px, true)
}

fn window_styled(r: Rect, title: &str, skin: Skin, closable: bool, ornate: bool, title_color: Option<Color>, factor: f32) -> (Rect, bool) {
    let k = k();
    // The title strip is 26 px at the original's size, taller by the title's factor (a growing window
    // only; `FIXED_TITLE` keeps 26 px), and 1.5× on a phone (`phone_strip_height`, a phone choice). The
    // phone's extra height grows upward: the frame starts `grow` higher, so the strip ends where the
    // desktop's does and `inner` below is the same as on the desktop. Where the screen's safe top leaves
    // no room above, the window keeps its rect and the strip covers its top; the callers place their
    // windows with that room (`window_layout`, `battle_view`; `phone_title_grow`).
    let dp = super::display::dp_px();
    let tb0 = strip_height(k, factor);
    let tb = phone_strip_height(tb0, dp);
    let grow = tb - tb0;
    let safe = super::display::safe_area();
    let r = grown_upward(r, grow, safe.top);
    BIGGEST.with(|b| {
        if b.get().is_none_or(|o| o.w * o.h < r.w * r.h) {
            b.set(Some(Rect::new(r.x, r.y, r.w + 6.0 * k, r.h + 6.0 * k)));
        }
    });
    draw_rectangle(r.x + 6.0 * k, r.y + 6.0 * k, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.35));
    surface(r, skin);
    // A one-finger slide over the window pans it when the canvas overflows (`ui::pan`).
    super::pan::window_area(r);
    let closed = title_bar_styled(Rect::new(r.x, r.y, r.w, tb), title, skin, closable, ornate, title_color, factor);
    // The frame round the window and the line under its title (0x48d3f8): the line stays where the
    // desktop's is, under the strip's 26 px (or its text-size growth).
    window_frames_grown(r, opx(), tb - (26.0 * k).round());
    // Each caller draws the frames for its own content and footer.
    (Rect::new(r.x + 2.0 * k, r.y + tb + 1.0, r.w - 4.0 * k, r.h - tb - 3.0 * k), closed)
}

/// The original's 628×25 hint strip `ae2548` (built at 0x4dbcf3), under the army, barracks,
/// garrison and battle hints: Win-paper's rows from y = 200 with (148, 132, 148) taken off,
/// and the title corners taken off it at x = −51 and x = 555, y = 1, uncut otherwise
/// ([`divider_ornaments`]). Razdor's strips are wider or taller than 628×25: the paper repeats
/// across, the corners sit at both ends, centred in the height; the silver edge is Razdor's.
pub fn army_divider(r: Rect) {
    army_divider_fill(r);
    silver_frame(r, k().max(1.0));
}

/// Content of a strip whose structural border is supplied by its enclosing window.
pub fn army_divider_fill(r: Rect) {
    let k = k();
    if let Some(t) = win_scaled("Win-paper", [1024; 3], [-148, -132, -148]) {
        let src_h = (r.h / k).min(t.height() - DIVIDER_PAPER_Y);
        let mut x = r.x;
        while x < r.right() && src_h > 0.0 {
            let w = (t.width() * k).min(r.right() - x);
            tex_src(&t, Rect::new(0.0, DIVIDER_PAPER_Y, w / k, src_h), Rect::new(x, r.y, w, r.h), WHITE);
            x += w;
        }
    } else {
        text_box_base(r);
    }
    let ornament_h = (23.0 * k).min(r.h);
    let y = r.y + (r.h - ornament_h) / 2.0;
    for (name, right) in [("Corner-Left", false), ("Corner-Right", true)] {
        if let Some(t) = win(name) {
            t.set_filter(FilterMode::Nearest);
            let (src_x, src_w, dst_x) = divider_ornaments(t.width(), r.w / k, right);
            let src = Rect::new(src_x, 0.0, src_w, ornament_h / k);
            subtractive(|| tex_src(&t, src, Rect::new(r.x + dst_x * k, y, src_w * k, ornament_h), WHITE));
        }
    }
}

/// Win-paper with `minus` (r, g, b) taken off, its texels from `from` on (the original copies
/// the paper from that point with 0x476b8c, then takes the colour off with 0x4768f8 in its
/// subtracting mode), drawn over `r` at the screen's scale; the paper repeats past its edge.
/// The market's icon column and the sanctuary's spell list are built this way.
pub fn paper_minus(r: Rect, from: (f32, f32), minus: [i32; 3]) {
    let k = k();
    let Some(t) = win_scaled("Win-paper", [1024; 3], minus.map(|m| -m)) else {
        surface(r, Skin::Brown);
        return;
    };
    let (tw, th) = (t.width(), t.height());
    if tw < 1.0 || th < 1.0 || r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    let (mut sy, mut y) = (from.1.rem_euclid(th), r.y);
    while y < r.bottom() {
        let h = ((th - sy) * k).min(r.bottom() - y);
        let (mut sx, mut x) = (from.0.rem_euclid(tw), r.x);
        while x < r.right() {
            let w = ((tw - sx) * k).min(r.right() - x);
            tex_src(&t, Rect::new(sx, sy, w / k, h / k), Rect::new(x, y, w, h), WHITE);
            x += w;
            sx = 0.0;
        }
        y += h;
        sy = 0.0;
    }
}

/// The hint strip's paper starts this far down Win-paper (0x4dbd0b: 0x476b8c from (0, 200)).
const DIVIDER_PAPER_Y: f32 = 200.0;

/// Where a title corner lands on a hint strip `strip_w` wide, in the art's pixels: the source
/// column it starts at, the width shown and the x on the strip. Corner-Left is drawn at
/// x = −51 (0x4dbd58), so only its columns from 51 show; Corner-Right at 555 on the 628 strip
/// (0x4dbd74), so its first 73 columns show, against the right end.
fn divider_ornaments(art_w: f32, strip_w: f32, right: bool) -> (f32, f32, f32) {
    let shown = (art_w - 51.0).clamp(0.0, (strip_w / 2.0).max(0.0));
    if right { (0.0, shown, strip_w - shown) } else { (art_w - shown, shown, 0.0) }
}

/// A thin ornamental divider (the strip between the battle's formations, without text).
pub fn divider(r: Rect) {
    if let Some(t) = win("HintFrame") {
        three_slice(&t, r, 40.0, WHITE);
    } else {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.06, 0.04, 0.03, 0.75));
        draw_line(r.x, r.y + 2.0, r.x + r.w, r.y + 2.0, 1.0, SILVER);
        draw_line(r.x, r.y + r.h - 2.0, r.x + r.w, r.y + r.h - 2.0, 1.0, SILVER);
    }
}

/// The hint of a strip, drawn directly over its parchment background.
pub fn hint_text(r: Rect, hint: &str, color: Color) {
    super::typography::Role::Hint.with(|| hint_text_in_face(r, hint, color));
}

fn hint_text_in_face(r: Rect, hint: &str, color: Color) {
    let room = r.w - 3.0 * r.h;
    // Smaller if it must be, then shortened with "…".
    let native = super::dt_font::native_size(super::dt_font::Face::Body, (r.h * 0.62).round().max(11.0));
    let size = super::widgets::fit_size(hint, room, native);
    let s = super::widgets::ellipsize(hint, room, size);
    shadow_centered(&s, r.x + r.w / 2.0, r.y + r.h * 0.5 + size * 0.36, size, color);
}

/// Corner ornaments of a text box.
fn corners(r: Rect) {
    let s = (26.0 * k()).min(r.h / 2.0).min(r.w / 2.0);
    if s <= 0.0 {
        return;
    }
    for (name, x, y) in [
        ("Corner_Frame-LU", r.x, r.y),
        ("Corner_Frame-RU", r.x + r.w - s, r.y),
        ("Corner_Frame-LD", r.x, r.y + r.h - s),
        ("Corner_Frame-RD", r.x + r.w - s, r.y + r.h - s),
    ] {
        if let Some(t) = win(name) {
            tex(&t, Rect::new(x, y, s, s), WHITE);
        }
    }
}

/// Tile mirrored parchment ornaments at their native scale. Narrow panels reveal less of
/// the same source strip on each side instead of squeezing the flourish horizontally.
fn tile_side_strips(t: &Texture2D, r: Rect, max_frac: f32, flip_left: bool, tint: Color) {
    let scale = k();
    let strip_w = (t.width() * scale).min(r.w * max_frac);
    if strip_w <= 0.0 || r.h <= 0.0 {
        return;
    }
    let src_w = strip_w / scale;
    let mut y = r.y;
    while y < r.y + r.h {
        let h = (t.height() * scale).min(r.y + r.h - y);
        let src_h = h / scale;
        let src = Rect::new(0.0, 0.0, src_w, src_h);
        let left = Rect::new(r.x, y, strip_w, h);
        let right = Rect::new(r.x + r.w - strip_w, y, strip_w, h);
        if flip_left { tex_src_flipped_x(t, src, left, tint); } else { tex_src(t, src, left, tint); }
        if flip_left { tex_src(t, src, right, tint); } else { tex_src_flipped_x(t, src, right, tint); }
        y += h;
    }
}

pub fn text_box_base(r: Rect) {
    surface(r, Skin::Brown);
}

/// The brown text box's side ornaments (0x48e4e8): Big_Frame_1a 4 px in from the left and
/// Big_Frame_2a 4 px in from the right, both loaded with (−223, −215, 0) added (0x4dbc4f,
/// 0x4dbc6a), repeated down from the top and taken off the paper (0x476790 mode 1). A box no
/// wider than both and 8 px has none.
pub fn text_box_sides(r: Rect) {
    let k = k();
    let left = win_scaled("Big_Frame_1a", [1024; 3], [-223, -215, 0]);
    let right = win_scaled("Big_Frame_2a", [1024; 3], [-223, -215, 0]);
    let (Some(left), Some(right)) = (left, right) else { return };
    if r.w / k <= right.width() * 2.0 + 8.0 {
        return;
    }
    subtractive(|| {
        for (t, x) in [(&left, r.x + 4.0 * k), (&right, r.right() - (right.width() + 4.0) * k)] {
            t.set_filter(FilterMode::Nearest);
            let mut y = r.y;
            while y < r.bottom() {
                let h = (t.height() * k).min(r.bottom() - y);
                tex_src(t, Rect::new(0.0, 0.0, t.width(), h / k), Rect::new(x, y, t.width() * k, h), WHITE);
                y += h;
            }
        }
    });
}

/// The brown text box's silver edge and its four 34 px corners, at the art's size (0x48e4e8:
/// 0x476a3c at (0, 0), (w − 34, 0), (w − 34, h − 34), (0, h − 34)).
pub fn text_box_frame(r: Rect) {
    silver_frame(r, k().max(1.0));
    for (name, right, bottom) in [
        ("Corner_Frame-LU", false, false),
        ("Corner_Frame-RU", true, false),
        ("Corner_Frame-LD", false, true),
        ("Corner_Frame-RD", true, true),
    ] {
        if let Some(t) = win(name) {
            let (w, h) = (t.width() * k(), t.height() * k());
            let x = if right { r.right() - w } else { r.x };
            let y = if bottom { r.bottom() - h } else { r.y };
            tex(&t, Rect::new(x, y, w, h), WHITE);
        }
    }
}

pub fn text_box_decoration(r: Rect) {
    text_box_sides(r);
    text_box_frame(r);
}

/// The brown text box without its corner pieces (0x48e4e8 asked for its frame but not its
/// corners: the hero window's boxes, 0x4d1814 → 0x48e750 at 4d18c5, 4d190d, 4d1924).
pub fn text_box_no_corners(r: Rect) {
    text_box_base(r);
    text_box_sides(r);
    silver_frame(r, k().max(1.0));
}

/// The red-brown parchment box of story and description texts (0x48e4e8).
pub fn text_box(r: Rect) {
    text_box_base(r);
    text_box_decoration(r);
}

/// A translucent dark backing over art, for the village's text panel.
pub fn shaded_text_box(r: Rect, alpha: f32) {
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.02, 0.015, 0.01, alpha.clamp(0.0, 1.0)));
    silver_frame(r, (k() * 0.9375).max(1.0));
    corners(r);
}

/// The original parchment treatment used behind a battle unit's hover details.
pub fn unit_parchment(r: Rect) {
    paper_panel(r);
    if let Some(t) = win_fx("UnitFrame", Fx::Shade) {
        tex(&t, r, Color::new(1.0, 1.0, 1.0, 0.8));
    }
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, Color::new(0.75, 0.6, 0.3, 1.0));
}

/// Draw the parchment background and its side ornament without a frame.
pub fn parchment_fill(r: Rect, ornaments: bool) {
    surface(r, Skin::Paper);
    if ornaments {
        // These panels use the repeatable gold side flourish, rather than the
        // silver portrait frame. Keep its original scale and crop the final tile.
        if let Some(t) = win_fx("Big_Frame_2", Fx::KeyBlack) {
            t.set_filter(FilterMode::Linear);
            let opacity = super::window_layout::visual().white_ornament;
            tile_side_strips(&t, r, 0.22, true, Color::new(1.0, 0.78, 0.05, opacity));
        }
    }
    // The original darkens the parchment towards the edges.
    let visual = super::window_layout::visual();
    let e = (visual.paper_edge_width * k()).min(r.w / 2.0).min(r.h / 2.0);
    let dark = Color::new(0.25, 0.12, 0.02, visual.paper_edge_opacity);
    draw_rectangle(r.x, r.y, r.w, e, dark);
    draw_rectangle(r.x, r.y + r.h - e, r.w, e, dark);
    draw_rectangle(r.x, r.y + e, e, r.h - 2.0 * e, dark);
    draw_rectangle(r.x + r.w - e, r.y + e, e, r.h - 2.0 * e, dark);
}

/// The bands grey 48 is taken off round the hall list's edges, in its pixels (0x4bef1b, 0x4befcd).
const QUEST_LIST_BANDS: [f32; 2] = [2.0, 4.0];

/// How much each channel loses `d` pixels in from the hall list's edge.
#[cfg(test)]
fn quest_list_edge_loss(d: f32) -> u32 {
    QUEST_LIST_BANDS.iter().filter(|&&b| d < b).count() as u32 * 48
}

/// The main hall's quest and rumour list (0x4bedec): Win-paper as it is, copied from (1, 1);
/// the brown box's side ornaments taken off it ([`text_box_sides`]: on light paper they come
/// out gold, with no dark rim); then grey 48 taken off a 2 px band round the edges and again
/// off a 4 px band (0x4bef1b..0x4bf00f), so the outer 2 px lose 96 and the next 2 px 48.
///
/// Kept from the original: its ornament loop places every copy at the same height (the count
/// times the art's height, not the copy's index, 0x4beecc), which only matters for a list
/// taller than the art (397 px); Razdor's is shorter, so one copy from the top shows.
pub fn quest_list_parchment(r: Rect) {
    let k = k();
    match win("Win-paper") {
        Some(t) => tile(&t, r, k, WHITE),
        None => surface(r, Skin::Paper),
    }
    text_box_sides(r);
    let grey = Color::from_rgba(48, 48, 48, 255);
    subtractive(|| {
        for band in QUEST_LIST_BANDS {
            let b = (band * k).min(r.w / 2.0).min(r.h / 2.0);
            draw_rectangle(r.x, r.y, r.w, b, grey);
            draw_rectangle(r.x, r.bottom() - b, r.w, b, grey);
            draw_rectangle(r.x, r.y + b, b, r.h - 2.0 * b, grey);
            draw_rectangle(r.right() - b, r.y + b, b, r.h - 2.0 * b, grey);
        }
    });
}

/// Light parchment, e.g. the unit panel; ornaments darken the corners when the art is there.
pub fn parchment(r: Rect, ornaments: bool) {
    parchment_fill(r, ornaments);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, Color::new(0.75, 0.6, 0.3, 1.0));
}

// ------------------------------------------------------------------------------------------
// Buttons
// ------------------------------------------------------------------------------------------

/// A text button on green marble with a silver edge (the original's `Btn*`). True when
/// clicked this frame.
pub fn marble_button(r: Rect, label: &str, enabled: bool, hover: bool) {
    marble_button_tinted(r, label, enabled, hover, false);
}

/// The quest action uses the same marble pattern in the original's warm red palette.
/// The event window's buttons in its defeat look: the green ones with their channels turned
/// (red takes green: 0x4dba90 → 0x48dd40(2, 3, 1); 0x4aa263 swaps them in).
pub fn defeat_button(r: Rect, label: &str, enabled: bool, hover: bool) {
    let name = if r.w < 110.0 { "Btn1" } else if r.w < 145.0 { "Btn2" } else { "Btn3" };
    let state = if hover && enabled { "Down" } else { "Up" };
    match win_fx(&format!("{name}{state}"), Fx::RotateRgb) {
        Some(t) if enabled => {
            three_slice(&t, r, 14.0, WHITE);
            button_label(r, label, enabled, hover, false);
        }
        _ => marble_button(r, label, enabled, hover),
    }
}

/// [`defeat_button`] with its art named (`Btn1`, `Btn2`, `Btn3`) rather than picked by the
/// width on screen, for windows laid out in the original's pixels times a scale.
pub fn defeat_button_art(r: Rect, art: &str, label: &str, hover: bool) {
    let state = if hover { "Down" } else { "Up" };
    match win_fx(&format!("{art}{state}"), Fx::RotateRgb) {
        Some(t) => {
            three_slice(&t, r, 14.0, WHITE);
            button_label(r, label, true, hover, false);
        }
        None => marble_button(r, label, true, hover),
    }
}

/// The quest action uses the same button art, with the original's red enabled state.
pub fn quest_button(r: Rect, label: &str, enabled: bool, hover: bool) {
    marble_button_tinted(r, label, enabled, hover, true);
}

fn marble_button_tinted(r: Rect, label: &str, enabled: bool, hover: bool, quest_red: bool) {
    let name = if r.w < 110.0 { "Btn1" } else if r.w < 145.0 { "Btn2" } else { "Btn3" };
    let state = if hover && enabled { "Down" } else { "Up" };
    let fx = if !enabled { Fx::Grey } else if quest_red { Fx::ButtonRed } else { Fx::Plain };
    if let Some(t) = win_fx(&format!("{name}{state}"), fx) {
        three_slice(&t, r, 14.0, WHITE);
    } else {
        let bg = match (enabled, hover, quest_red) {
            (false, _, _) => Color::new(0.2, 0.22, 0.21, 1.0),
            (true, true, true) => Color::new(0.55, 0.09, 0.04, 1.0),
            (true, false, true) => Color::new(0.45, 0.06, 0.03, 1.0),
            (true, true, false) => Color::new(0.16, 0.36, 0.28, 1.0),
            (true, false, false) => Color::new(0.10, 0.25, 0.19, 1.0),
        };
        draw_rectangle(r.x, r.y, r.w, r.h, bg);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, if enabled { SILVER } else { SILVER_DARK });
        draw_rectangle_lines(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0, 1.0, Color::new(0.0, 0.0, 0.0, 0.5));
    }
    button_label(r, label, enabled, hover, quest_red);
}

/// A button's label, centred, smaller if it must be to fit.
pub fn button_label(r: Rect, label: &str, enabled: bool, _hover: bool, _quest_red: bool) {
    let color = if !enabled {
        Color::new(0.6, 0.6, 0.58, 1.0)
    } else {
        Color::new(1.0, 0.98, 0.94, 1.0)
    };
    // Button labels in the buttons' and names' face.
    super::typography::Role::Button.with(|| {
        let mut size = (r.h * 0.56).clamp(12.0, (22.0 * k()).max(12.0)).round();
        // In the original's fonts a label is the sheet at the button's own scale (the
        // original's buttons are 36 px high), never larger: 0.56 of a tall button made
        // labels run edge to edge on large screens.
        if super::dt_font::mode() == super::dt_font::FontMode::Original {
            size = size.min(label_size_original(r.h));
        }
        let size = super::widgets::fit_size(label, r.w - 10.0, size);
        let label = super::widgets::ellipsize(label, r.w - 10.0, size);
        let label = label.as_str();
        let d = measure(label, size);
        let x = r.x + (r.w - d.width) / 2.0;
        let y = r.y + (r.h + d.offset_y) / 2.0 - 1.0;
        let offset = (size / 22.0).max(1.0);
        text(label, x + offset, y + offset, size, Color::new(0.0, 0.0, 0.0, 0.35 * color.a));
        text(label, x, y, size, color);
    });
}

/// A button label's size in the original's fonts: the Benguiat sheet at the button's
/// scale against the original's 36 px buttons, snapped like the sheets.
fn label_size_original(h: f32) -> f32 {
    super::dt_font::sheet_size(super::dt_font::Face::Subtitle, super::dt_font::whole_scale(h / 36.0))
}

/// The small pill buttons of the barracks ("Hire" green, "Heal" blue). They pay: the
/// press plays `Item-Gold` (interface.md §14).
pub fn pill_button(r: Rect, label: &str, enabled: bool, green: bool) -> bool {
    let (hover, down, pressed) = super::widgets::button_state(r, enabled);
    let name = if !enabled {
        "smb-disable"
    } else if down {
        "smb-down"
    } else {
        "smb-up"
    };
    match win_fx(name, if green && enabled { Fx::KeyBlackSwapGreenBlue } else { Fx::KeyBlack }) {
        Some(t) if green && enabled => {
            // The blue pill with its green and blue swapped, as the original makes the green
            // one from smb-up and smb-down at start (0x48dd40(…, 1, 3, 2)).
            three_slice(&t, r, 12.0, WHITE);
        }
        Some(t) => three_slice(&t, r, 12.0, WHITE),
        None => {
            let c = match (enabled, green) {
                (false, _) => Color::new(0.35, 0.35, 0.35, 1.0),
                (true, true) => Color::new(0.15, 0.55, 0.15, 1.0),
                (true, false) => Color::new(0.12, 0.25, 0.65, 1.0),
            };
            draw_rectangle(r.x, r.y, r.w, r.h, c);
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, if hover { GOLD } else { SILVER });
        }
    }
    super::typography::Role::Body.with(|| {
        let size = super::widgets::fit_size(label, r.w - 8.0, (r.h * 0.62).round().clamp(11.0, (18.0 * k()).max(11.0)));
        let label = super::widgets::ellipsize(label, r.w - 8.0, size);
        let label = label.as_str();
        let d = measure(label, size);
        shadow_text(label, r.x + (r.w - d.width) / 2.0, r.y + (r.h + d.offset_y) / 2.0 - 1.0, size, if enabled { WHITE } else { Color::new(0.8, 0.8, 0.8, 1.0) });
    });
    if pressed {
        super::audio::cue(super::audio::Cue::Gold);
    }
    pressed
}

// ------------------------------------------------------------------------------------------
// Cards and cells
// ------------------------------------------------------------------------------------------

/// The picture of an empty formation cell: where each kind of unit belongs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellIcon {
    /// Front row: crossed swords.
    Swords,
    /// Back row: a bow on a shield.
    Bow,
    /// Reserve (the ends of the wide back row, or the reserve row): a tent.
    Tent,
}

impl CellIcon {
    /// The icon of `slot`: the reserve's cells are the tents, the four ends of the 2 × 6
    /// grid in the short row, the back line's two ends in the wide one, whose front line is
    /// all swords (the cell backgrounds 0x4de2xx bakes in).
    pub fn of(_f: razdor::rules::formation::Formation, slot: razdor::rules::formation::Slot) -> CellIcon {
        use razdor::rules::formation::Row;
        match slot.row {
            Row::Reserve => CellIcon::Tent,
            Row::Front => CellIcon::Swords,
            Row::Back => CellIcon::Bow,
        }
    }

    fn art(self) -> &'static str {
        match self {
            CellIcon::Swords => "wb1",
            CellIcon::Bow => "wb2",
            CellIcon::Tent => "wb3",
        }
    }
}

/// Placeholder drawings of the cell icons, grey like the original's.
fn draw_cell_icon_fallback(icon: CellIcon, r: Rect) {
    let c = Color::new(0.62, 0.62, 0.64, 0.75);
    let d = Color::new(0.25, 0.25, 0.27, 0.8);
    let (cx, cy, s) = (r.x + r.w / 2.0, r.y + r.h / 2.0, r.w.min(r.h));
    match icon {
        CellIcon::Swords => {
            for dir in [-1.0f32, 1.0] {
                let (x0, y0) = (cx - dir * s * 0.34, cy - s * 0.34);
                let (x1, y1) = (cx + dir * s * 0.30, cy + s * 0.30);
                draw_line(x0, y0, x1, y1, s * 0.06, c);
                // Cross guard and pommel.
                let (gx, gy) = (cx + dir * s * 0.18, cy + s * 0.18);
                draw_line(gx - s * 0.08, gy - dir * s * 0.08, gx + s * 0.08, gy + dir * s * 0.08, s * 0.05, d);
                draw_circle(x1, y1, s * 0.04, c);
            }
        }
        CellIcon::Bow => {
            // Shield.
            let (w, h) = (s * 0.62, s * 0.72);
            let (x, y) = (cx - w / 2.0, cy - h / 2.0);
            draw_triangle(vec2(x, y + h * 0.45), vec2(x + w, y + h * 0.45), vec2(cx, y + h), d);
            draw_rectangle(x, y, w, h * 0.46, d);
            draw_rectangle_lines(x, y, w, h * 0.46, 2.0, c);
            draw_line(x, y + h * 0.45, cx, y + h, 2.0, c);
            draw_line(x + w, y + h * 0.45, cx, y + h, 2.0, c);
            // Bow and arrow.
            for i in 0..8 {
                let a0 = -1.1 + i as f32 * 0.275;
                let a1 = a0 + 0.275;
                let rr = s * 0.24;
                draw_line(cx + rr * a0.sin(), cy + rr * a0.cos(), cx + rr * a1.sin(), cy + rr * a1.cos(), 2.0, c);
            }
            draw_line(cx, cy - s * 0.26, cx, cy + s * 0.26, 1.5, c);
        }
        CellIcon::Tent => {
            let (w, h) = (s * 0.62, s * 0.62);
            let (x, y) = (cx - w / 2.0, cy - h / 2.0 + s * 0.04);
            draw_triangle(vec2(cx, y), vec2(x, y + h), vec2(x + w, y + h), d);
            draw_triangle(vec2(cx, y + h * 0.35), vec2(cx - w * 0.14, y + h), vec2(cx + w * 0.14, y + h), Color::new(0.1, 0.1, 0.1, 0.7));
            draw_line(cx, y, x, y + h, 2.0, c);
            draw_line(cx, y, x + w, y + h, 2.0, c);
            draw_line(cx, y, cx, y - s * 0.12, 2.0, c);
            draw_triangle(vec2(cx, y - s * 0.12), vec2(cx + s * 0.14, y - s * 0.09), vec2(cx, y - s * 0.05), c);
        }
    }
}

/// An empty cell: the dark square with its icon and, below it (`ornament`), the silver
/// ornament the original draws under empty cells.
pub fn empty_cell(r: Rect, icon: CellIcon, ornament: bool) {
    let k = k();
    if let Some(t) = win("Shadow-Empty") {
        // The shadow's box is a little larger than the card; its ornament hangs below.
        let sw = r.w * 101.0 / 92.0;
        let sh = sw * 141.0 / 101.0;
        let src_h = if ornament { 141.0 } else { 96.0 };
        tex_src(&t, Rect::new(0.0, 0.0, 101.0, src_h), Rect::new(r.x - 1.0 * k, r.y - 1.0 * k, sw, sh * src_h / 141.0), Color::new(1.0, 1.0, 1.0, 0.55));
    } else {
        draw_rectangle(r.x, r.y, r.w, r.w, Color::new(0.0, 0.0, 0.0, 0.4));
        if ornament {
            let (cx, y) = (r.x + r.w / 2.0, r.y + r.w + 4.0 * k);
            let c = Color::new(0.7, 0.7, 0.72, 0.6);
            draw_triangle(vec2(cx - r.w * 0.38, y), vec2(cx + r.w * 0.38, y), vec2(cx, y + 22.0 * k), Color::new(0.5, 0.5, 0.52, 0.35));
            draw_line(cx - r.w * 0.4, y, cx + r.w * 0.4, y, 2.0, c);
            draw_circle(cx, y + 10.0 * k, 4.0 * k, c);
        }
    }
    let sq = Rect::new(r.x, r.y, r.w, r.w);
    draw_rectangle_lines(sq.x, sq.y, sq.w, sq.h, 1.0, Color::new(0.55, 0.55, 0.58, 0.7));
    cell_icon(icon, sq);
}

/// A wounded unit's portrait, as the original shows it: dark red rises from the bottom to
/// the share of hits lost (the video, 19:52: the barracks' cards). Nothing at full health.
pub fn wounds(sq: Rect, hp: i32, max: i32) {
    let lost = wound_share(hp, max);
    if lost <= 0.0 {
        return;
    }
    let h = sq.h * lost;
    multiply(|| draw_rectangle(sq.x, sq.y + sq.h - h, sq.w, h, Color::new(0.66, 0.13, 0.08, 1.0)));
}

/// The share (0..1) of the portrait [`wounds`] fills.
pub fn wound_share(hp: i32, max: i32) -> f32 {
    if max <= 0 {
        return 0.0;
    }
    (1.0 - hp.max(0) as f32 / max as f32).clamp(0.0, 1.0)
}

/// A dead unit's portrait `sq` (493a64, at HP 0, over the wounds' red): `Army-Death` loaded
/// with 64 off each channel (48d02c at 4dbc26) and added (476790 mode 0). Without the art,
/// a dark veil and a red cross.
pub fn death_veil(sq: Rect) {
    if let Some(t) = win_scaled("Army-Death", [1024; 3], [-64; 3]) {
        additive(|| tex(&t, sq, WHITE));
    } else {
        let red = Color::new(0.9, 0.1, 0.1, 1.0);
        draw_rectangle(sq.x, sq.y, sq.w, sq.h, Color::new(0.0, 0.0, 0.0, 0.55));
        draw_line(sq.x + 10.0, sq.y + 10.0, sq.x + sq.w - 10.0, sq.y + sq.h - 10.0, 3.0, red);
        draw_line(sq.x + sq.w - 10.0, sq.y + 10.0, sq.x + 10.0, sq.y + sq.h - 10.0, 3.0, red);
    }
}

/// The shadow under an occupied card `r` (49462c, `r.w` the original's 92 px portrait):
/// `Shadow-Unit` (101×141, its alpha the mask, 476a3c) from the card's corner, 1 px up and
/// left of the portrait, its mask cleared over the card's 94×133 (47650c at 4dbc26), so
/// only the 7 px band on the right and the 8 rows below show. Without the art, a dark box.
pub fn unit_shadow(r: Rect) {
    let s = r.w / 92.0;
    if let Some(t) = win("Shadow-Unit") {
        let (x0, y0) = (r.x - s, r.y - s);
        tex_src(&t, Rect::new(94.0, 0.0, 7.0, 141.0), Rect::new(x0 + 94.0 * s, y0, 7.0 * s, 141.0 * s), WHITE);
        tex_src(&t, Rect::new(0.0, 133.0, 94.0, 8.0), Rect::new(x0, y0 + 133.0 * s, 94.0 * s, 8.0 * s), WHITE);
    } else {
        let k = k();
        draw_rectangle(r.x + 4.0 * k, r.y + 4.0 * k, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.45));
    }
}

/// The row of `exp` an experience card shows `elapsed_ms` into the won battle's hold
/// (4b09e8: `(now − start) / 3 mod 512`).
pub fn xp_strip_row(elapsed_ms: i64) -> i32 {
    (elapsed_ms / 3).rem_euclid(512) as i32
}

/// The experience card's picture over the portrait `sq` (4b0684): the portrait darkened
/// (grey 64 taken off, 4768f8 mode 1), the 92 rows of `exp` from `row` added (476790 mode 0;
/// the 92×604 strip loaded with red −250, green and blue −30), `exp-hole` at half
/// (48da5c, 512/1024) taken off (mode 1). False, and nothing drawn, without the art.
pub fn xp_veil(sq: Rect, row: i32) -> bool {
    let Some(hole) = win_scaled("exp-hole", [512; 3], [0; 3]) else { return false };
    xp_glow(sq, row) && {
        subtractive(|| tex(&hole, sq, WHITE));
        true
    }
}

/// The promoted unit's card while the promotion plays (4b05a8): [`xp_veil`] without the
/// `exp-hole`, the darkened portrait and the running `exp` strip only.
pub fn xp_glow(sq: Rect, row: i32) -> bool {
    let Some(strip) = win_scaled("exp", [1024; 3], [-250, -30, -30]) else { return false };
    let grey = 64.0 / 255.0;
    subtractive(|| draw_rectangle(sq.x, sq.y, sq.w, sq.h, Color::new(grey, grey, grey, 1.0)));
    let row = row.clamp(0, (strip.height() as i32 - 92).max(0)) as f32;
    additive(|| tex_src(&strip, Rect::new(0.0, row, 92.0, 92.0), sq, WHITE));
    true
}

/// Just the icon of an empty cell, filling `sq`.
pub fn cell_icon(icon: CellIcon, sq: Rect) {
    if let Some(t) = win(icon.art()) {
        tex(&t, sq, WHITE);
    } else {
        draw_cell_icon_fallback(icon, sq);
    }
}

/// The states of the original's card highlight (the `add194` / `add1c4` values, 4c57bc):
/// what a lit card or cell means. The value 0 (none) has no variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lit {
    /// 1: the AI's target on its own side (a heal or blessing): cyan.
    OwnTarget,
    /// 2: a cell the acting unit can act on or step to, on its own side: blue.
    CanAct,
    /// 3: the hostile target under the mouse (4c3e58, 4c4263), or the AI's target on the
    /// other side: orange.
    Aimed,
    /// 4: a hostile target: red.
    Enemy,
    /// 5: the unit that acts: green.
    Current,
}

impl Lit {
    /// The colour taken off the glow's grey rings at brightness `level` (0..15), as the
    /// original builds its 16 pictures per state at start-up (4dc118, 4de75e…4deb4f: 476374
    /// then 4768f8 mode 1).
    pub fn taken_off(self, level: u8) -> [i32; 3] {
        let l = level.min(15) as i32;
        match self {
            Lit::OwnTarget => [255, 255 - 8 * l, 255 - 16 * l],
            Lit::CanAct => [255, 255, 255 - 16 * l],
            Lit::Aimed => [255 - 16 * l, 255 - 8 * l, 255],
            Lit::Enemy => [255 - 16 * l, 255, 255],
            Lit::Current => [255, 255 - 16 * l, 255],
        }
    }

    /// The colour of ring `d` (0 at the portrait's edge, 7 innermost; the middle is black):
    /// grey 255 − 24d less [`Lit::taken_off`], each channel at least 0.
    pub fn ring(self, level: u8, d: u8) -> [u8; 3] {
        let g = if d >= 8 { 0 } else { 255 - 24 * d as i32 };
        self.taken_off(level).map(|t| (g - t).clamp(0, 255) as u8)
    }
}

/// The highlight's brightness at `ms` (4cd704, 4cd28a): `8 + tri((ms / 100) mod 15)`,
/// `tri(v) = v` up to 7, else `15 − v`, so it pulses 8..15..8 every 1.5 s.
pub fn lit_level(ms: u64) -> u8 {
    let v = ((ms / 100) % 15) as u8;
    8 + if v > 7 { 15 - v } else { v }
}

/// The original's card highlight over a portrait `sq` (92 px in the original): its picture
/// of 8 grey rings, 255 − 24 per pixel inwards, less the state's colour, added over the
/// portrait (493a64: 476790 mode 0 at the portrait's corner, under the card's signs).
pub fn card_highlight(sq: Rect, lit: Lit, ms: u64) {
    let s = sq.w / 92.0;
    let level = lit_level(ms);
    additive(|| {
        for d in 0..8u8 {
            let [r, g, b] = lit.ring(level, d);
            if r == 0 && g == 0 && b == 0 {
                continue;
            }
            let c = Color::from_rgba(r, g, b, 255);
            let f = d as f32 * s;
            let (x, y, w, h) = (sq.x + f, sq.y + f, sq.w - 2.0 * f, sq.h - 2.0 * f);
            draw_rectangle(x, y, w, s, c);
            draw_rectangle(x, y + h - s, w, s, c);
            draw_rectangle(x, y + s, s, h - 2.0 * s, c);
            draw_rectangle(x + w - s, y + s, s, h - 2.0 * s, c);
        }
    });
}

/// A coloured frame around a card or cell: green for the unit that acts, blue for a cell to
/// move to or a friendly target, red for a hostile target (green for the one under the mouse).
pub fn glow_frame(r: Rect, color: Color, strong: bool) {
    let k = k();
    let w = if strong { 3.0 * k } else { 2.0 * k };
    for i in 0..3 {
        let f = i as f32;
        let a = color.a * [0.95, 0.45, 0.2][i];
        let c = Color { a, ..color };
        draw_rectangle_lines(r.x - f * w * 0.7, r.y - f * w * 0.7, r.w + 2.0 * f * w * 0.7, r.h + 2.0 * f * w * 0.7, w, c);
    }
}

/// The original's signs along a card portrait's top (493a64): those that are on, 22 px badges
/// 23 px apart, 1 px below the portrait's top, from its left edge (`from_left`) or from the
/// right one. `sq` is the portrait (92 px in the original).
pub fn card_signs(sq: Rect, from_left: bool, signs: &[(bool, &str, Color)]) {
    let s = sq.w / 92.0;
    let bs = 22.0 * s;
    let (mut bx, step) = if from_left { (sq.x, 23.0 * s) } else { (sq.x + 70.0 * s, -23.0 * s) };
    for &(on, art, c) in signs {
        if on {
            badge(art, bx + bs / 2.0, sq.y + s + bs / 2.0, bs, c);
            bx += step;
        }
    }
}

/// A small badge in a card corner (`sign-*`, `army-*`, `Sign-Upgrade`), or a coloured dot.
pub fn badge(name: &str, cx: f32, cy: f32, size: f32, fallback: Color) {
    if let Some(t) = win(name) {
        tex(&t, Rect::new(cx - size / 2.0, cy - size / 2.0, size, size), WHITE);
    } else {
        draw_circle(cx, cy, size * 0.42, Color::new(0.0, 0.0, 0.0, 0.6));
        draw_circle(cx, cy, size * 0.34, fallback);
    }
}

/// A trait icon (`Bonus<N>.lit`, `Bonus-2Row`, `HeroBonus<N>`), or a small shield.
pub fn trait_icon(name: &str, x: f32, y: f32, size: f32) {
    if let Some(t) = win_fx(name, Fx::KeyCorner) {
        // The keyed pixels are transparent black: drawn premultiplied, the scaled picture's
        // edge takes no colour from them.
        premultiplied(|| tex(&t, Rect::new(x, y, size, size), WHITE));
    } else {
        let c = Color::new(0.25, 0.35, 0.75, 1.0);
        draw_rectangle(x + size * 0.15, y + size * 0.1, size * 0.7, size * 0.45, c);
        draw_triangle(vec2(x + size * 0.15, y + size * 0.55), vec2(x + size * 0.85, y + size * 0.55), vec2(x + size * 0.5, y + size * 0.95), c);
    }
}

// ------------------------------------------------------------------------------------------
// Animations
// ------------------------------------------------------------------------------------------

/// A spell or hit animation of the original, drawn centred on `c` with `size` width; `t` runs
/// 0..1. False when the install has no such animation (draw a placeholder instead).
pub fn effect(rel: &str, c: Vec2, size: f32, t: f32, color: Color) -> bool {
    let Some(frames) = animation(rel) else { return false };
    let n = frames.len();
    let i = ((t.clamp(0.0, 0.999)) * n as f32) as usize;
    let f = &frames[i.min(n - 1)];
    let h = size * f.height() / f.width();
    tex(f, Rect::new(c.x - size / 2.0, c.y - h / 2.0, size, h), color);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A phone's close click area is 44 dp at dp 2.75, its bottom never below the strip's, it holds
    /// the drawn cross, and it reaches up and right of it; the desktop's is the cross itself.
    #[test]
    fn a_phones_close_click_area_is_44_dp_and_never_below_the_strip() {
        let screen = (1080.0, 2400.0);
        let cross = Rect::new(1000.0, 70.0, 56.0, 56.0);
        let strip_bottom = 130.0;
        let hit = touch_hit(cross, strip_bottom, Some(2.75), screen);
        assert_eq!((hit.w, hit.h), ((44.0f32 * 2.75).round(), (44.0f32 * 2.75).round()), "44 dp square: {hit:?}");
        assert!(hit.bottom() <= strip_bottom + 1e-3, "never below the strip: {hit:?}");
        assert!(hit.x <= cross.x && cross.right() <= hit.right() + 1e-3, "holds the cross across: {hit:?}");
        assert!(hit.contains(cross.center()), "contains the cross: {hit:?}");
        assert!(hit.y < cross.y, "reaches up, outside the window: {hit:?}");
        // At the screen's right edge the square stays on the screen.
        let edge = touch_hit(Rect::new(1020.0, 70.0, 56.0, 56.0), strip_bottom, Some(2.75), screen);
        assert!(edge.right() <= screen.0 + 1e-3, "clamped to the screen: {edge:?}");
        // At the top of the screen the square is cut to the strip's height, never taller.
        let top = touch_hit(Rect::new(1000.0, 4.0, 56.0, 56.0), 60.0, Some(2.75), screen);
        assert!(top.y >= 0.0 && top.bottom() <= 60.0 + 1e-3, "cut at the top: {top:?}");
        // The desktop keeps the cross as its click area.
        assert_eq!(touch_hit(cross, strip_bottom, None, (1920.0, 1080.0)), cross);
    }

    /// Razdor's phone choice, not in the original: a phone's title strip is 1.5× the desktop's, and the
    /// cross sits inside it at the right, sized by the desktop's formula on the taller strip; the
    /// desktop's strip and cross are as they were.
    #[test]
    fn a_phones_title_strip_is_1_5_times_the_desktops_and_its_cross_sits_inside_it() {
        for kk in [1.0_f32, 1.125, 2.75] {
            let tb = strip_height(kk, FIXED_TITLE);
            assert_eq!(phone_strip_height(tb, None), tb, "k {kk}: the desktop's strip");
            let phone = phone_strip_height(tb, Some(2.75));
            assert!((phone - (tb * 1.5).round()).abs() < 1e-3, "k {kk}: {phone} is 1.5× {tb}");
            assert!((phone_title_grow(kk, FIXED_TITLE, Some(2.75)) - (phone - tb)).abs() < 1e-3, "the growth is the extra height");
            assert_eq!(phone_title_grow(kk, FIXED_TITLE, None), 0.0, "the desktop grows nothing");
        }
        // The desktop's cross: the strip less 6 px, 3 px down, 4 px in (as it was).
        let strip = Rect::new(100.0, 200.0, 500.0, 26.0);
        assert_eq!(title_close_cross(strip, 1.0), Rect::new(576.0, 203.0, 20.0, 20.0));
        // A phone's: the same formula on the taller strip, inside it at the right.
        let taller = Rect::new(100.0, 200.0, 500.0, 39.0);
        let cross = title_close_cross(taller, 1.0);
        assert_eq!(cross, Rect::new(596.0 - 33.0, 203.0, 33.0, 33.0));
        assert!(cross.y >= taller.y && cross.bottom() <= taller.bottom(), "inside the strip: {cross:?}");
        assert!(cross.right() <= taller.right() && cross.x >= taller.x, "at the right: {cross:?}");
    }

    /// The phone's click area for that cross is 44 dp (or the strip's height when that is less), never
    /// below the strip, and it holds the cross; the desktop's is the cross.
    #[test]
    fn a_phones_cross_click_area_is_44_dp_and_never_below_its_strip() {
        let strip = Rect::new(100.0, 200.0, 500.0, 39.0);
        let cross = title_close_cross(strip, 1.0);
        let hit = touch_hit(cross, strip.bottom(), Some(2.75), (1080.0, 2400.0));
        let side = (44.0f32 * 2.75).round();
        assert!(hit.w >= side.min(strip.h) - 1e-3 && hit.h >= side.min(strip.h) - 1e-3, "44 dp, or the strip's height: {hit:?}");
        assert!(hit.bottom() <= strip.bottom() + 1e-3, "never below the strip: {hit:?}");
        assert!(hit.contains(cross.center()), "holds the cross: {hit:?}");
        assert_eq!(touch_hit(cross, strip.bottom(), None, (1080.0, 2400.0)), cross, "the desktop's is the cross");
    }

    /// A phone's strip grows upward into the room above the window: the frame starts the growth higher
    /// and the content rect is unchanged; with no room above (the safe top), the window keeps its rect.
    #[test]
    fn a_phones_taller_strip_grows_upward_into_the_room_above_the_window() {
        let r = Rect::new(100.0, 300.0, 500.0, 400.0);
        assert_eq!(grown_upward(r, 13.0, 0.0), Rect::new(100.0, 287.0, 500.0, 413.0), "frame starts higher");
        assert_eq!(grown_upward(r, 13.0, 287.0), Rect::new(100.0, 287.0, 500.0, 413.0), "exactly to the safe top");
        assert_eq!(grown_upward(r, 13.0, 288.0), r, "no room above the safe top: the window keeps its rect");
        assert_eq!(grown_upward(r, 0.0, 0.0), r, "the desktop grows nothing");
    }

    /// The title stays clear of a phone's cross inside the strip: centred, its ends are 4 px short of
    /// the cross; a cross outside the strip leaves the room as it is.
    #[test]
    fn a_title_keeps_clear_of_a_phones_cross_inside_the_strip() {
        let strip = Rect::new(0.0, 0.0, 600.0, 40.0);
        let cross = Rect::new(600.0 - 121.0 - 4.0, 3.0, 121.0, 121.0);
        let room = room_clear_of(600.0, strip, cross, 1.0);
        assert!(300.0 + room / 2.0 <= cross.x - 4.0 + 1e-3, "room {room} under the cross at {cross:?}");
        assert_eq!(room_clear_of(600.0, strip, Rect::new(610.0, 0.0, 121.0, 121.0), 1.0), 600.0);
    }

    /// A fixed-layout window (the hero, army, building and settings windows, the save and the
    /// spell book) keeps the original's title whatever «Размер текста» is: its factor is
    /// `FIXED_TITLE`, its strip stays 26 px at every interface scale and text size.
    #[test]
    fn a_fixed_window_s_title_ignores_the_text_size() {
        assert_eq!(FIXED_TITLE, 1.0);
        for kk in [0.75_f32, 1.0, 1.5, 2.25] {
            assert_eq!(strip_height(kk, FIXED_TITLE), (26.0 * kk).round(), "k {kk}: the fixed strip is the original's");
        }
        // The growing windows take the text's factor instead (a phone's 1.25 is above 1).
        assert!(strip_height(1.5, 2.0) > strip_height(1.5, FIXED_TITLE));
    }

    /// The grey-out reads the screen copy on desktop only: phones draw the veil (see
    /// [`grey_by_screen_texture`]).
    #[test]
    fn the_grey_out_reads_the_screen_copy_on_desktop_only() {
        assert_eq!(grey_by_screen_texture(), !cfg!(any(target_os = "android", target_os = "ios")));
    }

    /// The outline's cut sits above a building's shadow plane and below its edges (12/15).
    #[test]
    fn the_silhouette_leaves_building_shadows_out() {
        let cut = 0.77;
        assert!(SILHOUETTE_FRAGMENT.contains("step(0.77,"));
        assert!(cut * 255.0 > razdor::dt::gfx::BUILDING_SHADOW_ALPHA as f32);
        assert!(cut * 255.0 < 204.0);
    }

    /// The hall list's edges as the original's (0x4bef1b..0x4bf00f): grey 48 off a 2 px band
    /// and again off a 4 px band, so the outer 2 px lose 96, the next 2 px 48, the rest none.
    #[test]
    fn the_hall_list_edges_lose_96_then_48() {
        assert_eq!(quest_list_edge_loss(0.0), 96);
        assert_eq!(quest_list_edge_loss(1.5), 96);
        assert_eq!(quest_list_edge_loss(2.0), 48);
        assert_eq!(quest_list_edge_loss(3.9), 48);
        assert_eq!(quest_list_edge_loss(4.0), 0);
    }

    /// The hint strip's corners as the original's (0x4dbd58, 0x4dbd74): the 124 px corners at
    /// x = −51 and 555 on the 628 px strip show their inner 73 columns, flush with each end.
    #[test]
    fn the_hint_strip_shows_the_corners_inner_73_columns() {
        assert_eq!(divider_ornaments(124.0, 628.0, false), (51.0, 73.0, 0.0));
        assert_eq!(divider_ornaments(124.0, 628.0, true), (0.0, 73.0, 555.0));
        // A strip narrower than both never overlaps them.
        assert_eq!(divider_ornaments(124.0, 100.0, true), (0.0, 50.0, 50.0));
    }

    /// The new-hero window's portraits (0x4d7d30, a player's report: Razdor's tint was a flat
    /// wash with lifted darks): grey copies scaled and shifted per channel, so black stays
    /// black and white takes the copy's colour; the vignette darkens the edge.
    #[test]
    fn the_hero_portraits_are_grey_copies_shifted_into_blue_and_dark_red() {
        let make = |which, rgb: [u8; 3]| {
            let mut img = Image { width: 160, height: 160, rgba: [rgb[0], rgb[1], rgb[2], 255].repeat(160 * 160) };
            hero_portrait(&mut img, which);
            (img.pixel(80, 80), img.pixel(0, 0), img.pixel(24, 80), img.pixel(32, 80))
        };
        let (mid, edge, inner, clear) = make(HeroPortrait::Offered, [255, 255, 255]);
        // 250 grey: (250·1300 >> 10) − (256, 80, 48), at 5-6-5 levels.
        assert_eq!(mid, [56, 232, 249, 255]);
        assert_eq!(clear, mid, "from 32 px in the vignette takes nothing");
        assert!(edge[1] < inner[1] && inner[1] < mid[1], "{edge:?} {inner:?}");
        let (mid, ..) = make(HeroPortrait::Locked, [255, 255, 255]);
        assert_eq!(mid, [249, 208, 129, 255], "(·1600 >> 10) − (48, 176, 256)");
        for which in [HeroPortrait::Offered, HeroPortrait::Locked, HeroPortrait::Picked] {
            let (mid, ..) = make(which, [0, 0, 0]);
            assert_eq!(mid, [0, 0, 0, 255], "black stays black ({which:?})");
        }
        // The picked one keeps its colours, ×1100/1024.
        let (mid, edge, ..) = make(HeroPortrait::Picked, [100, 50, 200]);
        assert_eq!(mid, [96, 48, 209, 255]);
        assert_eq!(edge, [32, 0, 145, 255], "64 off at the edge first");
    }

    /// The brown text box's paper (0x48e4e8): (148, 138, 156) taken off Win-paper, at the
    /// 16-bit surface's 5-6-5 levels.
    #[test]
    fn the_brown_box_paper_is_win_paper_less_its_colour_in_16_bit() {
        let mut p = Image { width: 1, height: 1, rgba: vec![230, 200, 150, 255] };
        subtract(&mut p, BROWN_BOX_TAKEN);
        assert_eq!(&p.rgba[..3], &[82, 62, 0]);
        quantize_565(&mut p);
        assert_eq!(&p.rgba[..3], &[82, 60, 0]);
        let mut white = Image { width: 1, height: 1, rgba: vec![255, 255, 255, 255] };
        quantize_565(&mut white);
        assert_eq!(&white.rgba[..3], &[255, 255, 255], "white stays white");
    }

    /// The title menu's English words (an owner's report): sized by the engraved words'
    /// capitals, not by the interface's sheet text size, and narrowed only when too wide.
    #[test]
    fn an_engraved_word_is_as_tall_as_asked_unless_too_wide() {
        // Capitals 0.7 of the size: 16 px capitals need size ~22.9.
        let s = engraved_size(16.0, 130.0, 0.7, 2.0);
        assert!((s - 16.0 / 0.7).abs() < 1e-4, "{s}");
        assert!(s * 0.7 > 15.9, "the word keeps its capital height");
        // A long word (width 8 per unit of size) is narrowed to 130 px.
        let s = engraved_size(16.0, 130.0, 0.7, 8.0);
        assert!((s * 8.0 - 130.0).abs() < 1e-4, "{s}");
        // An empty word keeps the height rule.
        assert!((engraved_size(16.0, 130.0, 0.7, 0.0) - 16.0 / 0.7).abs() < 1e-4);
    }

    /// The card highlight as the original's pictures (4dc118) and pulse (4cd2a2): grey rings
    /// 255 − 24 per pixel less the state's colour; brightness 8..15..8 every 1.5 s. The
    /// original's "can act" cell at the top of its pulse read 239, 222, 197, 173… blue on a
    /// black tent (the UI audit's battle screenshot): 240 − 24d, its black under the art.
    #[test]
    fn card_highlight_rings_and_pulse_as_the_original_s() {
        assert_eq!(Lit::CanAct.ring(15, 0), [0, 0, 240]);
        assert_eq!(Lit::CanAct.ring(15, 3), [0, 0, 168]);
        assert_eq!(Lit::CanAct.ring(15, 8), [0, 0, 0], "the middle is black");
        assert_eq!(Lit::Current.ring(15, 0), [0, 240, 0]);
        assert_eq!(Lit::Enemy.ring(15, 0), [240, 0, 0]);
        assert_eq!(Lit::Aimed.ring(15, 0), [240, 120, 0]);
        assert_eq!(Lit::OwnTarget.ring(15, 0), [0, 120, 240]);
        assert_eq!(Lit::CanAct.ring(8, 6), [0, 0, 0], "128 − 144 is below 0");
        assert_eq!(lit_level(0), 8);
        assert_eq!(lit_level(700), 15);
        assert_eq!(lit_level(800), 15);
        assert_eq!(lit_level(1400), 9);
        assert_eq!(lit_level(1500), 8);
        assert!((0..30_000).step_by(37).all(|ms| (8..=15).contains(&lit_level(ms))));
    }

    #[test]
    fn wounds_fill_the_share_of_hits_lost() {
        assert_eq!(wound_share(80, 80), 0.0);
        assert_eq!(wound_share(40, 80), 0.5);
        assert_eq!(wound_share(0, 80), 1.0);
        assert_eq!(wound_share(-5, 80), 1.0, "a corpse is all red");
        assert_eq!(wound_share(90, 80), 0.0, "blessed above the maximum");
        assert_eq!(wound_share(10, 0), 0.0);
    }

    fn img(px: &[[u8; 4]]) -> Image {
        Image { width: px.len() as u32, height: 1, rgba: px.iter().flatten().copied().collect() }
    }

    /// The defeat window's red button (0x4dba90): channels turned (2, 3, 1), then (0, 64, 128)
    /// taken off inside the 1 px edge, which keeps its silver. Measured on the original: the
    /// face (65, 39, 26) after the turn reads (76, 3, 0) on screen.
    #[test]
    fn a_label_in_the_original_s_font_grows_with_the_button_by_whole_steps() {
        // The sheet at ×1 on the original's 36 px button, ×2 on a 72 px one; a 61 px button
        // (2560×1440's ×1.7) keeps its exact scale instead of filling it edge to edge.
        let one = label_size_original(36.0);
        assert!((label_size_original(72.0) - 2.0 * one).abs() < 1e-3);
        assert!((label_size_original(61.2) - 1.7 * one).abs() < 1e-3);
        assert!(label_size_original(61.2) < 61.2 * 0.56);
    }

    #[test]
    fn the_defeat_button_is_turned_and_darkened_inside_its_edge() {
        let mut b = Image { width: 3, height: 3, rgba: [39, 26, 65, 255].repeat(9) };
        transform(&mut b, Fx::RotateRgb);
        let at = |x: usize, y: usize| { let o = (y * 3 + x) * 4; [b.rgba[o], b.rgba[o + 1], b.rgba[o + 2]] };
        assert_eq!(at(1, 1), [26, 1, 0], "inside: turned to (26, 65, 39), less (0, 64, 128)");
        for (x, y) in [(0, 0), (1, 0), (2, 1), (1, 2), (0, 2)] {
            assert_eq!(at(x, y), [26, 65, 39], "the edge is only turned");
        }
    }

    #[test]
    fn a_locked_portrait_is_dark_brown_with_a_dark_edge() {
        // 92×92 white: the middle (inset ≥ 18) keeps 255·1600/1024 − offsets, clamped.
        let mut white = Image { width: 92, height: 92, rgba: vec![255; 92 * 92 * 4] };
        lock_portrait(&mut white);
        let at = |i: &Image, x: usize, y: usize| { let o = (y * 92 + x) * 4; [i.rgba[o], i.rgba[o + 1], i.rgba[o + 2]] };
        assert_eq!(at(&white, 46, 46), [255, 222, 142]);
        // The edge loses 96 more, inset 1 loses 96 − 5.
        assert_eq!(at(&white, 0, 46), [159, 126, 46]);
        assert_eq!(at(&white, 46, 1), [164, 131, 51]);
        // Mid grey: 128 → 200, then −48 / −176 / −256.
        let mut grey = Image { width: 92, height: 92, rgba: vec![128; 92 * 92 * 4] };
        lock_portrait(&mut grey);
        assert_eq!(at(&grey, 46, 46), [152, 24, 0]);
    }

    #[test]
    fn black_keys_out_and_glow_turns_brightness_into_alpha() {
        let mut a = img(&[[0, 0, 0, 255], [200, 100, 50, 255]]);
        transform(&mut a, Fx::KeyBlack);
        assert_eq!(a.pixel(0, 0)[3], 0);
        assert_eq!(a.pixel(1, 0)[3], 255);
        let mut g = img(&[[0, 0, 0, 0], [100, 50, 0, 0]]);
        transform(&mut g, Fx::Glow);
        assert_eq!(g.pixel(0, 0)[3], 0);
        assert_eq!(g.pixel(1, 0), [255, 127, 0, 100]);
    }

    /// A world spell layer's pixel in the original: what is below plus texel × tint, per
    /// channel, saturated (stage 0 MODULATE of texture and diffuse, SRCBLEND = DESTBLEND =
    /// ONE; 0x4c9e63).
    fn original_spell_pixel(below: [u8; 3], texel: [u8; 3], tint: [u8; 3]) -> [u8; 3] {
        std::array::from_fn(|i| (below[i] as u32 + texel[i] as u32 * tint[i] as u32 / 255).min(255) as u8)
    }

    /// The same pixel drawn by Razdor: the texel made to glow, tinted, under [`additive`]
    /// (source × its alpha + what is below).
    fn razdor_spell_pixel(below: [u8; 3], texel: [u8; 3], tint: [u8; 3]) -> [u8; 3] {
        let mut g = img(&[[texel[0], texel[1], texel[2], 0]]);
        transform(&mut g, Fx::Glow);
        let p = g.pixel(0, 0);
        std::array::from_fn(|i| {
            let src = p[i] as f32 / 255.0 * tint[i] as f32 / 255.0;
            (below[i] as f32 + src * p[3] as f32).min(255.0) as u8
        })
    }

    #[test]
    fn a_spell_layer_adds_its_tinted_light_and_never_darkens() {
        // «Молния»'s flare: purple light over green grass, black adds nothing.
        let grass = [40, 90, 30];
        assert_eq!(original_spell_pixel(grass, [0, 0, 0], [120, 0, 100]), grass);
        assert_eq!(original_spell_pixel(grass, [255, 255, 255], [120, 0, 100]), [160, 90, 130]);
        assert_eq!(original_spell_pixel([200, 0, 0], [255, 0, 0], [255, 255, 255]), [255, 0, 0]);
        let levels = [0u8, 17, 68, 119, 170, 221, 255];
        for &a in &levels {
            for &b in &levels {
                for &c in &levels {
                    for tint in [[120, 0, 100], [0, 135, 155], [255, 255, 255], [10, 10, 10]] {
                        let (o, r) = (original_spell_pixel(grass, [a, b, c], tint), razdor_spell_pixel(grass, [a, b, c], tint));
                        assert!(o.iter().zip(r).all(|(&x, y)| x.abs_diff(y) <= 1), "{:?} {tint:?}: {o:?} vs {r:?}", [a, b, c]);
                        assert!(r.iter().zip(grass).all(|(&x, y)| x >= y), "never darker");
                    }
                }
            }
        }
    }

    #[test]
    fn channels_scale_then_shift_clamped() {
        // Army-Death: −64 each; exp: −250 / −30 / −30; exp-hole: half.
        let mut a = img(&[[200, 40, 64, 255]]);
        scale_channels(&mut a, [1024; 3], [-64; 3]);
        assert_eq!(a.pixel(0, 0), [136, 0, 0, 255]);
        let mut e = img(&[[255, 100, 20, 255]]);
        scale_channels(&mut e, [1024; 3], [-250, -30, -30]);
        assert_eq!(e.pixel(0, 0), [5, 70, 0, 255]);
        let mut h = img(&[[255, 101, 0, 7]]);
        scale_channels(&mut h, [512; 3], [0; 3]);
        assert_eq!(h.pixel(0, 0), [127, 50, 0, 7], "alpha untouched");
    }

    #[test]
    fn the_xp_strip_runs_a_row_every_3_ms_over_512() {
        assert_eq!(xp_strip_row(0), 0);
        assert_eq!(xp_strip_row(5), 1);
        assert_eq!(xp_strip_row(3 * 511), 511);
        assert_eq!(xp_strip_row(3 * 512), 0);
        assert_eq!(xp_strip_row(2500), 321);
    }

    #[test]
    fn a_spell_layers_colour_correction_is_taken_away() {
        // «Исцеление»'s rays: 160,40,100 off white leaves green.
        let mut a = img(&[[255, 255, 255, 255], [100, 30, 200, 255]]);
        subtract(&mut a, [160, 40, 100]);
        assert_eq!(a.pixel(0, 0), [95, 215, 155, 255]);
        assert_eq!(a.pixel(1, 0), [0, 0, 100, 255], "down to black, not below");
    }

    /// Every original picture the interface asks for is in the install and decodes (skipped
    /// without `RAZDOR_DT_DIR`).
    #[test]
    fn the_interface_art_is_in_the_install() {
        let Ok(install) = razdor::dt::install::DtInstall::from_env() else { return };
        let mut names: Vec<String> = [
            "Win-marble", "Win-red", "Win-paper", "Win2a", "SteelLine", "DownCorner", "HintFrame", "Corner-Left", "Corner-Right",
            "Corner_Frame-LU", "Corner_Frame-RU", "Corner_Frame-LD", "Corner_Frame-RD", "CloseButtonRed-Up", "CloseButtonGreen-Up",
            "CloseButtonRed-Down2", "CloseButtonGreen-Down2", "UnitFrame", "Shadow-Empty", "wb1", "wb2", "wb3", "army-2", "army-3",
            "sign-poison", "sign-payment", "Sign-Upgrade", "SI_Helm", "Bonus-2Row", "Bonus-InCastle", "Bonus-Ressurrect", "Bonus-NoPayment",
            "Army-Death", "Shadow-Unit", "exp", "exp-hole", "Btn1Up", "Btn2Up", "Btn3Up",
            "Btn1Down", "Btn2Down", "Btn3Down", "smb-up", "smb-down", "smb-disable", "MLBtn1Up", "MLBtn2Up", "MRBtn1Up", "MRBtn2Up",
            "MLBtn1Down", "MLBtn2Down", "MRBtn1Down", "MRBtn2Down", "Res-Magic", "Res-Money", "Res-Income", "Res-Payment",
            "MiniMap_Frame_400x400", "Inventory", "UpgradeTree", "BI_Town", "BI_Castle", "BI_Church", "BI_Ruin", "S_Town", "S_Castle",
            "S_Church", "S_Market", "S_Tavern", "S_Village", "S_Shipyard", "S_Ruin",
        ]
        .iter()
        .map(|n| format!("Windows/{n}.lit"))
        .collect();
        names.extend((1..=8).map(|n| format!("Windows/button-icon-{n}.lit")));
        names.extend((0..=3).flat_map(|n| [format!("Windows/res{n}.lit"), format!("Windows/res{n}-44.lit")]));
        names.extend((1..=6).flat_map(|n| ["TB-", "TBo", "TBd"].map(|s| format!("Windows/{s}{n}_RUS.lit"))));
        names.extend((1..=52).map(|n| format!("Windows/Bonus{n}.lit")));
        names.extend((1..=3).map(|n| format!("Windows/HeroBonus{n}.lit")));
        for a in ["KUSKI", "KUSKIBIG", "CURE"] {
            names.push(format!("Battle/--{a}.ugs"));
        }
        for s in ["S-Light-Front", "S-Fog", "S-Fire", "S-Fontain", "S-Swirl"] {
            names.push(format!("Spells/{s}.ugs"));
        }
        for n in names {
            let frames = install.graphic(&format!("Graphics/{n}")).unwrap_or_else(|e| panic!("{n}: {e}"));
            assert!(!frames.is_empty(), "{n}");
        }
        assert!(ui_text_from(&install.dir, "Army", "Bonus2").is_some_and(|t| !t.is_empty()));
    }

    /// A village's offer reads the install's text as markup: its `#\` breaks stay marks and
    /// make lines, none is drawn as a stray character (skipped without `RAZDOR_DT_DIR`).
    #[test]
    fn the_village_offer_keeps_its_marks() {
        let Ok(install) = razdor::dt::install::DtInstall::from_env() else { return };
        set_install(install.dir.clone());
        let raw = ui_markup("Event", "VillageBonus1").expect("VillageBonus1");
        assert!(raw.contains("#\\") && !raw.contains('\n'), "{raw}");
        let lines = razdor::dt::markup::parse(&raw);
        assert!(lines.iter().all(|l| !l.text.contains(['\n', '#', '@'])), "{lines:?}");
        assert_eq!(lines.last().map(|l| l.ink), Some(razdor::dt::markup::Ink::At));
    }

    #[test]
    fn placeholder_marble_tiles() {
        let n = 64;
        let v = marble_noise(n);
        assert_eq!(v.len(), n * n);
        assert!(v.iter().all(|&b| b >= 150));
        assert!(v.iter().any(|&b| b > 200) && v.iter().any(|&b| b < 200), "not flat");
        // Opposite edges meet: the texture repeats without a seam.
        let seam: i32 = (0..n).map(|y| (v[y * n] as i32 - v[y * n + n - 1] as i32).abs()).max().unwrap();
        let inner: i32 = (0..n).map(|y| (v[y * n + 20] as i32 - v[y * n + 21] as i32).abs()).max().unwrap();
        assert!(seam <= inner * 3 + 6, "seam {seam} vs neighbours {inner}");
        assert_eq!(v, marble_noise(n), "deterministic");
    }

    #[test]
    fn grey_and_shade() {
        let mut a = img(&[[255, 0, 0, 200]]);
        transform(&mut a, Fx::Grey);
        let p = a.pixel(0, 0);
        assert!(p[0] == p[1] && p[1] == p[2] && p[3] == 200);
        let mut s = img(&[[255, 255, 255, 255], [0, 0, 0, 255]]);
        transform(&mut s, Fx::Shade);
        assert_eq!(s.pixel(0, 0)[3], 255);
        assert_eq!(s.pixel(1, 0)[3], 0);
    }

    #[test]
    fn canvas_centred_only_on_screens_narrower_than_4_3() {
        // A phone in portrait: 1080 wide, the 720-line canvas at 1.125 is 810 tall in 2160.
        assert_eq!(centred_top(1080.0, 2160.0, 810.0), 675.0);
        // Odd remainders round to whole pixels.
        assert_eq!(centred_top(1000.0, 1801.0, 800.0), 501.0);
        // 4:3 and wider never move, whatever the canvas.
        assert_eq!(centred_top(1024.0, 768.0, 500.0), 0.0);
        assert_eq!(centred_top(2160.0, 1080.0, 500.0), 0.0);
        assert_eq!(centred_top(1920.0, 1080.0, 600.0), 0.0);
        // A canvas as tall as the screen, or taller, has no room to move.
        assert_eq!(centred_top(900.0, 1000.0, 1000.0), 0.0);
        assert_eq!(centred_top(900.0, 1000.0, 1200.0), 0.0);
    }

    #[test]
    fn cover_crops_the_picture_only_where_its_aspect_is_far_off() {
        // A 4:3 picture on a phone in portrait (9:18): the centre strip of its width.
        let r = cover_source((1024.0, 768.0), (1080.0, 2160.0)).expect("far off");
        assert_eq!((r.x, r.y, r.h), ((1024.0f32 - 768.0 * 0.5) / 2.0, 0.0, 768.0));
        assert_eq!(r.w, (768.0 * 0.5f32).round());
        // A narrow portrait screen (0.4) on a 0.6 picture: the centre strip of its width.
        let r = cover_source((600.0, 1000.0), (400.0, 1000.0)).expect("narrower screen");
        assert_eq!((r.x, r.y, r.w, r.h), (100.0, 0.0, 400.0, 1000.0));
        // A wide picture (2:1) on a portrait screen: the centre strip of its width.
        let r = cover_source((1000.0, 500.0), (500.0, 1000.0)).expect("wide picture");
        assert_eq!((r.x, r.y, r.w, r.h), (375.0, 0.0, 250.0, 500.0));
        // Within 20% of the picture's aspect: stretched as before.
        assert_eq!(cover_source((1024.0, 768.0), (1000.0, 800.0)), None);
        assert_eq!(cover_source((640.0, 800.0), (700.0, 1000.0)), None);
        assert_eq!(cover_source((1024.0, 768.0), (900.0, 1000.0)).map(|r| r.w), Some((768.0 * 0.9f32).round()));
        // Landscape and square screens: stretched as before (the desktop and the phone's landscape).
        assert_eq!(cover_source((1024.0, 768.0), (1920.0, 1080.0)), None);
        assert_eq!(cover_source((1024.0, 768.0), (2160.0, 1080.0)), None);
        assert_eq!(cover_source((1024.0, 768.0), (800.0, 800.0)), None);
    }
}
