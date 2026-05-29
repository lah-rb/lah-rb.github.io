//! Build-time DC-prefix probe (native only).
//!
//! For each `.jxl` path given as an argument, prints the smallest leading-byte
//! count that decodes to the full-canvas DC/low-res preview — the exact value
//! the Service Worker will Range-request for grid thumbnails. Reuses the same
//! `jxl-oxide` decode path as the in-browser worker so the two never disagree.
//!
//! Usage: `cargo run --release --bin jxl_probe -- a.jxl b.jxl ...`
//! Output (stdout): JSON object mapping file basename → dc_bytes.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use kipukas_server::image::preview_prefix_len;
    use std::path::Path;

    // Require each channel mean within this delta of the full image's, so the
    // preview is both populated (not black) and colored (chroma loaded), not
    // the grayscale early-progressive pass.
    const MAX_CHANNEL_DELTA: f64 = 6.0;

    let mut entries: Vec<String> = Vec::new();
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        let dc = preview_prefix_len(&bytes, MAX_CHANNEL_DELTA)
            .unwrap_or_else(|| panic!("{path}: file did not decode at all"));
        let name = Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&path)
            .replace('"', "\\\"");
        entries.push(format!("  \"{name}\": {dc}"));
    }
    println!("{{\n{}\n}}", entries.join(",\n"));
}

// The wasm build compiles all targets; keep the binary buildable (and inert)
// for wasm32 so `wasm-pack build` doesn't choke on the native-only probe.
#[cfg(target_arch = "wasm32")]
fn main() {}
