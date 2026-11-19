//! What is done to a camera frame around the model that finds QR codes and
//! the reader that reads them.
//!
//! Neither of those two is the engine's. ONNX Runtime runs the model and
//! zxing-cpp is the reader, each a module of its own that the engine's
//! worker holds (`assets/js/yolo-inference.js`, `zxing-decode.js`).
//! Everything between them is here:
//!
//! - the frame made into the model's input ([`model_input`]);
//! - the model's output made into boxes ([`boxes`]);
//! - a box cut out of the frame with a margin ([`cut_out`]);
//! - a picture thresholded for the reader ([`threshold`]);
//! - which pictures the reader is offered, and in what order
//!   ([`Scan::next`]).
//!
//! The worker hands a frame over (`scan_frame`). With computer vision on
//! and the model loaded, it runs the model on what `scan_model_input` gives
//! and hands the output back (`scan_found`). Then it gives the reader each
//! picture `scan_next` offers, until one is read or there are no more. What
//! a code says goes to the scanner (`routes/qr.rs`), which decides where it
//! leads.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;

/// The side of the square picture the model takes.
pub const MODEL_SIDE: usize = 640;

/// A box the model is less sure of than this is not taken for a code.
const LEAST_CONFIDENCE: f32 = 0.25;

/// Of two boxes that share more than this much of their joint area, only
/// the one the model is surer of is kept.
const MOST_OVERLAP: f64 = 0.45;

/// The margin left around a box when it is cut out for the reader, as a
/// share of the box. The model was trained on boxes that already hold a
/// margin about the code (CONTRIBUTING.md, The model that finds codes), so
/// little is added.
pub const MARGIN: f64 = 0.05;

/// The threshold looks this many pixels to each side of a pixel.
const REACH: usize = 21;

/// A pixel is dark when it is more than this much darker than the pixels
/// about it are on average.
const DARKER_BY: i64 = 10;

/// The grey of the bars that fill the model's square beside a frame that is
/// not square.
const BARS: u8 = 114;

/// The most pixels a frame may have. The scanner's frames are 640 square;
/// the sums the threshold keeps would not hold many times that.
const MOST_PIXELS: usize = 4096 * 4096;

/// A picture: four bytes a pixel (red, green, blue, alpha), row by row.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Picture {
    pub rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

/// A box the model found in a frame, in the frame's pixels: its left and
/// top edge, its size, and how sure the model is of it (0 to 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Found {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub confidence: f32,
}

/// Round half up, as the scripts this replaced did (`Math.round`).
fn round(value: f64) -> f64 {
    (value + 0.5).floor()
}

// ── A picture thresholded for the reader ───────────────────────────

/// The picture in black and white: a pixel is black where it is darker than
/// the pixels about it (a square reaching [`REACH`] to each side) are on
/// average, by more than [`DARKER_BY`]. That evens out poor light and the
/// shine of a card's surface, which the reader's own binarizer struggles
/// with. Four bytes a pixel, as the picture came.
pub fn threshold(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let pixels = width * height;
    let grey: Vec<u8> = rgba
        .chunks_exact(4)
        .take(pixels)
        .map(|p| ((77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32) >> 8) as u8)
        .collect();
    if grey.len() < pixels {
        return Vec::new();
    }

    // sums[(y + 1) * across + (x + 1)] is the sum of the grey of every pixel
    // above and to the left of (x, y), with it. Row 0 and column 0 are zero.
    let across = width + 1;
    let mut sums = vec![0u32; across * (height + 1)];
    for y in 0..height {
        let mut row = 0u32;
        for x in 0..width {
            row += grey[y * width + x] as u32;
            sums[(y + 1) * across + x + 1] = row + sums[y * across + x + 1];
        }
    }

    let mut out = vec![255u8; pixels * 4];
    for y in 0..height {
        let top = y.saturating_sub(REACH);
        let bottom = (y + REACH + 1).min(height);
        for x in 0..width {
            let left = x.saturating_sub(REACH);
            let right = (x + REACH + 1).min(width);
            let area = ((bottom - top) * (right - left)) as i64;
            let sum = (sums[bottom * across + right] as i64 + sums[top * across + left] as i64)
                - (sums[top * across + right] as i64 + sums[bottom * across + left] as i64);
            // Dark where grey < sum / area - DARKER_BY, kept in whole numbers.
            if (grey[y * width + x] as i64 + DARKER_BY) * area < sum {
                let at = (y * width + x) * 4;
                out[at] = 0;
                out[at + 1] = 0;
                out[at + 2] = 0;
            }
        }
    }
    out
}

