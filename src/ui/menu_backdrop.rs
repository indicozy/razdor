//! The main menu's background, `Windows/Castle.lit` (512 × 512), enlarged to the screen as the owner
//! chose (2026-11-22): what ImageMagick 7 (Q16) does with `-filter Lanczos -resize WxH! -unsharp
//! 0x1.4+0.7+0.02`, Razdor-Art's «Lanczos + sharpness». A Lanczos-3 enlargement one axis at a time,
//! then an unsharp mask. Worked out on the CPU once per screen size and frame (a worker thread; the
//! snapshots wait for it) and kept as a texture in memory: nothing is written to disk. Until it is
//! ready the menu draws the picture as before.
//!
//! The numbers follow ImageMagick's code so the frames match its output: 16-bit values rounded and
//! clamped after every pass, the axis with the larger factor first (`ResizeImage`), the contributions
//! of `HorizontalFilter`/`VerticalFilter`, the Gaussian of the `blur` kernel (`KernelRank` 3,
//! the width of `GetOptimalKernelWidth1D`) with the edge pixels repeated, and `UnsharpMaskImage`'s
//! threshold. The tests compare with frames ImageMagick made.

use std::sync::mpsc::{self, Receiver};

use macroquad::prelude::*;

use super::chrome;

/// Lanczos' lobes (ImageMagick's `Lanczos`: a sinc windowed by a sinc, support 3).
const LOBES: f64 = 3.0;
/// The unsharp mask: `-unsharp 0x1.4+0.7+0.02` (radius 0: from sigma; sigma in screen pixels; the gain;
/// the threshold, a fraction of the full range).
const UNSHARP_SIGMA: f64 = 1.4;
const UNSHARP_GAIN: f64 = 0.7;
const UNSHARP_THRESHOLD: f64 = 0.02;
/// ImageMagick's `MagickEpsilon`.
const EPSILON: f64 = 1.0e-12;
/// ImageMagick's Q16 range.
const Q: f64 = 65535.0;
/// The largest side made; a larger screen keeps the plain drawing.
const MAX_SIDE: usize = 8192;

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        let p = std::f64::consts::PI * x;
        p.sin() / p
    }
}

/// The Lanczos-3 weight at distance `x` (in source pixels at enlargement).
pub fn lanczos3(x: f64) -> f64 {
    let x = x.abs();
    if x < LOBES {
        sinc(x) * sinc(x / LOBES)
    } else {
        0.0
    }
}

/// For each of `n_out` pixels made from `n_in`: the first source pixel and the weights of it and the
/// ones after it, summing to 1 (ImageMagick's `HorizontalFilter`: the centre `(x + 0.5) / factor`,
/// the support widened by the reduction when shrinking, the pixels past the edge left out).
pub fn contributions(n_in: usize, n_out: usize) -> Vec<(usize, Vec<f64>)> {
    let factor = n_out as f64 / n_in as f64;
    let widen = (1.0 / factor + EPSILON).max(1.0);
    let support = widen * LOBES;
    let scale = 1.0 / widen;
    (0..n_out)
        .map(|x| {
            let bisect = (x as f64 + 0.5) / factor + EPSILON;
            let start = (bisect - support + 0.5).max(0.0) as usize;
            let stop = ((bisect + support + 0.5).min(n_in as f64) as usize).max(start + 1).min(n_in);
            let mut w: Vec<f64> = (start..stop).map(|j| lanczos3(scale * (j as f64 - bisect + 0.5))).collect();
            let density: f64 = w.iter().sum();
            if density != 0.0 && density != 1.0 {
                w.iter_mut().for_each(|v| *v /= density);
            }
            (start, w)
        })
        .collect()
}

