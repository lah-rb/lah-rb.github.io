/// This is from the failed experimental trial for historical purposes only. 
/// The scanner was able to detect our codes put only with adaptive_thresh + perfect camera alignment.
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

// ── Strategy Constants ─────────────────────────────────────────────

const STRAT_RAW: u8 = 0;
const STRAT_YELLOW: u8 = 1;
const STRAT_YELLOW_BLUR: u8 = 2;
const STRAT_BLUR_LIGHT: u8 = 3;
const STRAT_BLUR_HEAVY: u8 = 4;
const STRAT_MED3: u8 = 5;
const STRAT_MED5: u8 = 6;
const STRAT_TEMPORAL_AVG: u8 = 7;
const STRAT_TEMPORAL_AVG_BLUR: u8 = 8;
const STRAT_TEMPORAL_MED: u8 = 9;
const STRAT_CLAHE_2: u8 = 10;
const STRAT_CLAHE_3: u8 = 11;
const STRAT_CLAHE_BLUR: u8 = 12;
const STRAT_CONTRAST_STRETCH: u8 = 13;
const STRAT_ADAPTIVE_THRESH: u8 = 14;
const STRAT_BLUR_OTSU: u8 = 15;
const STRAT_DOWNSCALE_2X: u8 = 16;
const STRAT_DOWNSCALE_4X: u8 = 17;
const STRAT_MORPH_CLOSE: u8 = 18;
const STRAT_QUIET_ZONE: u8 = 19;
const STRAT_LOCAL_NORM: u8 = 20;
const STRAT_OTSU: u8 = 21;
// Phase A: Tilt compensation — adaptive_thresh variants
const STRAT_AT_FINE: u8 = 22;
const STRAT_AT_COARSE: u8 = 23;
const STRAT_AT_STRETCH_X10: u8 = 24;
const STRAT_AT_STRETCH_X20: u8 = 25;
const STRAT_AT_STRETCH_Y10: u8 = 26;
const STRAT_AT_STRETCH_Y20: u8 = 27;

const NUM_STRATEGIES: usize = 28;

const STRATEGY_NAMES: [&str; NUM_STRATEGIES] = [
    "raw",
    "yellow",
    "yellow_blur",
    "blur_light",
    "blur_heavy",
    "med3x3",
    "med5x5",
    "temporal_avg",
    "temporal_avg_blur",
    "temporal_med",
    "clahe_2",
    "clahe_3",
    "clahe_blur",
    "contrast_stretch",
    "adaptive_thresh",
    "blur_otsu",
    "downscale_2x",
    "downscale_4x",
    "morph_close",
    "quiet_zone",
    "local_norm",
    "otsu",
    "at_fine",
    "at_coarse",
    "at_stretch_x10",
    "at_stretch_x20",
    "at_stretch_y10",
    "at_stretch_y20",
];

/// Default order: adaptive_thresh family first (proven winners for Kipukas
/// camouflaged cards), then remaining strategies for generality.
const DEFAULT_ORDER: [u8; NUM_STRATEGIES] = [
    // Tier 1: adaptive_thresh family — addresses tilt, distance, and glossy surface
    STRAT_ADAPTIVE_THRESH,
    STRAT_AT_FINE,
    STRAT_AT_COARSE,
    STRAT_AT_STRETCH_X10,
    STRAT_AT_STRETCH_X20,
    STRAT_AT_STRETCH_Y10,
    STRAT_AT_STRETCH_Y20,
    // Tier 2: other strategies that may help in edge cases
    STRAT_RAW,
    STRAT_YELLOW,
    STRAT_YELLOW_BLUR,
    STRAT_BLUR_LIGHT,
    STRAT_BLUR_HEAVY,
    STRAT_MED3,
    STRAT_MED5,
    STRAT_TEMPORAL_AVG,
    STRAT_TEMPORAL_AVG_BLUR,
    STRAT_TEMPORAL_MED,
    STRAT_CLAHE_2,
    STRAT_CLAHE_3,
    STRAT_CLAHE_BLUR,
    STRAT_CONTRAST_STRETCH,
    STRAT_BLUR_OTSU,
    STRAT_DOWNSCALE_2X,
    STRAT_DOWNSCALE_4X,
    STRAT_MORPH_CLOSE,
    STRAT_QUIET_ZONE,
    STRAT_LOCAL_NORM,
    STRAT_OTSU,
];

