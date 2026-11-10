//! Original Discord Times art, decoded at runtime from the player's install (`RAZDOR_DT_DIR`)
//! and turned into textures on first use. Nothing is written to disk.

// Part of the API is for the building screens of the next stages.
#![allow(dead_code)]

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::path::Path;

use macroquad::prelude::*;

use razdor::dt::gfx::{self, water_frame_file, Image, ObjectSprite, ObjectSprites, ITEM_ICONS, MAP_OBJECTS, TERRAIN_TEXTURES, TEXTURES_DIR, UNIT_FIGURES, UNIT_PORTRAITS, WATER_FRAMES};
use razdor::depth::{self, queue::{self, Job, Overlay}};
use razdor::dt::cards::{self, Card};
use razdor::dt::preload;
use std::path::PathBuf;
use razdor::dt::install::DtInstall;
use razdor::dt::DtError;

use super::terrain::TerrainLayer;
use super::world_view::surface_color;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Portrait(u32),
    LockedPortrait(u32),
    Figure(u32),
    Item(u32),
    Terrain(u8),
    Object(u8, u8),
    Building(u8, u8),
}

/// A texture [`DtArt::warm`] makes ahead, once its picture is decoded.
#[derive(Clone, Copy)]
enum Warm {
    Terrain(u8),
    Portrait(u32),
    Item(u32),
}

/// Every map object and building sprite in one texture, so the world map draws in few batches.
pub struct Atlas {
    pub texture: Texture2D,
    /// (section, category, index) → source rectangle in `texture`.
    rects: RectTable,
    /// The 2.5D cards of the building sprites that have shipped cut data ([`cards`]), by the same key.
    cards: HashMap<(u32, u32, u32), &'static [Card]>,
    /// The cards the depth model cut, or the cache held, for the building sprites with no shipped
    /// cut (`depth`). Grows while the workers answer ([`DtArt::depth_tick`]).
    live: RefCell<Overlay>,
    /// The building sprites (positions in the object sprites) the model still has to cut, in order.
    pending: RefCell<VecDeque<usize>>,
    /// The building band's share of the texture's height: every building picture lies above it, every
    /// other sprite below (`gfx::pack_atlas_banded`). The lit shader reads an opening's glow from it.
    band: f32,
}

/// The atlas's rectangles as a dense table over the two sections and the 8-bit category and index
/// that [`Atlas::decoration`] and [`Atlas::building`] look up (a `HashMap` cost a hash per sprite a
/// frame). A sprite key outside them can never be asked for, so it is not kept.
struct RectTable(Vec<Option<Rect>>);

impl RectTable {
    fn slot(section: u32, cat: u32, idx: u32) -> Option<usize> {
        (section < 2 && cat < 256 && idx < 256).then_some(((section << 16) | (cat << 8) | idx) as usize)
    }

    /// The table of `entries`; a later entry with the same key replaces an earlier one.
    fn from_entries(entries: impl IntoIterator<Item = ((u32, u32, u32), Rect)>) -> RectTable {
        let mut table = vec![None; 2 << 16];
        for ((section, cat, idx), r) in entries {
            if let Some(k) = Self::slot(section, cat, idx) {
                table[k] = Some(r);
            }
        }
        RectTable(table)
    }

    fn get(&self, section: u32, cat: u32, idx: u32) -> Option<Rect> {
        Self::slot(section, cat, idx).and_then(|k| self.0[k])
    }
}

impl Atlas {
    /// The building band's share of the texture's height (texture v below it is a building picture).
    pub fn building_band(&self) -> f32 {
        self.band
    }

    /// Sprite of a `.DTm` map object (class, sprite id).
    pub fn decoration(&self, class: u8, sprite: u8) -> Option<Rect> {
        self.rects.get(ObjectSprite::DECORATIONS, class.into(), sprite.into())
    }

    /// The atlas rectangle of a sprite by its section, category and index (`ObjectSprite`).
    pub fn rect_of(&self, section: u32, cat: u32, idx: u32) -> Option<Rect> {
        self.rects.get(&(section, cat, idx)).copied()
    }

    /// Sprite of a building by picture type and variant.
    pub fn building(&self, picture_type: u8, variant: u8) -> Option<Rect> {
        self.rects.get(ObjectSprite::BUILDINGS, picture_type.into(), variant.into())
    }

