//! Game state module — in-memory game state for damage tracking, turn tracking,
//! and state persistence. State lives in WASM memory (thread_local) for the
//! lifetime of the Web Worker.
//!
//! Phase 3b: Single-player state management (local user state).
//! Phase 4: Room state (global/shared) for multiplayer via WebRTC.
//! All structs derive Serialize/Deserialize for WebRTC diffs.

pub mod crdt;
pub mod damage;
pub mod room;
pub mod state;
pub mod turns;