/// The blur kernel's width for `sigma` (ImageMagick's `GetOptimalKernelWidth1D` with radius 0, Q16):
/// 13 for 1.4.
pub fn kernel_width(sigma: f64) -> usize {
    let alpha = 1.0 / (2.0 * sigma * sigma);
    let beta = 1.0 / ((2.0 * std::f64::consts::PI).sqrt() * sigma);
    let mut width = 5usize;
    loop {
        let j = (width as i64 - 1) / 2;
        let normalize: f64 = (-j..=j).map(|i| (-((i * i) as f64) * alpha).exp() * beta).sum();
        let value = (-((j * j) as f64) * alpha).exp() * beta / normalize;
        if value < 1.0 / Q || value < EPSILON {
            return width - 2;
        }
        width += 2;
    }
}

/// The Gaussian blur kernel for `sigma` (ImageMagick's `blur` kernel: each tap the sum of three
/// sub-samples, `KernelRank` 3), normalised to 1.
pub fn blur_kernel(sigma: f64) -> Vec<f64> {
    const RANK: i64 = 3;
    let width = kernel_width(sigma);
    let v = (width as i64 * RANK - 1) / 2;
    let s = sigma * RANK as f64;
    let alpha = 1.0 / (2.0 * s * s);
    let beta = 1.0 / ((2.0 * std::f64::consts::PI).sqrt() * s);
    let mut k = vec![0.0; width];
    for u in -v..=v {
        k[((u + v) / RANK) as usize] += (-((u * u) as f64) * alpha).exp() * beta;
    }
    let sum: f64 = k.iter().sum();
    k.iter_mut().for_each(|x| *x /= sum);
    k
}

/// A 16-bit RGB picture, `w` × `h`, three values a pixel.
struct Plane {
    w: usize,
    h: usize,
    px: Vec<u16>,
}

/// A value rounded and clamped to 16 bits (ImageMagick's `ClampToQuantum`, Q16).
fn q16(v: f64) -> u16 {
    if v <= 0.0 {
        0
    } else if v >= Q {
        u16::MAX
    } else {
        (v + 0.5) as u16
    }
}

/// `img` resampled along x (`along_x`) or y to `n` pixels with `weights` ([`contributions`]).
fn resample(img: &Plane, n: usize, along_x: bool) -> Plane {
    let weights = contributions(if along_x { img.w } else { img.h }, n);
    let (w, h) = if along_x { (n, img.h) } else { (img.w, n) };
    let mut px = vec![0u16; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let (start, ws) = &weights[if along_x { x } else { y }];
            let mut sum = [0.0f64; 3];
            for (i, wt) in ws.iter().enumerate() {
                let (sx, sy) = if along_x { (start + i, y) } else { (x, start + i) };
                let p = (sy * img.w + sx) * 3;
                for (s, &v) in sum.iter_mut().zip(&img.px[p..p + 3]) {
                    *s += wt * v as f64;
                }
            }
            let o = (y * w + x) * 3;
            for (d, s) in px[o..o + 3].iter_mut().zip(sum) {
                *d = q16(s);
            }
        }
    }
    Plane { w, h, px }
}

/// `img` convolved with the symmetric `k` along x or y, the edge pixels repeated past the edge.
fn convolve(img: &Plane, k: &[f64], along_x: bool) -> Plane {
    let r = (k.len() - 1) as i64 / 2;
    let (w, h) = (img.w, img.h);
    let mut px = vec![0u16; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0.0f64; 3];
            for (i, kv) in k.iter().enumerate() {
                let d = i as i64 - r;
                let (sx, sy) = if along_x {
                    ((x as i64 + d).clamp(0, w as i64 - 1) as usize, y)
                } else {
                    (x, (y as i64 + d).clamp(0, h as i64 - 1) as usize)
                };
                let p = (sy * w + sx) * 3;
                for (s, &v) in sum.iter_mut().zip(&img.px[p..p + 3]) {
                    *s += kv * v as f64;
                }
            }
            let o = (y * w + x) * 3;
            for (d, s) in px[o..o + 3].iter_mut().zip(sum) {
                *d = q16(s);
            }
        }
    }
    Plane { w, h, px }
}