/// Number of successful decodes between automatic strategy reordering.
const AUTO_REORDER_INTERVAL: u32 = 5;

// ── Decode Stats & Strategy Config ─────────────────────────────────

struct DecodeStats {
    hits: [u32; NUM_STRATEGIES],
    total_decodes: u32,
}

impl DecodeStats {
    fn new() -> Self {
        Self {
            hits: [0; NUM_STRATEGIES],
            total_decodes: 0,
        }
    }
}

struct StrategyConfig {
    order: Vec<u8>,
}

impl StrategyConfig {
    fn new() -> Self {
        Self {
            order: DEFAULT_ORDER.to_vec(),
        }
    }
}

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
    static DECODE_STATS: RefCell<DecodeStats> = RefCell::new(DecodeStats::new());
    static STRATEGY_CONFIG: RefCell<StrategyConfig> = RefCell::new(StrategyConfig::new());
}

// ── Public WASM API ────────────────────────────────────────────────

/// Decode a QR code from raw RGBA pixel data using multi-strategy rqrr cascade.
///
/// Called from kipukas-worker.js on each camera frame.
/// Returns `"strategy_id|strategy_name|decoded_text"` on success, or empty
/// string if no QR found. The pipe-delimited format lets JS extract telemetry
/// without adding serde to the WASM boundary.
#[wasm_bindgen]
pub fn decode_qr_frame(rgba: &[u8], width: usize, height: usize) -> String {
    if rgba.len() < width * height * 4 {
        return String::new();
    }

    let grey = rgba_to_greyscale(rgba, width, height);
    let yellow = rgba_to_yellow_aware(rgba, width, height);

    // Accumulate frame for temporal strategies
    FRAME_BUF.with(|fb| fb.borrow_mut().push(&grey, width, height));

    // Run configurable strategy cascade
    if let Some((id, text)) = run_strategies(rgba, &grey, &yellow, width, height) {
        let name = if (id as usize) < NUM_STRATEGIES {
            STRATEGY_NAMES[id as usize]
        } else {
            "unknown"
        };
        format!("{}|{}|{}", id, name, text)
    } else {
        String::new()
    }
}

/// Reset the frame accumulator (call when scanner closes).
#[wasm_bindgen]
pub fn reset_qr_frames() {
    FRAME_BUF.with(|fb| fb.borrow_mut().reset());
}

/// Return JSON with per-strategy hit counts and total decodes.
/// Example: `{"total":42,"strategies":{"raw":5,"yellow":12,"clahe_2":8}}`
#[wasm_bindgen]
pub fn get_qr_stats() -> String {
    DECODE_STATS.with(|s| {
        let stats = s.borrow();
        let mut entries = Vec::new();
        for (i, &count) in stats.hits.iter().enumerate() {
            if count > 0 {
                entries.push(format!("\"{}\":{}", STRATEGY_NAMES[i], count));
            }
        }
        format!(
            "{{\"total\":{},\"strategies\":{{{}}}}}",
            stats.total_decodes,
            entries.join(",")
        )
    })
}