    /// The 2.5D cards of a building's sprite, back to front: the shipped cut, else the depth
    /// model's. `None` for a picture with no cut data (it stands as one billboard).
    pub fn building_cards(&self, picture_type: u8, variant: u8) -> Option<&[Card]> {
        let id = (ObjectSprite::BUILDINGS, picture_type.into(), variant.into());
        self.cards.get(&id).copied().or_else(|| self.live.borrow().get(id))
    }
}

/// Width of the map-object atlas in pixels.
const ATLAS_WIDTH: u32 = 2048;

/// Lazily decoded sheets and a texture cache. Missing or broken art gives `None`
/// (logged once), so callers fall back to placeholders.
pub struct DtArt {
    pub install: DtInstall,
    portraits: OnceCell<Vec<Image>>,
    figures: OnceCell<Vec<Image>>,
    items: OnceCell<Vec<Image>>,
    objects: OnceCell<ObjectSprites>,
    atlas: OnceCell<Option<Atlas>>,
    terrain_layer: OnceCell<Option<TerrainLayer>>,
    /// The terrain textures, for the minimap's colours.
    terrain_images: OnceCell<Vec<Option<Image>>>,
    textures: RefCell<HashMap<Key, Option<Texture2D>>>,
    /// Map figures (`Graphics/Units/*.ugs`) as 8×8 sheets of 64×64 frames, by file stem.
    figures_sheets: RefCell<HashMap<String, Option<Texture2D>>>,
    /// Textures [`DtArt::warm`] makes ahead, in order, with the picture each waits for.
    warm: RefCell<VecDeque<(PathBuf, Warm)>>,
    /// The depth model's workers, once they are started ([`DtArt::depth_tick`]).
    depth: RefCell<Option<depth::Service>>,
    /// The shadow atlas of «От солнца» (`shadow`): the map atlas's layout, each sprite's blurred
    /// silhouette in its alpha. Made on the first frame that casts sun shadows.
    shadows: OnceCell<Option<Texture2D>>,
}

fn or_log<T: Default>(what: &str, r: Result<T, DtError>) -> T {
    r.unwrap_or_else(|e| {
        razdor::diag!("Discord Times art: {what}: {e}");
        T::default()
    })
}

/// The picture files under `dir`, in path order: `.lit`, `.ugs`, `.spi` and `.bmp`.
fn graphics_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = entries.filter_map(|e| Some(e.ok()?.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            graphics_files(&p, out);
        } else if matches!(p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref(), Some("lit" | "ugs" | "spi" | "bmp")) {
            out.push(p);
        }
    }
}

fn texture(img: &Image) -> Option<Texture2D> {
    let (w, h) = (u16::try_from(img.width).ok()?, u16::try_from(img.height).ok()?);
    let tex = Texture2D::from_rgba8(w, h, &img.rgba);
    tex.set_filter(FilterMode::Linear);
    Some(tex)
}

impl DtArt {
    pub fn load(install: DtInstall) -> DtArt {
        DtArt {
            install,
            portraits: OnceCell::new(),
            figures: OnceCell::new(),
            items: OnceCell::new(),
            objects: OnceCell::new(),
            atlas: OnceCell::new(),
            terrain_layer: OnceCell::new(),
            terrain_images: OnceCell::new(),
            textures: RefCell::new(HashMap::new()),
            figures_sheets: RefCell::new(HashMap::new()),
            warm: RefCell::new(VecDeque::new()),
            depth: RefCell::new(None),
            shadows: OnceCell::new(),
        }
    }

    /// The player's install ([`DtInstall::from_env`]: `RAZDOR_DT_DIR`, the remembered folder,
    /// or one found in the usual places), if there is one and it loads.
    pub fn from_env() -> Option<DtArt> {
        match DtInstall::from_env() {
            Ok(install) => Some(DtArt::load(install)),
            Err(e) => {
                razdor::diag!("Discord Times install not usable, using placeholders: {e}");
                None
            }
        }
    }

    fn cached(&self, key: Key, make: impl FnOnce() -> Option<Texture2D>) -> Option<Texture2D> {
        if let Some(t) = self.textures.borrow().get(&key) {
            return t.clone();
        }
        let t = make();
        self.textures.borrow_mut().insert(key, t.clone());
        t
    }

