use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use macroquad::prelude::*;

use razdor::rules::battle::Team;
use razdor::rules::content::{ArtefactType, Content, ItemId, UnitId};
use razdor::rules::items::heal_amount;
use razdor::rules::units::Stats;

use super::dt_art::DtArt;
use super::widgets::{measure, text};

/// Every unit and item picture goes through here. Defaults to coloured tokens; PNGs named
/// `<key>.png` (the unit's or item's `Key=`) in the `RAZDOR_ASSETS` directory override them.
/// With a Discord Times install (`RAZDOR_DT_DIR`), the original portraits and item icons are
/// drawn instead of tokens.
pub struct Assets {
    /// Content the pictures are for (the demo or the install's).
    content: Arc<Content>,
    /// The demo content the `RAZDOR_ASSETS` sprites were loaded for.
    demo: Arc<Content>,
    sprites: HashMap<UnitId, Texture2D>,
    item_sprites: HashMap<ItemId, Texture2D>,
    /// Original art from the player's install, if present.
    pub dt: Option<DtArt>,
}

/// Discord Times unit (`GlobalIndex`) whose portrait stands in for a demo unit, by its `Key=`.
fn dt_stand_in(key: &str) -> Option<u32> {
    Some(match key {
        "knight" => 1,
        "archmage" => 2,
        "ranger" => 3,
        "spearman" => 6,
        "archer" => 21,
        "swordsman" => 12,
        "healer" => 26,
        "bandit" => 62,
        "bandit_archer" => 80,
        "bandit_chief" => 89,
        _ => return None,
    })
}

fn item_token(content: &Content, item: ItemId) -> (Color, &'static str) {
    use ArtefactType::*;
    let def = content.item(item);
    match def.kind {
        BlowWeapon => (Color::from_rgba(190, 190, 200, 255), "W"),
        ShotWeapon => (Color::from_rgba(150, 190, 120, 255), "B"),
        Staff => (Color::from_rgba(160, 120, 220, 255), "T"),
        Armor => (Color::from_rgba(140, 140, 150, 255), "A"),
        Helm => (Color::from_rgba(160, 130, 90, 255), "H"),
        Shield => (Color::from_rgba(150, 100, 60, 255), "S"),
        Ring => (Color::from_rgba(230, 200, 80, 255), "R"),
        Amulet => (Color::from_rgba(90, 200, 190, 255), "M"),
        Item => (Color::from_rgba(200, 200, 170, 255), "$"),
        Potion if heal_amount(def) > 0 => (Color::from_rgba(220, 70, 70, 255), "P"),
        Potion => (Color::from_rgba(200, 120, 230, 255), "P"),
    }
}

async fn load_png(path: &str) -> Option<Texture2D> {
    if !Path::new(path).exists() {
        return None;
    }
    match load_texture(path).await {
        Ok(tex) => {
            tex.set_filter(FilterMode::Nearest);
            Some(tex)
        }
        Err(e) => {
            razdor::diag!("could not load {path}: {e}");
            None
        }
    }
}

/// Placeholder token: colour by role, the first letter of the name.
fn token(content: &Content, kind: UnitId) -> (Color, String) {
    let s = Stats::of_level(content, kind, 1);
    let fill = if s.is_mage() {
        Color::from_rgba(150, 120, 220, 255)
    } else if s.is_shooter() {
        Color::from_rgba(110, 180, 120, 255)
    } else {
        Color::from_rgba(185, 170, 150, 255)
    };
    let letter = content.unit(kind).name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    (fill, letter)
}

pub fn team_color(team: Team) -> Color {
    match team {
        Team::Player => Color::from_rgba(70, 130, 230, 255),
        Team::Enemy => Color::from_rgba(220, 60, 50, 255),
    }
}

impl Assets {
    /// The original unit whose pictures stand for `kind`: a demo unit's stand-in, original
    /// content its own `GlobalIndex`.
    fn dt_id(&self, kind: UnitId) -> Option<u32> {
        match self.content.unit(kind).extra.get("Key") {
            Some(key) => dt_stand_in(key),
            None => Some(kind.0),
        }
    }

