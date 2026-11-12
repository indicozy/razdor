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
use razdor::dt::lights::{self, Kind};
use razdor::dt::preload;
use razdor::dt::windows;
use std::path::PathBuf;
use razdor::dt::install::DtInstall;
use razdor::dt::DtError;

use super::atlas_cut;
use super::living;
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

/// Where a sprite's picture is in an atlas (Razdor's cut, `atlas_cut`): the box of its opaque texels, not the whole
/// sprite, so a quad covers the box only. A draw places the box at the sprite's origin plus `off` (sprite pixels,
/// scaled by the zoom) and takes `rect` as its source; the sprite's pixel `p` is texel `base + p` at full resolution.
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// The box in the texture's texels.
    pub rect: Rect,
    /// The box's top-left and size in sprite pixels (at full resolution).
    pub off: Vec2,
    pub size: Vec2,
    /// The texel of the sprite's pixel (0, 0) at full resolution.
    pub base: Vec2,
    /// The whole sprite's size in sprite pixels.
    pub full: Vec2,
    /// The whole sprite's size in texels: `full`, or the smaller copy's (`Atlas::decoration_lod`), where `full`
    /// stays the sprite's own size (the draw keeps its place and size; only the texels are the copy's).
    pub tex: Vec2,
}

impl Slot {
    /// How the sprite's pixels lie on its texture ([`Frame`]).
    pub fn frame(&self) -> Frame {
        Frame { base: self.base, full: self.full }
    }
}

/// How a sprite's pixels lie on a texture: its pixel `p` is texel `base + p` and the sprite is `full` pixels big. The
/// meshes that cut a sprite into pieces (the cards, the moving parts) take their uvs and shading from it.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub base: Vec2,
    pub full: Vec2,
}

/// A texture [`DtArt::warm`] makes ahead, once its picture is decoded.
#[derive(Clone, Copy)]
enum Warm {
    Terrain(u8),
    Portrait(u32),
    Item(u32),
}

/// Every map object and building sprite in one texture, so the world map draws in few batches.
/// The soft shadow atlas's blur, in passes of [1 2 1] / 4 (about 3 px at the tip).
const SOFT_PASSES: usize = 3;

pub struct Atlas {
    pub texture: Texture2D,
    /// (section, category, index) → the sprite's slot in `texture` ([`Slot`]).
    rects: RectTable<Slot>,
    /// The 2.5D cards of the building sprites that have shipped cut data ([`cards`]), by the same key.
    cards: HashMap<(u32, u32, u32), &'static [Card]>,
    /// The cards the depth model cut, or the cache held, for the building sprites with no shipped
    /// cut (`depth`). Grows while the workers answer ([`DtArt::depth_tick`]).
    live: RefCell<Overlay>,
    /// The building sprites (positions in the object sprites) the model still has to cut, in order.
    pending: RefCell<VecDeque<usize>>,
    /// The half and the quarter copies of the map objects in this texture (`decoration_lod`), by the same key; `None`
    /// when they do not fit the GL texture limit (`lod_copies`), and then the draws use the full sprites.
    lod: Option<RectTable<[Slot; 2]>>,
    /// The living map's parts of the building sprites that have light points (`dt::lights`), by the
    /// same key: their chimneys and their moving parts (`living`).
    living: HashMap<(u32, u32, u32), Living>,
}

/// A building sprite's living parts (Razdor's «Живая карта», `living`): its chimneys, the boxes
/// the smoke rises from, and its moving parts (flags). A sprite with moving parts is drawn
/// from `body`, a copy with their areas cleared, whose texture is made on first use.
pub struct Living {
    /// Each chimney's box in sprite pixels.
    pub chimneys: Vec<[f32; 4]>,
    pub parts: Vec<living::Part>,
    /// The cleared copy of the picture box only (the box of the sprite's [`Slot`]).
    body: Option<Image>,
    texture: OnceCell<Option<Texture2D>>,
}

impl Living {
    /// The cleared copy's texture (the sprite's size), `None` for a sprite with no moving part.
    pub fn body(&self) -> Option<&Texture2D> {
        self.texture.get_or_init(|| self.body.as_ref().and_then(texture)).as_ref()
    }
}

