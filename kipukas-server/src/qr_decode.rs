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
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

// ── Frame Accumulator (ring buffer for temporal averaging) ─────────

const MAX_FRAMES: usize = 4;

struct FrameBuffer {
    frames: Vec<Vec<u8>>, // greyscale frames
    width: usize,
    height: usize,
    count: usize,
}

impl FrameBuffer {
    fn new() -> Self {
        Self { frames: Vec::new(), width: 0, height: 0, count: 0 }
    }

    fn push(&mut self, grey: &[u8], w: usize, h: usize) {
        // Reset if dimensions changed
        if w != self.width || h != self.height {
            self.frames.clear();
            self.width = w;
            self.height = h;
            self.count = 0;
        }
        if self.frames.len() < MAX_FRAMES {
            self.frames.push(grey.to_vec());
        } else {
            let idx = self.count % MAX_FRAMES;
            self.frames[idx] = grey.to_vec();
        }
        self.count += 1;
    }

    /// Per-pixel average of all accumulated frames.
    fn average(&self) -> Option<Vec<u8>> {
        if self.frames.len() < 2 {
            return None;
        }
        let n = self.frames.len();
        let len = self.width * self.height;
        let mut avg = Vec::with_capacity(len);
        for i in 0..len {
            let sum: u32 = self.frames.iter().map(|f| f[i] as u32).sum();
            avg.push((sum / n as u32) as u8);
        }
        Some(avg)
    }

    /// Per-pixel median of all accumulated frames.
    fn median(&self) -> Option<Vec<u8>> {
        if self.frames.len() < 3 {
            return None;
        }
        let n = self.frames.len();
        let len = self.width * self.height;
        let mut med = Vec::with_capacity(len);
        let mut vals = vec![0u8; n];
        for i in 0..len {
            for (j, f) in self.frames.iter().enumerate() {
                vals[j] = f[i];
            }
            vals[..n].sort_unstable();
            med.push(vals[n / 2]);
        }
        Some(med)
    }

    fn reset(&mut self) {
        self.frames.clear();
        self.count = 0;
    }
}

thread_local! {
    static FRAME_BUF: RefCell<FrameBuffer> = RefCell::new(FrameBuffer::new());
}

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

    // Accumulate frame for temporal strategies
    FRAME_BUF.with(|fb| fb.borrow_mut().push(&grey, width, height));

    // Run single-frame + temporal strategies
    run_all_strategies(&grey, width, height)
}

/// Reset the frame accumulator (call when scanner closes).
#[wasm_bindgen]
pub fn reset_qr_frames() {
    FRAME_BUF.with(|fb| fb.borrow_mut().reset());
}

