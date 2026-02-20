//! Game state module — in-memory game state for damage tracking, turn tracking,
//! and state persistence. State lives in WASM memory (thread_local) for the
//! lifetime of the Web Worker.
//!
//! Phase 3b: Single-player state management.
//! Phase 4 prep: All structs derive Serialize/Deserialize for WebRTC diffs.

pub mod damage;
pub mod state;
pub mod turns;