/// The `crop` (x, y, w, h) of an RGBA picture `src_w` pixels wide, enlarged to `out` (w, h) with
/// Lanczos-3 and sharpened with the unsharp mask, as RGBA bytes (opaque). An empty crop or size: none.
pub fn enlarge(src: &[u8], src_w: usize, crop: (usize, usize, usize, usize), out: (usize, usize)) -> Option<Vec<u8>> {
    let (cx, cy, cw, ch) = crop;
    let (ow, oh) = out;
    if cw == 0 || ch == 0 || ow == 0 || oh == 0 || src.len() < (cy + ch) * src_w * 4 || cx + cw > src_w {
        return None;
    }
    let mut px = Vec::with_capacity(cw * ch * 3);
    for y in cy..cy + ch {
        for x in cx..cx + cw {
            let p = (y * src_w + x) * 4;
            px.extend(src[p..p + 3].iter().map(|&v| v as u16 * 257));
        }
    }
    let img = Plane { w: cw, h: ch, px };
    // The axis enlarged more first, as ImageMagick does.
    let big = if ow as f64 / cw as f64 > oh as f64 / ch as f64 {
        resample(&resample(&img, ow, true), oh, false)
    } else {
        resample(&resample(&img, oh, false), ow, true)
    };
    let k = blur_kernel(UNSHARP_SIGMA);
    let blur = convolve(&convolve(&big, &k, true), &k, false);
    let threshold = Q * UNSHARP_THRESHOLD;
    let mut rgba = Vec::with_capacity(ow * oh * 4);
    for (i, (&p, &b)) in big.px.iter().zip(&blur.px).enumerate() {
        let d = p as f64 - b as f64;
        let v = if (2.0 * d).abs() < threshold { p } else { q16(p as f64 + UNSHARP_GAIN * d) };
        // 16 bits to 8 as ImageMagick's PNG writer gives them.
        rgba.push((v / 257) as u8);
        if i % 3 == 2 {
            rgba.push(255);
        }
    }
    Some(rgba)
}

/// What a background is made for: the crop of the picture and the size in physical pixels.
type Key = ((usize, usize, usize, usize), (usize, usize));

#[derive(Default)]
struct State {
    /// The last background made and what for (`None`: it could not be made).
    done: Option<(Key, Option<Texture2D>)>,
    /// The worker under way, and what for. One at a time: a new size waits for it.
    job: Option<(Key, Receiver<Option<Vec<u8>>>)>,
}

impl State {
    /// Takes the worker's background when it is ready.
    fn collect(&mut self) {
        if let Some((k, rx)) = self.job.take() {
            match rx.try_recv() {
                Ok(made) => {
                    let texture = made.map(|rgba| {
                        let t = Texture2D::from_rgba8(k.1 .0 as u16, k.1 .1 as u16, &rgba);
                        t.set_filter(FilterMode::Nearest);
                        t
                    });
                    self.done = Some((k, texture));
                }
                Err(mpsc::TryRecvError::Empty) => self.job = Some((k, rx)),
                Err(mpsc::TryRecvError::Disconnected) => self.done = Some((k, None)),
            }
        }
    }
}

razdor::local! {
    static STATE: std::cell::RefCell<State> = std::cell::RefCell::new(State::default());
}