/// Set the strategy execution order. `order_csv` is a comma-separated list
/// of strategy IDs (e.g. `"1,0,10,3"`). Strategies not listed are appended
/// in default order. Invalid IDs are skipped.
#[wasm_bindgen]
pub fn set_qr_strategy_order(order_csv: &str) {
    let mut order: Vec<u8> = order_csv
        .split(',')
        .filter_map(|s| s.trim().parse::<u8>().ok())
        .filter(|&id| (id as usize) < NUM_STRATEGIES)
        .collect();
    // Append any missing strategies in default order
    for &id in &DEFAULT_ORDER {
        if !order.contains(&id) {
            order.push(id);
        }
    }
    STRATEGY_CONFIG.with(|c| c.borrow_mut().order = order);
}

/// Reset strategy order to default and clear stats.
#[wasm_bindgen]
pub fn reset_qr_strategy_order() {
    STRATEGY_CONFIG.with(|c| c.borrow_mut().order = DEFAULT_ORDER.to_vec());
    DECODE_STATS.with(|s| *s.borrow_mut() = DecodeStats::new());
}

// ── Strategy Execution Engine ──────────────────────────────────────

/// Run the configurable strategy cascade. Returns `(strategy_id, decoded_text)`
/// on first successful decode, or `None` if all strategies fail.
fn run_strategies(
    rgba: &[u8],
    grey: &[u8],
    yellow: &[u8],
    w: usize,
    h: usize,
) -> Option<(u8, String)> {
    let order = STRATEGY_CONFIG.with(|c| c.borrow().order.clone());

    for &id in &order {
        if let Some(text) = execute_strategy(id, rgba, grey, yellow, w, h) {
            record_hit(id);
            return Some((id, text));
        }
    }
    None
}

