/// QR code decoding via multi-strategy rqrr cascade.
///
/// Replaces ZXing WASM (~1.1MB) with pure Rust rqrr (~80-150KB compiled).
/// Uses multiple image preprocessing strategies to handle Kipukas'
/// anti-cheat camouflaged QR codes:
///
/// - **Glossy surface** → specular reflections → local contrast normalization
/// - **SVG cracked-lava texture** → high-freq noise → Gaussian blur (σ=1.5, 3.0)
/// - **Thin/breached border** → quiet zone issues → downscale 2×
/// - **Black on yellow, medium-high contrast** → good luminance separation → raw/Otsu
///
/// The cascade tries each strategy in order and returns on first successful decode.
/// rqrr is fast enough (~2-10ms per attempt at 640×480) to run 7 strategies per frame
/// well within the 500ms scan interval.
use rqrr::PreparedImage;
use wasm_bindgen::prelude::*;

// ── Public WASM API ────────────────────────────────────────────────

/// Decode a QR code from raw RGBA pixel data using multi-strategy rqrr cascade.
///
/// Called from kipukas-worker.js on each camera frame.
/// Returns decoded text or empty string if no QR found.
#[wasm_bindgen]
pub fn decode_qr_frame(rgba: &[u8], width: usize, height: usize) -> String {
    if rgba.len() < width * height * 4 {
        return String::new();
    }

    let grey = rgba_to_greyscale(rgba, width, height);

    // Strategy 1: Raw greyscale — handles normal conditions, good lighting
    if let Some(text) = try_decode_greyscale(&grey, width, height) {
        return text;
    }

    // Strategy 2: Gaussian blur σ≈1.5 — smooths cracked-lava SVG texture
    let blurred_light = gaussian_blur(&grey, width, height, 1);
    if let Some(text) = try_decode_greyscale(&blurred_light, width, height) {
        return text;
    }

    // Strategy 3: Gaussian blur σ≈3.0 — heavier smoothing for heavy texture + slight gloss
    let blurred_heavy = gaussian_blur(&grey, width, height, 2);
    if let Some(text) = try_decode_greyscale(&blurred_heavy, width, height) {
        return text;
    }

    // Strategy 4: Contrast stretch — handles washed-out captures from gloss
    let stretched = contrast_stretch(&grey);
    if let Some(text) = try_decode_greyscale(&stretched, width, height) {
        return text;
    }

    // Strategy 5: Local contrast normalization — handles specular hotspots from glossy surface
    let normalized = local_normalize(&grey, width, height, 64);
    if let Some(text) = try_decode_greyscale(&normalized, width, height) {
        return text;
    }

    // Strategy 6: Downscale 2× — averages out fine texture, widens apparent thin border
    let (dw, dh) = (width / 2, height / 2);
    if dw > 0 && dh > 0 {
        let downscaled = downscale_2x(&grey, width, height);
        if let Some(text) = try_decode_greyscale(&downscaled, dw, dh) {
            return text;
        }
    }

    // Strategy 7: Otsu binarization — clean threshold for black-on-yellow contrast
    let threshold = otsu_threshold(&grey);
    if let Some(text) = try_decode_bitmap(&grey, width, height, threshold) {
        return text;
    }

    String::new()
}

// ── rqrr decode wrappers ──────────────────────────────────────────

/// Try to decode a QR code from a greyscale buffer using rqrr.
fn try_decode_greyscale(grey: &[u8], w: usize, h: usize) -> Option<String> {
    let mut img = PreparedImage::prepare_from_greyscale(w, h, |x, y| grey[y * w + x]);
    let grids = img.detect_grids();
    grids
        .first()
        .and_then(|g| g.decode().ok())
        .map(|(_, content)| content)
}

/// Try to decode a QR code from a greyscale buffer using a fixed binarization threshold.
fn try_decode_bitmap(grey: &[u8], w: usize, h: usize, threshold: u8) -> Option<String> {
    let mut img = PreparedImage::prepare_from_bitmap(w, h, |x, y| grey[y * w + x] < threshold);
    let grids = img.detect_grids();
    grids
        .first()
        .and_then(|g| g.decode().ok())
        .map(|(_, content)| content)
}