/// Draws the background's `crop` (picture pixels) over `screen` enlarged and sharpened, once it is
/// made; starts making it when the crop or the screen's size changed. False while it is not ready (or
/// cannot be made): the caller draws the picture its own way.
pub fn show(crop: Rect, screen: Rect) -> bool {
    let dpi = screen_dpi_scale();
    let size = ((screen.w * dpi).round().max(0.0) as usize, (screen.h * dpi).round().max(0.0) as usize);
    let crop = (crop.x.max(0.0) as usize, crop.y.max(0.0) as usize, crop.w.max(0.0) as usize, crop.h.max(0.0) as usize);
    if size.0 == 0 || size.1 == 0 || size.0 > MAX_SIDE || size.1 > MAX_SIDE {
        return false;
    }
    let key = (crop, size);
    let texture = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.collect();
        if s.job.is_none() && s.done.as_ref().is_none_or(|(k, _)| *k != key) {
            s.job = start(key);
            // A snapshot's is made at once.
            s.collect();
        }
        s.done.as_ref().filter(|(k, _)| *k == key).and_then(|(_, t)| t.clone())
    });
    match texture {
        Some(t) => {
            // Pixel for pixel: the texture is the screen's size in physical pixels.
            chrome::filtered(chrome::Filter::Smooth, || chrome::tex(&t, screen, WHITE));
            true
        }
        None => false,
    }
}

