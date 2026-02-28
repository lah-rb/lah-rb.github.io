/// rqrr-decode — QR decode stage for the YOLO+rqrr two-stage pipeline.
///
/// This is a trimmed version of the original qr_decode.rs, optimized for
/// the two-stage pipeline where YOLO has already localized the QR code.
/// Since we receive a tight crop (~200×200) instead of a full frame (640×480),
/// we need fewer strategies and each runs much faster.
///
/// Key differences from the full-frame scanner:
/// - No full-frame grid detection (YOLO handles localization)
/// - Fewer strategies needed (tight crop has higher effective resolution)
/// - Adaptive threshold family still leads (best for camouflage texture)
/// - Added quiet zone padding (crop may clip QR edges)
use rqrr::PreparedImage;
use wasm_bindgen::prelude::*;

// ── Strategy cascade for cropped QR regions ────────────────────────

const NUM_STRATEGIES: usize = 10;

const STRATEGY_NAMES: [&str; NUM_STRATEGIES] = [
    "adaptive_thresh",
    "at_fine",
    "at_coarse",
    "quiet_zone_at",
    "raw",
    "clahe",
    "blur_at",
    "contrast_stretch_at",
    "yellow_aware",
    "otsu",
];

// ── Public WASM API ────────────────────────────────────────────────

/// Decode a QR code from a cropped RGBA region provided by YOLO detection.
///
/// This is the main entry point called from the Web Worker after YOLO
/// has localized the QR bounding box. The crop should be the RGBA pixels
/// of just the QR region (with some padding).
///
/// Returns the decoded text on success, or empty string if decode fails.
#[wasm_bindgen]
pub fn decode_qr_crop(rgba: &[u8], width: usize, height: usize) -> String {
    if rgba.len() < width * height * 4 || width == 0 || height == 0 {
        return String::new();
    }

    let grey = rgba_to_greyscale(rgba, width, height);
    let yellow = rgba_to_yellow_aware(rgba, width, height);

    // Always add quiet zone first — YOLO crops may clip QR edges
    let pad = (width.min(height) / 10).max(4);
    let (padded, pw, ph) = add_quiet_zone(&grey, width, height, pad);
    let (padded_y, _, _) = add_quiet_zone(&yellow, width, height, pad);

    // Strategy cascade — ordered by effectiveness for Kipukas cards
    // On a tight crop, we typically decode on the first 1-3 strategies
    let strategies: [fn(&[u8], &[u8], usize, usize) -> Option<String>; NUM_STRATEGIES] = [
        |g, _, w, h| { let a = adaptive_threshold(g, w, h, 15, 8); try_decode(& a, w, h) },
        |g, _, w, h| { let a = adaptive_threshold(g, w, h, 11, 6); try_decode(&a, w, h) },
        |g, _, w, h| { let a = adaptive_threshold(g, w, h, 21, 10); try_decode(&a, w, h) },
        |g, _, w, h| try_decode(g, w, h), // quiet-zone padded raw
        |g, _, w, h| try_decode(g, w, h), // will get original grey below
        |g, _, w, h| { let c = clahe(g, w, h, 4, 4, 2.0); try_decode(&c, w, h) },
        |g, _, w, h| { let b = gaussian_blur(g, w, h, 1); let a = adaptive_threshold(&b, w, h, 15, 8); try_decode(&a, w, h) },
        |g, _, w, h| { let s = contrast_stretch(g); let a = adaptive_threshold(&s, w, h, 15, 8); try_decode(&a, w, h) },
        |_, y, w, h| try_decode(y, w, h),
        |g, _, w, h| { let t = otsu_threshold(g); try_decode_bitmap(g, w, h, t) },
    ];

    // Try padded versions first (strategies 0-3)
    for (i, strat) in strategies[..4].iter().enumerate() {
        if let Some(text) = strat(&padded, &padded_y, pw, ph) {
            return format!("{}|{}|{}", i, STRATEGY_NAMES[i], text);
        }
    }

    // Try on original (unpadded) crop (strategies 4-9)
    for (i, strat) in strategies[4..].iter().enumerate() {
        let idx = i + 4;
        if let Some(text) = strat(&grey, &yellow, width, height) {
            return format!("{}|{}|{}", idx, STRATEGY_NAMES[idx], text);
        }
    }

    String::new()
}

// ── rqrr decode wrappers ──────────────────────────────────────────

fn try_decode(grey: &[u8], w: usize, h: usize) -> Option<String> {
    let mut img = PreparedImage::prepare_from_greyscale(w, h, |x, y| grey[y * w + x]);
    let grids = img.detect_grids();
    grids.first().and_then(|g| g.decode().ok()).map(|(_, content)| content)
}

