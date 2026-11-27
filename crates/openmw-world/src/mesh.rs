//! Placeholder meshes (spec §3.6, §3.7): Collada 1.4.1 text, Z-up, in OpenMW units (about 70 to a
//! metre; the example Collada files in OpenMW's own data are in those units too), one flat colour
//! per part and no texture files. Every shape is built from integer coordinates and an integer
//! 8-gon table, so the output is the same on every platform. The only float operation is the
//! square root of the face normals, which is correctly rounded everywhere.
//!
//! Solids are convex or star-shaped around the point given as `inside`. Each triangle is turned
//! so that its normal points away from that point, so no face depends on the order it was built in.

use crate::heightmap::keyed;

/// A point in units, `[x, y, z]` (x east, y north, z up).
pub type P = [i64; 3];

/// Octagon directions times 1000 (cos, sin of k·45°), rounded: the only trigonometry here.
const C8: [i64; 8] = [1000, 707, 0, -707, -1000, -707, 0, 707];
const S8: [i64; 8] = [0, 707, 1000, 707, 0, -707, -1000, -707];

/// The colours of the placeholders. Index = the order of [`Paint::ALL`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Paint {
    Bark,
    DeadBark,
    Leaf,
    DarkLeaf,
    Stone,
    Rock,
    Plaster,
    Roof,
    Wood,
    Iron,
    Cloth,
    Dark,
}

impl Paint {
    pub const ALL: [Paint; 12] = [
        Paint::Bark,
        Paint::DeadBark,
        Paint::Leaf,
        Paint::DarkLeaf,
        Paint::Stone,
        Paint::Rock,
        Paint::Plaster,
        Paint::Roof,
        Paint::Wood,
        Paint::Iron,
        Paint::Cloth,
        Paint::Dark,
    ];

    /// The diffuse colour, 0..1 per channel.
    pub fn rgb(self) -> [f64; 3] {
        match self {
            Paint::Bark => [0.36, 0.25, 0.15],
            Paint::DeadBark => [0.42, 0.40, 0.36],
            Paint::Leaf => [0.23, 0.45, 0.18],
            Paint::DarkLeaf => [0.14, 0.28, 0.12],
            Paint::Stone => [0.58, 0.56, 0.52],
            Paint::Rock => [0.46, 0.45, 0.43],
            Paint::Plaster => [0.80, 0.74, 0.62],
            Paint::Roof => [0.45, 0.20, 0.14],
            Paint::Wood => [0.52, 0.38, 0.22],
            Paint::Iron => [0.30, 0.30, 0.32],
            Paint::Cloth => [0.66, 0.16, 0.14],
            Paint::Dark => [0.12, 0.11, 0.10],
        }
    }
}

/// One placeholder: its triangles (each with its colour) and the labels of the parts that made
/// it, in the order they were built (tests count them).
#[derive(Debug, Default)]
pub struct Builder {
    tris: Vec<(Paint, [P; 3])>,
    pub pieces: Vec<&'static str>,
}

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: P, b: P) -> P {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: P, b: P) -> i64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The octagon ring of an ellipse-like 8-gon centred on `(cx, cy)` at height `z`.
pub fn ring(cx: i64, cy: i64, z: i64, rx: i64, ry: i64) -> Vec<P> {
    (0..8).map(|k| [cx + rx * C8[k] / 1000, cy + ry * S8[k] / 1000, z]).collect()
}

/// A ring of the given corners (in order) at height `z`.
pub fn poly(points: &[(i64, i64)], z: i64) -> Vec<P> {
    points.iter().map(|&(x, y)| [x, y, z]).collect()
}

/// An axis-aligned rectangle's corners, counter-clockwise, at height `z`.
pub fn rect(x0: i64, x1: i64, y0: i64, y1: i64, z: i64) -> Vec<P> {
    poly(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)], z)
}

fn apex(x: i64, y: i64, z: i64, n: usize) -> Vec<P> {
    vec![[x, y, z]; n]
}

impl Builder {
    pub fn new() -> Builder {
        Builder::default()
    }

    /// Adds one triangle, turned so its normal points away from `inside`. Degenerate ones are dropped.
    fn push(&mut self, paint: Paint, a: P, b: P, c: P, inside: P) {
        let n = cross(sub(b, a), sub(c, a));
        if n == [0, 0, 0] {
            return;
        }
        // Sign of n . (centroid - inside), times 3: the centroid's sum is a+b+c.
        let away = dot(n, sub([a[0] + b[0] + c[0], a[1] + b[1] + c[1], a[2] + b[2] + c[2]], [3 * inside[0], 3 * inside[1], 3 * inside[2]]));
        if away < 0 {
            self.tris.push((paint, [a, c, b]));
        } else {
            self.tris.push((paint, [a, b, c]));
        }
    }

