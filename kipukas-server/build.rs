//! Builds the card catalog into the engine.
//!
//! Reads the card files in `../cards/` and writes them as Rust for
//! `src/catalog.rs` to include. Cargo runs this before compiling the engine,
//! and again whenever a card file changes, so the engine is always compiled
//! from the card files as they are. How a card file is read, and what makes
//! one wrong, is in the `catalog/` crate.
//!
//! A card file that is wrong fails the build, with every problem listed.

use std::env;
use std::fs;
use std::path::Path;
use std::process::exit;

fn main() {
    let engine = env::var("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR");
    let cards = Path::new(&engine).join("..").join("cards");
    // The folder, so that a card added or removed is noticed too.
    println!("cargo:rerun-if-changed={}", cards.display());
    println!("cargo:rerun-if-changed=build.rs");

    match kipukas_catalog::read_cards(&cards) {
        Ok(catalog) => {
            let out = env::var("OUT_DIR").expect("Cargo sets OUT_DIR");
            fs::write(Path::new(&out).join("cards.rs"), kipukas_catalog::emit(&catalog))
                .expect("the catalog can be written to Cargo's output folder");
        }
        Err(problems) => {
            eprintln!("\nThe card files cannot be built into the engine:\n");
            for problem in &problems {
                eprintln!("  {}", problem);
            }
            eprintln!();
            exit(1);
        }
    }
}