/// Execute a single strategy by ID.
fn execute_strategy(
    id: u8,
    _rgba: &[u8],
    grey: &[u8],
    yellow: &[u8],
    w: usize,
    h: usize,
) -> Option<String> {
    match id {
        STRAT_RAW => try_decode_greyscale(grey, w, h),

        STRAT_YELLOW => try_decode_greyscale(yellow, w, h),

        STRAT_YELLOW_BLUR => {
            let blurred = gaussian_blur(yellow, w, h, 1);
            try_decode_greyscale(&blurred, w, h)
        }

        STRAT_BLUR_LIGHT => {
            let blurred = gaussian_blur(grey, w, h, 1);
            try_decode_greyscale(&blurred, w, h)
        }

        STRAT_BLUR_HEAVY => {
            let blurred = gaussian_blur(grey, w, h, 2);
            try_decode_greyscale(&blurred, w, h)
        }

        STRAT_MED3 => {
            let med = median_filter(grey, w, h, 1);
            try_decode_greyscale(&med, w, h)
        }

        STRAT_MED5 => {
            let med = median_filter(grey, w, h, 2);
            try_decode_greyscale(&med, w, h)
        }

        STRAT_TEMPORAL_AVG => FRAME_BUF.with(|fb| {
            let buf = fb.borrow();
            buf.average()
                .and_then(|avg| try_decode_greyscale(&avg, buf.width, buf.height))
        }),

        STRAT_TEMPORAL_AVG_BLUR => FRAME_BUF.with(|fb| {
            let buf = fb.borrow();
            buf.average().and_then(|avg| {
                let blurred = gaussian_blur(&avg, buf.width, buf.height, 1);
                try_decode_greyscale(&blurred, buf.width, buf.height)
            })
        }),

        STRAT_TEMPORAL_MED => FRAME_BUF.with(|fb| {
            let buf = fb.borrow();
            buf.median()
                .and_then(|med| try_decode_greyscale(&med, buf.width, buf.height))
        }),

        STRAT_CLAHE_2 => {
            let enhanced = clahe(grey, w, h, 8, 8, 2.0);
            try_decode_greyscale(&enhanced, w, h)
        }

        STRAT_CLAHE_3 => {
            let enhanced = clahe(grey, w, h, 8, 8, 3.0);
            try_decode_greyscale(&enhanced, w, h)
        }

        STRAT_CLAHE_BLUR => {
            let enhanced = clahe(grey, w, h, 8, 8, 2.0);
            let blurred = gaussian_blur(&enhanced, w, h, 1);
            try_decode_greyscale(&blurred, w, h)
        }

        STRAT_CONTRAST_STRETCH => {
            let stretched = contrast_stretch(grey);
            try_decode_greyscale(&stretched, w, h)
        }

        STRAT_ADAPTIVE_THRESH => {
            let adaptive = adaptive_threshold(grey, w, h, 15, 8);
            try_decode_greyscale(&adaptive, w, h)
        }

        STRAT_BLUR_OTSU => {
            let blurred = gaussian_blur(grey, w, h, 2);
            let thresh = otsu_threshold(&blurred);
            try_decode_bitmap(&blurred, w, h, thresh)
        }

        STRAT_DOWNSCALE_2X => {
            let (dw, dh) = (w / 2, h / 2);
            if dw > 0 && dh > 0 {
                let ds = downscale_2x(grey, w, h);
                try_decode_greyscale(&ds, dw, dh)
            } else {
                None
            }
        }

        STRAT_DOWNSCALE_4X => {
            let (dw, dh) = (w / 4, h / 4);
            if dw > 20 && dh > 20 {
                let ds = downscale_4x(grey, w, h);
                try_decode_greyscale(&ds, dw, dh)
            } else {
                None
            }
        }

        STRAT_MORPH_CLOSE => {
            let threshold = otsu_threshold(grey);
            let binary: Vec<u8> = grey
                .iter()
                .map(|&p| if p < threshold { 0 } else { 255 })
                .collect();
            let closed = morphological_close(&binary, w, h, 1);
            try_decode_greyscale(&closed, w, h)
        }

        STRAT_QUIET_ZONE => {
            let (padded, pw, ph) = add_quiet_zone(grey, w, h, 20);
            try_decode_greyscale(&padded, pw, ph)
        }

        STRAT_LOCAL_NORM => {
            let normalized = local_normalize(grey, w, h, 64);
            try_decode_greyscale(&normalized, w, h)
        }

        STRAT_OTSU => {
            let threshold = otsu_threshold(grey);
            try_decode_bitmap(grey, w, h, threshold)
        }

        // Phase A: adaptive_thresh variants for tilt compensation
        STRAT_AT_FINE => {
            let adaptive = adaptive_threshold(grey, w, h, 11, 6);
            try_decode_greyscale(&adaptive, w, h)
        }

        STRAT_AT_COARSE => {
            let adaptive = adaptive_threshold(grey, w, h, 21, 10);
            try_decode_greyscale(&adaptive, w, h)
        }

        STRAT_AT_STRETCH_X10 => {
            let (stretched, sw, sh) = anisotropic_stretch(grey, w, h, 110, 100);
            let adaptive = adaptive_threshold(&stretched, sw, sh, 15, 8);
            try_decode_greyscale(&adaptive, sw, sh)
        }

        STRAT_AT_STRETCH_X20 => {
            let (stretched, sw, sh) = anisotropic_stretch(grey, w, h, 120, 100);
            let adaptive = adaptive_threshold(&stretched, sw, sh, 15, 8);
            try_decode_greyscale(&adaptive, sw, sh)
        }

        STRAT_AT_STRETCH_Y10 => {
            let (stretched, sw, sh) = anisotropic_stretch(grey, w, h, 100, 110);
            let adaptive = adaptive_threshold(&stretched, sw, sh, 15, 8);
            try_decode_greyscale(&adaptive, sw, sh)
        }

        STRAT_AT_STRETCH_Y20 => {
            let (stretched, sw, sh) = anisotropic_stretch(grey, w, h, 100, 120);
            let adaptive = adaptive_threshold(&stretched, sw, sh, 15, 8);
            try_decode_greyscale(&adaptive, sw, sh)
        }

        _ => None,
    }
}

