//! JPEG XL decode for the in-browser WASM binary.
//!
//! The Service Worker fetches a `.jxl` and hands the bytes here. We decode the
//! full image with `jxl-oxide` and return a 24-bit BMP byte buffer — a format
//! every browser renders directly from an `<img>`, trivial to encode (no
//! compression pass). Card art is opaque, so we drop alpha and emit BGR.
//!
//! Entry point exported to JS:
//! - `decode_jxl(bytes, max_dim)` — decode the full image, then optionally
//!   downscale so the longest side is at most `max_dim` (0 = no downscale).
//!
//! Grid tiles pass a small `max_dim` (e.g. 512) so the decoded BMP is tile-sized
//! rather than the multi-megabyte full canvas; the detail view passes 0.

use jxl_oxide::{InitializeResult, JxlImage};
use wasm_bindgen::prelude::*;

/// Decode a JXL file to a 24-bit BMP buffer, downscaled so its longest side is
/// at most `max_dim` pixels (0 = no downscale).
///
/// Returns the BMP bytes as a `Uint8Array` to JS. Errors become a thrown
/// `JsError` so the Service Worker can fall back / surface a broken image.
#[wasm_bindgen]
pub fn decode_jxl(bytes: &[u8], max_dim: u32) -> Result<Vec<u8>, JsError> {
    let (w, h, rgb) = decode_to_rgb(bytes).map_err(|e| JsError::new(&e))?;
    let (w, h, rgb) = downscale_rgb(w, h, rgb, max_dim);
    Ok(encode_bmp_bgr(w, h, &rgb))
}

/// Box-average downscale of interleaved RGB so the longest side ≤ `max_dim`.
/// Returns the input unchanged when `max_dim` is 0 or already small enough.
fn downscale_rgb(w: u32, h: u32, rgb: Vec<u8>, max_dim: u32) -> (u32, u32, Vec<u8>) {
    if max_dim == 0 || w.max(h) <= max_dim {
        return (w, h, rgb);
    }
    let scale = max_dim as f64 / w.max(h) as f64;
    let dw = (w as f64 * scale).round().max(1.0) as u32;
    let dh = (h as f64 * scale).round().max(1.0) as u32;
    let (wu, dwu, dhu) = (w as usize, dw as usize, dh as usize);

    let mut out = vec![0u8; dwu * dhu * 3];
    for dy in 0..dhu {
        // Source row span covered by this destination row.
        let sy0 = dy * h as usize / dhu;
        let sy1 = ((dy + 1) * h as usize / dhu).max(sy0 + 1);
        for dx in 0..dwu {
            let sx0 = dx * wu / dwu;
            let sx1 = ((dx + 1) * wu / dwu).max(sx0 + 1);
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for sy in sy0..sy1 {
                let row = sy * wu * 3;
                for sx in sx0..sx1 {
                    let p = row + sx * 3;
                    r += rgb[p] as u32;
                    g += rgb[p + 1] as u32;
                    b += rgb[p + 2] as u32;
                    n += 1;
                }
            }
            let o = (dy * dwu + dx) * 3;
            out[o] = (r / n) as u8;
            out[o + 1] = (g / n) as u8;
            out[o + 2] = (b / n) as u8;
        }
    }
    (dw, dh, out)
}

/// Decode to interleaved 8-bit RGB (3 bytes/pixel, row-major, top-down).
///
/// We feed the whole file and render the complete keyframe (`render_frame`) for
/// a full-resolution, fully-colored image. The `render_loading_frame` branch is
/// only a fallback if a complete keyframe somehow isn't loaded (e.g. a truncated
/// byte range); the normal grid/detail paths supply the whole file.
fn decode_to_rgb(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut uninit = JxlImage::builder().build_uninit();
    uninit
        .feed_bytes(bytes)
        .map_err(|e| format!("feed_bytes: {e}"))?;

    let mut image = match uninit.try_init().map_err(|e| format!("try_init: {e}"))? {
        InitializeResult::Initialized(img) => img,
        InitializeResult::NeedMoreData(_) => {
            return Err("not enough data to initialize JXL header".into());
        }
    };

    let render = if image.num_loaded_keyframes() > 0 {
        image.render_frame(0)
    } else {
        image.render_loading_frame()
    }
    .map_err(|e| format!("render: {e}"))?;

    let stream = render.stream();
    let width = stream.width();
    let height = stream.height();
    let channels = stream.channels() as usize;

    let mut fb = vec![0f32; width as usize * height as usize * channels];
    let mut stream = stream;
    stream.write_to_buffer(&mut fb);

    // Interleaved f32 samples in [0,1] → RGB u8. Handle gray / RGB / RGBA.
    let px = width as usize * height as usize;
    let mut rgb = vec![0u8; px * 3];
    for i in 0..px {
        let base = i * channels;
        let (r, g, b) = match channels {
            1 => (fb[base], fb[base], fb[base]),
            _ => (fb[base], fb[base + 1], fb[base + 2]),
        };
        rgb[i * 3] = to_u8(r);
        rgb[i * 3 + 1] = to_u8(g);
        rgb[i * 3 + 2] = to_u8(b);
    }

    Ok((width, height, rgb))
}

