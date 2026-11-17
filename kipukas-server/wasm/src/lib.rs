//! The engine as the WASM module the browser loads.
//!
//! All of the code is in the engine crate (`kipukas-server`, one folder up).
//! What the browser can call is whatever the engine marks `#[wasm_bindgen]`:
//! `handle_request` (src/lib.rs) and `decode_jxl` (src/image.rs). Using the
//! engine here is what links it into this module.

pub use kipukas_server::*;