/// Record a successful decode hit and trigger auto-reorder if threshold reached.
fn record_hit(id: u8) {
    let should_reorder = DECODE_STATS.with(|s| {
        let mut stats = s.borrow_mut();
        stats.hits[id as usize] += 1;
        stats.total_decodes += 1;
        stats.total_decodes % AUTO_REORDER_INTERVAL == 0
    });

    if should_reorder {
        reorder_strategies();
    }
}

/// Sort strategies by hit count descending. Strategies with equal hits
/// preserve their relative default order (stable sort).
fn reorder_strategies() {
    let hits = DECODE_STATS.with(|s| s.borrow().hits);
    STRATEGY_CONFIG.with(|c| {
        let mut config = c.borrow_mut();
        config.order.sort_by(|a, b| hits[*b as usize].cmp(&hits[*a as usize]));
    });
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

// ── Anisotropic stretch (tilt compensation) ───────────────────────

/// Stretch image along X and/or Y axis to compensate for perspective
/// distortion from tilted cards. Uses bilinear interpolation.
///
/// `scale_x_pct` and `scale_y_pct` are percentages: 100 = no change,
/// 110 = stretch 10%, 120 = stretch 20%. Only stretching (≥100) is supported;
/// values below 100 are clamped to 100.
///
/// Returns `(stretched_pixels, new_width, new_height)`.
fn anisotropic_stretch(
    grey: &[u8],
    w: usize,
    h: usize,
    scale_x_pct: usize,
    scale_y_pct: usize,
) -> (Vec<u8>, usize, usize) {
    let sx = scale_x_pct.max(100);
    let sy = scale_y_pct.max(100);
    if sx == 100 && sy == 100 {
        return (grey.to_vec(), w, h);
    }

    let nw = (w * sx + 50) / 100; // round
    let nh = (h * sy + 50) / 100;
    if nw == 0 || nh == 0 {
        return (grey.to_vec(), w, h);
    }

    let mut out = Vec::with_capacity(nw * nh);

    for ny in 0..nh {
        // Map output y back to source y (floating point)
        let src_y = ny as f32 * (h as f32 - 1.0) / (nh as f32 - 1.0).max(1.0);
        let y0 = (src_y as usize).min(h - 1);
        let y1 = (y0 + 1).min(h - 1);
        let fy = src_y - y0 as f32;

        for nx in 0..nw {
            let src_x = nx as f32 * (w as f32 - 1.0) / (nw as f32 - 1.0).max(1.0);
            let x0 = (src_x as usize).min(w - 1);
            let x1 = (x0 + 1).min(w - 1);
            let fx = src_x - x0 as f32;

            // Bilinear interpolation
            let tl = grey[y0 * w + x0] as f32;
            let tr = grey[y0 * w + x1] as f32;
            let bl = grey[y1 * w + x0] as f32;
            let br = grey[y1 * w + x1] as f32;

            let top = tl * (1.0 - fx) + tr * fx;
            let bot = bl * (1.0 - fx) + br * fx;
            let val = top * (1.0 - fy) + bot * fy;

            out.push(val.round().min(255.0).max(0.0) as u8);
        }
    }

    (out, nw, nh)
}

// ── Yellow-aware & CLAHE preprocessing ────────────────────────────

/// Convert RGBA to yellow-aware greyscale optimized for black-on-yellow QR codes.
/// Uses `max(R,G) - B` which gives: black→0, yellow(255,255,0)→255,
/// white/blue glare→0. Immune to blue-tinted specular reflections.
fn rgba_to_yellow_aware(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let len = width * height;
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let base = i * 4;
        let r = rgba[base];
        let g = rgba[base + 1];
        let b = rgba[base + 2];
        let max_rg = r.max(g);
        out.push(max_rg.saturating_sub(b));
    }
    out
}