    /// Colour bust (92×92) of a unit, by `GlobalIndex`, with its painted sky: the picture's
    /// alpha only masks the figure, the original draws the whole square.
    pub fn unit_portrait(&self, unit_id: u32) -> Option<Texture2D> {
        self.cached(Key::Portrait(unit_id), || {
            let sheet = self.portraits.get_or_init(|| or_log("unit portraits", self.install.unit_portraits()));
            let mut img = sheet.get(self.install.unit_picture_frame(unit_id)?)?.clone();
            img.rgba.chunks_exact_mut(4).for_each(|p| p[3] = 255);
            texture(&img)
        })
    }

    /// The bust of [`DtArt::unit_portrait`] as the promotion screen shows it when the unit
    /// cannot be promoted ([`super::chrome::lock_portrait`]).
    pub fn unit_portrait_locked(&self, unit_id: u32) -> Option<Texture2D> {
        self.cached(Key::LockedPortrait(unit_id), || {
            let sheet = self.portraits.get_or_init(|| or_log("unit portraits", self.install.unit_portraits()));
            let mut img = sheet.get(self.install.unit_picture_frame(unit_id)?)?.clone();
            img.rgba.chunks_exact_mut(4).for_each(|p| p[3] = 255);
            super::chrome::lock_portrait(&mut img);
            texture(&img)
        })
    }

    /// Full-body sepia portrait of a unit, by `GlobalIndex`.
    pub fn unit_figure(&self, unit_id: u32) -> Option<Texture2D> {
        self.cached(Key::Figure(unit_id), || {
            let sheet = self.figures.get_or_init(|| or_log("unit figures", self.install.unit_figures()));
            texture(sheet.get(self.install.unit_picture_frame(unit_id)?)?)
        })
    }

    /// Icon (53×53) of an artefact, by `GlobalIndex`.
    pub fn item_icon(&self, artefact_id: u32) -> Option<Texture2D> {
        self.cached(Key::Item(artefact_id), || {
            let sheet = self.items.get_or_init(|| or_log("item icons", self.install.item_icons()));
            texture(sheet.get(self.install.artefact_icon_frame(artefact_id)?)?)
        })
    }

    /// World-map texture of a terrain code (tiles seamlessly).
    pub fn terrain(&self, code: u8) -> Option<Texture2D> {
        self.cached(Key::Terrain(code), || {
            texture(&or_log("terrain texture", self.install.terrain_texture(code).map(Some))?)
        })
    }

    /// The blended terrain renderer over all terrain textures (`None` if none loads or the
    /// shader fails).
    /// Queues every picture of the install for the decode store ([`preload::preload`]), most
    /// likely first: the map's terrain, water and figures, the map objects and the minimap's
    /// art, the building windows' portraits and item icons, then every other picture under
    /// `Graphics/` in path order. The store keeps up to its cap ([`preload`]), so the rest
    /// decodes on demand. Also queues the textures [`DtArt::warm`] makes: the terrain, the
    /// portraits and the item icons. The window art was queued before ([`super::chrome::set_install`]).
    pub fn preload_all(&self) {
        let file = |rel: &str| self.install.file(rel).ok();
        let mut paths: Vec<PathBuf> = TERRAIN_TEXTURES.iter().filter_map(|name| file(&format!("{TEXTURES_DIR}/{name}"))).collect();
        paths.extend((0..WATER_FRAMES).filter_map(|i| file(&water_frame_file(i))));
        if let Ok(units) = self.install.file("Graphics/Units") {
            if let Ok(entries) = std::fs::read_dir(units) {
                let mut figures: Vec<PathBuf> = entries.filter_map(|e| Some(e.ok()?.path())).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ugs"))).collect();
                figures.sort();
                paths.extend(figures);
            }
        }
        paths.extend(file(MAP_OBJECTS));
        paths.extend(file("Graphics/Windows/MM_Icons.ugs"));
        paths.extend(file("Graphics/Windows/MiniMap_Frame_400x400.lit"));
        paths.extend(file(UNIT_PORTRAITS));
        paths.extend(file(UNIT_FIGURES));
        paths.extend(file(ITEM_ICONS));
        if let Ok(graphics) = self.install.file("Graphics") {
            graphics_files(&graphics, &mut paths);
        }
        preload::preload(paths);

        let mut warm: VecDeque<(PathBuf, Warm)> = VecDeque::new();
        for code in 0..TERRAIN_TEXTURES.len() as u8 {
            if let Some(p) = file(&format!("{TEXTURES_DIR}/{}", TERRAIN_TEXTURES[code as usize])) {
                warm.push_back((p, Warm::Terrain(code)));
            }
        }
        if let Some(p) = file(UNIT_PORTRAITS) {
            warm.extend(self.install.unit_pictures.iter().map(|&(id, _)| (p.clone(), Warm::Portrait(id))));
        }
        if let Some(p) = file(ITEM_ICONS) {
            warm.extend(self.install.artefacts.iter().map(|a| (p.clone(), Warm::Item(a.id))));
        }
        *self.warm.borrow_mut() = warm;
    }