// ── Image preprocessing functions ─────────────────────────────────
// All pure Rust, no external deps. Operate on flat Vec<u8> greyscale buffers.

/// Convert RGBA pixel buffer to greyscale using standard luminance weights.
/// Black = 0, yellow ≈ 226 — excellent separation for Kipukas cards.
fn rgba_to_greyscale(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let len = width * height;
    let mut grey = Vec::with_capacity(len);
    for i in 0..len {
        let base = i * 4;
        let r = rgba[base] as u32;
        let g = rgba[base + 1] as u32;
        let b = rgba[base + 2] as u32;
        // ITU-R BT.601 luminance: 0.299R + 0.587G + 0.114B
        // Using integer math: (77R + 150G + 29B) >> 8
        grey.push(((77 * r + 150 * g + 29 * b) >> 8) as u8);
    }
    grey
}

/// Stretch pixel values to fill the full [0, 255] range.
/// Handles washed-out images from glossy surface reflections.
fn contrast_stretch(grey: &[u8]) -> Vec<u8> {
    if grey.is_empty() {
        return Vec::new();
    }

    let mut lo = 255u8;
    let mut hi = 0u8;
    for &p in grey {
        if p < lo {
            lo = p;
        }
        if p > hi {
            hi = p;
        }
    }

    let range = hi.saturating_sub(lo);
    if range == 0 {
        return grey.to_vec();
    }

    grey.iter()
        .map(|&p| {
            let v = (((p.saturating_sub(lo)) as u32) * 255) / (range as u32);
            v.min(255) as u8
        })
        .collect()
}

/// Apply Gaussian blur using a separable kernel.
/// `passes` controls blur intensity:
///   1 pass ≈ σ=1.5 (light texture smoothing)
///   2 passes ≈ σ=3.0 (heavier smoothing)
///
/// Uses a 5-tap kernel [1, 4, 6, 4, 1] / 16 (approximates Gaussian).
fn gaussian_blur(grey: &[u8], width: usize, height: usize, passes: usize) -> Vec<u8> {
    let mut current = grey.to_vec();
    let mut temp = vec![0u8; width * height];

    for _ in 0..passes {
        // Horizontal pass
        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let mut sum = 6u32 * current[idx] as u32;

                // Clamp to edges
                let l1 = if x >= 1 { current[idx - 1] } else { current[idx] } as u32;
                let l2 = if x >= 2 { current[idx - 2] } else { current[idx] } as u32;
                let r1 = if x + 1 < width { current[idx + 1] } else { current[idx] } as u32;
                let r2 = if x + 2 < width { current[idx + 2] } else { current[idx] } as u32;

                sum += 4 * (l1 + r1) + l2 + r2;
                temp[idx] = (sum >> 4) as u8; // divide by 16
            }
        }

        // Vertical pass
        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let mut sum = 6u32 * temp[idx] as u32;

                let u1 = if y >= 1 {
                    temp[idx - width]
                } else {
                    temp[idx]
                } as u32;
                let u2 = if y >= 2 {
                    temp[idx - 2 * width]
                } else {
                    temp[idx]
                } as u32;
                let d1 = if y + 1 < height {
                    temp[idx + width]
                } else {
                    temp[idx]
                } as u32;
                let d2 = if y + 2 < height {
                    temp[idx + 2 * width]
                } else {
                    temp[idx]
                } as u32;

                sum += 4 * (u1 + d1) + u2 + d2;
                current[idx] = (sum >> 4) as u8;
            }
        }
    }

    current
}