// ── The frame as the model takes it ────────────────────────────────

/// The frame as the model's input: a square of [`MODEL_SIDE`], the frame
/// scaled to fit it with its shape kept and grey bars beside it, as three
/// planes (all the red, all the green, all the blue) of numbers from 0 to 1.
pub fn model_input(rgba: &[u8], width: usize, height: usize) -> Vec<f32> {
    let side = MODEL_SIDE;
    let plane = side * side;
    // A byte as the model wants it. Divided as a double and then narrowed,
    // which is what the script this replaced gave.
    let unit: [f32; 256] = std::array::from_fn(|value| (value as f64 / 255.0) as f32);
    let mut data = vec![unit[BARS as usize]; 3 * plane];
    if width == 0 || height == 0 || rgba.len() < width * height * 4 {
        return data;
    }

    let scale = (side as f64 / width as f64).min(side as f64 / height as f64);
    let scaled_width = (round(width as f64 * scale) as usize).min(side);
    let scaled_height = (round(height as f64 * scale) as usize).min(side);
    let bar_left = (round((side - scaled_width) as f64 / 2.0) as usize).min(side - scaled_width);
    let bar_top = (round((side - scaled_height) as f64 / 2.0) as usize).min(side - scaled_height);

    // Each of the square's pixels takes the frame's pixel nearest above and
    // to the left of where it falls.
    for y in 0..scaled_height {
        let from_y = ((y as f64 / scale).floor() as usize).min(height - 1);
        for x in 0..scaled_width {
            let from_x = ((x as f64 / scale).floor() as usize).min(width - 1);
            let from = (from_y * width + from_x) * 4;
            let to = (y + bar_top) * side + x + bar_left;
            data[to] = unit[rgba[from] as usize];
            data[plane + to] = unit[rgba[from + 1] as usize];
            data[2 * plane + to] = unit[rgba[from + 2] as usize];
        }
    }
    data
}

// ── What the model found ───────────────────────────────────────────

/// The boxes in the model's output, in the frame's pixels, the surest
/// first.
///
/// The output is five rows of `places` numbers: for each of the places the
/// model looks at, a box's centre (x, then y), its width, its height, and
/// how sure the model is that a code is there. The boxes are in the pixels
/// of the model's square, so the scaling and the bars of [`model_input`] are
/// taken out again. A box is cut to the frame, and one that overlaps a
/// surer one by more than [`MOST_OVERLAP`] is dropped.
pub fn boxes(output: &[f32], places: usize, width: usize, height: usize) -> Vec<Found> {
    if places == 0 || output.len() < 5 * places || width == 0 || height == 0 {
        return Vec::new();
    }
    let side = MODEL_SIDE as f64;
    let (frame_width, frame_height) = (width as f64, height as f64);
    let scale = (side / frame_width).min(side / frame_height);
    let bar_left = (side - frame_width * scale) / 2.0;
    let bar_top = (side - frame_height * scale) / 2.0;

    let mut found = Vec::new();
    for place in 0..places {
        let confidence = output[4 * places + place];
        // Written this way round so that what is not a number is not a box.
        if !(confidence >= LEAST_CONFIDENCE) {
            continue;
        }
        let centre_x = output[place] as f64;
        let centre_y = output[places + place] as f64;
        let box_width = output[2 * places + place] as f64;
        let box_height = output[3 * places + place] as f64;
        if !(centre_x.is_finite() && centre_y.is_finite() && box_width.is_finite() && box_height.is_finite()) {
            continue;
        }
        let x = ((centre_x - box_width / 2.0 - bar_left) / scale).min(frame_width).max(0.0);
        let y = ((centre_y - box_height / 2.0 - bar_top) / scale).min(frame_height).max(0.0);
        let w = (box_width / scale).min(frame_width - x);
        let h = (box_height / scale).min(frame_height - y);
        if w > 0.0 && h > 0.0 {
            found.push(Found { x, y, w, h, confidence });
        }
    }

    // The surest first. Boxes the model is equally sure of stay in the
    // model's order.
    found.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));

    let mut kept: Vec<Found> = Vec::new();
    for candidate in found {
        if kept.iter().all(|surer| overlap(surer, &candidate) <= MOST_OVERLAP) {
            kept.push(candidate);
        }
    }
    kept
}