fn try_decode_bitmap(grey: &[u8], w: usize, h: usize, threshold: u8) -> Option<String> {
    let mut img = PreparedImage::prepare_from_bitmap(w, h, |x, y| grey[y * w + x] < threshold);
    let grids = img.detect_grids();
    grids.first().and_then(|g| g.decode().ok()).map(|(_, content)| content)
}

// ── Image preprocessing (subset from qr_decode.rs) ────────────────

fn rgba_to_greyscale(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let len = width * height;
    let mut grey = Vec::with_capacity(len);
    for i in 0..len {
        let base = i * 4;
        let r = rgba[base] as u32;
        let g = rgba[base + 1] as u32;
        let b = rgba[base + 2] as u32;
        grey.push(((77 * r + 150 * g + 29 * b) >> 8) as u8);
    }
    grey
}

fn rgba_to_yellow_aware(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let len = width * height;
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let base = i * 4;
        let r = rgba[base];
        let g = rgba[base + 1];
        let b = rgba[base + 2];
        out.push(r.max(g).saturating_sub(b));
    }
    out
}

fn add_quiet_zone(grey: &[u8], w: usize, h: usize, pad: usize) -> (Vec<u8>, usize, usize) {
    let nw = w + 2 * pad;
    let nh = h + 2 * pad;
    let mut out = vec![255u8; nw * nh];
    for y in 0..h {
        for x in 0..w {
            out[(y + pad) * nw + (x + pad)] = grey[y * w + x];
        }
    }
    (out, nw, nh)
}

fn adaptive_threshold(grey: &[u8], w: usize, h: usize, block: usize, c: i32) -> Vec<u8> {
    let iw = w + 1;
    let mut integral = vec![0i64; iw * (h + 1)];
    for y in 0..h {
        let mut row_sum = 0i64;
        for x in 0..w {
            row_sum += grey[y * w + x] as i64;
            integral[(y + 1) * iw + (x + 1)] = row_sum + integral[y * iw + (x + 1)];
        }
    }
    let mut result = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let y0 = y.saturating_sub(block);
            let x0 = x.saturating_sub(block);
            let y1 = (y + block + 1).min(h);
            let x1 = (x + block + 1).min(w);
            let area = ((y1 - y0) * (x1 - x0)) as i64;
            let sum = integral[y1 * iw + x1] - integral[y0 * iw + x1]
                - integral[y1 * iw + x0] + integral[y0 * iw + x0];
            let thresh = sum / area - c as i64;
            result[y * w + x] = if (grey[y * w + x] as i64) < thresh { 0 } else { 255 };
        }
    }
    result
}

fn gaussian_blur(grey: &[u8], w: usize, h: usize, passes: usize) -> Vec<u8> {
    let mut current = grey.to_vec();
    let mut temp = vec![0u8; w * h];
    for _ in 0..passes {
        for y in 0..h {
            for x in 0..w {
                let idx = y * w + x;
                let mut sum = 6u32 * current[idx] as u32;
                let l1 = if x >= 1 { current[idx - 1] } else { current[idx] } as u32;
                let l2 = if x >= 2 { current[idx - 2] } else { current[idx] } as u32;
                let r1 = if x + 1 < w { current[idx + 1] } else { current[idx] } as u32;
                let r2 = if x + 2 < w { current[idx + 2] } else { current[idx] } as u32;
                sum += 4 * (l1 + r1) + l2 + r2;
                temp[idx] = (sum >> 4) as u8;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let idx = y * w + x;
                let mut sum = 6u32 * temp[idx] as u32;
                let u1 = if y >= 1 { temp[idx - w] } else { temp[idx] } as u32;
                let u2 = if y >= 2 { temp[idx - 2 * w] } else { temp[idx] } as u32;
                let d1 = if y + 1 < h { temp[idx + w] } else { temp[idx] } as u32;
                let d2 = if y + 2 < h { temp[idx + 2 * w] } else { temp[idx] } as u32;
                sum += 4 * (u1 + d1) + u2 + d2;
                current[idx] = (sum >> 4) as u8;
            }
        }
    }
    current
}

fn contrast_stretch(grey: &[u8]) -> Vec<u8> {
    if grey.is_empty() { return Vec::new(); }
    let (mut lo, mut hi) = (255u8, 0u8);
    for &p in grey { if p < lo { lo = p; } if p > hi { hi = p; } }
    let range = hi.saturating_sub(lo);
    if range == 0 { return grey.to_vec(); }
    grey.iter().map(|&p| {
        let v = ((p.saturating_sub(lo) as u32) * 255) / range as u32;
        v.min(255) as u8
    }).collect()
}