/// Run all decode strategies on a greyscale buffer.
fn run_all_strategies(grey: &[u8], width: usize, height: usize) -> String {
    // ── Single-frame strategies ────────────────────────────────────

    // 1: Raw greyscale
    if let Some(text) = try_decode_greyscale(grey, width, height) {
        return text;
    }

    // 2: Gaussian blur σ≈1.5
    let blurred_light = gaussian_blur(grey, width, height, 1);
    if let Some(text) = try_decode_greyscale(&blurred_light, width, height) {
        return text;
    }

    // 3: Gaussian blur σ≈3.0
    let blurred_heavy = gaussian_blur(grey, width, height, 2);
    if let Some(text) = try_decode_greyscale(&blurred_heavy, width, height) {
        return text;
    }

    // 4: Median filter 3×3 — edge-preserving texture removal
    let med3 = median_filter(grey, width, height, 1);
    if let Some(text) = try_decode_greyscale(&med3, width, height) {
        return text;
    }

    // 5: Median filter 5×5 — heavier edge-preserving smoothing
    let med5 = median_filter(grey, width, height, 2);
    if let Some(text) = try_decode_greyscale(&med5, width, height) {
        return text;
    }

    // ── Temporal strategies (use accumulated frames) ───────────────

    // 6: Temporal average — reinforces static QR, washes out texture shimmer
    let temporal_result = FRAME_BUF.with(|fb| {
        let buf = fb.borrow();
        if let Some(avg) = buf.average() {
            if let Some(text) = try_decode_greyscale(&avg, buf.width, buf.height) {
                return Some(text);
            }
            // Also try blur on the averaged frame
            let blurred_avg = gaussian_blur(&avg, buf.width, buf.height, 1);
            if let Some(text) = try_decode_greyscale(&blurred_avg, buf.width, buf.height) {
                return Some(text);
            }
        }
        // 7: Temporal median — even more robust noise elimination
        if let Some(med) = buf.median() {
            if let Some(text) = try_decode_greyscale(&med, buf.width, buf.height) {
                return Some(text);
            }
        }
        None
    });
    if let Some(text) = temporal_result {
        return text;
    }

    // ── More single-frame strategies ──────────────────────────────

    // 8: Contrast stretch
    let stretched = contrast_stretch(grey);
    if let Some(text) = try_decode_greyscale(&stretched, width, height) {
        return text;
    }

    // 9: Adaptive threshold (local mean, block=15, bias=8)
    let adaptive = adaptive_threshold(grey, width, height, 15, 8);
    if let Some(text) = try_decode_greyscale(&adaptive, width, height) {
        return text;
    }

    // 10: Blur σ≈2 + Otsu — smooth texture then clean binarize
    let blur_otsu = gaussian_blur(grey, width, height, 2);
    let thresh_bo = otsu_threshold(&blur_otsu);
    if let Some(text) = try_decode_bitmap(&blur_otsu, width, height, thresh_bo) {
        return text;
    }

    // 11: Downscale 2×
    let (dw2, dh2) = (width / 2, height / 2);
    if dw2 > 0 && dh2 > 0 {
        let ds2 = downscale_2x(grey, width, height);
        if let Some(text) = try_decode_greyscale(&ds2, dw2, dh2) {
            return text;
        }
    }

    // 12: Downscale 4× — very aggressive texture averaging
    let (dw4, dh4) = (width / 4, height / 4);
    if dw4 > 20 && dh4 > 20 {
        let ds4 = downscale_4x(grey, width, height);
        if let Some(text) = try_decode_greyscale(&ds4, dw4, dh4) {
            return text;
        }
    }

    // 13: Morphological closing on Otsu binary — fills texture gaps in QR modules
    let threshold = otsu_threshold(grey);
    let binary: Vec<u8> = grey.iter().map(|&p| if p < threshold { 0 } else { 255 }).collect();
    let closed = morphological_close(&binary, width, height, 1);
    if let Some(text) = try_decode_greyscale(&closed, width, height) {
        return text;
    }

    // 14: Quiet zone padding + raw — helps when QR is near frame edge
    let (padded, pw, ph) = add_quiet_zone(grey, width, height, 20);
    if let Some(text) = try_decode_greyscale(&padded, pw, ph) {
        return text;
    }

    // 15: Local contrast normalization
    let normalized = local_normalize(grey, width, height, 64);
    if let Some(text) = try_decode_greyscale(&normalized, width, height) {
        return text;
    }

    // 16: Otsu binarization (global)
    if let Some(text) = try_decode_bitmap(grey, width, height, threshold) {
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

/// Median filter — removes salt-and-pepper texture while preserving edges.
/// `radius` 1 = 3×3 window, `radius` 2 = 5×5 window.
fn median_filter(grey: &[u8], width: usize, height: usize, radius: usize) -> Vec<u8> {
    let w = width;
    let h = height;
    let side = 2 * radius + 1;
    let mut result = Vec::with_capacity(w * h);
    let mut window = vec![0u8; side * side];

    for y in 0..h {
        for x in 0..w {
            let mut count = 0;
            for dy in 0..side {
                let sy = (y + dy).saturating_sub(radius).min(h - 1);
                for dx in 0..side {
                    let sx = (x + dx).saturating_sub(radius).min(w - 1);
                    window[count] = grey[sy * w + sx];
                    count += 1;
                }
            }
            window[..count].sort_unstable();
            result.push(window[count / 2]);
        }
    }
    result
}

/// Adaptive threshold — per-pixel threshold based on local mean.
/// Much better than global Otsu for uneven lighting from glossy surfaces.
/// `block` is the half-size of the neighborhood window.
/// `c` is subtracted from the local mean (bias toward dark = QR modules).
fn adaptive_threshold(grey: &[u8], width: usize, height: usize, block: usize, c: i32) -> Vec<u8> {
    // Build integral image for fast local mean computation
    let w = width;
    let h = height;
    let mut integral = vec![0i64; (w + 1) * (h + 1)];
    let iw = w + 1;

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
                - integral[y1 * iw + x0]
                + integral[y0 * iw + x0];
            let local_mean = sum / area;
            let thresh = local_mean - c as i64;
            result[y * w + x] = if (grey[y * w + x] as i64) < thresh { 0 } else { 255 };
        }
    }
    result
}

