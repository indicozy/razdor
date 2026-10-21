//! The original's window art: marble and parchment surfaces, title bars, silver frames,
//! buttons, card frames, empty-cell icons, resource icons and the battle's spell
//! animations. Everything is decoded at runtime from the player's install
//! (`Graphics/Windows`, `Graphics/Spells`, `Graphics/Battle`) and cached as textures; nothing
//! decoded is written anywhere. Without an install each piece falls back to our own
//! placeholder style with the same layout.
//!
//! Layout numbers of the screens that follow the original closely (battle, bottom bar) are
//! in pixels of the 960×720 gameplay video (`docs/reference/video-notes.md`), scaled by [`k`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use macroquad::prelude::*;

use razdor::dt::gfx::{self, Image};
use razdor::dt::ini::Ini;
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
    /// Green and blue swapped (0x48dd40 with 1, 3, 2): the blue pill made the green one.
    SwapGreenBlue,
    /// The colour of the bottom-left pixel is transparent, compared at 16 bits (the ability
    /// icons, [`gfx::key_bottom_left_565`]).
    KeyCorner,
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
    textures: HashMap<String, [Option<Option<Texture2D>>; 11]>,
    animations: HashMap<String, Option<Vec<Texture2D>>>,
    texts: Option<Option<Ini>>,
}

thread_local! {
    static CHROME: RefCell<Option<Chrome>> = const { RefCell::new(None) };
}

/// Use the art of the install in `dir` from now on.
pub fn set_install(dir: PathBuf) {
    CHROME.with(|c| *c.borrow_mut() = Some(Chrome { dir, textures: HashMap::new(), animations: HashMap::new(), texts: None }));
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
    for p in img.rgba.chunks_exact_mut(4) {
        match fx {
            Fx::Plain => {}
            Fx::KeyBlack => {
                let m = p[0].max(p[1]).max(p[2]) as u32;
                let a = (m.saturating_sub(10) * 16).min(255);
                p[3] = p[3].min(a as u8);
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
            Fx::SwapGreenBlue => p.swap(1, 2),
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
        .and_then(|p| gfx::decode_file(&p))
        .map_err(|e| razdor::diag!("Discord Times art: {rel}: {e}"))
        .ok()
        .and_then(|mut frames| (!frames.is_empty()).then(|| frames.swap_remove(0)))
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
            .and_then(|p| gfx::decode_file(&p))
            .map_err(|e| razdor::diag!("Discord Times art: {rel}: {e}"))
            .ok()
            .and_then(|mut frames| {
                let img = frames.get_mut(0)?;
                transform(img, fx);
                to_texture(img)
            });
        c.textures.entry(rel.to_string()).or_default()[fx as usize] = Some(t.clone());
        t
    })
}

/// A spell's picture as the original composes it: its `Icon1..3` layers from
/// `Graphics/Spells`, each with its `ColorC` taken away (the ini's "colour correction
/// (-RGB)": 160,40,100 leaves «Исцеление»'s rays green), the backgrounds (`_` names) first,
/// all glowing (black adds nothing). Draws nothing without an install.
pub fn spell_icon(icons: &[razdor::dt::data::SpellIcon], r: Rect) {
    spell_icon_tinted(icons, r, WHITE);
}

/// [`spell_icon`] with every layer drawn in `tint` (the book's hovered card: each colour
/// brightened by 1400/1024, 4c2cf4 through FUN_0048da5c; the alpha is kept).
pub fn spell_icon_tinted(icons: &[razdor::dt::data::SpellIcon], r: Rect, tint: Color) {
    let mut layers: Vec<(&str, Option<[i32; 3]>)> = icons.iter().filter_map(|i| Some((i.image.as_deref()?, i.tint))).collect();
    layers.sort_by_key(|(name, _)| !name.starts_with('_'));
    for (name, colour_correction) in layers {
        let Some(t) = spell_layer(name, colour_correction.unwrap_or([0; 3])) else { continue };
        draw_texture_ex(&t, r.x, r.y, tint, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), ..Default::default() });
    }
}

/// One layer of a spell's picture with `minus` taken off every pixel, made to glow.
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
            .and_then(|p| gfx::decode_file(&p))
            .map_err(|e| razdor::diag!("Discord Times art: {rel}: {e}"))
            .ok()
            .and_then(|mut frames| {
                let img = frames.get_mut(0)?;
                subtract(img, minus);
                transform(img, Fx::Glow);
                to_texture(img)
            });
        c.textures.entry(key).or_default()[Fx::Glow as usize] = Some(t.clone());
        t
    })
}