    /// Makes the textures [`DtArt::preload_all`] queued whose pictures the decode store has ready,
    /// within `budget_ms` (see [`preload::upload_within_budget`]); a few per frame.
    pub fn warm(&self, budget_ms: f64) {
        // Snapshot runs make their textures on demand, as before.
        if std::env::var_os("RAZDOR_SNAPSHOT").is_some() {
            return;
        }
        let mut queue = std::mem::take(&mut *self.warm.borrow_mut());
        if queue.is_empty() {
            return;
        }
        let start = std::time::Instant::now();
        preload::upload_within_budget(
            &mut queue,
            |(path, _)| preload::is_ready(path),
            |(_, item)| match *item {
                Warm::Terrain(code) => {
                    self.terrain(code);
                }
                Warm::Portrait(id) => {
                    self.unit_portrait(id);
                }
                Warm::Item(id) => {
                    self.item_icon(id);
                }
            },
            || start.elapsed().as_secs_f64() * 1000.0,
            budget_ms,
        );
        *self.warm.borrow_mut() = queue;
    }

    pub fn terrain_layer(&self) -> Option<&TerrainLayer> {
        self.terrain_layer
            .get_or_init(|| {
                let textures = std::array::from_fn(|code| self.install.terrain_texture(code as u8).ok());
                let fill = std::array::from_fn(|code| surface_color(code as u8).into());
                let water = or_log("water frames", self.install.water_frames());
                TerrainLayer::new(&textures, &fill, &water)
            })
            .as_ref()
    }

    /// Minimap colour of cell `(x, y)` with terrain `code`: the terrain texture's own pixel
    /// there (so the minimap has the grain of the ground, as the original's), `None` without
    /// the texture.
    pub fn minimap_ground(&self, code: u8, x: i32, y: i32) -> Option<[u8; 3]> {
        let images = self.terrain_images.get_or_init(|| (0..16u8).map(|c| self.install.terrain_texture(c).ok()).collect());
        let img = images.get(code as usize)?.as_ref()?;
        // Average a 4×4 patch where the cell's middle falls in the texture.
        let (cx, cy) = ((x * 32 + 16).rem_euclid(img.width as i32) as u32, (y * 22 + 11).rem_euclid(img.height as i32) as u32);
        let mut sum = [0u32; 3];
        for j in 0..4 {
            for i in 0..4 {
                let p = img.pixel((cx + i * 3) % img.width, (cy + j * 3) % img.height);
                (0..3).for_each(|c| sum[c] += p[c] as u32);
            }
        }
        Some(sum.map(|v| (v / 16) as u8))
    }

    /// Minimap colour of a map object (hills, trees, rocks): the average colour stored with
    /// its sprite (`Objects.ugs` section A, B-G-R).
    pub fn minimap_object(&self, class: u8, sprite: u8) -> Option<[u8; 3]> {
        let e = &self.objects().decoration(class, sprite)?.extra;
        (e.len() >= 3).then(|| [e[2], e[1], e[0]])
    }

    /// The decoded `Objects.ugs` (the editor's palette).
    pub fn objects(&self) -> &ObjectSprites {
        self.objects.get_or_init(|| or_log("map objects", self.install.map_objects()))
    }

