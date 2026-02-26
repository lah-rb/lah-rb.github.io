//! Shared data types used by player_doc and crdt modules.

use serde::{Deserialize, Serialize};

/// A single turn alarm countdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alarm {
    /// Remaining diel cycles. Decrements on each tick. 0 = complete, removed on next tick.
    pub remaining: i32,
    /// Optional human-readable name for the timer.
    #[serde(default)]
    pub name: String,
    /// Color set identifier: "red", "green", "blue", "yellow", "pink".
    #[serde(default = "default_color_set")]
    pub color_set: String,
}

fn default_color_set() -> String {
    "red".to_string()
}
