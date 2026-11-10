//! Depth Anything V2 Small on tract: the depth of a sprite, higher = closer, at the sprite's size.
//!
//! The file is the shape-fixed fp16 export (`tools/depth/fix_onnx.py`, 49.6 MB). At load its
//! fp16 weights and casts become fp32 (tract's own fp16 path runs at 23 to 36 s per image; its
//! fp32 path at 0.3 to 0.5 s on one desktop thread). The loaded weights take 99 MB.
//!
//! Two input sizes come from the one file: [`SMALL`] (266) for most sprites and [`LARGE`] (518)
//! for big ones. Each size is a plan of its own; the 518 plan is made on first use (about 130 MB
//! more while loaded), so a install of small sprites never pays for it.
//!
//! Thread safety: [`DepthModel`] is `Send` and `Sync`. A run keeps its state in its own call, so
//! the workers share one model (and its weights) and each cuts one sprite at a time.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use half::f16;
use image::imageops::{self, FilterType};
use image::RgbImage;
use tract_onnx::pb;
use tract_onnx::prelude::*;

use super::queue::{input_size, LARGE, SMALL};
use crate::dt::gfx::Image;
/// The background the sprite is composited over before the model sees it.
const GREY: f32 = 128.0;
/// ImageNet mean and standard deviation, per RGB channel.
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// The ONNX element types used here (`TensorProto.DataType`).
const ONNX_FLOAT: i32 = 1;
const ONNX_FLOAT16: i32 = 10;

type Plan = std::sync::Arc<TypedRunnableModel>;

/// Depth Anything V2 Small, loaded and optimised by tract.
pub struct DepthModel {
    path: PathBuf,
    small: Plan,
    /// The plan at the large input, made from the same file on first use. `None` when that failed.
    large: OnceLock<Option<Plan>>,
}

impl DepthModel {
    /// Loads the model file at the small input. `None`, with the reason on stderr, when the file
    /// cannot be read or tract refuses it (not a shape-fixed Depth Anything V2 Small file).
    pub fn load(path: &Path) -> Option<DepthModel> {
        match load_plan(path, SMALL) {
            Ok(small) => Some(DepthModel { path: path.to_path_buf(), small, large: OnceLock::new() }),
            Err(e) => {
                eprintln!("depth model {}: {e:?}", path.display());
                None
            }
        }
    }

    /// The plan for an input side: the small one, or the large one (made now if it is not yet).
    fn plan(&self, size: usize) -> Option<&Plan> {
        if size == SMALL {
            return Some(&self.small);
        }
        self.large
            .get_or_init(|| match load_plan(&self.path, LARGE) {
                Ok(plan) => Some(plan),
                Err(e) => {
                    eprintln!("depth model {} at {LARGE} px: {e:?}", self.path.display());
                    None
                }
            })
            .as_ref()
    }

    /// The sprite's depth map, `width * height` values, row-major, higher = closer. The sprite is
    /// composited over grey, resized to the model's input, run, and the depth resized back to the
    /// sprite's size (bilinear, align_corners off, as PyTorch's `interpolate`). Empty when the
    /// sprite has no pixels or the run fails (the failure is printed on stderr).
    pub fn depth(&self, rgba: &Image) -> Vec<f32> {
        if rgba.width == 0 || rgba.height == 0 {
            return Vec::new();
        }
        match self.run(rgba) {
            Ok(depth) => depth,
            Err(e) => {
                eprintln!("depth model run: {e:?}");
                Vec::new()
            }
        }
    }

    fn run(&self, rgba: &Image) -> TractResult<Vec<f32>> {
        let (w, h) = (rgba.width as usize, rgba.height as usize);
        if rgba.rgba.len() != w * h * 4 {
            return Ok(Vec::new());
        }
        let size = input_size(rgba.width, rgba.height);
        let Some(plan) = self.plan(size) else { return Ok(Vec::new()) };
        let input = preprocess(rgba, size);
        let tensor = Tensor::from_shape(&[1, 3, size, size], &input)?;
        let out = plan.run(tvec!(tensor.into()))?;
        let view = out[0].to_plain_array_view::<f32>()?;
        let depth: Vec<f32> = view.iter().copied().collect();
        if depth.len() != size * size {
            return Ok(Vec::new());
        }
        Ok(resize_bilinear(&depth, size, size, w, h))
    }
}