fn otsu_threshold(grey: &[u8]) -> u8 {
    let mut hist = [0u32; 256];
    for &p in grey { hist[p as usize] += 1; }
    let total = grey.len() as f64;
    let mut sum_all = 0.0f64;
    for (i, &count) in hist.iter().enumerate() { sum_all += i as f64 * count as f64; }
    let (mut best_t, mut best_var) = (0u8, 0.0f64);
    let (mut w_bg, mut sum_bg) = (0.0f64, 0.0f64);
    for t in 0..256 {
        w_bg += hist[t] as f64;
        if w_bg == 0.0 { continue; }
        let w_fg = total - w_bg;
        if w_fg == 0.0 { break; }
        sum_bg += t as f64 * hist[t] as f64;
        let mean_bg = sum_bg / w_bg;
        let mean_fg = (sum_all - sum_bg) / w_fg;
        let var = w_bg * w_fg * (mean_bg - mean_fg) * (mean_bg - mean_fg);
        if var > best_var { best_var = var; best_t = t as u8; }
    }
    best_t
}

fn clahe(grey: &[u8], w: usize, h: usize, tiles_x: usize, tiles_y: usize, clip_limit: f32) -> Vec<u8> {
    if w == 0 || h == 0 || tiles_x == 0 || tiles_y == 0 { return grey.to_vec(); }
    let tw = w / tiles_x;
    let th = h / tiles_y;
    if tw == 0 || th == 0 { return grey.to_vec(); }

    let mut maps = vec![[0u8; 256]; tiles_x * tiles_y];
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let (x0, y0) = (tx * tw, ty * th);
            let x1 = if tx == tiles_x - 1 { w } else { x0 + tw };
            let y1 = if ty == tiles_y - 1 { h } else { y0 + th };
            let tile_pixels = (x1 - x0) * (y1 - y0);
            let mut hist = [0u32; 256];
            for row in y0..y1 { for col in x0..x1 { hist[grey[row * w + col] as usize] += 1; } }
            let clip = (clip_limit * tile_pixels as f32 / 256.0) as u32;
            let clip = clip.max(1);
            let mut excess = 0u32;
            for bin in hist.iter_mut() { if *bin > clip { excess += *bin - clip; *bin = clip; } }
            let per_bin = excess / 256;
            let remainder = (excess % 256) as usize;
            for (i, bin) in hist.iter_mut().enumerate() { *bin += per_bin; if i < remainder { *bin += 1; } }
            let mut cdf = [0u32; 256];
            cdf[0] = hist[0];
            for i in 1..256 { cdf[i] = cdf[i - 1] + hist[i]; }
            let cdf_min = *cdf.iter().find(|&&v| v > 0).unwrap_or(&0);
            let denom = cdf[255].saturating_sub(cdf_min);
            let idx = ty * tiles_x + tx;
            for i in 0..256 {
                if denom == 0 { maps[idx][i] = i as u8; }
                else { maps[idx][i] = ((cdf[i].saturating_sub(cdf_min) as f32 / denom as f32) * 255.0).min(255.0) as u8; }
            }
        }
    }

    let mut result = vec![0u8; w * h];
    let (tw_f, th_f) = (tw as f32, th as f32);
    for y in 0..h {
        for x in 0..w {
            let pixel = grey[y * w + x] as usize;
            let fx = (x as f32 + 0.5) / tw_f - 0.5;
            let fy = (y as f32 + 0.5) / th_f - 0.5;
            let tx0 = (fx.floor() as i32).max(0).min(tiles_x as i32 - 1) as usize;
            let tx1 = (fx.floor() as i32 + 1).max(0).min(tiles_x as i32 - 1) as usize;
            let ty0 = (fy.floor() as i32).max(0).min(tiles_y as i32 - 1) as usize;
            let ty1 = (fy.floor() as i32 + 1).max(0).min(tiles_y as i32 - 1) as usize;
            let (ax, ay) = (fx - fx.floor(), fy - fy.floor());
            let v00 = maps[ty0 * tiles_x + tx0][pixel] as f32;
            let v10 = maps[ty0 * tiles_x + tx1][pixel] as f32;
            let v01 = maps[ty1 * tiles_x + tx0][pixel] as f32;
            let v11 = maps[ty1 * tiles_x + tx1][pixel] as f32;
            let top = v00 * (1.0 - ax) + v10 * ax;
            let bot = v01 * (1.0 - ax) + v11 * ax;
            result[y * w + x] = (top * (1.0 - ay) + bot * ay).round().min(255.0).max(0.0) as u8;
        }
    }
    result
}