#[inline]
fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Encode interleaved top-down RGB into a 24-bit BMP (BITMAPINFOHEADER).
///
/// BMP rows are bottom-up and padded to a 4-byte boundary; pixels are BGR.
fn encode_bmp_bgr(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let row_bytes = (width as usize * 3 + 3) & !3; // pad to 4 bytes
    let pixel_data_size = row_bytes * height as usize;
    let file_size = 14 + 40 + pixel_data_size;

    let mut out = Vec::with_capacity(file_size);

    // --- BITMAPFILEHEADER (14 bytes) ---
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    out.extend_from_slice(&54u32.to_le_bytes()); // pixel data offset (14+40)

    // --- BITMAPINFOHEADER (40 bytes) ---
    out.extend_from_slice(&40u32.to_le_bytes()); // header size
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes()); // positive => bottom-up
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB (no compression)
    out.extend_from_slice(&(pixel_data_size as u32).to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes()); // x ppm (~72 dpi)
    out.extend_from_slice(&2835i32.to_le_bytes()); // y ppm
    out.extend_from_slice(&0u32.to_le_bytes()); // colors used
    out.extend_from_slice(&0u32.to_le_bytes()); // important colors

    // --- pixel data: bottom-up rows, BGR, padded ---
    let w = width as usize;
    let pad = row_bytes - w * 3;
    for y in (0..height as usize).rev() {
        let row_start = y * w * 3;
        for x in 0..w {
            let p = row_start + x * 3;
            out.push(rgb[p + 2]); // B
            out.push(rgb[p + 1]); // G
            out.push(rgb[p]); // R
        }
        out.extend(std::iter::repeat(0u8).take(pad));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/card.jxl");

    #[test]
    fn decodes_full_jxl_to_rgb() {
        let (w, h, rgb) = decode_to_rgb(FIXTURE).expect("decode");
        assert_eq!((w, h), (160, 160));
        assert_eq!(rgb.len(), 160 * 160 * 3);
        // A real card is not all-black: at least one non-zero sample.
        assert!(rgb.iter().any(|&b| b != 0));
    }

    #[test]
    fn emits_valid_bmp() {
        let bmp = decode_jxl(FIXTURE, 0).expect("decode");
        assert_eq!(&bmp[0..2], b"BM");
        // file_size field matches actual length
        let file_size = u32::from_le_bytes([bmp[2], bmp[3], bmp[4], bmp[5]]) as usize;
        assert_eq!(file_size, bmp.len());
        // pixel data offset = 54, 24bpp
        assert_eq!(u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]), 54);
        assert_eq!(u16::from_le_bytes([bmp[28], bmp[29]]), 24);
        // width/height in header reflect the full fixture (no downscale).
        assert_eq!(i32::from_le_bytes([bmp[18], bmp[19], bmp[20], bmp[21]]), 160);
    }

    #[test]
    fn downscales_to_max_dim() {
        // 160x160 fixture, cap the longest side at 64 → 64x64.
        let bmp = decode_jxl(FIXTURE, 64).expect("decode");
        assert_eq!(i32::from_le_bytes([bmp[18], bmp[19], bmp[20], bmp[21]]), 64);
        assert_eq!(i32::from_le_bytes([bmp[22], bmp[23], bmp[24], bmp[25]]), 64);
        // smaller than the full-size BMP
        assert!(bmp.len() < decode_jxl(FIXTURE, 0).unwrap().len());
    }
}