/// Windows art `name` with its channels scaled and shifted as loaded ([`scale_channels`]),
/// cached by both.
pub fn win_scaled(name: &str, k: [i32; 3], offset: [i32; 3]) -> Option<Texture2D> {
    let rel = format!("Windows/{name}.lit");
    let key = format!("{rel}#{k:?}{offset:?}");
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if let Some(t) = c.textures.get(&key).and_then(|v| v[Fx::Plain as usize].as_ref()) {
            return t.clone();
        }
        let t = find_path(&c.dir, &format!("Graphics/{rel}"))
            .and_then(|p| gfx::decode_file(&p))
            .map_err(|e| razdor::diag!("Discord Times art: {rel}: {e}"))
            .ok()
            .and_then(|mut frames| {
                let img = frames.get_mut(0)?;
                scale_channels(img, k, offset);
                to_texture(img)
            });
        c.textures.entry(key).or_default()[Fx::Plain as usize] = Some(t.clone());
        t
    })
}

/// Windows art (`Graphics/Windows/<name>.lit`).
pub fn win(name: &str) -> Option<Texture2D> {
    art(&format!("Windows/{name}.lit"), Fx::Plain)
}

use gfx::quantize_565;

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

/// [`strip_picture`] as a texture (`None` without the install's paper), made once and cached.
pub fn strip_texture(panel: super::unit_sheet::StripPanel) -> Option<Texture2D> {
    use super::unit_sheet::StripPanel;
    let rel = "Windows/Win-paper.lit";
    let key = match panel {
        StripPanel::Plain => "#card-strip",
        StripPanel::Hero => "#card-strip-hero",
        StripPanel::Named => "#card-strip-named",
    };
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if let Some(t) = c.textures.get(key).and_then(|v| v[Fx::Plain as usize].as_ref()) {
            return t.clone();
        }
        let t = find_path(&c.dir, &format!("Graphics/{rel}"))
            .and_then(|p| gfx::decode_file(&p))
            .map_err(|e| razdor::diag!("Discord Times art: {rel}: {e}"))
            .ok()
            .and_then(|frames| to_texture(&strip_picture(frames.first()?, panel)));
        c.textures.entry(key.to_string()).or_default()[Fx::Plain as usize] = Some(t.clone());
        t
    })
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
        if c.texts.is_none() {
            c.texts = Some(read_texts(&c.dir));
        }
        text_of(c.texts.as_ref()?.as_ref()?, section, key)
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

