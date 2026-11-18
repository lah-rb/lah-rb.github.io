//! A card's preview picture: what a shared link to the card shows.
//!
//! When a card's address is pasted into a chat or a post, the app showing it
//! fetches the page and looks in its head for a picture (`og:image`, see
//! `site::render_head`). Those apps read JPEG. The card art is kept as JPEG
//! XL only, which they do not read, so the build makes a JPEG of each card.
//!
//! A visitor's browser never requests a preview: it is named in a head tag,
//! not shown on the page, and the service worker does not precache it.
//!
//! ## What the picture is
//!
//! The whole art, never cropped, scaled to fit a canvas of [`WIDTH`] ×
//! [`HEIGHT`] and centred on a mat. That canvas is the shape link previews
//! are shown in; an app given another shape cuts it to this one, and for
//! upright art that leaves a strip across the middle of the figure. The mat
//! is the art's own average colour, darkened, so it suits every card without
//! anyone choosing it.
//!
//! ## Made once
//!
//! Decoding and scaling a card's art takes far longer than anything else the
//! build does for it, and the result depends only on the art and on the
//! constants here. [`cache_key`] names a preview by both, so the build
//! (`src/bin/card-pages.rs`) keeps each one and makes it again only when the
//! art or the recipe changes.
//!
//! Like `site`, this is pure: bytes in, bytes out. It is left out of the
//! browser's engine.

use jpeg_encoder::{ChromaSubsamplingMethod, ColorType, Encoder};

use crate::image::decode_to_rgb;

/// The canvas. Every preview is exactly this size.
pub const WIDTH: u32 = 1200;
pub const HEIGHT: u32 = 630;

/// How bright the mat is, as a share of the art's average colour.
const MAT: f32 = 0.38;

/// JPEG quality, 1 to 100.
const QUALITY: u8 = 82;

/// Raise this when [`render`] changes what it draws in a way the constants
/// above do not show, so previews made by the old code are not kept.
const RECIPE: u32 = 1;

/// Everything besides the art that decides what a preview looks like.
fn recipe() -> String {
    format!(
        "preview {}: {}x{}, mat {}, quality {}",
        RECIPE, WIDTH, HEIGHT, MAT, QUALITY
    )
}

