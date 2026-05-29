//! JPEG XL decode for the in-browser WASM binary.
//!
//! The Service Worker fetches a `.jxl` (or a low-res truncated prefix of one)
//! and hands the bytes here. We decode with `jxl-oxide` and return a 24-bit
//! BMP byte buffer — a format every browser renders directly from an `<img>`,
//! and trivial to encode (no compression pass). Card art is opaque, so we drop
//! any alpha channel and emit BGR.
//!
//! Entry point exported to JS:
//! - `decode_jxl(bytes, max_dim)` — decode whatever frame is available (full
//!   file → full image; truncated prefix → upscaled DC/low-res preview), then
//!   optionally downscale so the longest side is at most `max_dim` (0 = none).
//!
//! Downscaling matters: jxl-oxide returns the DC preview **upscaled to the full
//! canvas size**, so without it a 160px grid tile would receive a multi-megabyte
//! full-dimension BMP. Grid tiles pass a small `max_dim`; the detail view passes 0.

use jxl_oxide::{InitializeResult, JxlImage};
use wasm_bindgen::prelude::*;

/// Decode JXL bytes (complete file or truncated prefix) to a 24-bit BMP buffer,
/// downscaled so its longest side is at most `max_dim` pixels (0 = no downscale).
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
/// Uses the incremental `feed_bytes` / `try_init` path so a truncated prefix
/// still initializes and renders its DC/low-res frame instead of erroring.
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

    // Complete keyframe → render it; otherwise render the partially-loaded
    // keyframe (the DC/low-res preview from a progressive prefix). Mirrors
    // jxl-oxide's own integration adapter.
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

/// Smallest leading-byte count of a progressive `.jxl` that *decodes at all*
/// (the structural boundary). NOTE: at this point the DC pixels are typically
/// still empty (black) — use [`preview_prefix_len`] for a populated preview.
/// Kept for tests/diagnostics. Native-only (build tooling).
#[cfg(not(target_arch = "wasm32"))]
pub fn dc_prefix_len(bytes: &[u8]) -> Option<usize> {
    if decode_to_rgb(bytes).is_err() {
        return None; // even the full file doesn't decode — caller's problem
    }
    let (mut lo, mut hi) = (1usize, bytes.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if decode_to_rgb(&bytes[..mid]).is_ok() {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Some(lo)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
fn mean_brightness(bytes: &[u8]) -> f64 {
    match decode_to_rgb(bytes) {
        Ok((_, _, rgb)) if !rgb.is_empty() => {
            rgb.iter().map(|&b| b as u64).sum::<u64>() as f64 / rgb.len() as f64
        }
        _ => 0.0,
    }
}

/// Per-channel (R,G,B) mean of a decoded prefix, or None if it didn't decode.
/// `None` channel means are returned as `[-1.0; 3]` sentinel so callers can
/// treat undecodable prefixes as "far" from the target.
#[cfg(not(target_arch = "wasm32"))]
fn channel_means(bytes: &[u8]) -> [f64; 3] {
    match decode_to_rgb(bytes) {
        Ok((_, _, rgb)) if rgb.len() >= 3 => {
            let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
            for px in rgb.chunks_exact(3) {
                r += px[0] as u64;
                g += px[1] as u64;
                b += px[2] as u64;
            }
            let n = (rgb.len() / 3) as f64;
            [r as f64 / n, g as f64 / n, b as f64 / n]
        }
        _ => [-1.0; 3],
    }
}

/// Smallest leading-byte count of a progressive `.jxl` that decodes to a
/// *populated and colored* DC/low-res preview.
///
/// Two failure modes the prefix must clear: (1) the minimal-decodable prefix
/// renders fully black (`render_loading_frame` succeeds before DC pixels
/// arrive); (2) early progressive passes carry luma before chroma, so a
/// brightness-only target yields a grayscale preview. We therefore require all
/// three channel means to be within `max_channel_delta` of the full image's.
/// The summed per-channel error falls monotonically as the DC (then chroma)
/// fills in, so we binary-search the crossing. Native-only (build tooling).
#[cfg(not(target_arch = "wasm32"))]
pub fn preview_prefix_len(bytes: &[u8], max_channel_delta: f64) -> Option<usize> {
    let total = bytes.len();
    let full = channel_means(bytes);
    if full[0] < 0.0 {
        return None; // didn't decode
    }
    let error = |prefix: &[u8]| -> f64 {
        let m = channel_means(prefix);
        if m[0] < 0.0 {
            return f64::MAX; // undecodable prefix is maximally far
        }
        (0..3).map(|i| (m[i] - full[i]).abs()).fold(0.0, f64::max)
    };
    let (mut lo, mut hi) = (1usize, total);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if error(&bytes[..mid]) <= max_channel_delta {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Some(lo)
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

    #[test]
    fn truncated_prefix_still_decodes_preview() {
        // Feed only the first ~40% of the file — should still init + render a
        // (DC/low-res) frame rather than erroring.
        let prefix = &FIXTURE[..FIXTURE.len() * 2 / 5];
        match decode_to_rgb(prefix) {
            Ok((w, h, _)) => assert_eq!((w, h), (160, 160)),
            Err(e) => panic!("truncated decode failed: {e}"),
        }
    }

    /// The preview prefix must decode to a *populated and colored* image — not
    /// the black minimal-decodable prefix, nor a grayscale luma-only pass.
    /// Guards both regressions (black tiles, then desaturated tiles).
    #[test]
    fn preview_prefix_is_populated_and_colored() {
        const DELTA: f64 = 6.0;
        let full = channel_means(FIXTURE);
        let dc = preview_prefix_len(FIXTURE, DELTA).expect("decodes");
        assert!(dc < FIXTURE.len(), "preview must be smaller than whole file");
        let m = channel_means(&FIXTURE[..dc]);
        for i in 0..3 {
            assert!(
                (m[i] - full[i]).abs() <= DELTA,
                "channel {i} off: prefix {:.1} vs full {:.1}",
                m[i],
                full[i]
            );
        }
        // Brighter than the bare (black) minimal-decodable prefix.
        let bare = dc_prefix_len(FIXTURE).unwrap();
        assert!(mean_brightness(&FIXTURE[..dc]) > mean_brightness(&FIXTURE[..bare]));
    }
}