thread_local! {
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

thread_local! {
    static ADDITIVE: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
    static MULTIPLY: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
    static PREMULTIPLIED: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
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

fn subtract_with(cell: &'static std::thread::LocalKey<std::cell::OnceCell<Option<Material>>>, fragment: &'static str, draw: impl FnOnce()) {
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

thread_local! {
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

thread_local! {
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

/// Turns what is drawn so far in `r` grey and a little darker. Without the shader, a dark
/// veil.
pub fn grey_out(r: Rect) {
    let material = GREY.with(|m| {
        m.get_or_init(|| {
            load_material(ShaderSource::Glsl { vertex: GREY_VERTEX, fragment: GREY_FRAGMENT }, MaterialParams::default())
                .map_err(|e| razdor::diag!("grey material: {e}"))
                .ok()
        })
        .clone()
    });
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

/// Screen pixels per pixel of the 960×720 reference video (the original ran at 1024×768).
pub fn k() -> f32 {
    ((screen_width() / 1024.0).min(screen_height() / 768.0) / 0.9375).max(0.5)
}

/// Height of the bottom game bar.
pub fn bar_height() -> f32 {
    (80.0 * k()).round()
}

pub fn tex(t: &Texture2D, r: Rect, color: Color) {
    draw_texture_ex(t, r.x, r.y, color, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), ..Default::default() });
}

pub fn tex_src(t: &Texture2D, src: Rect, dst: Rect, color: Color) {
    draw_texture_ex(t, dst.x, dst.y, color, DrawTextureParams { dest_size: Some(vec2(dst.w, dst.h)), source: Some(src), ..Default::default() });
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
    // From the original's sheets: their shadow sheet, 1 px down and right (4e5c9c, 4dbc26).
    if !super::dt_font::draw_shadow(s, x, y, size, color.a) {
        let o = (size / 14.0).clamp(1.0, 2.0);
        text(s, x + o, y + o, size, Color::new(0.0, 0.0, 0.0, 0.75 * color.a));
    }
    text(s, x, y, size, color);
}

/// Text with a heavier dark edge, readable over a picture (the unit panel's stats over the
/// figure).
pub fn strong_text(s: &str, x: f32, y: f32, size: f32, color: Color) {
    let dark = Color::new(0.08, 0.04, 0.0, 0.8 * color.a);
    for (dx, dy) in [(1.0, 1.0), (1.0, 0.0), (0.0, 1.0), (-0.6, 0.0), (0.0, -0.6)] {
        text(s, x + dx, y + dy, size, dark);
    }
    text(s, x, y, size, color);
}

pub fn strong_centered(s: &str, cx: f32, y: f32, size: f32, color: Color) {
    let w = measure(s, size).width;
    strong_text(s, cx - w / 2.0, y, size, color);
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
        let paper = |c: Color| win("Win-paper").map(|t| (t, c));
        match self {
            Skin::Marble => win("Win-marble").map(|t| (t, WHITE)),
            Skin::Red => win_scaled("Win-red", [1024; 3], [-20, -40, -100]).map(|t| (t, WHITE)),
            Skin::Paper => paper(WHITE),
            Skin::Brown => paper(Color::new(0.33, 0.21, 0.02, 1.0)),
            Skin::Strip => paper(Color::new(0.37, 0.265, 0.02, 1.0)),
        }
    }

    /// The placeholder colour.
    pub fn color(self) -> Color {
        match self {
            Skin::Marble => Color::new(0.10, 0.22, 0.18, 1.0),
            Skin::Red => Color::new(0.30, 0.06, 0.04, 1.0),
            Skin::Paper => Color::new(0.86, 0.68, 0.45, 1.0),
            Skin::Brown => Color::new(0.38, 0.18, 0.10, 1.0),
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

thread_local! {
    static NOISE: RefCell<Option<Texture2D>> = const { RefCell::new(None) };
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

/// A skin at opacity `alpha` (the map's tooltips let the ground show through).
pub fn surface_alpha(r: Rect, skin: Skin, alpha: f32) {
    if let Some((t, c)) = skin.texture() {
        tile(&t, r, k().min(1.4), Color::new(c.r, c.g, c.b, alpha));
        return;
    }
    let base = skin.color();
    let lift = |v: f32| (v * 1.25).min(1.0);
    tile(&noise_texture(), r, 2.0, Color::new(lift(base.r), lift(base.g), lift(base.b), alpha));
}

/// The original's neutral frame (style 2 of 0x48d3f8): the green marble with its green
/// channel in all three (0x48dd40) scaled ×1, ×0.67, ×0.44 (0x48da5c), a dull brown. Razdor
/// greys the marble by its brightness, near enough to its green.
pub fn neutral_surface_alpha(r: Rect, alpha: f32) {
    let (r_, g, b) = (1.0, 0x2aa as f32 / 1024.0, 0x1c7 as f32 / 1024.0);
    if let Some(t) = win_fx("Win-marble", Fx::Grey) {
        tile(&t, r, k().min(1.4), Color::new(r_, g, b, alpha));
        return;
    }
    tile(&noise_texture(), r, 2.0, Color::new(0.6 * r_, 0.6 * g, 0.6 * b, alpha));
}

/// Screen pixels per pixel of the original's 1024×768 screen (`k` is per pixel of the 960×720
/// reference: the original's pixel is 0.9375 of it).
pub fn opx() -> f32 {
    0.9375 * k()
}

/// `SteelLine.lit` transposed (0xae2538's column for the runs down), cached with the windows art.
fn steel_column() -> Option<Texture2D> {
    let key = "Windows/SteelLine.lit#transposed".to_string();
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if let Some(t) = c.textures.get(&key).and_then(|v| v[Fx::Plain as usize].as_ref()) {
            return t.clone();
        }
        let t = find_path(&c.dir, "Graphics/Windows/SteelLine.lit")
            .and_then(|p| gfx::decode_file(&p))
            .map_err(|e| razdor::diag!("Discord Times art: Windows/SteelLine.lit: {e}"))
            .ok()
            .and_then(|mut frames| {
                let img = frames.get_mut(0)?;
                let (w, h) = (img.width as usize, img.height as usize);
                let mut out = vec![0u8; w * h * 4];
                for y in 0..h {
                    for x in 0..w {
                        let (from, to) = ((y * w + x) * 4, (x * h + y) * 4);
                        out[to..to + 4].copy_from_slice(&img.rgba[from..from + 4]);
                    }
                }
                *img = Image { width: h as u32, height: w as u32, rgba: out };
                to_texture(img)
            });
        c.textures.entry(key).or_default()[Fx::Plain as usize] = Some(t.clone());
        t
    })
}

/// `SteelLine.lit` (0xae2538, loaded by 0x4dbb0c): the row texture and its transpose, for the
/// frame lines' runs along and down.
fn steel_textures() -> Option<(Texture2D, Texture2D)> {
    let row = win("SteelLine")?;
    let col = steel_column()?;
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


/// A frame of the original's line round `r`: 2 px when `width` is two of the original's
/// pixels or more, else 1 px (0x48d1d4).
pub fn silver_frame(r: Rect, width: f32) {
    let width = width.max(1.0);
    let thick = if width >= 1.5 * opx() { 2 } else { 1 };
    steel_outline(r, opx(), thick, width / f32::from(thick));
}
/// (2, 27) to (w − 2, 29) under the title. `px` as in the window frames of 0x48d710.
pub fn window_frames(r: Rect, px: f32) {
    steel_frame(r, px, 2);
    steel_frame(Rect::new(r.x + 2.0 * px, r.y + 27.0 * px, r.w - 4.0 * px, 2.0 * px), px, 2);
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
    let Some(t) = win("SteelLine") else {
        draw_rectangle_lines(r.x, r.y, r.w, r.h, px.max(1.0), SILVER);
        return;
    };
    // Each side takes the next 55 source pixels of the row, drawn along it from its start.
    let sides = [
        (vec2(r.x, r.y), vec2(r.x + r.w, r.y)),
        (vec2(r.x + r.w, r.y), vec2(r.x + r.w, r.y + r.h)),
        (vec2(r.x + r.w, r.y + r.h), vec2(r.x, r.y + r.h)),
        (vec2(r.x, r.y + r.h), vec2(r.x, r.y)),
    ];
    for (n, (from, to)) in sides.into_iter().enumerate() {
        let len = from.distance(to);
        let angle = (to.y - from.y).atan2(to.x - from.x);
        // The strip lies inside the square: shifted half its thickness inwards.
        let inward = vec2(-(to - from).y, (to - from).x).normalize() * px / 2.0;
        let mid = (from + to) / 2.0 + inward;
        draw_texture_ex(&t, mid.x - len / 2.0, mid.y - px / 2.0, WHITE, DrawTextureParams {
            dest_size: Some(vec2(len, px)),
            source: Some(Rect::new(n as f32 * SIDE, 0.0, SIDE, 1.0)),
            rotation: angle,
            ..Default::default()
        });
    }
}

/// The window's close box; true when clicked.
pub fn close_button(x: f32, y: f32, s: f32, red: bool) -> bool {
    let hover = super::widgets::mouse_in(x, y, s, s);
    let name = match (red, hover) {
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
    hover && super::widgets::clicked()
}

/// Title bar of a window: the window's own skin, the title centred; `ornate` puts the
/// corner ornaments and the Benguiat font on a wide one (the town window's title is plain).
/// Returns true when its close box (if any) was clicked.
fn title_bar_styled(r: Rect, title: &str, skin: Skin, closable: bool, ornate: bool) -> bool {
    let k = k();
    surface(r, skin);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.15));
    // Ornaments at both ends of a wide title: orange on the battle's red, silver-green on
    // marble (the load window, the army screen).
    // They give way to a title that would not fit between them.
    let face = if ornate { super::dt_font::Face::Title } else { super::dt_font::Face::Body };
    let size = (16.0 * k).round();
    let title_w = super::dt_font::with_face(face, || measure(title, size).width);
    let ornate = ornate && matches!(skin, Skin::Red | Skin::Marble) && r.w > 400.0 && title_w < r.w - 300.0 * k;
    if let Some(t) = win_fx("Corner-Left", Fx::KeyBlack).filter(|_| ornate) {
        let tint = if skin == Skin::Red { Color::new(1.0, 0.45, 0.1, 0.9) } else { Color::new(0.55, 0.8, 0.7, 0.8) };
        let h = r.h * 0.8;
        let w = t.width() * h / t.height();
        tex(&t, Rect::new(r.x + 4.0 * k, r.y + (r.h - h) / 2.0, w, h), tint);
        if let Some(t2) = win_fx("Corner-Right", Fx::KeyBlack) {
            let right = r.x + r.w - w - 4.0 * k - if closable { r.h + 4.0 * k } else { 0.0 };
            tex(&t2, Rect::new(right, r.y + (r.h - h) / 2.0, w, h), tint);
        }
    }
    draw_line(r.x, r.y + r.h, r.x + r.w, r.y + r.h, 1.0 * k, SILVER);
    let color = if skin == Skin::Red { GOLD } else { CREAM };
    let room = if ornate { r.w - 300.0 * k } else { r.w - 2.0 * r.h - 8.0 * k };
    // Titles are in the original's Benguiat, a plain one in its body font.
    super::dt_font::with_face(face, || {
        // Benguiat is wide: shrink to fit first (at most to 70%); what still does not fit is cut.
        let size = super::widgets::fit_size(title, room, size);
        let mut t = title.to_string();
        while measure(&t, size).width > room && t.chars().count() > 4 {
            t.pop();
        }
        shadow_centered(&t, r.x + r.w / 2.0, r.y + r.h * 0.5 + size * 0.36, size, color);
    });
    if closable {
        let s = r.h - 6.0 * k;
        return close_button(r.x + r.w - s - 4.0 * k, r.y + 3.0 * k, s, skin == Skin::Red);
    }
    false
}

/// A modal window: background, silver frame, title bar. Returns the area below the title
/// bar and whether the close box was clicked.
pub fn window(r: Rect, title: &str, skin: Skin, closable: bool) -> (Rect, bool) {
    window_styled(r, title, skin, closable, true)
}

/// A window with a plain title: no ornaments, the body font (the town window).
pub fn window_plain(r: Rect, title: &str, skin: Skin, closable: bool) -> (Rect, bool) {
    window_styled(r, title, skin, closable, false)
}

fn window_styled(r: Rect, title: &str, skin: Skin, closable: bool, ornate: bool) -> (Rect, bool) {
    let k = k();
    BIGGEST.with(|b| {
        if b.get().is_none_or(|o| o.w * o.h < r.w * r.h) {
            b.set(Some(Rect::new(r.x, r.y, r.w + 6.0 * k, r.h + 6.0 * k)));
        }
    });
    draw_rectangle(r.x + 6.0 * k, r.y + 6.0 * k, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.35));
    surface(r, skin);
    let tb = (26.0 * k).round();
    let closed = title_bar_styled(Rect::new(r.x, r.y, r.w, tb), title, skin, closable, ornate);
    window_frames(r, opx());
    (Rect::new(r.x + 2.0 * k, r.y + tb + 1.0, r.w - 4.0 * k, r.h - tb - 3.0 * k), closed)
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

/// The divider with a hint in the middle (the battle's "press ESC" strip).
pub fn hint_strip(r: Rect, hint: &str, color: Color) {
    divider(r);
    hint_text(r, hint, color);
}

/// The hint of a strip, in the middle of `r` over a dark band.
pub fn hint_text(r: Rect, hint: &str, color: Color) {
    draw_rectangle(r.x + r.h, r.y + r.h * 0.22, r.w - 2.0 * r.h, r.h * 0.56, Color::new(0.08, 0.03, 0.02, 0.55));
    let room = r.w - 3.0 * r.h;
    // Smaller if it must be, then shortened with "…".
    let size = super::widgets::fit_size(hint, room, (r.h * 0.62).round().max(11.0));
    let s = super::widgets::ellipsize(hint, room, size);
    shadow_centered(&s, r.x + r.w / 2.0, r.y + r.h * 0.5 + size * 0.36, size, color);
}

/// Corner ornaments of a text box.
pub fn corners(r: Rect) {
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

/// The red-brown parchment box of story and description texts, with silver corners.
pub fn text_box(r: Rect) {
    surface(r, Skin::Brown);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, Color::new(0.72, 0.62, 0.5, 1.0));
    corners(r);
}

/// Light parchment, e.g. the unit panel; ornaments darken the corners when the art is there.
pub fn parchment(r: Rect, ornaments: bool) {
    surface(r, Skin::Paper);
    if ornaments {
        if let Some(t) = win_fx("UnitFrame", Fx::Shade) {
            tex(&t, r, Color::new(1.0, 1.0, 1.0, 0.8));
        }
    }
    // The original darkens the parchment towards the edges.
    let e = 14.0 * k();
    let dark = Color::new(0.25, 0.12, 0.02, 0.18);
    draw_rectangle(r.x, r.y, r.w, e, dark);
    draw_rectangle(r.x, r.y + r.h - e, r.w, e, dark);
    draw_rectangle(r.x, r.y + e, e, r.h - 2.0 * e, dark);
    draw_rectangle(r.x + r.w - e, r.y + e, e, r.h - 2.0 * e, dark);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, Color::new(0.75, 0.6, 0.3, 1.0));
}

// ------------------------------------------------------------------------------------------
// Buttons
// ------------------------------------------------------------------------------------------

/// A text button on green marble with a silver edge (the original's `Btn*`). True when
/// clicked this frame.
pub fn marble_button(r: Rect, label: &str, enabled: bool, hover: bool) {
    marble_button_plain(r, label, enabled, hover);
}

/// The event window's buttons in its defeat look: the green ones with their channels turned
/// (red takes green: 0x4dba90 → 0x48dd40(2, 3, 1); 0x4aa263 swaps them in).
pub fn defeat_button(r: Rect, label: &str, enabled: bool, hover: bool) {
    let name = if r.w < 110.0 { "Btn1" } else if r.w < 145.0 { "Btn2" } else { "Btn3" };
    let state = if hover && enabled { "Down" } else { "Up" };
    match win_fx(&format!("{name}{state}"), Fx::RotateRgb) {
        Some(t) if enabled => {
            three_slice(&t, r, 14.0, WHITE);
            button_label(r, label, enabled, hover);
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
            button_label(r, label, true, hover);
        }
        None => marble_button(r, label, true, hover),
    }
}

fn marble_button_plain(r: Rect, label: &str, enabled: bool, hover: bool) {
    let name = if r.w < 110.0 { "Btn1" } else if r.w < 145.0 { "Btn2" } else { "Btn3" };
    let state = if hover && enabled { "Down" } else { "Up" };
    let fx = if enabled { Fx::Plain } else { Fx::Grey };
    if let Some(t) = win_fx(&format!("{name}{state}"), fx) {
        three_slice(&t, r, 14.0, WHITE);
    } else {
        let bg = match (enabled, hover) {
            (false, _) => Color::new(0.2, 0.22, 0.21, 1.0),
            (true, true) => Color::new(0.16, 0.36, 0.28, 1.0),
            (true, false) => Color::new(0.10, 0.25, 0.19, 1.0),
        };
        draw_rectangle(r.x, r.y, r.w, r.h, bg);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, if enabled { SILVER } else { SILVER_DARK });
        draw_rectangle_lines(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0, 1.0, Color::new(0.0, 0.0, 0.0, 0.5));
    }
    button_label(r, label, enabled, hover);
}

/// A button's label, centred, smaller if it must be to fit.
fn button_label(r: Rect, label: &str, enabled: bool, hover: bool) {
    let mut size: f32 = (r.h * 0.56).clamp(12.0, (22.0 * k()).max(12.0)).round();
    while size > 7.0 && measure(label, size).width > r.w - 10.0 {
        size -= 1.0;
    }
    let d = measure(label, size);
    let color = if !enabled {
        Color::new(0.6, 0.6, 0.58, 1.0)
    } else if hover {
        GOLD
    } else {
        CREAM
    };
    shadow_text(label, r.x + (r.w - d.width) / 2.0, r.y + (r.h + d.offset_y) / 2.0 - 1.0, size, color);
}

/// The small pill buttons of the barracks ("Hire" green, "Heal" blue). They pay: the
/// press plays `Item-Gold` (interface.md §14).
pub fn pill_button(r: Rect, label: &str, enabled: bool, green: bool) -> bool {
    let hover = enabled && super::widgets::mouse_in(r.x, r.y, r.w, r.h);
    let name = if !enabled {
        "smb-disable"
    } else if hover {
        "smb-down"
    } else {
        "smb-up"
    };
    match win(name) {
        Some(t) if green && enabled => {
            // The blue pill with its green and blue swapped, as the original makes the green
            // one from smb-up and smb-down at start (0x48dd40(…, 1, 3, 2)).
            let g = win_fx(name, Fx::SwapGreenBlue).unwrap_or(t);
            three_slice(&g, r, 12.0, WHITE);
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
    let size = super::widgets::fit_size(label, r.w - 8.0, (r.h * 0.62).round().clamp(11.0, (18.0 * k()).max(11.0)));
    let d = measure(label, size);
    shadow_text(label, r.x + (r.w - d.width) / 2.0, r.y + (r.h + d.offset_y) / 2.0 - 1.0, size, if enabled { WHITE } else { Color::new(0.8, 0.8, 0.8, 1.0) });
    let pressed = hover && super::widgets::clicked();
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