/// CLAHE — Contrast Limited Adaptive Histogram Equalization.
/// Better than `local_normalize` for glossy surface glare: limits contrast
/// amplification via `clip_limit` and bilinearly interpolates between tile
/// mappings to avoid block boundary artifacts.
fn clahe(grey: &[u8], w: usize, h: usize, tiles_x: usize, tiles_y: usize, clip_limit: f32) -> Vec<u8> {
    if w == 0 || h == 0 || tiles_x == 0 || tiles_y == 0 {
        return grey.to_vec();
    }
    let tile_w = w / tiles_x;
    let tile_h = h / tiles_y;
    if tile_w == 0 || tile_h == 0 {
        return grey.to_vec();
    }

    // 1. Compute clipped histogram + CDF mapping for each tile
    let mut maps = vec![[0u8; 256]; tiles_x * tiles_y];

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let x0 = tx * tile_w;
            let y0 = ty * tile_h;
            let x1 = if tx == tiles_x - 1 { w } else { x0 + tile_w };
            let y1 = if ty == tiles_y - 1 { h } else { y0 + tile_h };
            let tile_pixels = (x1 - x0) * (y1 - y0);

            let mut hist = [0u32; 256];
            for row in y0..y1 {
                for col in x0..x1 {
                    hist[grey[row * w + col] as usize] += 1;
                }
            }

            // Clip and redistribute
            let clip = (clip_limit * tile_pixels as f32 / 256.0) as u32;
            let clip = clip.max(1);
            let mut excess = 0u32;
            for bin in hist.iter_mut() {
                if *bin > clip {
                    excess += *bin - clip;
                    *bin = clip;
                }
            }
            let per_bin = excess / 256;
            let remainder = (excess % 256) as usize;
            for (i, bin) in hist.iter_mut().enumerate() {
                *bin += per_bin;
                if i < remainder {
                    *bin += 1;
                }
            }

            // Build CDF → mapping LUT
            let mut cdf = [0u32; 256];
            cdf[0] = hist[0];
            for i in 1..256 {
                cdf[i] = cdf[i - 1] + hist[i];
            }
            let cdf_min = *cdf.iter().find(|&&v| v > 0).unwrap_or(&0);
            let denom = cdf[255].saturating_sub(cdf_min);

            let idx = ty * tiles_x + tx;
            for i in 0..256 {
                if denom == 0 {
                    maps[idx][i] = i as u8;
                } else {
                    let val = ((cdf[i].saturating_sub(cdf_min) as f32 / denom as f32) * 255.0) as u32;
                    maps[idx][i] = val.min(255) as u8;
                }
            }
        }
    }

    // 2. Map each pixel with bilinear interpolation between 4 nearest tiles
    let mut result = vec![0u8; w * h];
    let tw_f = tile_w as f32;
    let th_f = tile_h as f32;

    for y in 0..h {
        for x in 0..w {
            let pixel = grey[y * w + x] as usize;

            let fx = (x as f32 + 0.5) / tw_f - 0.5;
            let fy = (y as f32 + 0.5) / th_f - 0.5;

            let tx0 = (fx.floor() as i32).max(0).min(tiles_x as i32 - 1) as usize;
            let tx1 = (fx.floor() as i32 + 1).max(0).min(tiles_x as i32 - 1) as usize;
            let ty0 = (fy.floor() as i32).max(0).min(tiles_y as i32 - 1) as usize;
            let ty1 = (fy.floor() as i32 + 1).max(0).min(tiles_y as i32 - 1) as usize;

            let ax = fx - fx.floor();
            let ay = fy - fy.floor();

            let v00 = maps[ty0 * tiles_x + tx0][pixel] as f32;
            let v10 = maps[ty0 * tiles_x + tx1][pixel] as f32;
            let v01 = maps[ty1 * tiles_x + tx0][pixel] as f32;
            let v11 = maps[ty1 * tiles_x + tx1][pixel] as f32;

            let top = v00 * (1.0 - ax) + v10 * ax;
            let bot = v01 * (1.0 - ax) + v11 * ax;
            let val = top * (1.0 - ay) + bot * ay;

            result[y * w + x] = val.round().min(255.0).max(0.0) as u8;
        }
    }

    result
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

    // ── Yellow-aware tests ─────────────────────────────────────────

    #[test]
    fn yellow_aware_black_is_zero() {
        let rgba = vec![0, 0, 0, 255];
        let y = rgba_to_yellow_aware(&rgba, 1, 1);
        assert_eq!(y[0], 0);
    }

    #[test]
    fn yellow_aware_yellow_is_max() {
        let rgba = vec![255, 255, 0, 255];
        let y = rgba_to_yellow_aware(&rgba, 1, 1);
        assert_eq!(y[0], 255);
    }

    #[test]
    fn yellow_aware_white_glare_is_zero() {
        // White (255,255,255): max(255,255) - 255 = 0
        let rgba = vec![255, 255, 255, 255];
        let y = rgba_to_yellow_aware(&rgba, 1, 1);
        assert_eq!(y[0], 0);
    }

    #[test]
    fn yellow_aware_blue_glare_is_zero() {
        // Blue-ish glare (100,100,255): max(100,100) - 255 = 0 (saturating)
        let rgba = vec![100, 100, 255, 255];
        let y = rgba_to_yellow_aware(&rgba, 1, 1);
        assert_eq!(y[0], 0);
    }

    #[test]
    fn yellow_aware_red_is_high() {
        // Red (255,0,0): max(255,0) - 0 = 255
        let rgba = vec![255, 0, 0, 255];
        let y = rgba_to_yellow_aware(&rgba, 1, 1);
        assert_eq!(y[0], 255);
    }

    // ── CLAHE tests ────────────────────────────────────────────────

    #[test]
    fn clahe_preserves_dimensions() {
        let grey = vec![128; 80 * 60];
        let result = clahe(&grey, 80, 60, 8, 8, 2.0);
        assert_eq!(result.len(), 80 * 60);
    }

    #[test]
    fn clahe_uniform_image_stays_uniform() {
        let grey = vec![128; 64 * 64];
        let result = clahe(&grey, 64, 64, 8, 8, 2.0);
        // Uniform input → all pixels map to same value
        let first = result[0];
        assert!(result.iter().all(|&v| v == first));
    }

    #[test]
    fn clahe_zero_dimensions_returns_copy() {
        let grey = vec![128; 4];
        assert_eq!(clahe(&grey, 0, 0, 8, 8, 2.0), grey);
    }

    // ── Stats & strategy order tests ───────────────────────────────

    #[test]
    fn get_qr_stats_returns_valid_json() {
        // Reset first
        reset_qr_strategy_order();
        let json = get_qr_stats();
        assert!(json.starts_with('{'));
        assert!(json.contains("\"total\""));
        assert!(json.contains("\"strategies\""));
    }

    #[test]
    fn set_strategy_order_accepts_csv() {
        reset_qr_strategy_order();
        set_qr_strategy_order("10,1,0");
        let order = STRATEGY_CONFIG.with(|c| c.borrow().order.clone());
        // First 3 should be the ones we specified
        assert_eq!(order[0], 10);
        assert_eq!(order[1], 1);
        assert_eq!(order[2], 0);
        // All strategies should still be present
        assert_eq!(order.len(), NUM_STRATEGIES);
        reset_qr_strategy_order();
    }

    #[test]
    fn set_strategy_order_ignores_invalid_ids() {
        reset_qr_strategy_order();
        set_qr_strategy_order("0,99,1");
        let order = STRATEGY_CONFIG.with(|c| c.borrow().order.clone());
        assert_eq!(order[0], 0);
        assert_eq!(order[1], 1); // 99 was skipped
        assert_eq!(order.len(), NUM_STRATEGIES);
        reset_qr_strategy_order();
    }

    #[test]
    fn reset_strategy_order_restores_default() {
        set_qr_strategy_order("21,20,19");
        reset_qr_strategy_order();
        let order = STRATEGY_CONFIG.with(|c| c.borrow().order.clone());
        assert_eq!(order, DEFAULT_ORDER.to_vec());
    }

    #[test]
    fn record_hit_increments_stats() {
        reset_qr_strategy_order();
        record_hit(STRAT_YELLOW);
        record_hit(STRAT_YELLOW);
        record_hit(STRAT_CLAHE_2);
        let stats = DECODE_STATS.with(|s| {
            let s = s.borrow();
            (s.hits[STRAT_YELLOW as usize], s.hits[STRAT_CLAHE_2 as usize], s.total_decodes)
        });
        assert!(stats.0 >= 2);
        assert!(stats.1 >= 1);
        reset_qr_strategy_order(); // clears stats
    }

    // ── Anisotropic stretch tests ──────────────────────────────────

    #[test]
    fn stretch_noop_at_100() {
        let grey = vec![10, 20, 30, 40];
        let (out, nw, nh) = anisotropic_stretch(&grey, 2, 2, 100, 100);
        assert_eq!(nw, 2);
        assert_eq!(nh, 2);
        assert_eq!(out, grey);
    }

    #[test]
    fn stretch_x_increases_width() {
        let grey = vec![128; 10 * 10];
        let (out, nw, nh) = anisotropic_stretch(&grey, 10, 10, 120, 100);
        assert_eq!(nw, 12); // 10 * 120 / 100 = 12
        assert_eq!(nh, 10);
        assert_eq!(out.len(), 12 * 10);
    }

    #[test]
    fn stretch_y_increases_height() {
        let grey = vec![128; 10 * 10];
        let (out, nw, nh) = anisotropic_stretch(&grey, 10, 10, 100, 110);
        assert_eq!(nw, 10);
        assert_eq!(nh, 11); // 10 * 110 / 100 = 11
        assert_eq!(out.len(), 10 * 11);
    }

    #[test]
    fn stretch_preserves_corners() {
        // 3×3 image, stretch X by 10%
        let grey = vec![0, 128, 255, 50, 100, 200, 10, 80, 250];
        let (out, nw, nh) = anisotropic_stretch(&grey, 3, 3, 110, 100);
        assert!(nw >= 3);
        assert_eq!(nh, 3);
        // Corners should be preserved exactly (they map to source corners)
        assert_eq!(out[0], 0); // top-left
        assert_eq!(out[nw - 1], 255); // top-right
        assert_eq!(out[(nh - 1) * nw], 10); // bottom-left
        assert_eq!(out[nh * nw - 1], 250); // bottom-right
    }

    #[test]
    fn stretch_clamps_below_100() {
        let grey = vec![128; 4];
        let (out, nw, nh) = anisotropic_stretch(&grey, 2, 2, 50, 80);
        // Should clamp to 100, returning original
        assert_eq!(nw, 2);
        assert_eq!(nh, 2);
        assert_eq!(out, grey);
    }

    #[test]
    fn default_order_starts_with_adaptive_thresh() {
        assert_eq!(DEFAULT_ORDER[0], STRAT_ADAPTIVE_THRESH);
        assert_eq!(DEFAULT_ORDER[1], STRAT_AT_FINE);
        assert_eq!(DEFAULT_ORDER[2], STRAT_AT_COARSE);
        assert_eq!(DEFAULT_ORDER[3], STRAT_AT_STRETCH_X10);
    }

    #[test]
    fn all_28_strategies_in_default_order() {
        assert_eq!(DEFAULT_ORDER.len(), NUM_STRATEGIES);
        // Every strategy ID 0..27 should appear exactly once
        for id in 0..NUM_STRATEGIES as u8 {
            assert!(
                DEFAULT_ORDER.contains(&id),
                "strategy {} missing from DEFAULT_ORDER",
                id
            );
        }
    }
}
