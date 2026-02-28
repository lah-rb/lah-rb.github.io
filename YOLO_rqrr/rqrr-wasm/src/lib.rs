/// rqrr-decode — QR decode stage for the YOLO+rqrr two-stage pipeline.
///
/// Stripped to adaptive threshold only — the only preprocessing strategy
/// that consistently wins on Kipukas camouflaged QR codes. All other
/// strategies (CLAHE, Otsu, contrast stretch, etc.) never outperform it
/// in production testing, so they've been removed to minimize latency.
///
/// Pipeline: YOLO crop → greyscale → quiet zone → adaptive threshold → rqrr decode
use rqrr::PreparedImage;
use wasm_bindgen::prelude::*;

// ── Public WASM API ────────────────────────────────────────────────

/// Decode a QR code from a cropped RGBA region provided by YOLO detection.
///
/// Applies adaptive threshold with three block-size variants (medium, fine,
/// coarse), first on a quiet-zone-padded version, then on the raw crop.
/// Returns "strategyIdx|strategyName|decodedText" on success, or empty string.
#[wasm_bindgen]
pub fn decode_qr_crop(rgba: &[u8], width: usize, height: usize) -> String {
    if rgba.len() < width * height * 4 || width == 0 || height == 0 {
        return String::new();
    }

    let grey = rgba_to_greyscale(rgba, width, height);

    // Quiet zone padding — YOLO crops may clip QR edges
    let pad = (width.min(height) / 10).max(4);
    let (padded, pw, ph) = add_quiet_zone(&grey, width, height, pad);

    // Adaptive threshold variants (block_size, constant) — ordered by win rate
    const VARIANTS: [(usize, i32, &str); 3] = [
        (15, 8, "at_15_8"),
        (11, 6, "at_11_6"),
        (21, 10, "at_21_10"),
    ];

    // Try on padded crop first (handles clipped QR edges)
    for (i, &(block, c, name)) in VARIANTS.iter().enumerate() {
        let thresh = adaptive_threshold(&padded, pw, ph, block, c);
        if let Some(text) = try_decode(&thresh, pw, ph) {
            return format!("{}|{}|{}", i, name, text);
        }
    }

    // Try on raw crop (no padding)
    for (i, &(block, c, name)) in VARIANTS.iter().enumerate() {
        let idx = i + 3;
        let thresh = adaptive_threshold(&grey, width, height, block, c);
        if let Some(text) = try_decode(&thresh, width, height) {
            return format!("{}|raw_{}|{}", idx, name, text);
        }
    }

    String::new()
}

// ── rqrr decode ───────────────────────────────────────────────────

fn try_decode(grey: &[u8], w: usize, h: usize) -> Option<String> {
    let mut img = PreparedImage::prepare_from_greyscale(w, h, |x, y| grey[y * w + x]);
    let grids = img.detect_grids();
    grids.first().and_then(|g| g.decode().ok()).map(|(_, content)| content)
}

// ── Image preprocessing ───────────────────────────────────────────

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