    /// Sprite of a map object (`.DTm` object class and sprite id).
    pub fn object(&self, class: u8, sprite: u8) -> Option<Texture2D> {
        self.cached(Key::Object(class, sprite), || texture(&self.objects().decoration(class, sprite)?.image))
    }

    /// All map objects and buildings packed into one texture.
    pub fn map_atlas(&self) -> Option<&Atlas> {
        self.atlas
            .get_or_init(|| {
                let sprites = &self.objects().sprites;
                let images: Vec<&Image> = sprites.iter().map(|s| &s.image).collect();
                // The building pictures are packed first, in their own band (the lit shader's openings).
                let buildings: Vec<bool> = sprites.iter().map(|s| s.section == ObjectSprite::BUILDINGS).collect();
                let Some((image, pos, band_px)) = gfx::pack_atlas_banded(&images, ATLAS_WIDTH, &buildings) else {
                    razdor::diag!("Discord Times art: map objects do not fit an atlas");
                    return None;
                };
                let texture = texture(&image)?;
                let rects = RectTable::from_entries(
                    sprites
                        .iter()
                        .zip(pos)
                        .map(|(s, (x, y))| ((s.section, s.cat, s.idx), Rect::new(x as f32, y as f32, s.image.width as f32, s.image.height as f32))),
                );
                // The cut data is keyed by the sprite's pixels (`tools/depth`); a key whose size
                // differs is no match. A building with no shipped cut takes its cached depth-model
                // cut now (no model needed), and the rest queue for the model (`depth_tick`).
                let mut cards = HashMap::new();
                let mut live = Overlay::default();
                let mut known = Vec::with_capacity(sprites.len());
                for s in sprites.iter() {
                    let id = (s.section, s.cat, s.idx);
                    let building = s.section == ObjectSprite::BUILDINGS;
                    let key = cards::sprite_key(&s.image);
                    let entry = building.then(|| cards::builtin().get(&key).filter(|k| k.w == s.image.width && k.h == s.image.height)).flatten();
                    if let Some(k) = entry {
                        cards.insert(id, k.cards.as_slice());
                    }
                    // A one-card building is shipped as known too: it stands as one billboard.
                    let shipped = building && (entry.is_some() || cards::builtin_one_card().contains(&key));
                    let mut cached = false;
                    if building && !shipped {
                        if let Some(c) = depth::cached(&s.image) {
                            live.insert(id, c);
                            cached = true;
                        }
                    }
                    known.push(queue::Known { building, shipped, cached });
                }
                let pending: VecDeque<usize> = queue::unknown(known).into();
                depth::set_unknown(pending.len());
                let band = band_px as f32 / image.height.max(1) as f32;
                Some(Atlas { texture, rects, cards, live: RefCell::new(live), pending: RefCell::new(pending), band })
            })
            .as_ref()
    }

    /// A map figure sheet (`Graphics/Units/<stem>.ugs`): 8 rows (facings, clockwise from
    /// north-west) of 8 walking (or rowing) frames, square: 64×64 for the figures and the
    /// hero's galley, 128×128 for the pirate and merchant ships. The frame size is the
    /// sheet's width / 8.
    pub fn figure_sheet(&self, stem: &str) -> Option<Texture2D> {
        if let Some(t) = self.figures_sheets.borrow().get(stem) {
            return t.clone();
        }
        let frames = or_log("map figures", self.install.graphic(&format!("Graphics/Units/{stem}.ugs")));
        let n = frames.first().map_or(0, |f| f.width as usize);
        let t = (frames.len() == 64 && n > 0 && frames.iter().all(|f| f.width as usize == n && f.height as usize == n))
            .then(|| {
                let side = n * 8;
                let mut rgba = vec![0u8; side * side * 4];
                for (i, f) in frames.iter().enumerate() {
                    let (ox, oy) = ((i % 8) * n, (i / 8) * n);
                    for row in 0..n {
                        let dst = ((oy + row) * side + ox) * 4;
                        rgba[dst..dst + n * 4].copy_from_slice(&f.rgba[row * n * 4..(row + 1) * n * 4]);
                    }
                }
                texture(&Image { width: side as u32, height: side as u32, rgba })
            })
            .flatten();
        self.figures_sheets.borrow_mut().insert(stem.to_string(), t.clone());
        t
    }

