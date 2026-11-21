//! Game state module — in-memory game state for damage tracking, turn tracking,
//! and state persistence. State lives in WASM memory (thread_local) for the
//! lifetime of the Web Worker.
//!
//! Phase 3b: Single-player state management (local user state).
//! Phase 4: Room state for multiplayer. Shared state (timers, fists combat)
//! lives in a yrs room document synced through the WebSocket relay.

pub mod backup;
pub mod card_panel;
pub mod connection;
pub mod crdt;
pub mod crypto;
pub mod damage;
pub mod device_sync;
pub mod fists;
#[cfg(test)]
mod history_sim;
pub mod matchup;
pub mod player_doc;
pub mod recipes;
pub mod room;
pub mod state;
pub mod turns;