    fn dt_portrait(&self, kind: UnitId) -> Option<Texture2D> {
        self.dt.as_ref()?.unit_portrait(self.dt_id(kind)?)
    }

    /// The full-body sepia figure of `kind` (the unit panels), if the install has it.
    pub fn figure(&self, kind: UnitId) -> Option<Texture2D> {
        self.dt.as_ref()?.unit_figure(self.dt_id(kind)?)
    }

    /// Where the body of [`Assets::figure`] stands across the picture (0 left, 1 right).
    pub fn figure_center(&self, kind: UnitId) -> Option<f32> {
        self.dt.as_ref()?.unit_figure_center(self.dt_id(kind)?)
    }

    /// The promotion screen's locked portrait (494340) filling `r`: the original's bust
    /// greyed, darkened and vignetted; without it the plain portrait under a dark veil.
    pub fn draw_portrait_locked(&self, kind: UnitId, team: Team, r: Rect) {
        if let Some(tex) = self.dt_id(kind).and_then(|id| self.dt.as_ref()?.unit_portrait_locked(id)) {
            draw_texture_ex(&tex, r.x, r.y, WHITE, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), ..Default::default() });
            return;
        }
        self.draw_portrait(kind, team, r);
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.2, 0.1, 0.0, 0.6));
    }

    /// A card portrait filling the square `r`: the original's bust, a custom sprite, or a
    /// placeholder (a sky in the team's colour with the unit's token).
    pub fn draw_portrait(&self, kind: UnitId, team: Team, r: Rect) {
        let custom = self.custom_sprites().then(|| self.sprites.get(&kind).cloned()).flatten();
        if let Some(tex) = self.dt_portrait(kind) {
            draw_texture_ex(&tex, r.x, r.y, WHITE, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), ..Default::default() });
            return;
        }
        let sky = match team {
            Team::Player => (Color::from_rgba(70, 110, 170, 255), Color::from_rgba(170, 190, 215, 255)),
            Team::Enemy => (Color::from_rgba(120, 60, 50, 255), Color::from_rgba(210, 160, 120, 255)),
        };
        let bands = 8;
        for i in 0..bands {
            let t = i as f32 / (bands - 1) as f32;
            let c = Color::new(sky.0.r + (sky.1.r - sky.0.r) * t, sky.0.g + (sky.1.g - sky.0.g) * t, sky.0.b + (sky.1.b - sky.0.b) * t, 1.0);
            draw_rectangle(r.x, r.y + r.h * i as f32 / bands as f32, r.w, r.h / bands as f32 + 0.5, c);
        }
        if let Some(tex) = custom {
            let params = DrawTextureParams { dest_size: Some(vec2(r.w * 0.8, r.h * 0.8)), ..Default::default() };
            draw_texture_ex(&tex, r.x + r.w * 0.1, r.y + r.h * 0.1, WHITE, params);
            return;
        }
        let (fill, letter) = token(&self.content, kind);
        let (cx, cy, s) = (r.x + r.w / 2.0, r.y + r.h / 2.0, r.w.min(r.h));
        // Shoulders and head, like a bust.
        let (top, bottom) = (cy + s * 0.12, r.y + r.h);
        let (a, b, c, d) = (vec2(cx - s * 0.2, top), vec2(cx + s * 0.2, top), vec2(cx + s * 0.42, bottom), vec2(cx - s * 0.42, bottom));
        draw_triangle(a, b, c, fill);
        draw_triangle(a, c, d, fill);
        draw_circle(cx, cy - s * 0.06, s * 0.22, fill);
        draw_circle_lines(cx, cy - s * 0.06, s * 0.22, 2.0, Color::new(0.0, 0.0, 0.0, 0.5));
        let fs = (s * 0.3).round();
        let dim = measure(&letter, fs);
        text(&letter, cx - dim.width / 2.0, cy - s * 0.06 + dim.offset_y / 2.0, fs, BLACK);
    }

    /// Original item icon, for original content only (demo items have their own keys).
    fn dt_item_icon(&self, item: ItemId) -> Option<Texture2D> {
        if self.content.item(item).extra.contains_key("Key") {
            return None;
        }
        self.dt.as_ref()?.item_icon(item.0)
    }

    pub async fn load(content: Arc<Content>) -> Self {
        let mut sprites = HashMap::new();
        let mut item_sprites = HashMap::new();
        if let Ok(dir) = std::env::var("RAZDOR_ASSETS") {
            for kind in content.unit_ids() {
                if let Some(tex) = load_png(&format!("{dir}/{}.png", content.unit_key(kind))).await {
                    sprites.insert(kind, tex);
                }
            }
            for item in content.item_ids() {
                if let Some(tex) = load_png(&format!("{dir}/{}.png", content.item_key(item))).await {
                    item_sprites.insert(item, tex);
                }
            }
        }
        let dt = DtArt::from_env();
        if let Some(d) = &dt {
            super::chrome::set_install(d.install.dir.clone());
        }
        Assets { demo: content.clone(), content, sprites, item_sprites, dt }
    }

    /// Draw pictures for `content` from now on (a new game on other content).
    pub fn set_content(&mut self, content: Arc<Content>) {
        self.content = content;
    }

    pub fn content(&self) -> &Arc<Content> {
        &self.content
    }

    fn custom_sprites(&self) -> bool {
        Arc::ptr_eq(&self.content, &self.demo)
    }

    /// Draw an item icon filling the square at (x, y).
    pub fn draw_item(&self, item: ItemId, x: f32, y: f32, size: f32) {
        let custom = self.custom_sprites().then(|| self.item_sprites.get(&item).cloned()).flatten();
        if let Some(tex) = custom.or_else(|| self.dt_item_icon(item)) {
            let params = DrawTextureParams { dest_size: Some(vec2(size, size)), ..Default::default() };
            draw_texture_ex(&tex, x, y, WHITE, params);
            return;
        }
        let (fill, letter) = item_token(&self.content, item);
        let pad = size * 0.12;
        draw_rectangle(x + pad, y + pad, size - 2.0 * pad, size - 2.0 * pad, fill);
        draw_rectangle_lines(x + pad, y + pad, size - 2.0 * pad, size - 2.0 * pad, 2.0, BLACK);
        let fs = (size * 0.5) as u16;
        let dim = measure(letter, fs as f32);
        text(letter, x + (size - dim.width) / 2.0, y + (size + dim.offset_y) / 2.0, fs as f32, BLACK);
    }

    /// Draw a unit centred on (cx, cy) inside a square of `size`.
    pub fn draw_unit(&self, kind: UnitId, team: Team, cx: f32, cy: f32, size: f32) {
        let ring = team_color(team);
        let dt_portrait = || self.dt_portrait(kind);
        let custom = self.custom_sprites().then(|| self.sprites.get(&kind).cloned()).flatten();
        if let Some(tex) = custom.or_else(dt_portrait) {
            draw_circle(cx, cy + size * 0.38, size * 0.4, Color { a: 0.5, ..ring });
            draw_texture_ex(
                &tex,
                cx - size / 2.0,
                cy - size / 2.0,
                WHITE,
                DrawTextureParams { dest_size: Some(vec2(size, size)), ..Default::default() },
            );
            return;
        }
        let (fill, letter) = token(&self.content, kind);
        let r = size * 0.38;
        draw_circle(cx, cy, r + 3.0, ring);
        draw_circle(cx, cy, r, fill);
        let fs = (size * 0.5) as u16;
        let dim = measure(&letter, fs as f32);
        text(&letter, cx - dim.width / 2.0, cy + dim.offset_y / 2.0, fs as f32, BLACK);
    }
}