/// The atlas's rectangles as a dense table over the two sections and the 8-bit category and index
/// that [`Atlas::decoration`] and [`Atlas::building`] look up (a `HashMap` cost a hash per sprite a
/// frame). A sprite key outside them can never be asked for, so it is not kept.
struct RectTable<T: Copy>(Vec<Option<T>>);

impl<T: Copy> RectTable<T> {
    fn index(section: u32, cat: u32, idx: u32) -> Option<usize> {
        (section < 2 && cat < 256 && idx < 256).then_some(((section << 16) | (cat << 8) | idx) as usize)
    }

    /// The table of `entries`; a later entry with the same key replaces an earlier one.
    fn from_entries(entries: impl IntoIterator<Item = ((u32, u32, u32), T)>) -> RectTable<T> {
        let mut table = vec![None; 2 << 16];
        for ((section, cat, idx), r) in entries {
            if let Some(k) = Self::index(section, cat, idx) {
                table[k] = Some(r);
            }
        }
        RectTable(table)
    }

    fn get(&self, section: u32, cat: u32, idx: u32) -> Option<T> {
        Self::index(section, cat, idx).and_then(|k| self.0[k])
    }
}

impl Atlas {
    /// Sprite of a `.DTm` map object (class, sprite id).
    pub fn decoration(&self, class: u8, sprite: u8) -> Option<Slot> {
        self.rects.get(ObjectSprite::DECORATIONS, class.into(), sprite.into())
    }

    /// The copy of a map object at `1/divisor` of its size (2 or 4): its slot in this texture, the full sprite's place
    /// and size with the copy's texels. `None` for another divisor, or when the copies did not fit.
    pub fn decoration_lod(&self, class: u8, sprite: u8, divisor: u32) -> Option<Slot> {
        let i = match divisor {
            2 => 0,
            4 => 1,
            _ => return None,
        };
        self.lod.as_ref()?.get(ObjectSprite::DECORATIONS, class.into(), sprite.into()).map(|c| c[i])
    }

    /// The slot of a sprite by its section, category and index (`ObjectSprite`).
    pub fn slot_of(&self, section: u32, cat: u32, idx: u32) -> Option<Slot> {
        self.rects.get(section, cat, idx)
    }

    /// Sprite of a building by picture type and variant.
    pub fn building(&self, picture_type: u8, variant: u8) -> Option<Slot> {
        self.rects.get(ObjectSprite::BUILDINGS, picture_type.into(), variant.into())
    }

    /// The 2.5D cards of a building's sprite, back to front: the shipped cut, else the depth
    /// model's. `None` for a picture with no cut data (it stands as one billboard).
    pub fn building_cards(&self, picture_type: u8, variant: u8) -> Option<&[Card]> {
        // Off by default (`building_cards_on`): every building stands as one whole picture.
        if !building_cards_on() {
            return None;
        }
        let id = (ObjectSprite::BUILDINGS, picture_type.into(), variant.into());
        self.cards.get(&id).copied().or_else(|| self.live.borrow().get(id))
    }

    /// The living parts of a building's sprite (`None` for a picture with no light points).
    pub fn living(&self, picture_type: u8, variant: u8) -> Option<&Living> {
        self.living.get(&(ObjectSprite::BUILDINGS, picture_type.into(), variant.into()))
    }
}

/// The shadow textures of the map's sprites (`DtArt::shadow_layout`): the silhouette (`sharp`) and its blur (`soft`), in
/// one layout. Each sprite's slot holds the box its shadow reaches; the texels in it are those the uncut atlas held
/// (its blur spreads over the neighbours, so they are copied from it).
pub struct ShadowAtlas {
    sharp: Texture2D,
    soft: Texture2D,
    slots: RectTable<Slot>,
}

/// Whether the 2.5D view folds a building into its cut cards (and the depth model cuts the rest). Off by default: the
/// owner's decision (2026-11-12) is that a building stands as one whole picture, as in v1.4.1. `RAZDOR_BUILDING_CARDS=1`
/// turns the cards on for development only; with them off the depth model does not run and its setting row is hidden.
pub fn building_cards_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("RAZDOR_BUILDING_CARDS").is_ok_and(|v| v.trim() == "1"))
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
    /// The shadow atlas of «От солнца» (`shadow`): each sprite's silhouette and its blur, cut to the boxes
    /// the shadows reach ([`ShadowAtlas`]). Made on the first frame that casts shadows.
    shadows: OnceCell<Option<ShadowAtlas>>,
    /// The building mask of the map atlas (`building_emissive`), built on the first lit frame.
    building_mask: OnceCell<Option<Texture2D>>,
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