/// How much of the area two boxes cover together they both cover (their
/// intersection over their union), from 0 to 1.
fn overlap(a: &Found, b: &Found) -> f64 {
    let across = ((a.x + a.w).min(b.x + b.w) - a.x.max(b.x)).max(0.0);
    let down = ((a.y + a.h).min(b.y + b.h) - a.y.max(b.y)).max(0.0);
    let both = across * down;
    let either = a.w * a.h + b.w * b.h - both;
    if either > 0.0 { both / either } else { 0.0 }
}

/// A box cut out of the frame, with `margin` (a share of the box) added on
/// each side and cut to the frame.
pub fn cut_out(rgba: &[u8], width: usize, height: usize, found: &Found, margin: f64) -> Picture {
    let margin_x = round(found.w * margin);
    let margin_y = round(found.h * margin);
    let left = (round(found.x) - margin_x).max(0.0) as usize;
    let top = (round(found.y) - margin_y).max(0.0) as usize;
    let right = ((round(found.x + found.w) + margin_x).max(0.0) as usize).min(width);
    let bottom = ((round(found.y + found.h) + margin_y).max(0.0) as usize).min(height);
    if right <= left || bottom <= top || rgba.len() < width * height * 4 {
        return Picture::default();
    }

    let (cut_width, cut_height) = (right - left, bottom - top);
    let mut cut = Vec::with_capacity(cut_width * cut_height * 4);
    for row in top..bottom {
        let from = (row * width + left) * 4;
        cut.extend_from_slice(&rgba[from..from + cut_width * 4]);
    }
    Picture { rgba: cut, width: cut_width, height: cut_height }
}

// ── A frame in hand ────────────────────────────────────────────────

/// A frame the worker has handed over, and how far the reader has got with
/// it.
#[derive(Default)]
struct Scan {
    frame: Picture,
    /// The boxes the model found in the frame. `None` when the model was
    /// not run on it: the reader is then given the frame whole.
    found: Option<Vec<Found>>,
    /// How many pictures the reader has been offered.
    offered: usize,
    /// The box cut out last, kept for its second offering.
    cut: Option<Picture>,
    /// The size of the picture offered last.
    size: (usize, usize),
}

impl Scan {
    fn of(frame: Picture) -> Self {
        Scan { frame, ..Scan::default() }
    }