fn load_plan(path: &Path, size: usize) -> TractResult<Plan> {
    let mut proto = tract_onnx::onnx().proto_model_for_path(path)?;
    upcast_fp16(&mut proto);
    let plan = tract_onnx::onnx()
        .model_for_proto_model(&proto)?
        .with_input_fact(0, f32::fact([1, 3, size, size]).into())?
        .into_optimized()?
        .into_runnable()?;
    Ok(plan)
}

/// The fp16 tensors of the graph (initializers, Constant values) and the Cast targets become fp32.
/// The shapes are not touched.
fn upcast_fp16(proto: &mut pb::ModelProto) {
    let Some(graph) = proto.graph.as_mut() else { return };
    for t in graph.initializer.iter_mut() {
        upcast_tensor(t);
    }
    for node in graph.node.iter_mut() {
        for attr in node.attribute.iter_mut() {
            if node.op_type == "Cast" && attr.name == "to" && attr.i == i64::from(ONNX_FLOAT16) {
                attr.i = i64::from(ONNX_FLOAT);
            }
            if node.op_type == "Constant" && attr.name == "value" {
                if let Some(t) = attr.t.as_mut() {
                    upcast_tensor(t);
                }
            }
        }
    }
}

fn upcast_tensor(t: &mut pb::TensorProto) {
    if t.data_type != ONNX_FLOAT16 {
        return;
    }
    let raw: Vec<u8> = if !t.raw_data.is_empty() {
        t.raw_data.chunks_exact(2).flat_map(|b| f16::from_le_bytes([b[0], b[1]]).to_f32().to_le_bytes()).collect()
    } else {
        // Without raw data the fp16 bit patterns sit in int32_data (ONNX's rule for fp16).
        t.int32_data.iter().flat_map(|&bits| f16::from_bits(bits as u16).to_f32().to_le_bytes()).collect()
    };
    t.raw_data = raw;
    t.int32_data.clear();
    t.data_type = ONNX_FLOAT;
}

/// The sprite over grey, RGB bytes, row-major.
fn composite(rgba: &Image) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.rgba.len() / 4 * 3);
    for px in rgba.rgba.chunks_exact(4) {
        let a = px[3] as f32 / 255.0;
        for &c in &px[..3] {
            let v = c as f32 * a + GREY * (1.0 - a);
            rgb.push(v.round_ties_even().clamp(0.0, 255.0) as u8);
        }
    }
    rgb
}

/// The composite resized to the model's square input of `size` (CatmullRom; the spike's measured
/// choice), normalised with ImageNet's mean and deviation, channels first.
fn preprocess(rgba: &Image, size: usize) -> Vec<f32> {
    let img = RgbImage::from_raw(rgba.width, rgba.height, composite(rgba)).expect("the composite is width x height");
    let img = imageops::resize(&img, size as u32, size as u32, FilterType::CatmullRom);
    let n = size * size;
    let mut out = vec![0f32; 3 * n];
    for (i, px) in img.pixels().enumerate() {
        for c in 0..3 {
            out[c * n + i] = (px[c] as f32 / 255.0 - MEAN[c]) / STD[c];
        }
    }
    out
}

