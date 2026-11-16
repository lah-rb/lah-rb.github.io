//! Shared data types used by player_doc and crdt modules.

use serde::{Deserialize, Serialize};

/// A single turn alarm countdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alarm {
    /// Stable identity of this timer. It travels with the timer between a
    /// player's own timers and a room's shared timers, so the same timer held
    /// by both players merges into one instead of appearing twice. Empty only
    /// for timers saved before ids existed.
    #[serde(default)]
    pub id: String,
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

/// A new timer id: 12 hex characters of randomness, unique across players and
/// page loads for any realistic number of timers.
pub fn new_alarm_id() -> String {
    format!("{:012x}", fastrand::u64(..) & 0xFFFF_FFFF_FFFF)
}

/// Whether `id` is safe to use as a timer id. Ids arrive from the other player
/// through the room document and are written into HTML attributes, so only a
/// conservative character set is accepted.
pub fn is_valid_alarm_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ids_are_valid_and_distinct() {
        let a = new_alarm_id();
        let b = new_alarm_id();
        assert_eq!(a.len(), 12);
        assert!(is_valid_alarm_id(&a));
        assert_ne!(a, b);
    }

    #[test]
    fn ids_that_could_break_out_of_markup_are_rejected() {
        assert!(is_valid_alarm_id("a1b2-c3_d4"));
        assert!(!is_valid_alarm_id(""));
        assert!(!is_valid_alarm_id("x');alert(1);('"));
        assert!(!is_valid_alarm_id("\"><script>"));
        assert!(!is_valid_alarm_id(&"a".repeat(65)));
    }
}