    /// The next picture for the reader, or `None` when the frame has no
    /// more to offer.
    ///
    /// Each thing to read is offered twice: thresholded first, which reads
    /// most pictures the reader cannot read as they are, and then as it is,
    /// for the picture the threshold spoils. Without the model that thing
    /// is the whole frame. With it, it is each box cut out of the frame, the
    /// surest first, and a frame the model found nothing in offers nothing.
    fn next(&mut self) -> Option<Vec<u8>> {
        let Some(found) = &self.found else {
            if self.frame.width == 0 {
                // No frame in hand.
                return None;
            }
            let turn = self.offered;
            self.offered += 1;
            self.size = (self.frame.width, self.frame.height);
            return match turn {
                0 => Some(threshold(&self.frame.rgba, self.frame.width, self.frame.height)),
                // The last use of the frame: it is given away.
                1 => Some(std::mem::take(&mut self.frame.rgba)),
                _ => None,
            };
        };
        loop {
            let (which, second) = (self.offered / 2, self.offered % 2 == 1);
            let at = found.get(which)?;
            if second {
                self.offered += 1;
                // Its size is the one already said.
                return self.cut.take().map(|cut| cut.rgba);
            }
            let cut = cut_out(&self.frame.rgba, self.frame.width, self.frame.height, at, MARGIN);
            if cut.rgba.is_empty() {
                // Too thin a box to hold a pixel: on to the next.
                self.offered += 2;
                continue;
            }
            self.offered += 1;
            self.size = (cut.width, cut.height);
            let thresholded = threshold(&cut.rgba, cut.width, cut.height);
            self.cut = Some(cut);
            return Some(thresholded);
        }
    }
}

thread_local! {
    static SCAN: RefCell<Scan> = RefCell::new(Scan::default());
}

// ── For the worker ─────────────────────────────────────────────────

/// A camera frame to look for a code in: four bytes a pixel. It takes the
/// place of the frame before it. False, and nothing is in hand, if the
/// bytes are not a picture of that size.
#[wasm_bindgen]
pub fn scan_frame(rgba: Vec<u8>, width: u32, height: u32) -> bool {
    let (width, height) = (width as usize, height as usize);
    let whole = width > 0 && height > 0 && width * height <= MOST_PIXELS && rgba.len() == width * height * 4;
    SCAN.with(|scan| {
        *scan.borrow_mut() = if whole { Scan::of(Picture { rgba, width, height }) } else { Scan::default() };
    });
    whole
}

/// The frame in hand as the model takes it: see [`model_input`].
#[wasm_bindgen]
pub fn scan_model_input() -> Vec<f32> {
    SCAN.with(|scan| {
        let scan = scan.borrow();
        model_input(&scan.frame.rgba, scan.frame.width, scan.frame.height)
    })
}

/// What the model gave for the frame in hand: `output` is its five rows of
/// `places` numbers (see [`boxes`]). From here on the reader is offered the
/// boxes and not the whole frame. Gives the boxes back for the page to draw
/// over the camera's picture: five numbers a box (left, top, width, height,
/// how sure), in the frame's pixels.
#[wasm_bindgen]
pub fn scan_found(output: &[f32], places: u32) -> Vec<f32> {
    SCAN.with(|scan| {
        let mut scan = scan.borrow_mut();
        let found = boxes(output, places as usize, scan.frame.width, scan.frame.height);
        let drawn = found
            .iter()
            .flat_map(|at| [at.x as f32, at.y as f32, at.w as f32, at.h as f32, at.confidence])
            .collect();
        scan.found = Some(found);
        scan.offered = 0;
        scan.cut = None;
        drawn
    })
}

/// The next picture of the frame in hand for the reader, four bytes a
/// pixel, or nothing when there are no more: see [`Scan::next`]. Its size
/// is [`scan_width`] by [`scan_height`].
#[wasm_bindgen]
pub fn scan_next() -> Option<Vec<u8>> {
    SCAN.with(|scan| scan.borrow_mut().next())
}

/// The width of the picture [`scan_next`] gave last.
#[wasm_bindgen]
pub fn scan_width() -> u32 {
    SCAN.with(|scan| scan.borrow().size.0 as u32)
}