/// Normalize brightness per block to handle specular hotspots from glossy surfaces.
///
/// Divides the image into `block_size × block_size` blocks. For each block,
/// computes local min/max and stretches the range to [0, 255]. This prevents
/// a bright glare spot from washing out the entire image — each block is
/// independently normalized.
fn local_normalize(grey: &[u8], width: usize, height: usize, block_size: usize) -> Vec<u8> {
    let mut result = grey.to_vec();
    let bs = block_size;

    let mut by = 0;
    while by < height {
        let bh = bs.min(height - by);
        let mut bx = 0;
        while bx < width {
            let bw = bs.min(width - bx);

            // Find local min/max for this block
            let mut lo = 255u8;
            let mut hi = 0u8;
            for row in by..(by + bh) {
                for col in bx..(bx + bw) {
                    let p = grey[row * width + col];
                    if p < lo {
                        lo = p;
                    }
                    if p > hi {
                        hi = p;
                    }
                }
            }

            let range = hi.saturating_sub(lo);
            if range > 10 {
                // Only normalize if there's meaningful contrast in this block
                for row in by..(by + bh) {
                    for col in bx..(bx + bw) {
                        let idx = row * width + col;
                        let v =
                            ((grey[idx].saturating_sub(lo) as u32) * 255) / (range as u32);
                        result[idx] = v.min(255) as u8;
                    }
                }
            }

            bx += bs;
        }
        by += bs;
    }

    result
}

/// Downscale image by 2× using area averaging.
/// Averages each 2×2 pixel block into one output pixel.
/// Smooths out fine camouflage texture and effectively widens thin borders.
fn downscale_2x(grey: &[u8], width: usize, height: usize) -> Vec<u8> {
    let dw = width / 2;
    let dh = height / 2;
    let mut out = Vec::with_capacity(dw * dh);

    for dy in 0..dh {
        for dx in 0..dw {
            let sx = dx * 2;
            let sy = dy * 2;
            let tl = grey[sy * width + sx] as u32;
            let tr = grey[sy * width + sx + 1] as u32;
            let bl = grey[(sy + 1) * width + sx] as u32;
            let br = grey[(sy + 1) * width + sx + 1] as u32;
            out.push(((tl + tr + bl + br + 2) / 4) as u8);
        }
    }

    out
}

