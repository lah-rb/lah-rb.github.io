//! Global game state container.
//!
//! Uses `thread_local!` + `RefCell` for safe mutable access in single-threaded
//! WASM. The Web Worker keeps the WASM module alive, so state persists across
//! `handle_request` calls for the entire browser session.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;

/// Per-card damage state: tracks which keal means slots are checked and wasted status.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CardDamageState {
    /// Indexed by sequential slot number (1-based, matching the UI).
    /// Key is the slot index, value is whether it's checked.
    pub slots: HashMap<u8, bool>,
    /// Whether the card is wasted (all keal means defeated + wasted checked).
    pub wasted: bool,
}

/// A single turn alarm countdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alarm {
    /// Remaining diel cycles. Decrements on each tick. 0 = complete, removed on next tick.
    pub remaining: i32,
}

/// Complete game state — everything needed to reconstruct the UI and sync with
/// another player in Phase 4.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    /// Per-card damage tracking, keyed by card slug.
    pub cards: HashMap<String, CardDamageState>,
    /// Active turn alarms.
    pub alarms: Vec<Alarm>,
    /// Whether the alarm panel is visible.
    pub show_alarms: bool,
}

impl Default for GameState {
    fn default() -> Self {
        Self {
            cards: HashMap::new(),
            alarms: Vec::new(),
            show_alarms: true, // Alarms visible by default
        }
    }
}

thread_local! {
    static STATE: RefCell<GameState> = RefCell::new(GameState {
        cards: HashMap::new(),
        alarms: Vec::new(),
        show_alarms: true,
    });
}

/// Execute a closure with read access to the game state.
pub fn with_state<F, R>(f: F) -> R
where
    F: FnOnce(&GameState) -> R,
{
    STATE.with(|s| f(&s.borrow()))
}

/// Execute a closure with mutable access to the game state.
pub fn with_state_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut GameState) -> R,
{
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// Replace the entire game state (used by import).
pub fn replace_state(new_state: GameState) {
    STATE.with(|s| {
        *s.borrow_mut() = new_state;
    });
}

/// Export the entire game state as JSON.
pub fn export_state_json() -> String {
    with_state(|state| serde_json::to_string(state).unwrap_or_else(|_| "{}".to_string()))
}

/// Import game state from JSON. Returns Ok(()) on success.
pub fn import_state_json(json: &str) -> Result<(), String> {
    let new_state: GameState =
        serde_json::from_str(json).map_err(|e| format!("Invalid game state JSON: {}", e))?;
    replace_state(new_state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_empty() {
        with_state(|s| {
            assert!(s.cards.is_empty());
            assert!(s.alarms.is_empty());
            assert!(s.show_alarms);
        });
    }

    #[test]
    fn state_roundtrip_json() {
        let mut state = GameState::default();
        state.show_alarms = true;
        state.alarms.push(Alarm { remaining: 5 });
        let mut damage = CardDamageState::default();
        damage.slots.insert(1, true);
        damage.slots.insert(2, false);
        damage.wasted = false;
        state.cards.insert("test_card".to_string(), damage);

        let json = serde_json::to_string(&state).unwrap();
        let restored: GameState = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.alarms.len(), 1);
        assert_eq!(restored.alarms[0].remaining, 5);
        assert!(restored.cards.contains_key("test_card"));
        assert_eq!(restored.cards["test_card"].slots[&1], true);
        assert_eq!(restored.cards["test_card"].slots[&2], false);
    }

    #[test]
    fn import_export_roundtrip() {
        // Set some state
        with_state_mut(|s| {
            s.alarms.push(Alarm { remaining: 3 });
            s.cards.insert(
                "roundtrip_test".to_string(),
                CardDamageState {
                    slots: HashMap::from([(1, true)]),
                    wasted: false,
                },
            );
        });

        let json = export_state_json();
        assert!(json.contains("roundtrip_test"));

        // Clear and reimport
        replace_state(GameState::default());
        with_state(|s| assert!(s.cards.is_empty()));

        import_state_json(&json).unwrap();
        with_state(|s| {
            assert!(s.cards.contains_key("roundtrip_test"));
            assert_eq!(s.alarms.len(), 1);
        });

        // Clean up thread-local state for other tests
        replace_state(GameState::default());
    }

    #[test]
    fn import_invalid_json_returns_error() {
        let result = import_state_json("not valid json {{{");
        assert!(result.is_err());
    }
}