    /// Sprite of a building by picture type and variant (`.DTm` building bytes 5 and 4).
    pub fn building(&self, picture_type: u8, variant: u8) -> Option<Texture2D> {
        self.cached(Key::Building(picture_type, variant), || {
            texture(&self.objects().building(picture_type, variant)?.image)
        })
    }

    /// The shadow atlas (`shadow::silhouette` of every sprite, in the map atlas's rectangles; the
    /// silhouette is the alpha, the colour black). `None` without the map atlas.
    pub fn shadow_texture(&self) -> Option<Texture2D> {
        self.shadows.get_or_init(|| self.build_shadow_texture()).clone()
    }

    fn build_shadow_texture(&self) -> Option<Texture2D> {
        let atlas = self.map_atlas()?;
        let (w, h) = (atlas.texture.width() as u32, atlas.texture.height() as u32);
        // White colour, alpha 0: the shadow's colour comes from the vertices (`shadow::strength_colour`).
        let mut rgba: Vec<u8> = [255u8, 255, 255, 0].repeat((w * h) as usize);
        for s in &self.objects().sprites {
            let Some(r) = atlas.rect_of(s.section, s.cat, s.idx) else { continue };
            let mask = super::shadow::silhouette(&s.image);
            for y in 0..s.image.height {
                for x in 0..s.image.width {
                    let alpha = mask[(y * s.image.width + x) as usize];
                    let at = (((r.y as u32 + y) * w + r.x as u32 + x) * 4 + 3) as usize;
                    rgba[at] = alpha;
                }
            }
        }
        texture(&Image { width: w, height: h, rgba })
    }

    /// Runs the built-in depth model for the 2.5D view, once a frame of the world map: when the
    /// model file is there, the workers start and the building sprites with no cut are handed to
    /// them as they have room; the cards that come back join the atlas. Does nothing flat, without
    /// the feature, in a snapshot run, or without a model file (the model is never fetched here).
    pub fn depth_tick(&self, tilted: bool) {
        if !tilted || !depth::available() || std::env::var_os("RAZDOR_SNAPSHOT").is_some() {
            return;
        }
        let Some(atlas) = self.map_atlas() else { return };
        let mut work = self.depth.borrow_mut();
        if work.is_none() {
            if atlas.pending.borrow().is_empty() {
                return;
            }
            let Some(model) = depth::fetch::model_file() else { return };
            *work = depth::Service::start(model, depth::cache_folder());
        }
        let Some(service) = work.as_mut() else { return };
        let done = service.drain();
        atlas.live.borrow_mut().add(done);
        let mut pending = atlas.pending.borrow_mut();
        while service.room() > 0 {
            let Some(i) = pending.pop_front() else { break };
            let s = &self.objects().sprites[i];
            if !service.submit(Job { id: (s.section, s.cat, s.idx), img: s.image.clone() }) {
                pending.push_front(i);
                break;
            }
        }
        depth::set_unknown(pending.len() + service.in_flight());
    }
}

#[cfg(test)]
mod rect_table_tests {
    use super::*;

    fn r(x: f32) -> Rect {
        Rect::new(x, x + 1.0, 2.0, 3.0)
    }

    #[test]
    fn the_table_gives_what_a_map_gives_for_every_key_the_atlas_asks_for() {
        // A duplicate key (the last one wins, as a map's insert does), a key past 8 bits (never asked for).
        let entries = [((0u32, 3u32, 7u32), r(1.0)), ((1, 0, 0), r(2.0)), ((0, 3, 7), r(3.0)), ((0, 300, 1), r(4.0)), ((2, 0, 0), r(5.0)), ((0, 255, 255), r(6.0))];
        let map: HashMap<(u32, u32, u32), Rect> = entries.iter().copied().collect();
        let table = RectTable::from_entries(entries.iter().copied());
        for section in 0..2 {
            for cat in 0..256 {
                for idx in 0..256 {
                    assert_eq!(table.get(section, cat, idx), map.get(&(section, cat, idx)).copied(), "({section}, {cat}, {idx})");
                }
            }
        }
        assert_eq!(table.get(0, 3, 7), Some(r(3.0)));
        assert_eq!(table.get(0, 255, 255), Some(r(6.0)));
        assert_eq!(table.get(2, 0, 0), None, "a section the atlas never asks for");
    }
}