/// The height of the picture [`scan_next`] gave last.
#[wasm_bindgen]
pub fn scan_height() -> u32 {
    SCAN.with(|scan| scan.borrow().size.1 as u32)
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture of one grey all over.
    fn flat(width: usize, height: usize, grey: u8) -> Vec<u8> {
        [grey, grey, grey, 255].repeat(width * height)
    }

    /// A picture that is the same from run to run and looks like noise.
    fn noise(width: usize, height: usize, seed: u32) -> Vec<u8> {
        let mut state = seed;
        (0..width * height * 4)
            .map(|at| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                if at % 4 == 3 { 255 } else { (state >> 24) as u8 }
            })
            .collect()
    }

    fn paint(rgba: &mut [u8], width: usize, x: usize, y: usize, grey: u8) {
        let at = (y * width + x) * 4;
        rgba[at..at + 3].copy_from_slice(&[grey, grey, grey]);
    }

    fn grey_at(rgba: &[u8], width: usize, x: usize, y: usize) -> u8 {
        rgba[(y * width + x) * 4]
    }

    // -- threshold --

    /// The threshold as the script this replaced worked it out: the mean as
    /// a fraction, compared with the pixel.
    fn threshold_by_fractions(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
        let grey: Vec<f64> =
            rgba.chunks_exact(4).map(|p| ((77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32) >> 8) as f64).collect();
        let mut out = Vec::with_capacity(rgba.len());
        for y in 0..height {
            for x in 0..width {
                let (top, bottom) = (y.saturating_sub(21), (y + 22).min(height));
                let (left, right) = (x.saturating_sub(21), (x + 22).min(width));
                let mut sum = 0.0;
                for row in top..bottom {
                    for column in left..right {
                        sum += grey[row * width + column];
                    }
                }
                let mean = sum / ((bottom - top) * (right - left)) as f64;
                let value = if grey[y * width + x] < mean - 10.0 { 0 } else { 255 };
                out.extend_from_slice(&[value, value, value, 255]);
            }
        }
        out
    }

    #[test]
    fn a_picture_of_one_grey_thresholds_to_white() {
        for grey in [0, 9, 128, 255] {
            let out = threshold(&flat(50, 30, grey), 50, 30);
            assert_eq!(out, flat(50, 30, 255), "grey {}", grey);
        }
    }

    #[test]
    fn what_is_darker_than_its_surroundings_is_black_and_the_rest_white() {
        let (width, height) = (60, 60);
        let mut picture = flat(width, height, 200);
        for y in 28..32 {
            for x in 28..32 {
                paint(&mut picture, width, x, y, 40);
            }
        }
        // Only a little darker than what is about it: not dark enough.
        paint(&mut picture, width, 5, 5, 195);
        let out = threshold(&picture, width, height);
        assert_eq!(out.len(), picture.len());
        assert_eq!(grey_at(&out, width, 30, 30), 0);
        assert_eq!(grey_at(&out, width, 10, 30), 255);
        assert_eq!(grey_at(&out, width, 5, 5), 255);
        assert!(out.chunks_exact(4).all(|p| p[3] == 255 && p[0] == p[1] && p[1] == p[2] && (p[0] == 0 || p[0] == 255)));
    }

    #[test]
    fn the_threshold_is_the_one_worked_out_with_fractions() {
        // Sizes smaller and larger than the square it looks about a pixel,
        // and not square.
        for (width, height, seed) in [(1, 1, 1), (7, 3, 2), (64, 48, 3), (90, 131, 4)] {
            let picture = noise(width, height, seed);
            assert_eq!(
                threshold(&picture, width, height),
                threshold_by_fractions(&picture, width, height),
                "{} by {}",
                width,
                height
            );
        }
        // Soft steps of grey, where the mean falls on and about whole numbers.
        let (width, height) = (80, 60);
        let steps: Vec<u8> = (0..width * height).flat_map(|at| [((at % width) * 3 % 256) as u8; 3].into_iter().chain([255])).collect();
        assert_eq!(threshold(&steps, width, height), threshold_by_fractions(&steps, width, height));
    }

    #[test]
    fn bytes_that_are_not_a_whole_picture_threshold_to_nothing() {
        assert!(threshold(&flat(4, 4, 10), 5, 5).is_empty());
    }

    // -- the model's input --

    #[test]
    fn a_frame_of_the_models_size_is_its_three_colours_as_planes_from_0_to_1() {
        let side = MODEL_SIDE;
        let mut frame = flat(side, side, 0);
        // One pixel of each colour, and one white.
        frame[0..3].copy_from_slice(&[255, 0, 0]);
        frame[4..7].copy_from_slice(&[0, 255, 0]);
        frame[8..11].copy_from_slice(&[0, 0, 255]);
        let last = (side * side - 1) * 4;
        frame[last..last + 3].copy_from_slice(&[255, 255, 255]);
        let input = model_input(&frame, side, side);
        let plane = side * side;
        assert_eq!(input.len(), 3 * plane);
        assert_eq!([input[0], input[plane], input[2 * plane]], [1.0, 0.0, 0.0]);
        assert_eq!([input[1], input[plane + 1], input[2 * plane + 1]], [0.0, 1.0, 0.0]);
        assert_eq!([input[2], input[plane + 2], input[2 * plane + 2]], [0.0, 0.0, 1.0]);
        assert_eq!([input[plane - 1], input[2 * plane - 1], input[3 * plane - 1]], [1.0, 1.0, 1.0]);
        assert_eq!(input[3], 0.0);
        // A byte is the fraction of 255 it is, as near as the model's numbers go.
        let grey = model_input(&flat(side, side, 77), side, side);
        assert_eq!(grey[1234], (77.0f64 / 255.0) as f32);
    }

    #[test]
    fn a_frame_that_is_not_square_keeps_its_shape_with_grey_bars_beside_it() {
        // Twice as wide as high: scaled to 640 by 320, with bars of 160
        // above and below.
        let (width, height) = (1280, 640);
        let mut frame = flat(width, height, 255);
        // The frame's top-left pixel, black.
        paint(&mut frame, width, 0, 0, 0);
        let input = model_input(&frame, width, height);
        let side = MODEL_SIDE;
        let bars = (114.0f64 / 255.0) as f32;
        assert_eq!(input[0], bars);
        assert_eq!(input[159 * side + 300], bars);
        assert_eq!(input[160 * side], 0.0, "the frame's first pixel, under the top bar");
        assert_eq!(input[160 * side + 1], 1.0);
        assert_eq!(input[479 * side + 639], 1.0);
        assert_eq!(input[480 * side], bars);
        assert_eq!(input[3 * side * side - 1], bars);

        // Higher than wide: the bars are to the sides.
        let input = model_input(&flat(320, 640, 255), 320, 640);
        assert_eq!(input[159], bars);
        assert_eq!(input[160], 1.0);
        assert_eq!(input[479], 1.0);
        assert_eq!(input[480], bars);
    }

    #[test]
    fn no_frame_is_grey_all_over() {
        let bars = (114.0f64 / 255.0) as f32;
        assert!(model_input(&[], 0, 0).iter().all(|value| *value == bars));
        assert_eq!(model_input(&[1, 2, 3], 640, 640).len(), 3 * MODEL_SIDE * MODEL_SIDE);
    }

    // -- the model's boxes --

    /// The model's output for these boxes: centre x, centre y, width,
    /// height and confidence each.
    fn output(of: &[[f32; 5]]) -> Vec<f32> {
        (0..5).flat_map(|row| of.iter().map(move |one| one[row])).collect()
    }

    #[test]
    fn a_box_is_given_by_its_corner_and_size_and_the_surest_comes_first() {
        let found = boxes(&output(&[[100.0, 200.0, 40.0, 60.0, 0.5], [400.0, 300.0, 100.0, 100.0, 0.9]]), 2, 640, 640);
        assert_eq!(
            found,
            [
                Found { x: 350.0, y: 250.0, w: 100.0, h: 100.0, confidence: 0.9 },
                Found { x: 80.0, y: 170.0, w: 40.0, h: 60.0, confidence: 0.5 },
            ]
        );
    }

    #[test]
    fn what_the_model_is_not_sure_of_is_not_a_box() {
        let found = boxes(
            &output(&[[100.0, 100.0, 40.0, 40.0, 0.24], [300.0, 300.0, 40.0, 40.0, 0.25], [500.0, 500.0, 40.0, 40.0, f32::NAN]]),
            3,
            640,
            640,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].confidence, 0.25);
        // Nor is a box of no size, or one that is not numbers.
        assert!(boxes(&output(&[[100.0, 100.0, 0.0, 40.0, 0.9]]), 1, 640, 640).is_empty());
        assert!(boxes(&output(&[[f32::INFINITY, 100.0, 10.0, 40.0, 0.9]]), 1, 640, 640).is_empty());
    }

    #[test]
    fn of_boxes_that_overlap_much_only_the_surest_is_kept() {
        let found = boxes(
            &output(&[
                [200.0, 200.0, 100.0, 100.0, 0.6],
                // Nearly the same box, surer.
                [205.0, 200.0, 100.0, 100.0, 0.8],
                // Touching the others' corner: a code of its own.
                [290.0, 290.0, 100.0, 100.0, 0.7],
            ]),
            3,
            640,
            640,
        );
        assert_eq!(found.iter().map(|at| at.confidence).collect::<Vec<_>>(), [0.8, 0.7]);
    }

    #[test]
    fn boxes_are_put_back_into_a_frame_that_is_not_square_and_cut_to_it() {
        // 1280 by 640 was shown to the model at half size under a bar of 160.
        let found = boxes(&output(&[[320.0, 320.0, 100.0, 50.0, 0.9]]), 1, 1280, 640);
        assert_eq!(found, [Found { x: 540.0, y: 270.0, w: 200.0, h: 100.0, confidence: 0.9 }]);

        // Over the frame's edges: cut to it.
        let found = boxes(&output(&[[10.0, 630.0, 100.0, 100.0, 0.9]]), 1, 640, 640);
        assert_eq!(found, [Found { x: 0.0, y: 580.0, w: 100.0, h: 60.0, confidence: 0.9 }]);
    }

    #[test]
    fn an_output_that_is_not_five_rows_gives_no_boxes() {
        assert!(boxes(&[], 0, 640, 640).is_empty());
        assert!(boxes(&[1.0; 9], 2, 640, 640).is_empty());
        assert!(boxes(&output(&[[100.0, 100.0, 40.0, 40.0, 0.9]]), 1, 0, 640).is_empty());
    }

    // -- a box cut out --

    #[test]
    fn a_box_is_cut_out_with_its_margin() {
        let (width, height) = (100, 80);
        let mut frame = flat(width, height, 255);
        paint(&mut frame, width, 38, 28, 7);
        let cut = cut_out(&frame, width, height, &Found { x: 40.0, y: 30.0, w: 20.0, h: 10.0, confidence: 1.0 }, 0.1);
        // A margin of 2 to the sides and 1 above and below.
        assert_eq!((cut.width, cut.height), (24, 12));
        assert_eq!(cut.rgba.len(), 24 * 12 * 4);
        assert_eq!(grey_at(&cut.rgba, 24, 0, 0), 255);
        assert_eq!(grey_at(&cut.rgba, 24, 0, 0), grey_at(&frame, width, 38, 29));
        let cut = cut_out(&frame, width, height, &Found { x: 40.0, y: 30.0, w: 20.0, h: 20.0, confidence: 1.0 }, 0.1);
        assert_eq!(grey_at(&cut.rgba, 24, 0, 0), 7, "the frame's pixel at the cut's corner");
    }

    #[test]
    fn a_box_at_the_frames_edge_is_cut_to_the_frame() {
        let frame = flat(100, 80, 9);
        let cut = cut_out(&frame, 100, 80, &Found { x: 0.0, y: 70.0, w: 30.0, h: 10.0, confidence: 1.0 }, 0.5);
        assert_eq!((cut.width, cut.height), (45, 15));
        // A box too thin to hold a pixel, or beyond the frame, is no picture.
        assert_eq!(cut_out(&frame, 100, 80, &Found { x: 10.2, y: 10.0, w: 0.2, h: 10.0, confidence: 1.0 }, 0.0), Picture::default());
        assert_eq!(cut_out(&frame, 100, 80, &Found { x: 300.0, y: 10.0, w: 10.0, h: 10.0, confidence: 1.0 }, 0.0), Picture::default());
    }

    // -- what the reader is offered --

    #[test]
    fn without_the_model_the_reader_is_offered_the_frame_thresholded_and_then_as_it_is() {
        let frame = noise(64, 48, 9);
        assert!(scan_frame(frame.clone(), 64, 48));
        assert_eq!(scan_next(), Some(threshold(&frame, 64, 48)));
        assert_eq!((scan_width(), scan_height()), (64, 48));
        assert_eq!(scan_next(), Some(frame));
        assert_eq!((scan_width(), scan_height()), (64, 48));
        assert_eq!(scan_next(), None);
        assert_eq!(scan_next(), None);
    }

    #[test]
    fn with_the_model_the_reader_is_offered_each_box_the_surest_first() {
        let side = MODEL_SIDE;
        let frame = noise(side, side, 5);
        assert!(scan_frame(frame.clone(), side as u32, side as u32));
        assert_eq!(scan_model_input(), model_input(&frame, side, side));
        let drawn = scan_found(&output(&[[100.0, 100.0, 40.0, 40.0, 0.5], [400.0, 300.0, 100.0, 60.0, 0.9]]), 2);
        assert_eq!(drawn, [350.0, 270.0, 100.0, 60.0, 0.9, 80.0, 80.0, 40.0, 40.0, 0.5]);

        let surest = cut_out(&frame, side, side, &Found { x: 350.0, y: 270.0, w: 100.0, h: 60.0, confidence: 0.9 }, MARGIN);
        assert_eq!((surest.width, surest.height), (110, 66));
        assert_eq!(scan_next(), Some(threshold(&surest.rgba, surest.width, surest.height)));
        assert_eq!((scan_width(), scan_height()), (110, 66));
        assert_eq!(scan_next(), Some(surest.rgba));
        assert_eq!((scan_width(), scan_height()), (110, 66));

        let other = cut_out(&frame, side, side, &Found { x: 80.0, y: 80.0, w: 40.0, h: 40.0, confidence: 0.5 }, MARGIN);
        assert_eq!(scan_next(), Some(threshold(&other.rgba, other.width, other.height)));
        assert_eq!((scan_width(), scan_height()), (44, 44));
        assert_eq!(scan_next(), Some(other.rgba));
        assert_eq!(scan_next(), None);
    }

    #[test]
    fn a_frame_the_model_finds_nothing_in_offers_the_reader_nothing() {
        assert!(scan_frame(noise(64, 64, 1), 64, 64));
        assert!(scan_found(&output(&[[100.0, 100.0, 40.0, 40.0, 0.1]]), 1).is_empty());
        assert_eq!(scan_next(), None);
    }

    #[test]
    fn a_new_frame_takes_the_place_of_the_one_in_hand() {
        assert!(scan_frame(noise(64, 64, 1), 64, 64));
        scan_found(&output(&[[30.0, 30.0, 20.0, 20.0, 0.9]]), 1);
        assert!(scan_next().is_some());
        // The next frame, on which the model is not run.
        let frame = noise(32, 32, 2);
        assert!(scan_frame(frame.clone(), 32, 32));
        assert_eq!(scan_next(), Some(threshold(&frame, 32, 32)));
        assert_eq!(scan_next(), Some(frame));
        assert_eq!(scan_next(), None);
    }

    #[test]
    fn bytes_that_are_not_a_picture_of_the_size_said_are_refused() {
        assert!(scan_frame(noise(8, 8, 1), 8, 8));
        assert!(!scan_frame(vec![0; 10], 8, 8));
        assert_eq!(scan_next(), None, "the frame before it is not in hand either");
        assert!(!scan_frame(Vec::new(), 0, 0));
        assert!(!scan_frame(vec![0; 16], 0, 4));
        assert_eq!(scan_model_input().len(), 3 * MODEL_SIDE * MODEL_SIDE);
        assert!(scan_found(&[], 0).is_empty());
        assert_eq!(scan_next(), None);
    }
}