/// Starts making the background for `key` from the install's `Castle.lit`: on a worker thread, or at
/// once for a snapshot (so its frame shows it). None without the picture.
fn start(key: Key) -> Option<(Key, Receiver<Option<Vec<u8>>>)> {
    let img = chrome::image("Windows/Castle.lit")?;
    let (tx, rx) = mpsc::channel();
    let work = move || {
        let _ = tx.send(enlarge(&img.rgba, img.width as usize, key.0, key.1));
    };
    if super::snapshot::size().is_some() {
        work();
    } else {
        std::thread::spawn(work);
    }
    Some((key, rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small test picture, `w` × `h` RGBA (the same formula made ImageMagick's references below).
    fn synth(w: usize, h: usize) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.extend([((x * 53 + y * 97) % 256) as u8, ((x * x * 31 + y * 17 + 40) % 256) as u8, (((x ^ y) * 71 + 13) % 256) as u8, 255]);
            }
        }
        v
    }

    /// `magick s.png -filter Lanczos -resize 16x9! -unsharp 0x1.4+0.7+0.02 o.png` (ImageMagick 7.1.1-43
    /// Q16) on `synth(7, 5)`, RGB: the x factor is the larger, so x is enlarged first.
    const IM_7X5_TO_16X9: [u8; 432] = [0, 33, 0, 0, 30, 0, 0, 28, 57, 32, 61, 114, 47, 137, 117, 67, 184, 116, 107, 161, 195, 164, 85, 255, 244, 25, 229, 255, 12, 77, 255, 12, 0, 108, 5, 31, 0, 30, 134, 0, 86, 175, 12, 128, 166, 55, 148, 168, 0, 38, 18, 0, 35, 6, 11, 32, 0, 57, 66, 27, 122, 142, 109, 160, 189, 197, 144, 166, 255, 104, 89, 255, 126, 30, 200, 182, 17, 95, 179, 16, 10, 75, 10, 0, 0, 35, 45, 19, 91, 149, 92, 133, 229, 131, 153, 255, 63, 51, 102, 68, 48, 44, 80, 46, 0, 139, 79, 0, 237, 155, 104, 255, 202, 255, 169, 179, 255, 7, 104, 220, 0, 43, 145, 0, 30, 127, 44, 30, 74, 54, 23, 0, 73, 48, 0, 135, 102, 120, 190, 146, 255, 214, 167, 255, 188, 64, 117, 214, 60, 109, 251, 58, 96, 255, 92, 94, 250, 168, 119, 169, 215, 143, 64, 191, 143, 0, 115, 126, 0, 56, 125, 0, 42, 139, 43, 42, 144, 160, 35, 122, 225, 60, 118, 187, 116, 143, 115, 158, 170, 96, 179, 149, 211, 73, 125, 251, 68, 182, 255, 66, 255, 255, 99, 255, 160, 175, 135, 8, 222, 0, 0, 199, 0, 49, 124, 19, 113, 63, 104, 128, 50, 149, 173, 50, 220, 255, 43, 255, 255, 68, 255, 124, 124, 178, 0, 166, 25, 0, 187, 0, 70, 79, 255, 96, 76, 255, 142, 73, 255, 155, 107, 255, 81, 183, 126, 21, 230, 0, 58, 206, 0, 169, 132, 0, 255, 71, 12, 255, 58, 204, 255, 57, 255, 215, 51, 255, 105, 76, 255, 18, 130, 162, 0, 173, 37, 0, 194, 0, 0, 92, 255, 0, 88, 239, 6, 86, 191, 53, 119, 140, 113, 195, 122, 161, 242, 105, 183, 219, 28, 193, 143, 0, 242, 83, 30, 255, 70, 195, 255, 70, 255, 121, 63, 255, 0, 88, 180, 11, 144, 131, 94, 186, 126, 144, 207, 126, 76, 103, 84, 80, 101, 83, 93, 99, 88, 145, 133, 108, 229, 209, 137, 255, 255, 169, 200, 232, 209, 88, 156, 209, 53, 96, 184, 107, 83, 105, 146, 83, 42, 109, 76, 45, 82, 101, 86, 127, 157, 129, 184, 199, 156, 211, 220, 172, 155, 108, 0, 164, 106, 0, 176, 104, 39, 224, 137, 109, 255, 213, 146, 255, 255, 188, 187, 237, 255, 12, 159, 255, 0, 101, 255, 0, 88, 45, 60, 88, 0, 119, 81, 0, 168, 106, 49, 208, 162, 137, 222, 204, 163, 224, 225, 178];
    /// The same on `synth(6, 4)` to 15 × 10: equal factors, y first.
    const IM_6X4_TO_15X10: [u8; 450] = [0, 34, 0, 0, 32, 0, 7, 29, 48, 30, 47, 107, 41, 106, 125, 51, 168, 105, 79, 181, 133, 124, 135, 227, 185, 65, 255, 255, 20, 205, 255, 8, 57, 255, 17, 0, 111, 30, 52, 0, 42, 136, 0, 50, 182, 0, 37, 0, 0, 35, 6, 6, 31, 24, 37, 49, 61, 72, 108, 101, 101, 170, 131, 119, 184, 180, 125, 138, 248, 139, 68, 255, 197, 23, 191, 255, 10, 68, 219, 19, 5, 101, 32, 41, 0, 45, 100, 0, 52, 133, 8, 42, 48, 12, 41, 24, 21, 37, 0, 55, 55, 0, 124, 114, 58, 185, 176, 173, 189, 190, 255, 127, 144, 255, 65, 74, 235, 74, 28, 166, 132, 16, 91, 141, 25, 36, 83, 38, 25, 25, 51, 37, 0, 58, 45, 65, 54, 107, 69, 51, 61, 77, 47, 0, 113, 65, 0, 196, 125, 33, 255, 186, 201, 235, 199, 255, 111, 154, 255, 0, 84, 186, 0, 38, 136, 0, 26, 122, 57, 35, 85, 78, 49, 36, 80, 59, 1, 84, 70, 0, 149, 61, 127, 165, 59, 101, 189, 55, 58, 221, 73, 32, 249, 133, 76, 237, 194, 162, 171, 208, 207, 63, 162, 182, 0, 92, 136, 0, 47, 123, 0, 34, 138, 37, 43, 132, 117, 57, 103, 169, 69, 75, 200, 78, 61, 209, 67, 111, 239, 64, 137, 255, 60, 179, 255, 78, 205, 252, 138, 163, 130, 199, 79, 34, 213, 29, 1, 167, 44, 12, 97, 92, 21, 52, 125, 37, 39, 143, 102, 48, 172, 182, 63, 202, 238, 73, 222, 255, 83, 232, 183, 75, 131, 215, 72, 177, 255, 68, 253, 255, 86, 255, 208, 146, 206, 62, 208, 40, 0, 221, 0, 15, 175, 0, 98, 105, 41, 150, 60, 114, 168, 47, 164, 185, 56, 219, 205, 71, 255, 217, 80, 255, 222, 91, 255, 97, 83, 190, 119, 82, 213, 159, 78, 248, 188, 96, 253, 152, 156, 181, 80, 218, 67, 51, 231, 0, 87, 185, 0, 158, 115, 0, 222, 70, 83, 255, 57, 192, 235, 66, 255, 177, 79, 255, 127, 92, 255, 97, 101, 247, 22, 89, 239, 31, 88, 232, 54, 84, 213, 87, 102, 177, 110, 162, 138, 122, 223, 110, 134, 237, 56, 154, 191, 0, 186, 121, 0, 247, 76, 58, 255, 63, 216, 255, 72, 255, 142, 85, 255, 43, 98, 197, 0, 107, 158, 0, 92, 255, 0, 90, 239, 0, 86, 190, 28, 104, 131, 85, 165, 113, 145, 226, 135, 184, 239, 103, 192, 193, 1, 201, 123, 0, 255, 78, 41, 255, 66, 218, 255, 74, 255, 120, 87, 248, 0, 100, 160, 0, 109, 111];

    fn rgb(rgba: &[u8]) -> Vec<u8> {
        rgba.chunks(4).flat_map(|p| p[..3].to_vec()).collect()
    }

    #[test]
    fn the_enlargement_matches_imagemagick_lanczos_and_unsharp() {
        let made = enlarge(&synth(7, 5), 7, (0, 0, 7, 5), (16, 9)).unwrap();
        assert_eq!(rgb(&made), IM_7X5_TO_16X9.to_vec());
        let made = enlarge(&synth(6, 4), 6, (0, 0, 6, 4), (15, 10)).unwrap();
        assert_eq!(rgb(&made), IM_6X4_TO_15X10.to_vec());
        assert!(made.chunks(4).all(|p| p[3] == 255));
    }

    #[test]
    fn a_crop_is_enlarged_alone() {
        // The 6 × 4 picture inside a larger one, at (2, 3): the same pixels as the picture alone.
        let small = synth(6, 4);
        let mut big = vec![0u8; 10 * 9 * 4];
        for y in 0..4 {
            for x in 0..6 {
                let (s, d) = ((y * 6 + x) * 4, ((y + 3) * 10 + x + 2) * 4);
                big[d..d + 4].copy_from_slice(&small[s..s + 4]);
            }
        }
        assert_eq!(enlarge(&big, 10, (2, 3, 6, 4), (15, 10)), enlarge(&small, 6, (0, 0, 6, 4), (15, 10)));
        assert_eq!(enlarge(&big, 10, (2, 3, 0, 4), (15, 10)), None);
    }

    #[test]
    fn lanczos_weights_and_the_blur_kernel_are_imagemagicks() {
        assert_eq!(lanczos3(0.0), 1.0);
        assert!((lanczos3(0.5) - 0.607_927_1).abs() < 1e-6);
        assert!(lanczos3(1.0).abs() < 1e-12 && lanczos3(2.0).abs() < 1e-12);
        assert_eq!(lanczos3(3.0), 0.0);
        // 512 → 1920: the first pixel takes source pixels 0–2, its weights summing to 1.
        let c = contributions(512, 1920);
        assert_eq!(c.len(), 1920);
        assert_eq!(c[0].0, 0);
        let w = &c[0].1;
        assert_eq!(w.len(), 3);
        assert!((w[0] - 1.177_623_1).abs() < 1e-6 && (w[1] + 0.224_079_1).abs() < 1e-6, "{w:?}");
        assert!(c.iter().all(|(_, w)| (w.iter().sum::<f64>() - 1.0).abs() < 1e-9));
        // The unsharp's Gaussian: 13 taps for sigma 1.4, the middle 0.27965.
        let k = blur_kernel(1.4);
        assert_eq!(k.len(), 13);
        assert!((k[6] - 0.279_650_6).abs() < 1e-6 && (k[0] - 3.948e-5).abs() < 1e-7, "{k:?}");
        assert!((k.iter().sum::<f64>() - 1.0).abs() < 1e-12);
    }
}