/// The GL texture size limit (`GL_MAX_TEXTURE_SIZE`) of the context the game draws with.
fn gl_max_texture_size() -> u32 {
    let mut v: i32 = 0;
    // SAFETY: the context is current on the thread that builds the map atlas (the draw thread); the call writes one int.
    unsafe { macroquad::miniquad::gl::glGetIntegerv(macroquad::miniquad::gl::GL_MAX_TEXTURE_SIZE as _, &mut v) };
    u32::try_from(v).unwrap_or(0).max(1)
}

/// The half and quarter copies of the map objects, cut like the full atlas ([`cut_pictures`]) and stacked under it in one
/// texture: a draw that switches copies keeps the texture, so its batch does not break. Returns the stacked texture and,
/// per sprite, its two copies' slots. When the stack passes the GL texture limit, the full texture alone and no copies:
/// the draws then use the full sprites.
fn lod_copies(sprites: &[ObjectSprite], full: &Image, slots: &[Slot]) -> (Image, Option<Vec<[Slot; 2]>>) {
    let mut parts: Vec<(f32, Image, Vec<Slot>)> = Vec::with_capacity(2);
    for k in [2u32, 4] {
        let small: Vec<Image> = sprites.iter().map(|s| downscale(&s.image, k)).collect();
        let refs: Vec<&Image> = small.iter().collect();
        let Some((img, sl)) = cut_pictures(&refs, &vec![None; refs.len()], full.width) else {
            razdor::diag!("sprite copies 1/{k} do not fit the atlas width; full sprites only");
            return (full.clone(), None);
        };
        parts.push((k as f32, img, sl));
    }
    let height = full.height + parts.iter().map(|p| p.1.height).sum::<u32>();
    let limit = gl_max_texture_size();
    if full.width > limit || height > limit {
        razdor::diag!("sprite copies: the stacked atlas {}x{} passes the GL limit {limit}; full sprites only", full.width, height);
        return (full.clone(), None);
    }
    let width = full.width;
    let mut rgba = full.rgba.clone();
    let mut dy = full.height as f32;
    let mut per: Vec<Vec<Slot>> = Vec::with_capacity(2);
    for (k, img, sl) in &parts {
        per.push(sl.iter().zip(slots).map(|(c, f)| copy_slot(c, *k, dy, f.full)).collect());
        dy += img.height as f32;
        rgba.extend_from_slice(&img.rgba);
    }
    let copies = (0..sprites.len()).map(|i| [per[0][i], per[1][i]]).collect();
    razdor::diag!("map atlas copies 1/2 and 1/4 stacked: {width}x{height} (GL limit {limit})");
    (Image { width, height, rgba }, Some(copies))
}

/// A copy's slot in the stacked texture: the copy's own texels moved down by `dy`, at the full sprite's size (`full`,
/// its box at `k` times the copy's box), so a draw places it where the full sprite goes.
fn copy_slot(copy: &Slot, k: f32, dy: f32, full: Vec2) -> Slot {
    let base = vec2(copy.base.x, copy.base.y + dy);
    let rect = Rect::new(base.x + copy.off.x, base.y + copy.off.y, copy.size.x, copy.size.y);
    Slot { rect, off: copy.off * k, size: copy.size * k, base, full, tex: copy.full }
}

/// `img` at `1/k` of its size: each `k`×`k` block averaged (the alpha too), the edge blocks cut short.
fn downscale(img: &Image, k: u32) -> Image {
    let (w, h) = (img.width.div_ceil(k), img.height.div_ceil(k));
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for sy in (y * k)..((y + 1) * k).min(img.height) {
                for sx in (x * k)..((x + 1) * k).min(img.width) {
                    let i = ((sy * img.width + sx) * 4) as usize;
                    for (c, sum) in acc.iter_mut().enumerate() {
                        *sum += u32::from(img.rgba[i + c]);
                    }
                    n += 1;
                }
            }
            let o = ((y * w + x) * 4) as usize;
            for (c, sum) in acc.iter().enumerate() {
                rgba[o + c] = (sum / n.max(1)) as u8;
            }
        }
    }
    Image { width: w, height: h, rgba }
}

#[cfg(test)]
mod downscale_tests {
    use super::*;

