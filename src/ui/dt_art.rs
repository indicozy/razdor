//! Original Discord Times art, decoded at runtime from the player's install (`RAZDOR_DT_DIR`)
//! and turned into textures on first use. Nothing is written to disk.

// Part of the API is for the building screens of the next stages.
#![allow(dead_code)]

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

use macroquad::prelude::*;

use razdor::dt::gfx::{self, Image, ObjectSprite, ObjectSprites};
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

/// Every map object and building sprite in one texture, so the world map draws in few batches.
pub struct Atlas {
    pub texture: Texture2D,
    /// (section, category, index) → source rectangle in `texture`.
    rects: HashMap<(u32, u32, u32), Rect>,
}

impl Atlas {
    /// Sprite of a `.DTm` map object (class, sprite id).
    pub fn decoration(&self, class: u8, sprite: u8) -> Option<Rect> {
        self.rects.get(&(ObjectSprite::DECORATIONS, class.into(), sprite.into())).copied()
    }

    /// Sprite of a building by picture type and variant.
    pub fn building(&self, picture_type: u8, variant: u8) -> Option<Rect> {
        self.rects.get(&(ObjectSprite::BUILDINGS, picture_type.into(), variant.into())).copied()
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
    /// [`DtArt::unit_figure_center`] by `GlobalIndex`.
    figure_centers: RefCell<HashMap<u32, Option<f32>>>,
}

fn or_log<T: Default>(what: &str, r: Result<T, DtError>) -> T {
    r.unwrap_or_else(|e| {
        razdor::diag!("Discord Times art: {what}: {e}");
        T::default()
    })
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
            figure_centers: RefCell::new(HashMap::new()),
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
            let mut img = sheet.get(gfx::portrait_frame(unit_id)?)?.clone();
            img.rgba.chunks_exact_mut(4).for_each(|p| p[3] = 255);
            texture(&img)
        })
    }

    /// The bust of [`DtArt::unit_portrait`] as the promotion screen shows it when the unit
    /// cannot be promoted ([`super::chrome::lock_portrait`]).
    pub fn unit_portrait_locked(&self, unit_id: u32) -> Option<Texture2D> {
        self.cached(Key::LockedPortrait(unit_id), || {
            let sheet = self.portraits.get_or_init(|| or_log("unit portraits", self.install.unit_portraits()));
            let mut img = sheet.get(gfx::portrait_frame(unit_id)?)?.clone();
            img.rgba.chunks_exact_mut(4).for_each(|p| p[3] = 255);
            super::chrome::lock_portrait(&mut img);
            texture(&img)
        })
    }

    /// Full-body sepia portrait of a unit, by `GlobalIndex`.
    pub fn unit_figure(&self, unit_id: u32) -> Option<Texture2D> {
        self.cached(Key::Figure(unit_id), || {
            let sheet = self.figures.get_or_init(|| or_log("unit figures", self.install.unit_figures()));
            texture(sheet.get(gfx::portrait_frame(unit_id)?)?)
        })
    }

    /// Where the body of [`DtArt::unit_figure`] stands across its frame (0 left, 1 right):
    /// the frames are cropped to the picture, so a long weapon to one side (the cuirassier's
    /// sword) puts the body off the frame's middle.
    pub fn unit_figure_center(&self, unit_id: u32) -> Option<f32> {
        if let Some(c) = self.figure_centers.borrow().get(&unit_id) {
            return *c;
        }
        let sheet = self.figures.get_or_init(|| or_log("unit figures", self.install.unit_figures()));
        let c = gfx::portrait_frame(unit_id).and_then(|i| sheet.get(i)).and_then(gfx::opaque_center_x);
        self.figure_centers.borrow_mut().insert(unit_id, c);
        c
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
                let Some((image, pos)) = gfx::pack_atlas(&images, ATLAS_WIDTH) else {
                    razdor::diag!("Discord Times art: map objects do not fit an atlas");
                    return None;
                };
                let texture = texture(&image)?;
                let rects = sprites
                    .iter()
                    .zip(pos)
                    .map(|(s, (x, y))| ((s.section, s.cat, s.idx), Rect::new(x as f32, y as f32, s.image.width as f32, s.image.height as f32)))
                    .collect();
                Some(Atlas { texture, rects })
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
}