/// A name for the preview of this art made by this recipe: the same for the
/// same art and recipe, different if either changes.
pub fn cache_key(art: &[u8]) -> String {
    let mut hash = hmac_sha256::Hash::new();
    hash.update(recipe().as_bytes());
    hash.update([0u8]);
    hash.update(art);
    hash.finalize()[..16]
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

/// The size art of `width` × `height` is drawn at: the largest that fits the
/// canvas and keeps the art's shape. Art is never drawn larger than it is.
fn fit(width: u32, height: u32) -> (u32, u32) {
    let scale = (WIDTH as f64 / width as f64)
        .min(HEIGHT as f64 / height as f64)
        .min(1.0);
    let side = |length: u32, limit: u32| ((length as f64 * scale).round() as u32).clamp(1, limit);
    (side(width, WIDTH), side(height, HEIGHT))
}

/// For scaling `from` samples down to `to`: the input samples each output
/// sample covers (the first one's index, then how much of each), so that an
/// output sample is the average of exactly the stretch of input it stands
/// for. Inputs covered in part count in part.
fn spans(from: usize, to: usize) -> Vec<(usize, Vec<f32>)> {
    let step = from as f64 / to as f64;
    (0..to)
        .map(|i| {
            let start = i as f64 * step;
            let end = ((i + 1) as f64 * step).min(from as f64);
            let first = (start.floor() as usize).min(from - 1);
            let last = ((end.ceil() as usize).max(first + 1)).min(from);
            let mut weights: Vec<f32> = (first..last)
                .map(|j| ((end.min(j as f64 + 1.0) - start.max(j as f64)).max(0.0)) as f32)
                .collect();
            let total: f32 = weights.iter().sum();
            if total > 0.0 {
                weights.iter_mut().for_each(|w| *w /= total);
            } else {
                weights = vec![1.0];
            }
            (first, weights)
        })
        .collect()
}

/// Scale interleaved RGB (3 bytes a pixel, rows top to bottom) to a new
/// size by averaging, across and then down.
fn scale(width: u32, height: u32, rgb: &[u8], to_width: u32, to_height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let (tw, th) = (to_width as usize, to_height as usize);

    // Across: every row from `w` to `tw` samples.
    let across = spans(w, tw);
    let mut rows = vec![0f32; tw * h * 3];
    for y in 0..h {
        let source = &rgb[y * w * 3..(y + 1) * w * 3];
        let target = &mut rows[y * tw * 3..(y + 1) * tw * 3];
        for (x, (first, weights)) in across.iter().enumerate() {
            let mut sum = [0f32; 3];
            for (k, weight) in weights.iter().enumerate() {
                let p = (first + k) * 3;
                sum[0] += source[p] as f32 * weight;
                sum[1] += source[p + 1] as f32 * weight;
                sum[2] += source[p + 2] as f32 * weight;
            }
            target[x * 3..x * 3 + 3].copy_from_slice(&sum);
        }
    }

    // Down: every column from `h` to `th` samples.
    let down = spans(h, th);
    let mut out = vec![0u8; tw * th * 3];
    let mut sum = vec![0f32; tw * 3];
    for (y, (first, weights)) in down.iter().enumerate() {
        sum.fill(0.0);
        for (k, weight) in weights.iter().enumerate() {
            let source = &rows[(first + k) * tw * 3..(first + k + 1) * tw * 3];
            for (total, sample) in sum.iter_mut().zip(source) {
                *total += sample * weight;
            }
        }
        let target = &mut out[y * tw * 3..(y + 1) * tw * 3];
        for (byte, total) in target.iter_mut().zip(&sum) {
            *byte = (total + 0.5).clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// The mat's colour for this art: its average colour, darkened.
fn mat_colour(rgb: &[u8]) -> [u8; 3] {
    let mut sum = [0u64; 3];
    for pixel in rgb.chunks_exact(3) {
        sum[0] += pixel[0] as u64;
        sum[1] += pixel[1] as u64;
        sum[2] += pixel[2] as u64;
    }
    let pixels = (rgb.len() / 3).max(1) as f32;
    sum.map(|channel| (channel as f32 / pixels * MAT + 0.5).clamp(0.0, 255.0) as u8)
}

/// Draw the preview of art given as interleaved RGB: the canvas, as
/// interleaved RGB of [`WIDTH`] × [`HEIGHT`].
pub fn compose(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 || rgb.len() != width as usize * height as usize * 3 {
        return Err(format!(
            "art of {} x {} does not have {} bytes",
            width,
            height,
            rgb.len()
        ));
    }
    let (art_width, art_height) = fit(width, height);
    let art = scale(width, height, rgb, art_width, art_height);

    let (cw, ch) = (WIDTH as usize, HEIGHT as usize);
    let (aw, ah) = (art_width as usize, art_height as usize);
    let mat = mat_colour(rgb);
    let mut canvas: Vec<u8> = mat.iter().copied().cycle().take(cw * ch * 3).collect();
    let (left, top) = ((cw - aw) / 2, (ch - ah) / 2);
    for y in 0..ah {
        let at = ((top + y) * cw + left) * 3;
        canvas[at..at + aw * 3].copy_from_slice(&art[y * aw * 3..(y + 1) * aw * 3]);
    }
    Ok(canvas)
}

/// Interleaved RGB as a JPEG file.
pub fn encode_jpeg(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
    let (Ok(w), Ok(h)) = (u16::try_from(width), u16::try_from(height)) else {
        return Err(format!("{} x {} is too large for a JPEG", width, height));
    };
    let mut file = Vec::new();
    let mut encoder = Encoder::new(&mut file, QUALITY);
    // Smaller files for the same picture: several passes over the image, and
    // code tables fitted to it.
    encoder.set_progressive(true);
    encoder.set_optimized_huffman_tables(true);
    // Colour is stored at half size; average each block, do not pick a pixel.
    encoder.set_chroma_subsampling_method(ChromaSubsamplingMethod::Average);
    encoder
        .encode(rgb, w, h, ColorType::Rgb)
        .map_err(|e| format!("JPEG encoding failed: {}", e))?;
    Ok(file)
}

/// A card's preview, as a JPEG file, from its art, a JPEG XL file.
pub fn render(art: &[u8]) -> Result<Vec<u8>, String> {
    let (width, height, rgb) = decode_to_rgb(art)?;
    let canvas = compose(width, height, &rgb)?;
    encode_jpeg(WIDTH, HEIGHT, &canvas)
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Art of one colour.
    fn flat(width: u32, height: u32, colour: [u8; 3]) -> Vec<u8> {
        colour
            .iter()
            .copied()
            .cycle()
            .take(width as usize * height as usize * 3)
            .collect()
    }

    fn pixel(canvas: &[u8], x: u32, y: u32) -> [u8; 3] {
        let at = (y as usize * WIDTH as usize + x as usize) * 3;
        [canvas[at], canvas[at + 1], canvas[at + 2]]
    }

    /// The size a JPEG file says it is: its frame header.
    fn jpeg_size(file: &[u8]) -> (u32, u32) {
        let mut at = 2;
        while at + 9 < file.len() {
            assert_eq!(file[at], 0xFF, "a marker at {}", at);
            let marker = file[at + 1];
            let length = u16::from_be_bytes([file[at + 2], file[at + 3]]) as usize;
            // Start of frame, baseline (C0) or progressive (C2).
            if marker == 0xC0 || marker == 0xC2 {
                let height = u16::from_be_bytes([file[at + 5], file[at + 6]]) as u32;
                let width = u16::from_be_bytes([file[at + 7], file[at + 8]]) as u32;
                return (width, height);
            }
            at += 2 + length;
        }
        panic!("no frame header in the JPEG");
    }

    #[test]
    fn upright_art_fills_the_canvas_height() {
        // The size of the card art.
        assert_eq!(fit(888, 1444), (387, 630));
    }

    #[test]
    fn wide_art_fills_what_it_can_without_losing_its_shape() {
        assert_eq!(fit(1444, 888), (1024, 630));
        // Wider than the canvas: the width is the limit.
        assert_eq!(fit(4800, 630), (1200, 158));
    }

    #[test]
    fn art_is_never_drawn_larger_than_it_is() {
        assert_eq!(fit(300, 200), (300, 200));
        assert_eq!(fit(1, 1), (1, 1));
        assert_eq!(fit(WIDTH, HEIGHT), (WIDTH, HEIGHT));
    }

    #[test]
    fn every_output_sample_averages_exactly_its_stretch_of_input() {
        for (from, to) in [(888, 387), (1444, 630), (1444, 1024), (10, 10), (7, 3), (3, 1), (1, 1)] {
            let spans = spans(from, to);
            assert_eq!(spans.len(), to);
            // How much each input sample counts in all, in input widths: an
            // even scaling uses every input sample equally.
            let mut used = vec![0f64; from];
            for (first, weights) in &spans {
                let total: f32 = weights.iter().sum();
                assert!((total - 1.0).abs() < 1e-4, "{} to {}", from, to);
                for (k, weight) in weights.iter().enumerate() {
                    used[first + k] += *weight as f64 * from as f64 / to as f64;
                }
            }
            for (j, amount) in used.iter().enumerate() {
                assert!((amount - 1.0).abs() < 1e-3, "{} to {}: input {} counts {}", from, to, j, amount);
            }
        }
    }

    #[test]
    fn scaling_keeps_a_flat_colour_and_the_overall_brightness() {
        let colour = [200, 120, 40];
        let scaled = scale(888, 1444, &flat(888, 1444, colour), 387, 630);
        assert_eq!(scaled.len(), 387 * 630 * 3);
        assert!(scaled.chunks_exact(3).all(|p| p == colour));

        // Left half black, right half white: the average stays mid grey.
        let mut halves = vec![0u8; 100 * 10 * 3];
        for y in 0..10 {
            for x in 50..100 {
                let at = (y * 100 + x) * 3;
                halves[at..at + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        let scaled = scale(100, 10, &halves, 43, 4);
        let mean = scaled.iter().map(|&b| b as f64).sum::<f64>() / scaled.len() as f64;
        assert!((mean - 127.5).abs() < 1.0, "mean {}", mean);
        // And the edge is still an edge: first sample black, last white.
        assert_eq!(&scaled[..3], &[0, 0, 0]);
        assert_eq!(&scaled[42 * 3..43 * 3], &[255, 255, 255]);
    }

    #[test]
    fn the_whole_art_is_centred_on_a_mat_of_its_own_colour_darkened() {
        let colour = [200, 100, 50];
        let canvas = compose(888, 1444, &flat(888, 1444, colour)).unwrap();
        assert_eq!(canvas.len(), (WIDTH * HEIGHT * 3) as usize);

        // 387 wide in a 1200 canvas: art from 406 up to 793.
        let mat = [76, 38, 19];
        assert_eq!(pixel(&canvas, 0, 0), mat);
        assert_eq!(pixel(&canvas, 405, 315), mat);
        assert_eq!(pixel(&canvas, 406, 0), colour);
        assert_eq!(pixel(&canvas, 600, 315), colour);
        assert_eq!(pixel(&canvas, 792, 629), colour);
        assert_eq!(pixel(&canvas, 793, 315), mat);
        assert_eq!(pixel(&canvas, WIDTH - 1, HEIGHT - 1), mat);
    }

    #[test]
    fn small_art_is_centred_at_its_own_size() {
        let canvas = compose(200, 100, &flat(200, 100, [255, 255, 255])).unwrap();
        assert_eq!(pixel(&canvas, 500, 265), [255, 255, 255]);
        assert_eq!(pixel(&canvas, 699, 364), [255, 255, 255]);
        assert_eq!(pixel(&canvas, 499, 265), [97, 97, 97]);
        assert_eq!(pixel(&canvas, 700, 364), [97, 97, 97]);
    }

    #[test]
    fn art_whose_size_and_bytes_disagree_is_refused() {
        assert!(compose(10, 10, &[0u8; 10]).is_err());
        assert!(compose(0, 10, &[]).is_err());
    }

    #[test]
    fn the_file_is_a_jpeg_of_the_canvas_size() {
        let canvas = compose(888, 1444, &flat(888, 1444, [30, 90, 160])).unwrap();
        let file = encode_jpeg(WIDTH, HEIGHT, &canvas).unwrap();
        assert_eq!(&file[..2], &[0xFF, 0xD8], "starts as a JPEG");
        assert_eq!(&file[file.len() - 2..], &[0xFF, 0xD9], "ends as a JPEG");
        assert_eq!(jpeg_size(&file), (WIDTH, HEIGHT));
    }

    #[test]
    fn the_same_art_gets_the_same_name_and_other_art_another() {
        let key = cache_key(b"some art");
        assert_eq!(key, cache_key(b"some art"));
        assert_ne!(key, cache_key(b"other art"));
        assert_eq!(key.len(), 32);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn a_real_cards_art_becomes_a_preview() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/images/cloth.jxl");
        let art = std::fs::read(path).expect("the art of the Cloth card");
        let file = render(&art).expect("the art decodes");
        assert_eq!(jpeg_size(&file), (WIDTH, HEIGHT));
        // Making it again gives the same file: the cache may stand in for it.
        assert_eq!(file, render(&art).unwrap());
    }

    #[test]
    fn a_file_that_is_not_art_is_refused() {
        assert!(render(b"not a picture").is_err());
    }
}