/// Bilinear resize of a row-major `sw x sh` grid to `dw x dh`, PyTorch's rule with align_corners
/// off: the source position of output `o` is `(o + 0.5) * scale - 0.5`, clamped at 0.
fn resize_bilinear(src: &[f32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<f32> {
    let taps = |out: usize, len: usize| -> Vec<(usize, usize, f32)> {
        let scale = len as f64 / out as f64;
        (0..out)
            .map(|o| {
                let s = ((o as f64 + 0.5) * scale - 0.5).max(0.0);
                let i0 = (s.floor() as usize).min(len - 1);
                let i1 = (i0 + 1).min(len - 1);
                (i0, i1, (s - i0 as f64) as f32)
            })
            .collect()
    };
    let (xs, ys) = (taps(dw, sw), taps(dh, sh));
    let mut out = Vec::with_capacity(dw * dh);
    for &(y0, y1, fy) in &ys {
        for &(x0, x1, fx) in &xs {
            let top = src[y0 * sw + x0] * (1.0 - fx) + src[y0 * sw + x1] * fx;
            let bottom = src[y1 * sw + x0] * (1.0 - fx) + src[y1 * sw + x1] * fx;
            out.push(top * (1.0 - fy) + bottom * fy);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: u32, h: u32, rgba: Vec<u8>) -> Image {
        Image { width: w, height: h, rgba }
    }

    #[test]
    fn the_model_is_send_and_sync_so_the_workers_share_it() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DepthModel>();
    }

    /// The large plan comes from the same file: a sprite past the small side's limit gets a depth
    /// map of its own size. Needs `RAZDOR_DEPTH_MODEL`.
    #[test]
    fn a_big_sprite_and_a_small_one_both_get_a_depth_map() {
        let Some(path) = std::env::var_os(crate::depth::fetch::ENV_VAR) else { return };
        let model = DepthModel::load(Path::new(&path)).expect("the model loads");
        let small = image(40, 30, vec![90; 40 * 30 * 4]);
        let big = image(200, 150, vec![90; 200 * 150 * 4]);
        assert_eq!(model.depth(&small).len(), 40 * 30);
        let depth = model.depth(&big);
        assert_eq!(depth.len(), 200 * 150);
        assert!(depth.iter().all(|d| d.is_finite()));
        assert!(model.large.get().is_some_and(|p| p.is_some()), "the large plan was made");
    }

    #[test]
    fn transparent_pixels_composite_to_grey_and_opaque_ones_stay() {
        let img = image(2, 1, vec![200, 10, 30, 255, 200, 10, 30, 0]);
        assert_eq!(composite(&img), vec![200, 10, 30, 128, 128, 128]);
    }

    #[test]
    fn bilinear_resize_follows_align_corners_off() {
        // 4 -> 8 on a ramp: source positions (o + 0.5) / 2 - 0.5, clamped at both ends.
        let out = resize_bilinear(&[0.0, 1.0, 2.0, 3.0], 4, 1, 8, 1);
        let want = [0.0, 0.25, 0.75, 1.25, 1.75, 2.25, 2.75, 3.0];
        for (a, b) in out.iter().zip(want) {
            assert!((a - b).abs() < 1e-6, "{out:?}");
        }
    }

    #[test]
    fn fp16_tensors_and_casts_become_fp32() {
        let half = |v: f32| f16::from_f32(v).to_le_bytes().to_vec();
        let mut raw = half(1.0);
        raw.extend(half(-2.5));
        let mut proto = pb::ModelProto {
            graph: Some(pb::GraphProto {
                initializer: vec![pb::TensorProto {
                    data_type: ONNX_FLOAT16,
                    dims: vec![2],
                    raw_data: raw,
                    ..Default::default()
                }],
                node: vec![pb::NodeProto {
                    op_type: "Cast".into(),
                    attribute: vec![pb::AttributeProto { name: "to".into(), i: i64::from(ONNX_FLOAT16), ..Default::default() }],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        upcast_fp16(&mut proto);
        let graph = proto.graph.unwrap();
        let t = &graph.initializer[0];
        assert_eq!(t.data_type, ONNX_FLOAT);
        let floats: Vec<f32> = t.raw_data.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        assert_eq!(floats, vec![1.0, -2.5]);
        assert_eq!(graph.node[0].attribute[0].i, i64::from(ONNX_FLOAT));
    }
}
