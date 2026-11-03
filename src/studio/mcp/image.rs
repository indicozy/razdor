//! The canvas picture the `snapshot` tool sends: scaled down to a long side of [`MAX_SIDE`] pixels
//! (area average), encoded as PNG (the `png` crate the package terrains already use), and base64 for
//! the MCP image content block.

/// The longest side of a picture the tool sends, in pixels.
pub const MAX_SIDE: u32 = 1600;

/// RGBA `width` × `height` pixels, scaled down so the long side is at most `max_side` (an area
/// average of the source pixels; a smaller picture is returned as it is). An error when the buffer
/// does not hold `width * height` RGBA pixels.
pub fn fit(width: u32, height: u32, rgba: &[u8], max_side: u32) -> Result<(u32, u32, Vec<u8>), String> {
    if width == 0 || height == 0 {
        return Err("the picture is empty".into());
    }
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(format!("the picture has {} bytes for {width}x{height} pixels", rgba.len()));
    }
    let long = width.max(height);
    if long <= max_side {
        return Ok((width, height, rgba.to_vec()));
    }
    let scale = f64::from(max_side) / f64::from(long);
    let nw = ((f64::from(width) * scale).round() as u32).max(1);
    let nh = ((f64::from(height) * scale).round() as u32).max(1);
    let mut out = vec![0u8; nw as usize * nh as usize * 4];
    for y in 0..nh {
        let (y0, y1) = (u64::from(y) * u64::from(height) / u64::from(nh), (u64::from(y) + 1) * u64::from(height) / u64::from(nh));
        let y1 = y1.max(y0 + 1);
        for x in 0..nw {
            let (x0, x1) = (u64::from(x) * u64::from(width) / u64::from(nw), (u64::from(x) + 1) * u64::from(width) / u64::from(nw));
            let x1 = x1.max(x0 + 1);
            let mut sum = [0u64; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = (sy as usize * width as usize + sx as usize) * 4;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += u64::from(rgba[i + c]);
                    }
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            let o = (y as usize * nw as usize + x as usize) * 4;
            for (c, s) in sum.iter().enumerate() {
                out[o + c] = ((s + n / 2) / n) as u8;
            }
        }
    }
    Ok((nw, nh, out))
}

/// RGBA pixels as a PNG file's bytes.
pub fn png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| format!("cannot encode the picture: {e}"))?;
    writer.write_image_data(rgba).map_err(|e| format!("cannot encode the picture: {e}"))?;
    writer.finish().map_err(|e| format!("cannot encode the picture: {e}"))?;
    Ok(out)
}

/// Standard base64 (RFC 4648, with padding).
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_small_picture_is_kept_and_a_large_one_is_averaged_down() {
        let small = vec![10u8; 4 * 4 * 4];
        assert_eq!(fit(4, 4, &small, 1600).unwrap(), (4, 4, small.clone()));
        // 8x4 black and white columns, scaled to 2 px wide: each output pixel averages 4 columns.
        let mut wide = Vec::new();
        for _y in 0..4 {
            for x in 0..8 {
                wide.extend_from_slice(if x < 4 { &[0, 0, 0, 255] } else { &[255, 255, 255, 255] });
            }
        }
        let (w, h, out) = fit(8, 4, &wide, 2).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(&out[0..4], &[0, 0, 0, 255]);
        assert_eq!(&out[4..8], &[255, 255, 255, 255]);
        assert!(fit(4, 4, &small[..10], 2).is_err());
    }

    #[test]
    fn a_png_decodes_to_the_pixels_it_was_made_of() {
        let rgba: Vec<u8> = (0..3 * 2 * 4).map(|i| (i * 7) as u8).collect();
        let bytes = png(3, 2, &rgba).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
        let mut reader = png::Decoder::new(bytes.as_slice()).read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(&buf[..rgba.len()], rgba.as_slice());
    }
}