    fn quad(&mut self, paint: Paint, a: P, b: P, c: P, d: P, inside: P) {
        self.push(paint, a, b, c, inside);
        self.push(paint, a, c, d, inside);
    }

    pub fn piece(&mut self, label: &'static str) {
        self.pieces.push(label);
    }

    /// A box from `lo` to `hi` (both corners included).
    pub fn boxed(&mut self, paint: Paint, lo: P, hi: P) {
        let [x0, y0, z0] = lo;
        let [x1, y1, z1] = hi;
        let inside = [(x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2];
        let p000 = [x0, y0, z0];
        let p100 = [x1, y0, z0];
        let p110 = [x1, y1, z0];
        let p010 = [x0, y1, z0];
        let p001 = [x0, y0, z1];
        let p101 = [x1, y0, z1];
        let p111 = [x1, y1, z1];
        let p011 = [x0, y1, z1];
        self.quad(paint, p001, p101, p111, p011, inside);
        self.quad(paint, p000, p010, p110, p100, inside);
        self.quad(paint, p000, p100, p101, p001, inside);
        self.quad(paint, p010, p011, p111, p110, inside);
        self.quad(paint, p000, p001, p011, p010, inside);
        self.quad(paint, p100, p110, p111, p101, inside);
    }

    /// A solid between two rings of the same length (the sides and both caps).
    pub fn frustum(&mut self, paint: Paint, base: &[P], top: &[P], inside: P) {
        let n = base.len();
        assert_eq!(n, top.len(), "frustum rings differ in length");
        for i in 0..n {
            let j = (i + 1) % n;
            self.quad(paint, base[i], base[j], top[j], top[i], inside);
        }
        for k in 1..n - 1 {
            self.push(paint, base[0], base[k], base[k + 1], inside);
            self.push(paint, top[0], top[k], top[k + 1], inside);
        }
    }

    /// A pitched roof with its ridge along Y: the two slopes in `roof`, the two gable ends in `gable`.
    /// `x`, `y` and `z` are the (low, high) pairs of the footprint and of the eaves and ridge heights.
    pub fn gable(&mut self, roof: Paint, gable: Paint, x: (i64, i64), y: (i64, i64), z: (i64, i64)) {
        let ((x0, x1), (y0, y1), (z0, zr)) = (x, y, z);
        let xm = (x0 + x1) / 2;
        let inside = [xm, (y0 + y1) / 2, z0];
        let a = [x0, y0, z0];
        let b = [x0, y1, z0];
        let c = [xm, y0, zr];
        let d = [xm, y1, zr];
        let e = [x1, y0, z0];
        let f = [x1, y1, z0];
        self.quad(roof, a, b, d, c, inside);
        self.quad(roof, c, d, f, e, inside);
        self.push(gable, a, e, c, inside);
        self.push(gable, b, d, f, inside);
    }

    /// A house of `w` by `d` (ridge along Y, so `d >= w`), walls `wall` high, roof `roof` high.
    fn house(&mut self, cx: i64, cy: i64, w: i64, d: i64, wall: i64, roof: i64) {
        let (x0, x1, y0, y1) = (cx - w / 2, cx + w / 2, cy - d / 2, cy + d / 2);
        self.boxed(Paint::Plaster, [x0, y0, 0], [x1, y1, wall]);
        self.piece("walls");
        self.gable(Paint::Roof, Paint::Plaster, (x0, x1), (y0, y1), (wall, wall + roof));
        self.piece("roof");
    }

    pub fn triangles(&self) -> usize {
        self.tris.len()
    }

    /// The highest point of the mesh, in units (0 when empty).
    pub fn top(&self) -> i64 {
        self.tris.iter().flat_map(|(_, t)| t.iter()).map(|p| p[2]).max().unwrap_or(0)
    }

    /// The mesh as Collada 1.4.1 text. `name` is the geometry's name only.
    pub fn dae(&self, name: &str) -> String {
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
        out.push_str("<COLLADA xmlns=\"http://www.collada.org/2005/11/COLLADASchema\" version=\"1.4.1\">\n");
        out.push_str("<asset><contributor><author>Razdor</author><authoring_tool>razdor-openmw</authoring_tool></contributor>");
        out.push_str("<created>2026-11-27T00:00:00Z</created><modified>2026-11-27T00:00:00Z</modified>");
        out.push_str("<unit meter=\"1.0\" name=\"meter\"/><up_axis>Z_UP</up_axis></asset>\n");

        let used: Vec<Paint> = Paint::ALL.iter().copied().filter(|p| self.tris.iter().any(|(q, _)| q == p)).collect();
        out.push_str("<library_effects>\n");
        for p in &used {
            let [r, g, b] = p.rgb();
            let [ar, ag, ab] = [r * 0.6, g * 0.6, b * 0.6];
            out.push_str(&format!(
                "<effect id=\"fx_{i}\"><profile_COMMON><technique sid=\"common\"><lambert><ambient><color>{ar:.4} {ag:.4} {ab:.4} 1</color></ambient><diffuse><color>{r:.4} {g:.4} {b:.4} 1</color></diffuse></lambert></technique></profile_COMMON></effect>\n",
                i = *p as usize
            ));
        }
        out.push_str("</library_effects>\n<library_materials>\n");
        for p in &used {
            out.push_str(&format!("<material id=\"mat_{i}\" name=\"mat_{i}\"><instance_effect url=\"#fx_{i}\"/></material>\n", i = *p as usize));
        }
        out.push_str("</library_materials>\n<library_geometries>\n");

        // One vertex per triangle corner (flat shading), so the index list is 0,0 1,1 2,2 ...
        let mut positions = String::new();
        let mut normals = String::new();
        let mut groups = String::new();
        let mut corner = 0usize;
        for p in &used {
            let mut count = 0usize;
            let mut index = String::new();
            for (_, t) in self.tris.iter().filter(|(q, _)| q == p) {
                let n = normal(t);
                for v in t {
                    positions.push_str(&format!("{} {} {} ", v[0], v[1], v[2]));
                    normals.push_str(&format!("{:.6} {:.6} {:.6} ", n[0], n[1], n[2]));
                    index.push_str(&format!("{c} {c} ", c = corner));
                    corner += 1;
                }
                count += 1;
            }
            groups.push_str(&format!(
                "<triangles material=\"sym_{i}\" count=\"{count}\"><input semantic=\"VERTEX\" source=\"#geom-verts\" offset=\"0\"/><input semantic=\"NORMAL\" source=\"#geom-nrm\" offset=\"1\"/><p>{index}</p></triangles>\n",
                i = *p as usize,
                index = index.trim_end()
            ));
        }
        let vcount = corner;
        out.push_str(&format!("<geometry id=\"geom\" name=\"{name}\"><mesh>\n"));
        for (id, data) in [("geom-pos", &positions), ("geom-nrm", &normals)] {
            out.push_str(&format!(
                "<source id=\"{id}\"><float_array id=\"{id}-array\" count=\"{n}\">{data}</float_array><technique_common><accessor source=\"#{id}-array\" count=\"{vcount}\" stride=\"3\"><param name=\"X\" type=\"float\"/><param name=\"Y\" type=\"float\"/><param name=\"Z\" type=\"float\"/></accessor></technique_common></source>\n",
                data = data.trim_end(),
                n = vcount * 3
            ));
        }
        out.push_str("<vertices id=\"geom-verts\"><input semantic=\"POSITION\" source=\"#geom-pos\"/></vertices>\n");
        out.push_str(&groups);
        out.push_str("</mesh></geometry>\n</library_geometries>\n<library_visual_scenes><visual_scene id=\"scene\" name=\"scene\"><node id=\"root\" name=\"root\">\n");
        out.push_str("<instance_geometry url=\"#geom\"><bind_material><technique_common>\n");
        for p in &used {
            out.push_str(&format!("<instance_material symbol=\"sym_{i}\" target=\"#mat_{i}\"/>\n", i = *p as usize));
        }
        out.push_str("</technique_common></bind_material></instance_geometry>\n");
        out.push_str("</node></visual_scene></library_visual_scenes>\n<scene><instance_visual_scene url=\"#scene\"/></scene>\n</COLLADA>\n");
        out
    }
}

/// The unit normal of a triangle (f64 square root, correctly rounded on every platform).
fn normal(t: &[P; 3]) -> [f64; 3] {
    let c = cross(sub(t[1], t[0]), sub(t[2], t[0]));
    let v = [c[0] as f64, c[1] as f64, c[2] as f64];
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len == 0.0 {
        return [0.0, 0.0, 1.0];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

// ---- scenery ----------------------------------------------------------------------------------

fn trunk(b: &mut Builder, paint: Paint, height: i64, r0: i64, r1: i64) {
    let base = ring(0, 0, 0, r0, r0);
    let top = ring(0, 0, height, r1, r1);
    b.frustum(paint, &base, &top, [0, 0, height / 2]);
    b.piece("trunk");
}

/// A tree with a tall cone crown (`variant` 0) or a round, two-tier crown (`variant` 1).
/// About 800 units tall; the crown starts at 300, above a standing actor (spec §5 of the design).
pub fn tree(variant: u8) -> Builder {
    let mut b = Builder::new();
    trunk(&mut b, Paint::Bark, 420, 45, 35);
    if variant == 0 {
        let base = ring(0, 0, 300, 260, 260);
        b.frustum(Paint::Leaf, &base, &apex(0, 0, 800, 8), [0, 0, 500]);
        b.piece("crown");
    } else {
        b.frustum(Paint::Leaf, &ring(0, 0, 300, 200, 200), &ring(0, 0, 520, 320, 320), [0, 0, 420]);
        b.frustum(Paint::Leaf, &ring(0, 0, 520, 320, 320), &ring(0, 0, 680, 240, 240), [0, 0, 600]);
        b.frustum(Paint::Leaf, &ring(0, 0, 680, 240, 240), &apex(0, 0, 780, 8), [0, 0, 700]);
        b.piece("crown");
    }
    b
}

/// A bare trunk with three branches, about 700 units tall.
pub fn dead_tree() -> Builder {
    let mut b = Builder::new();
    let top = ring(0, 0, 700, 20, 20);
    b.frustum(Paint::DeadBark, &ring(0, 0, 0, 35, 35), &top, [0, 0, 350]);
    b.piece("trunk");
    b.boxed(Paint::DeadBark, [-10, -20, 450], [250, 20, 490]);
    b.piece("branch");
    b.boxed(Paint::DeadBark, [-250, -20, 580], [10, 20, 620]);
    b.piece("branch");
    b.boxed(Paint::DeadBark, [-20, -200, 520], [20, 10, 560]);
    b.piece("branch");
    b
}

/// A low, dense thicket: a squat blob about 470 units tall, closed on foot (its visible mesh is
/// its collision, spec §2.4).
pub fn thicket() -> Builder {
    let mut b = Builder::new();
    b.frustum(Paint::DarkLeaf, &ring(0, 0, 0, 380, 380), &ring(0, 0, 260, 380, 380), [0, 0, 130]);
    b.piece("blob");
    b.frustum(Paint::DarkLeaf, &ring(0, 0, 260, 380, 380), &ring(0, 0, 460, 220, 220), [0, 0, 300]);
    b.piece("blob");
    b.frustum(Paint::DarkLeaf, &ring(0, 0, 460, 220, 220), &apex(0, 0, 470, 8), [0, 0, 400]);
    b.piece("blob");
    b
}

/// An irregular boulder of diameter `diameter` (its base ring has per-corner jitter from a fixed
/// key, so the shape is the same on every run).
fn boulder(paint: Paint, diameter: i64, key: u64) -> Builder {
    let mut b = Builder::new();
    let r = diameter / 2;
    let jitter = |k: usize, salt: u64| -> i64 { (keyed(&[key, salt, k as u64]) % 301) as i64 - 150 };
    let base_r: Vec<i64> = (0..8).map(|k| r * (1000 + jitter(k, 1)) / 1000).collect();
    let top_r: Vec<i64> = (0..8).map(|k| r * (500 + jitter(k, 2) / 3) / 1000).collect();
    let h = diameter * 55 / 100;
    let ring_at = |z: i64, radii: &[i64]| -> Vec<P> { (0..8).map(|k| [radii[k] * C8[k] / 1000, radii[k] * S8[k] / 1000, z]).collect() };
    let skirt_base = ring_at(-30, &base_r);
    let base = ring_at(0, &base_r);
    b.frustum(paint, &skirt_base, &base, [0, 0, h / 3]);
    b.piece("skirt");
    let top = ring_at(h, &top_r);
    b.frustum(paint, &base, &top, [0, 0, h / 2]);
    b.piece("body");
    b.frustum(paint, &top, &apex(0, 0, h + diameter / 10, 8), [0, 0, h]);
    b.piece("cap");
    b
}

/// A small stone of the massif size `f` (1..=6): diameter 150 to 275.
pub fn stone(f: i64) -> Builder {
    boulder(Paint::Stone, 150 + 25 * (f.clamp(1, 6) - 1), 0x5713_0000 + f as u64)
}

/// A mountain's foot rock of the massif size `f` (1..=6): diameter 400 to 900.
pub fn rock(f: i64) -> Builder {
    boulder(Paint::Rock, 400 + 100 * (f.clamp(1, 6) - 1), 0x20C4_0000 + f as u64)
}

/// The rock's diameter, as [`rock`] builds it (the placement keeps it inside the massif's square).
pub fn rock_diameter(f: i64) -> i64 {
    400 + 100 * (f.clamp(1, 6) - 1)
}

/// A bridge deck: a slab `1280` square and 32 thick, its top at 0 (the reference puts it at the
/// deck height, spec §3.7). `wood` only changes the colour.
pub fn deck(wood: bool) -> Builder {
    let mut b = Builder::new();
    b.boxed(if wood { Paint::Wood } else { Paint::Stone }, [-640, -640, -32], [640, 640, 0]);
    b.piece("deck");
    b
}

// ---- buildings (Morrowind-scale, spec §3.7) ---------------------------------------------------

/// The curtain wall of a walled place of `w` by `d` units, with a gate on the south side and the
/// corner towers, the keep and the gate towers the caller asks for.
fn walled(b: &mut Builder, w: i64, d: i64, corners: &[(i64, i64)], keep: Option<(i64, i64)>) {
    const T: i64 = 128;
    const H: i64 = 420;
    let (w2, d2) = (w / 2, d / 2);
    b.boxed(Paint::Stone, [-w2, d2 - T, 0], [w2, d2, H]);
    b.piece("wall");
    b.boxed(Paint::Stone, [-w2, -d2, 0], [-w2 + T, d2, H]);
    b.piece("wall");
    b.boxed(Paint::Stone, [w2 - T, -d2, 0], [w2, d2, H]);
    b.piece("wall");
    // The south wall keeps a gate 320 wide in the middle (spec §3.7: a real opening).
    b.boxed(Paint::Stone, [-w2, -d2, 0], [-160, -d2 + T, H]);
    b.piece("wall");
    b.boxed(Paint::Stone, [160, -d2, 0], [w2, -d2 + T, H]);
    b.piece("wall");
    b.boxed(Paint::Stone, [-400, -d2, 0], [-160, -d2 + 240, 700]);
    b.piece("gate");
    b.boxed(Paint::Stone, [160, -d2, 0], [400, -d2 + 240, 700]);
    b.piece("gate");
    for &(cx, cy) in corners {
        b.boxed(Paint::Stone, [cx - 200, cy - 200, 0], [cx + 200, cy + 200, 1100]);
        b.boxed(Paint::Stone, [cx - 240, cy - 240, 1100], [cx + 240, cy + 240, 1200]);
        b.piece("tower");
    }
    if let Some((h, top)) = keep {
        b.boxed(Paint::Stone, [-h, -h, 0], [h, h, top - 120]);
        b.boxed(Paint::Stone, [-h - 40, -h - 40, top - 120], [h + 40, h + 40, top]);
        b.piece("keep");
    }
}

/// The castle: a curtain wall the size of its footprint, four corner towers, a gate, a keep.
pub fn castle(w: i64, d: i64) -> Builder {
    let mut b = Builder::new();
    let (cx, cy) = (w / 2 - 200, d / 2 - 200);
    walled(&mut b, w, d, &[(cx, cy), (-cx, cy), (cx, -cy), (-cx, -cy)], Some((400, 900)));
    b
}

/// The palace: the castle's plan with a taller keep.
pub fn palace(w: i64, d: i64) -> Builder {
    let mut b = Builder::new();
    let (cx, cy) = (w / 2 - 200, d / 2 - 200);
    walled(&mut b, w, d, &[(cx, cy), (-cx, cy), (cx, -cy), (-cx, -cy)], Some((450, 900)));
    b
}

/// The fort: a smaller curtain wall, a gate and one tower at the north-east corner.
pub fn fort(w: i64, d: i64) -> Builder {
    let mut b = Builder::new();
    walled(&mut b, w, d, &[(w / 2 - 200, d / 2 - 200)], None);
    b
}

/// A village or a town: one house at each of the given slot centres (relative to the middle).
pub fn houses(slots: &[(i64, i64)]) -> Builder {
    let mut b = Builder::new();
    for &(x, y) in slots {
        b.house(x, y, 512, 640, 300, 160);
    }
    b
}

pub fn tavern(w: i64, d: i64) -> Builder {
    let _ = (w, d);
    let mut b = Builder::new();
    b.house(0, 0, 768, 896, 300, 120);
    b
}

/// The church: a nave, and a bell tower with a spire at its north end (800 units).
pub fn church(w: i64, d: i64) -> Builder {
    let _ = (w, d);
    let mut b = Builder::new();
    b.house(0, 0, 640, 896, 400, 120);
    b.boxed(Paint::Stone, [-160, 140, 0], [160, 460, 700]);
    b.piece("belfry");
    b.frustum(Paint::Roof, &rect(-160, 160, 140, 460, 700), &apex(0, 300, 800, 4), [0, 300, 400]);
    b.piece("spire");
    b
}

/// The smithy: a house with a chimney (420 units).
pub fn smithy(w: i64, d: i64) -> Builder {
    let _ = (w, d);
    let mut b = Builder::new();
    b.house(0, 0, 512, 640, 300, 140);
    b.boxed(Paint::Stone, [88, 120, 0], [168, 200, 420]);
    b.piece("chimney");
    b
}

/// The market: four posts, a cloth awning on top and two counters (400 units).
pub fn market(w: i64, d: i64) -> Builder {
    let _ = (w, d);
    let mut b = Builder::new();
    for (x, y) in [(-480, -320), (480, -320), (-480, 320), (480, 320)] {
        b.boxed(Paint::Wood, [x - 30, y - 30, 0], [x + 30, y + 30, 360]);
        b.piece("post");
    }
    b.boxed(Paint::Cloth, [-512, -384, 360], [512, 384, 400]);
    b.piece("awning");
    b.boxed(Paint::Wood, [-400, -250, 0], [400, -90, 120]);
    b.piece("counter");
    b.boxed(Paint::Wood, [-400, 90, 0], [400, 250, 120]);
    b.piece("counter");
    b
}

/// The shipyard: a long shed with a gable roof (420 units).
pub fn shipyard(w: i64, d: i64) -> Builder {
    let _ = (w, d);
    let mut b = Builder::new();
    b.house(0, 0, 768, 1024, 340, 80);
    b
}

/// The altar: a stone block and a smaller block on it (300 units).
pub fn altar(w: i64, d: i64) -> Builder {
    let _ = (w, d);
    let mut b = Builder::new();
    b.boxed(Paint::Stone, [-128, -128, 0], [128, 128, 120]);
    b.piece("base");
    b.boxed(Paint::Stone, [-80, -80, 120], [80, 80, 300]);
    b.piece("top");
    b
}

/// The obelisk: a four-sided shaft tapering to 40 units (300 units).
pub fn obelisk(w: i64, d: i64) -> Builder {
    let _ = (w, d);
    let mut b = Builder::new();
    b.frustum(Paint::Stone, &rect(-80, 80, -80, 80, 0), &rect(-20, 20, -20, 20, 300), [0, 0, 150]);
    b.piece("shaft");
    b
}

/// The dungeon entrance: a low mound (260 units).
pub fn dungeon(w: i64, d: i64) -> Builder {
    let _ = (w, d);
    let mut b = Builder::new();
    b.frustum(Paint::Rock, &ring(0, 0, 0, 400, 400), &ring(0, 0, 260, 180, 180), [0, 0, 120]);
    b.piece("mound");
    b
}

/// Ruins: three broken wall pieces (200 units at most).
pub fn ruins(w: i64, d: i64) -> Builder {
    let _ = (w, d);
    let mut b = Builder::new();
    b.boxed(Paint::Stone, [-400, -300, 0], [100, -210, 200]);
    b.piece("wall");
    b.boxed(Paint::Stone, [100, 0, 0], [300, 90, 120]);
    b.piece("wall");
    b.boxed(Paint::Stone, [-200, 150, 0], [-110, 240, 160]);
    b.piece("wall");
    b
}

/// A hall-sized wooden shed used when a kind has no shape of its own.
pub fn shed(w: i64, d: i64) -> Builder {
    let _ = (w, d);
    let mut b = Builder::new();
    b.house(0, 0, 512, 640, 300, 160);
    b
}

// ---- tests ------------------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod check {
    /// A small well-formedness check for the Collada text: every start tag is closed by the end
    /// tag of the same name (or is self-closing), the declaration and comments are skipped, and
    /// the document has one root. Enough for the placeholders we write; not a general parser.
    pub fn well_formed(text: &str) -> Result<usize, String> {
        let mut stack: Vec<String> = Vec::new();
        let mut elements = 0usize;
        let mut rest = text;
        while let Some(start) = rest.find('<') {
            rest = &rest[start..];
            let end = rest.find('>').ok_or("unterminated tag")?;
            let tag = &rest[1..end];
            rest = &rest[end + 1..];
            if tag.starts_with('?') || tag.starts_with('!') {
                continue;
            }
            if let Some(name) = tag.strip_prefix('/') {
                let open = stack.pop().ok_or_else(|| format!("end tag </{name}> with nothing open"))?;
                if open != name.trim() {
                    return Err(format!("</{}> closes <{}>", name.trim(), open));
                }
            } else if let Some(body) = tag.strip_suffix('/') {
                let _ = body;
                elements += 1;
            } else {
                let name = tag.split_whitespace().next().unwrap_or("").to_string();
                if name.is_empty() {
                    return Err("empty tag name".into());
                }
                elements += 1;
                stack.push(name);
            }
        }
        if !stack.is_empty() {
            return Err(format!("unclosed {:?}", stack));
        }
        Ok(elements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_shapes() -> Vec<(&'static str, Builder)> {
        vec![
            ("tree0", tree(0)),
            ("tree1", tree(1)),
            ("dead", dead_tree()),
            ("thicket", thicket()),
            ("stone", stone(3)),
            ("rock", rock(2)),
            ("deck", deck(false)),
            ("castle", castle(4096, 4096)),
            ("palace", palace(4096, 4096)),
            ("fort", fort(2048, 2048)),
            ("houses", houses(&[(-512, 0), (512, 0)])),
            ("tavern", tavern(1024, 1024)),
            ("church", church(1024, 1024)),
            ("smithy", smithy(1024, 1024)),
            ("market", market(1024, 1024)),
            ("shipyard", shipyard(1024, 1024)),
            ("altar", altar(1024, 1024)),
            ("obelisk", obelisk(1024, 1024)),
            ("dungeon", dungeon(1024, 1024)),
            ("ruins", ruins(1024, 1024)),
        ]
    }

    #[test]
    fn every_dae_is_well_formed_xml() {
        for (name, b) in all_shapes() {
            let text = b.dae(name);
            let elements = check::well_formed(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(elements > 10, "{name}");
            assert!(b.triangles() > 0, "{name}");
        }
    }

    #[test]
    fn the_well_formedness_check_rejects_broken_text() {
        assert!(check::well_formed("<a><b></a></b>").is_err());
        assert!(check::well_formed("<a><b>").is_err());
        assert!(check::well_formed("<a/></a>").is_err());
        assert!(check::well_formed("<?xml version=\"1.0\"?><a><b x=\"1\"/></a>").is_ok());
    }

    #[test]
    fn the_shapes_have_the_planned_heights() {
        assert!((700..=900).contains(&tree(0).top()), "tree0 {}", tree(0).top());
        assert!((700..=900).contains(&tree(1).top()), "tree1 {}", tree(1).top());
        assert!((600..=800).contains(&dead_tree().top()), "dead {}", dead_tree().top());
        assert!(thicket().top() <= 600);
        let extent = |b: &Builder| {
            let xs: Vec<i64> = b.tris.iter().flat_map(|(_, t)| t.iter()).map(|p| p[0]).collect();
            xs.iter().max().unwrap() - xs.iter().min().unwrap()
        };
        // a stone's width is its diameter (150 to 275) with up to 15% jitter on each corner
        assert!((120..=300).contains(&extent(&stone(1))), "{}", extent(&stone(1)));
        assert!((120..=300).contains(&extent(&stone(6))), "{}", extent(&stone(6)));
        assert_eq!(castle(4096, 4096).top(), 1200);
        assert_eq!(church(1024, 1024).top(), 800);
        assert_eq!(market(1024, 1024).top(), 400);
        assert_eq!(obelisk(1024, 1024).top(), 300);
        assert_eq!(altar(1024, 1024).top(), 300);
        // a tree's crown starts above a standing actor (about 128 units tall)
        let crown_low = tree(0).tris.iter().filter(|(p, _)| *p == Paint::Leaf).flat_map(|(_, t)| t.iter()).map(|p| p[2]).min().unwrap();
        assert!(crown_low >= 200, "{crown_low}");
    }

    #[test]
    fn castles_have_four_towers_a_keep_and_a_gate_and_forts_one_tower() {
        let count = |b: &Builder, label: &str| b.pieces.iter().filter(|p| **p == label).count();
        let c = castle(4096, 4096);
        assert_eq!((count(&c, "tower"), count(&c, "keep"), count(&c, "gate")), (4, 1, 2));
        let f = fort(2048, 2048);
        assert_eq!((count(&f, "tower"), count(&f, "keep")), (1, 0));
        assert_eq!(count(&houses(&[(0, 0), (512, 0), (0, 896)]), "walls"), 3);
    }

    #[test]
    fn the_castle_gate_is_an_opening_of_at_least_256_units() {
        // No wall triangle may sit in the south wall's band between the two gate posts (x in -160..160).
        let (w, d) = (4096i64, 4096i64);
        let c = castle(w, d);
        let band = |p: &P| p[1] >= -d / 2 && p[1] <= -d / 2 + 128 && p[2] <= 420;
        for (paint, t) in &c.tris {
            if *paint != Paint::Stone {
                continue;
            }
            let cx = (t[0][0] + t[1][0] + t[2][0]) / 3;
            let cy = (t[0][1] + t[1][1] + t[2][1]) / 3;
            let cz = (t[0][2] + t[1][2] + t[2][2]) / 3;
            let inside_band = t.iter().all(band);
            let _ = cy;
            if inside_band && cz <= 420 {
                assert!(!(cx > -160 && cx < 160), "wall triangle in the gate at {t:?}");
            }
        }
    }

    #[test]
    fn each_kind_has_its_parts() {
        let count = |b: &Builder, label: &str| b.pieces.iter().filter(|p| **p == label).count();
        // (what it is, the builder, the labels and their counts it must have)
        type Case = (&'static str, Builder, Vec<(&'static str, usize)>);
        let cases: Vec<Case> = vec![
            ("palace", palace(4096, 4096), vec![("tower", 4), ("keep", 1), ("gate", 2), ("wall", 5)]),
            ("castle", castle(4096, 4096), vec![("tower", 4), ("keep", 1), ("gate", 2), ("wall", 5)]),
            ("fort", fort(2048, 2048), vec![("tower", 1), ("keep", 0), ("gate", 2), ("wall", 5)]),
            ("village of 3", houses(&[(-512, 0), (512, 0), (0, 896)]), vec![("walls", 3), ("roof", 3)]),
            ("tavern", tavern(1024, 1024), vec![("walls", 1), ("roof", 1)]),
            ("church", church(1024, 1024), vec![("walls", 1), ("roof", 1), ("belfry", 1), ("spire", 1)]),
            ("smithy", smithy(1024, 1024), vec![("walls", 1), ("roof", 1), ("chimney", 1)]),
            ("market", market(1024, 1024), vec![("post", 4), ("awning", 1), ("counter", 2)]),
            ("shipyard", shipyard(1024, 1024), vec![("walls", 1), ("roof", 1)]),
            ("altar", altar(1024, 1024), vec![("base", 1), ("top", 1)]),
            ("obelisk", obelisk(1024, 1024), vec![("shaft", 1)]),
            ("dungeon", dungeon(1024, 1024), vec![("mound", 1)]),
            ("ruins", ruins(1024, 1024), vec![("wall", 3)]),
            ("tree", tree(0), vec![("trunk", 1), ("crown", 1)]),
            ("dead tree", dead_tree(), vec![("trunk", 1), ("branch", 3)]),
            ("thicket", thicket(), vec![("blob", 3)]),
            ("stone", stone(2), vec![("skirt", 1), ("body", 1), ("cap", 1)]),
            ("deck", deck(true), vec![("deck", 1)]),
        ];
        for (what, b, parts) in cases {
            for (label, n) in parts {
                assert_eq!(count(&b, label), n, "{what}: {label}");
            }
        }
    }

    #[test]
    fn the_shapes_are_the_same_on_every_run() {
        for (name, b) in all_shapes() {
            let again = match name {
                "tree0" => tree(0),
                "tree1" => tree(1),
                "dead" => dead_tree(),
                "thicket" => thicket(),
                "stone" => stone(3),
                "rock" => rock(2),
                "deck" => deck(false),
                "castle" => castle(4096, 4096),
                "palace" => palace(4096, 4096),
                "fort" => fort(2048, 2048),
                "houses" => houses(&[(-512, 0), (512, 0)]),
                "tavern" => tavern(1024, 1024),
                "church" => church(1024, 1024),
                "smithy" => smithy(1024, 1024),
                "market" => market(1024, 1024),
                "shipyard" => shipyard(1024, 1024),
                "altar" => altar(1024, 1024),
                "obelisk" => obelisk(1024, 1024),
                "dungeon" => dungeon(1024, 1024),
                _ => ruins(1024, 1024),
            };
            assert_eq!(b.dae(name), again.dae(name), "{name}");
        }
    }
}