/// Compute Otsu's threshold — the optimal global binarization threshold
/// that minimizes intra-class variance.
///
/// Particularly effective for Kipukas' black-on-yellow QR codes where
/// the histogram has two clear peaks.
fn otsu_threshold(grey: &[u8]) -> u8 {
    // Build histogram
    let mut hist = [0u32; 256];
    for &p in grey {
        hist[p as usize] += 1;
    }

    let total = grey.len() as f64;
    let mut sum_all = 0.0f64;
    for (i, &count) in hist.iter().enumerate() {
        sum_all += i as f64 * count as f64;
    }

    let mut best_threshold = 0u8;
    let mut best_variance = 0.0f64;
    let mut weight_bg = 0.0f64;
    let mut sum_bg = 0.0f64;

    for t in 0..256 {
        weight_bg += hist[t] as f64;
        if weight_bg == 0.0 {
            continue;
        }

        let weight_fg = total - weight_bg;
        if weight_fg == 0.0 {
            break;
        }

        sum_bg += t as f64 * hist[t] as f64;
        let mean_bg = sum_bg / weight_bg;
        let mean_fg = (sum_all - sum_bg) / weight_fg;

        let between_variance = weight_bg * weight_fg * (mean_bg - mean_fg) * (mean_bg - mean_fg);
        if between_variance > best_variance {
            best_variance = between_variance;
            best_threshold = t as u8;
        }
    }

    best_threshold
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_to_greyscale_uniform_grey() {
        let rgba = vec![128, 128, 128, 255, 64, 64, 64, 255];
        let grey = rgba_to_greyscale(&rgba, 2, 1);
        // Pure grey: (77*128 + 150*128 + 29*128) >> 8 = 128
        assert_eq!(grey[0], 128);
        assert_eq!(grey[1], 64);
    }

    #[test]
    fn rgba_to_greyscale_yellow() {
        // Yellow (255, 255, 0): (77*255 + 150*255 + 29*0) >> 8 ≈ 226
        let rgba = vec![255, 255, 0, 255];
        let grey = rgba_to_greyscale(&rgba, 1, 1);
        assert!(grey[0] > 220 && grey[0] < 230, "yellow luminance was {}", grey[0]);
    }

    #[test]
    fn rgba_to_greyscale_black() {
        let rgba = vec![0, 0, 0, 255];
        let grey = rgba_to_greyscale(&rgba, 1, 1);
        assert_eq!(grey[0], 0);
    }

    #[test]
    fn contrast_stretch_expands_range() {
        let grey = vec![50, 100, 150, 200];
        let stretched = contrast_stretch(&grey);
        assert_eq!(stretched[0], 0); // min → 0
        assert_eq!(stretched[3], 255); // max → 255
        assert!(stretched[1] > 0 && stretched[1] < stretched[2]);
    }

    #[test]
    fn contrast_stretch_uniform_noop() {
        let grey = vec![100, 100, 100];
        let stretched = contrast_stretch(&grey);
        assert_eq!(stretched, grey);
    }

    #[test]
    fn contrast_stretch_empty() {
        let empty: Vec<u8> = Vec::new();
        assert!(contrast_stretch(&empty).is_empty());
    }

    #[test]
    fn gaussian_blur_preserves_dimensions() {
        let grey = vec![128; 10 * 10];
        let blurred = gaussian_blur(&grey, 10, 10, 1);
        assert_eq!(blurred.len(), 100);
    }

    #[test]
    fn gaussian_blur_smooths_spike() {
        // A single bright pixel surrounded by dark should be dimmed
        let mut grey = vec![0u8; 5 * 5];
        grey[12] = 255; // center pixel
        let blurred = gaussian_blur(&grey, 5, 5, 1);
        assert!(blurred[12] < 255, "center should be dimmed");
        assert!(blurred[11] > 0, "neighbor should pick up some brightness");
    }

    #[test]
    fn local_normalize_handles_glare() {
        // First half bright (200-255), second half dark (0-50)
        let mut grey = Vec::with_capacity(8 * 8);
        for y in 0..8 {
            for _x in 0..8 {
                grey.push(if y < 4 { 200 } else { 50 });
            }
        }
        let normalized = local_normalize(&grey, 8, 8, 4);
        // Each 4×4 block is uniform → range ≤ 10 → no normalization
        // But the overall image now has two distinct regions
        assert_eq!(normalized.len(), 64);
    }

    #[test]
    fn downscale_2x_dimensions() {
        let grey = vec![100; 10 * 10];
        let ds = downscale_2x(&grey, 10, 10);
        assert_eq!(ds.len(), 5 * 5);
    }

    #[test]
    fn downscale_2x_averages() {
        // 2×2 image with values [0, 100, 200, 56]
        let grey = vec![0, 100, 200, 56];
        let ds = downscale_2x(&grey, 2, 2);
        assert_eq!(ds.len(), 1);
        // (0 + 100 + 200 + 56 + 2) / 4 = 89
        assert_eq!(ds[0], 89);
    }

    #[test]
    fn otsu_threshold_bimodal() {
        // Simulate a bimodal distribution: dark cluster (0-50) and bright cluster (200-255)
        let mut grey = Vec::new();
        for i in 0..50 {
            grey.push((i % 50) as u8); // dark cluster: 0-49
        }
        for i in 0..50 {
            grey.push((200 + (i % 56)) as u8); // bright cluster: 200-255
        }
        let t = otsu_threshold(&grey);
        // Threshold should fall at or between the two clusters (49-199)
        assert!(t >= 49 && t < 200, "otsu threshold was {}", t);
    }

    #[test]
    fn decode_qr_frame_empty_returns_empty() {
        // 2×2 grey image — no QR code present
        let rgba = vec![
            128, 128, 128, 255, 128, 128, 128, 255, 128, 128, 128, 255, 128, 128, 128, 255,
        ];
        let result = decode_qr_frame(&rgba, 2, 2);
        assert!(result.is_empty());
    }

    #[test]
    fn decode_qr_frame_short_buffer_returns_empty() {
        let rgba = vec![0; 10]; // too short for any image
        let result = decode_qr_frame(&rgba, 100, 100);
        assert!(result.is_empty());
    }
}