    #[test]
    fn a_half_size_copy_averages_each_block_and_keeps_the_edge() {
        let img = Image { width: 3, height: 2, rgba: vec![10, 20, 30, 255, 30, 40, 50, 255, 90, 90, 90, 0, 0, 0, 0, 0, 50, 50, 50, 255, 50, 50, 50, 255] };
        let half = downscale(&img, 2);
        assert_eq!((half.width, half.height), (2, 1));
        assert_eq!(&half.rgba[0..4], &[22, 27, 32, 191], "the 2x2 block at the origin, alpha averaged too");
        assert_eq!(&half.rgba[4..8], &[70, 70, 70, 127], "the edge block takes the two pixels that are there");
    }
}

fn texture(img: &Image) -> Option<Texture2D> {
    let (w, h) = (u16::try_from(img.width).ok()?, u16::try_from(img.height).ok()?);
    let tex = Texture2D::from_rgba8(w, h, &img.rgba);
    tex.set_filter(FilterMode::Linear);
    Some(tex)
}

/// [`texture`] for a sprite the 2.5D pointer picks (a map atlas, a figure sheet): its alpha is kept on the CPU
/// from the image, for the pointer's alpha test ([`super::sprite_alpha`]), so the pointer never reads the GPU.
fn sprite_texture(img: &Image) -> Option<Texture2D> {
    let tex = texture(img)?;
    super::sprite_alpha::register(&tex, img);
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
            building_mask: OnceCell::new(),
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
                // The lit windows (`windows::emissive`) widen a building's box: their emission is drawn where the picture is.
                let emit: Vec<Option<Vec<u8>>> = sprites
                    .iter()
                    .map(|s| (s.section == ObjectSprite::BUILDINGS).then(|| windows::emissive(&s.image, windows::boxes_of(cards::sprite_key(&s.image)))))
                    .collect();
                let Some((image, slots)) = cut_pictures(&images, &emit, ATLAS_WIDTH) else {
                    razdor::diag!("Discord Times art: map objects do not fit an atlas");
                    return None;
                };
                let (image, copies) = lod_copies(sprites, &image, &slots);
                let texture = sprite_texture(&image)?;
                razdor::diag!("map atlas: {}x{} ({} bytes)", image.width, image.height, image.rgba.len());
                let lod = copies.map(|c| RectTable::from_entries(sprites.iter().zip(c).map(|(s, c)| ((s.section, s.cat, s.idx), c))));
                let rects = RectTable::from_entries(sprites.iter().zip(&slots).map(|(s, sl)| ((s.section, s.cat, s.idx), *sl)));
                // The cut data is keyed by the sprite's pixels (`tools/depth`); a key whose size
                // differs is no match. A building with no shipped cut takes its cached depth-model
                // cut now (no model needed), and the rest queue for the model (`depth_tick`).
                let mut cards = HashMap::new();
                let mut live = Overlay::default();
                let mut living_map = HashMap::new();
                let mut known = Vec::with_capacity(sprites.len());
                for (i, s) in sprites.iter().enumerate() {
                    let id = (s.section, s.cat, s.idx);
                    let building = s.section == ObjectSprite::BUILDINGS;
                    let key = cards::sprite_key(&s.image);
                    let entry = building.then(|| cards::builtin().get(&key).filter(|k| k.w == s.image.width && k.h == s.image.height)).flatten();
                    if let Some(k) = entry {
                        cards.insert(id, k.cards.as_slice());
                    }
                    // The light points (`dt::lights`) of a building: its chimneys and moving parts.
                    let points = building.then(|| lights::builtin().get(&key).filter(|k| k.w == s.image.width && k.h == s.image.height)).flatten();
                    if let Some(p) = points {
                        let w = s.image.width as f32;
                        let chimneys: Vec<[f32; 4]> = p.points.iter().filter(|p| p.kind == Kind::Chimney).map(|p| p.rect).collect();
                        // Only a box whose pixels look like cloth moves, so a
                        // windmill's roof or a figure's blob stays still.
                        // A flag waves when its box looks like cloth (`living::cloth_look`).
                        let parts: Vec<living::Part> = p.points.iter().enumerate().filter(|(_, q)| q.kind == Kind::Flag && living::cloth_look(&s.image, q.rect)).filter_map(|(i, q)| living::Part::of(q, w, i)).collect();
                        if !chimneys.is_empty() || !parts.is_empty() {
                            // The cleared copy holds the picture box only, as the atlas does.
                            let bx = box_of(&slots[i]);
                            let body = (!parts.is_empty()).then(|| atlas_cut::crop(&living::cleared(&s.image, &parts), bx));
                            living_map.insert(id, Living { chimneys, parts, body, texture: OnceCell::new() });
                        }
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
                Some(Atlas { texture, rects, cards, live: RefCell::new(live), pending: RefCell::new(pending), living: living_map, lod })
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
                sprite_texture(&Image { width: side as u32, height: side as u32, rgba })
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

    /// The shadow atlas (`shadow::silhouette` of every sprite; the silhouette is the alpha, the colour black), in its own
    /// layout ([`DtArt::shadow_decoration`]). `None` without the map atlas.
    pub fn shadow_texture(&self) -> Option<Texture2D> {
        self.shadow_layout().map(|s| s.sharp.clone())
    }

    /// The soft copy of the shadow atlas (`shadow::blur_alpha`, [`SOFT_PASSES`] passes): the shadows' tips.
    /// Built with the sharp one, once.
    pub fn shadow_soft_texture(&self) -> Option<Texture2D> {
        self.shadow_layout().map(|s| s.soft.clone())
    }

    /// The shadow slot of a map object (class, sprite id); `None` for a sprite with no shadow.
    pub fn shadow_decoration(&self, class: u8, sprite: u8) -> Option<Slot> {
        self.shadow_layout()?.slots.get(ObjectSprite::DECORATIONS, class.into(), sprite.into())
    }

    /// The shadow slot of a building by picture type and variant; `None` for a sprite with no shadow.
    pub fn shadow_building(&self, picture_type: u8, variant: u8) -> Option<Slot> {
        self.shadow_layout()?.slots.get(ObjectSprite::BUILDINGS, picture_type.into(), variant.into())
    }

    fn shadow_layout(&self) -> Option<&ShadowAtlas> {
        self.shadows.get_or_init(|| self.build_shadow_layout()).as_ref()
    }

    /// The emissive layer of the map atlas for the lit windows (G, `draw_building_windows`): the building mask,
    /// sampled linearly, so the windows' feathered edges interpolate. Built on the first lit frame.
    pub fn building_emissive(&self) -> Option<Texture2D> {
        self.building_mask.get_or_init(|| self.build_building_mask()).clone()
    }

    fn build_building_mask(&self) -> Option<Texture2D> {
        let atlas = self.map_atlas()?;
        let (w, h) = (atlas.texture.width() as u32, atlas.texture.height() as u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for i in (3..rgba.len()).step_by(4) {
            rgba[i] = 255;
        }
        // Each building picture's lit windows (`dt::windows`), computed once from its own pixels, over its region of the atlas.
        for s in self.objects().sprites.iter().filter(|s| s.section == ObjectSprite::BUILDINGS) {
            let Some(slot) = atlas.slot_of(s.section, s.cat, s.idx) else { continue };
            let emissive = windows::emissive(&s.image, windows::boxes_of(cards::sprite_key(&s.image)));
            write_mask(&mut rgba, w, &slot, &emissive);
        }
        texture(&Image { width: w, height: h, rgba })
    }

    /// The shadow layout ([`ShadowAtlas`]). The shadows are first made in the uncut layout (`gfx::pack_atlas`, as they
    /// always were): their blur spreads over the neighbouring sprites there, so each sprite's box is copied out of that
    /// layout, texel for texel, and the box is the part of it the shadow reaches.
    fn build_shadow_layout(&self) -> Option<ShadowAtlas> {
        let sprites = &self.objects().sprites;
        let images: Vec<&Image> = sprites.iter().map(|s| &s.image).collect();
        let (old, pos) = gfx::pack_atlas(&images, ATLAS_WIDTH)?;
        let (w, h) = (old.width, old.height);
        // White colour, alpha 0: the shadow's colour comes from the vertices (`shadow::strength_colour`).
        let mut sharp: Vec<u8> = [255u8, 255, 255, 0].repeat((w * h) as usize);
        for (s, &(x, y)) in sprites.iter().zip(&pos) {
            let mask = super::shadow::silhouette(&s.image);
            for yy in 0..s.image.height {
                for xx in 0..s.image.width {
                    let alpha = mask[(yy * s.image.width + xx) as usize];
                    sharp[(((y + yy) * w + x + xx) * 4 + 3) as usize] = alpha;
                }
            }
        }
        let mut soft = sharp.clone();
        super::shadow::blur_alpha(&mut soft, w as usize, h as usize, SOFT_PASSES);
        // Each sprite's shadow box: the texels of its rectangle that either texture holds (the sprite's own blur; the
        // quad covers the box, the texels past it are clear in the sprite's rectangle).
        let entries: Vec<((u32, u32, u32), Slot)> = sprites
            .iter()
            .zip(&pos)
            .filter_map(|(s, &at)| {
                let (sw, sh) = (s.image.width, s.image.height);
                let bx = atlas_cut::Bx::union(atlas_cut::nonzero_in(&sharp, w, h, at, sw, sh), atlas_cut::nonzero_in(&soft, w, h, at, sw, sh))?;
                Some(((s.section, s.cat, s.idx), slot_at(bx, at, (sw, sh))))
            })
            .collect();
        Some(ShadowAtlas {
            sharp: texture(&Image { width: w, height: h, rgba: sharp })?,
            soft: texture(&Image { width: w, height: h, rgba: soft })?,
            slots: RectTable::from_entries(entries),
        })
    }

    /// Runs the built-in depth model for the 2.5D view, once a frame of the world map: when the
    /// model file is there, the workers start and the building sprites with no cut are handed to
    /// them as they have room; the cards that come back join the atlas. Does nothing flat, without
    /// the feature, in a snapshot run, or without a model file (the model is never fetched here).
    pub fn depth_tick(&self, tilted: bool) {
        if !building_cards_on() || !tilted || !depth::available() || std::env::var_os("RAZDOR_SNAPSHOT").is_some() {
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

/// Writes a building's lit-window mask (`DtArt::building_emissive`) into `rgba` (`w` texels wide, the atlas's layout)
/// over the sprite's whole rectangle, as the mask always was: R is 255 (a building) and G its emission (`emissive`, one
/// byte a pixel of the sprite). The rest of the texture stays as it is (R and G 0).
pub fn write_mask(rgba: &mut [u8], w: u32, slot: &Slot, emissive: &[u8]) {
    let (fw, fh) = (slot.full.x as u32, slot.full.y as u32);
    let (bx, by) = (slot.base.x as u32, slot.base.y as u32);
    for j in 0..fh {
        for i in 0..fw {
            let t = (((by + j) * w + bx + i) * 4) as usize;
            rgba[t] = 255;
            rgba[t + 1] = emissive.get((j * fw + i) as usize).copied().unwrap_or(0);
        }
    }
}

/// The slot of a picture box `bx` of a sprite of `full` pixels whose pixel (0, 0) is at texel `at`.
fn slot_at(bx: atlas_cut::Bx, at: (u32, u32), full: (u32, u32)) -> Slot {
    let base = vec2(at.0 as f32, at.1 as f32);
    let (off, size) = (vec2(bx.x0 as f32, bx.y0 as f32), vec2(bx.w() as f32, bx.h() as f32));
    let full = vec2(full.0 as f32, full.1 as f32);
    Slot { rect: Rect::new(base.x + off.x, base.y + off.y, size.x, size.y), off, size, base, full, tex: full }
}

/// A slot's box as texels-free sprite pixels (for [`atlas_cut::crop`]).
fn box_of(slot: &Slot) -> atlas_cut::Bx {
    atlas_cut::Bx { x0: slot.off.x as u32, y0: slot.off.y as u32, x1: (slot.off.x + slot.size.x) as u32, y1: (slot.off.y + slot.size.y) as u32 }
}

/// The picture texture of `images` (`gfx::pack_atlas`, the layout the atlas always had) with each sprite's slot: its
/// picture box ([`atlas_cut::picture_box`], `emit` the lit texels of each). `None` when a picture is wider than `width`.
fn cut_pictures(images: &[&Image], emit: &[Option<Vec<u8>>], width: u32) -> Option<(Image, Vec<Slot>)> {
    let (image, pos) = gfx::pack_atlas(images, width)?;
    let slots = images
        .iter()
        .zip(emit)
        .zip(&pos)
        .map(|((img, e), &at)| slot_at(atlas_cut::picture_box(img, e.as_deref()), at, (img.width, img.height)))
        .collect();
    Some((image, slots))
}

#[cfg(test)]
mod cut_tests {
    use super::*;

    fn sprite(w: u32, h: u32, opaque: impl Fn(u32, u32) -> bool) -> Image {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&[(x * 9) as u8, (y * 7) as u8, 33, if opaque(x, y) { 255 } else { 0 }]);
            }
        }
        Image { width: w, height: h, rgba }
    }

    #[test]
    fn a_cut_picture_samples_the_same_texels_as_the_uncut_one() {
        // A 12×10 sprite with an opaque block in its lower right: its box is the block grown by one.
        let s = sprite(12, 10, |x, y| x >= 6 && y >= 4);
        let (atlas, slots) = cut_pictures(&[&s], &[None], 64).expect("fits");
        let slot = slots[0];
        assert_eq!(slot.off, vec2(5.0, 3.0));
        assert_eq!(slot.size, vec2(7.0, 7.0));
        assert_eq!(slot.full, vec2(12.0, 10.0));
        // Every texel of the box is the sprite's pixel at the same place: texel base + p (`base` may be negative: the
        // region starts before the sprite's pixel (0, 0)).
        for y in 3..10u32 {
            for x in 5..12u32 {
                let (tx, ty) = ((slot.base.x + x as f32) as u32, (slot.base.y + y as f32) as u32);
                let t = ((ty * atlas.width + tx) * 4) as usize;
                let p = ((y * 12 + x) * 4) as usize;
                assert_eq!(&atlas.rgba[t..t + 4], &s.rgba[p..p + 4], "sprite pixel ({x}, {y})");
            }
        }
        // Past the sprite's right edge the region repeats the nearest sprite pixel, as the uncut atlas's ring did:
        // sprite (12, 5) is texel base + (12, 5), the copy of sprite (11, 5).
        let past = (((slot.base.y + 5.0) as u32) * atlas.width + (slot.base.x + 12.0) as u32) as usize * 4;
        let edge = ((5 * 12 + 11) * 4) as usize;
        assert_eq!(&atlas.rgba[past..past + 4], &s.rgba[edge..edge + 4], "the replicated edge past the sprite");
    }

    #[test]
    fn a_copy_slot_is_the_copy_texels_at_the_full_sprite_size() {
        // A copy at 1/2: its box (2×1 texels at (1, 4) of its picture, the picture at (2, 1)) moved down by 100 texels.
        let copy = Slot { rect: Rect::new(3.0, 5.0, 2.0, 1.0), off: vec2(1.0, 4.0), size: vec2(2.0, 1.0), base: vec2(2.0, 1.0), full: vec2(4.0, 3.0), tex: vec2(4.0, 3.0) };
        let c = copy_slot(&copy, 2.0, 100.0, vec2(8.0, 6.0));
        assert_eq!(c.base, vec2(2.0, 101.0));
        assert_eq!(c.rect, Rect::new(3.0, 105.0, 2.0, 1.0), "the copy's texels");
        assert_eq!((c.off, c.size), (vec2(2.0, 8.0), vec2(4.0, 2.0)), "the box at the full sprite's size");
        assert_eq!((c.full, c.tex), (vec2(8.0, 6.0), vec2(4.0, 3.0)));
    }

    #[test]
    fn the_mask_marks_a_building_in_red_and_its_windows_in_green_over_its_whole_sprite() {
        // A 4×3 building whose window is its top-left texel, in an atlas 16 texels wide. The sprite's pixel (0, 0)
        // is its slot's base, one texel in from the atlas's border.
        let s = sprite(4, 3, |_, _| true);
        let (_, slots) = cut_pictures(&[&s], &[None], 16).expect("fits");
        let (bx, by) = (slots[0].base.x as u32, slots[0].base.y as u32);
        let mut rgba = vec![0u8; 16 * 16 * 4];
        write_mask(&mut rgba, 16, &slots[0], &[200, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let at = |x: u32, y: u32| {
            let t = (((by + y) * 16 + bx + x) * 4) as usize;
            (rgba[t], rgba[t + 1])
        };
        assert_eq!(at(0, 0), (255, 200), "the window: building and lit");
        assert_eq!(at(1, 0), (255, 0), "the stone: building, dark");
        assert_eq!(at(4, 0), (0, 0), "past the building's right edge: neither");
    }
}