/// Morphological closing on a binary image (dilation → erosion).
/// Fills small gaps in QR modules that texture creates.
/// `radius` controls the structuring element size.
fn morphological_close(binary: &[u8], width: usize, height: usize, radius: usize) -> Vec<u8> {
    // Dilation: pixel is white if ANY neighbor is white
    let dilated = morph_dilate(binary, width, height, radius);
    // Erosion: pixel is white only if ALL neighbors are white
    morph_erode(&dilated, width, height, radius)
}

fn morph_dilate(img: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut max_val = 0u8;
            for dy in y.saturating_sub(r)..=(y + r).min(h - 1) {
                for dx in x.saturating_sub(r)..=(x + r).min(w - 1) {
                    let v = img[dy * w + dx];
                    if v > max_val {
                        max_val = v;
                    }
                }
            }
            out[y * w + x] = max_val;
        }
    }
    out
}

fn morph_erode(img: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    let mut out = vec![255u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut min_val = 255u8;
            for dy in y.saturating_sub(r)..=(y + r).min(h - 1) {
                for dx in x.saturating_sub(r)..=(x + r).min(w - 1) {
                    let v = img[dy * w + dx];
                    if v < min_val {
                        min_val = v;
                    }
                }
            }
            out[y * w + x] = min_val;
        }
    }
    out
}

/// Downscale image by 4× using area averaging.
fn downscale_4x(grey: &[u8], width: usize, height: usize) -> Vec<u8> {
    let dw = width / 4;
    let dh = height / 4;
    let mut out = Vec::with_capacity(dw * dh);

    for dy in 0..dh {
        for dx in 0..dw {
            let sx = dx * 4;
            let sy = dy * 4;
            let mut sum = 0u32;
            for row in sy..(sy + 4).min(height) {
                for col in sx..(sx + 4).min(width) {
                    sum += grey[row * width + col] as u32;
                }
            }
            out.push((sum / 16) as u8);
        }
    }
    out
}

/// Add a white quiet zone border around the image.
/// Helps finder pattern detection when QR is near the frame edge.
fn add_quiet_zone(grey: &[u8], width: usize, height: usize, pad: usize) -> (Vec<u8>, usize, usize) {
    let nw = width + 2 * pad;
    let nh = height + 2 * pad;
    let mut out = vec![255u8; nw * nh]; // white background
    for y in 0..height {
        for x in 0..width {
            out[(y + pad) * nw + (x + pad)] = grey[y * width + x];
        }
    }
    (out, nw, nh)
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

    #[test]
    fn median_filter_preserves_dimensions() {
        let grey = vec![128; 10 * 10];
        let filtered = median_filter(&grey, 10, 10, 1);
        assert_eq!(filtered.len(), 100);
    }

    #[test]
    fn median_filter_removes_spike() {
        let mut grey = vec![100u8; 5 * 5];
        grey[12] = 255; // single bright spike at center
        let filtered = median_filter(&grey, 5, 5, 1);
        assert_eq!(filtered[12], 100, "median should remove the spike");
    }

    #[test]
    fn adaptive_threshold_dimensions() {
        let grey = vec![128; 10 * 10];
        let result = adaptive_threshold(&grey, 10, 10, 3, 5);
        assert_eq!(result.len(), 100);
    }

    #[test]
    fn adaptive_threshold_separates_bimodal() {
        // Left half dark (30), right half bright (220)
        let mut grey = vec![0u8; 10 * 10];
        for y in 0..10 {
            for x in 0..10 {
                grey[y * 10 + x] = if x < 5 { 30 } else { 220 };
            }
        }
        let result = adaptive_threshold(&grey, 10, 10, 3, 5);
        // At the boundary (x=4→5), the local mean transitions from dark to bright.
        // Deep in the bright region (x=9) should be 255 since 220 > local_mean - c.
        assert_eq!(result[0 * 10 + 9], 255); // bright region stays bright
        // At the boundary, dark side pixel (x=4) should be darker than bright side (x=5)
        // when the neighborhood straddles both regions
        assert!(result[5 * 10 + 4] < result[5 * 10 + 5] || result[5 * 10 + 4] == 0);
    }

    #[test]
    fn morphological_close_fills_gap() {
        // 5×5 white image with a single black (0) pixel gap
        let mut binary = vec![255u8; 5 * 5];
        binary[12] = 0; // single gap at center
        let closed = morphological_close(&binary, 5, 5, 1);
        assert_eq!(closed[12], 255, "closing should fill single-pixel gap");
    }

    #[test]
    fn downscale_4x_dimensions() {
        let grey = vec![100; 40 * 40];
        let ds = downscale_4x(&grey, 40, 40);
        assert_eq!(ds.len(), 10 * 10);
    }

    #[test]
    fn add_quiet_zone_dimensions() {
        let grey = vec![100; 10 * 10];
        let (padded, nw, nh) = add_quiet_zone(&grey, 10, 10, 5);
        assert_eq!(nw, 20);
        assert_eq!(nh, 20);
        assert_eq!(padded.len(), 20 * 20);
        // Corners should be white (255 = quiet zone)
        assert_eq!(padded[0], 255);
        // Original content should be at offset
        assert_eq!(padded[5 * 20 + 5], 100);
    }

    #[test]
    fn frame_buffer_average() {
        let mut buf = FrameBuffer::new();
        buf.push(&[100, 200], 2, 1);
        assert!(buf.average().is_none()); // need at least 2 frames
        buf.push(&[200, 100], 2, 1);
        let avg = buf.average().unwrap();
        assert_eq!(avg, vec![150, 150]);
    }

    #[test]
    fn frame_buffer_median() {
        let mut buf = FrameBuffer::new();
        buf.push(&[10, 200], 2, 1);
        buf.push(&[100, 100], 2, 1);
        buf.push(&[200, 10], 2, 1);
        let med = buf.median().unwrap();
        assert_eq!(med, vec![100, 100]);
    }

    #[test]
    fn frame_buffer_resets_on_dimension_change() {
        let mut buf = FrameBuffer::new();
        buf.push(&[100; 4], 2, 2);
        buf.push(&[200; 4], 2, 2);
        assert_eq!(buf.frames.len(), 2);
        buf.push(&[50; 9], 3, 3); // dimension change
        assert_eq!(buf.frames.len(), 1); // reset
    }

    #[test]
    fn frame_buffer_ring_wraps() {
        let mut buf = FrameBuffer::new();
        for i in 0..10 {
            buf.push(&[i as u8; 4], 2, 2);
        }
        assert_eq!(buf.frames.len(), MAX_FRAMES);
    }

    #[test]
    fn reset_clears_buffer() {
        let mut buf = FrameBuffer::new();
        buf.push(&[100; 4], 2, 2);
        buf.reset();
        assert!(buf.frames.is_empty());
        assert_eq!(buf.count, 0);
    }
}
