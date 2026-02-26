//! Player Document — persistent yrs CRDT document for all local player data.
//!
//! Replaces the serde_json `GameState` as the authoritative store for per-player
//! data (card damage, alarms, settings). Created once on first visit, persisted
//! to localStorage as base64 binary, restored on every page load.
//!
//! ## Doc Structure
//!
//! ```text
//! PLAYER_DOC (yrs::Doc)
//! ├── "cards" (YMap)
//! │   └── "brox_the_defiant" (YMap)
//! │       ├── "slots" (YArray<bool>)  — index 0 = slot 1
//! │       └── "wasted" (bool)
//! ├── "alarms" (YArray<YMap>)
//! │   └── [0] { "remaining": i32, "name": String, "color_set": String }
//! └── "settings" (YMap)
//!     └── "show_alarms" (bool)
//! ```
//!
//! ## Lifecycle
//!
//! - **First visit:** Fresh Doc created, seeded from any existing `kipukas_game_state` JSON
//! - **Every page load:** Restored from `kipukas_player_doc` localStorage key (base64 binary)
//! - **Every mutation:** Auto-persisted to localStorage via worker → main thread message
//! - **Multiplayer:** Independent of ROOM_DOC; alarms bridge via `crdt.rs` seed/export

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::cell::RefCell;
use yrs::updates::decoder::Decode;
use yrs::{Any, Array, ArrayPrelim, Doc, Map, MapPrelim, ReadTxn, StateVector, Transact, Update, WriteTxn};

use crate::game::state::Alarm;

thread_local! {
    static PLAYER_DOC: RefCell<Doc> = RefCell::new(new_player_doc());
}

/// Create a fresh player Doc with all root types pre-created.
fn new_player_doc() -> Doc {
    let doc = Doc::new();
    {
        let mut txn = doc.transact_mut();
        txn.get_or_insert_map("cards");
        txn.get_or_insert_array("alarms");
        let settings = txn.get_or_insert_map("settings");
        // Default: show_alarms = true
        settings.insert(&mut txn, "show_alarms", Any::from(true));
    }
    doc
}

// ── Doc lifecycle ──────────────────────────────────────────────────

/// Initialize a fresh player Doc (reset to empty defaults).
pub fn init_player_doc() {
    PLAYER_DOC.with(|cell| {
        *cell.borrow_mut() = new_player_doc();
    });
}

// ── Doc persistence ────────────────────────────────────────────────

/// Encode the full Doc state as a URL-safe base64 string for localStorage.
pub fn encode_full_state() -> String {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let state = doc.transact().encode_diff_v1(&StateVector::default());
        URL_SAFE_NO_PAD.encode(&state)
    })
}

/// Restore Doc from a previously persisted URL-safe base64 state.
pub fn restore_from_state(state_b64: &str) -> Result<(), String> {
    if state_b64.is_empty() {
        return Ok(());
    }
    let state_bytes = URL_SAFE_NO_PAD
        .decode(state_b64)
        .map_err(|e| format!("base64 decode error: {}", e))?;
    let update = Update::decode_v1(&state_bytes)
        .map_err(|e| format!("state decode error: {}", e))?;

    PLAYER_DOC.with(|cell| {
        // Replace with fresh doc then apply the update so root types exist
        let doc = Doc::new();
        {
            let mut txn = doc.transact_mut();
            txn.get_or_insert_map("cards");
            txn.get_or_insert_array("alarms");
            txn.get_or_insert_map("settings");
            txn.apply_update(update)
                .map_err(|e| format!("restore error: {}", e))?;
            Ok::<(), String>(())
        }?;
        *cell.borrow_mut() = doc;
        Ok(())
    })
}

// ── Card damage accessors ──────────────────────────────────────────

/// Ensure a card entry exists in the "cards" YMap with the right number of slots.
/// Slots are stored as a YArray<bool> (index 0 = slot 1).
pub fn ensure_card_state(slug: &str, total_slots: u8) {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let cards = doc.get_or_insert_map("cards");
        let mut txn = doc.transact_mut();

        // Check if this card already exists
        let existing = cards.get(&txn, slug);
        match existing {
            Some(yrs::Out::YMap(card_map)) => {
                // Card exists — ensure slots array has the right length
                match card_map.get(&txn, "slots") {
                    Some(yrs::Out::YArray(slots_arr)) => {
                        let current_len = slots_arr.len(&txn);
                        // Extend if needed (e.g. card data changed)
                        for _ in current_len..total_slots as u32 {
                            slots_arr.push_back(&mut txn, Any::from(false));
                        }
                    }
                    _ => {
                        // No slots array — create it
                        let bools: Vec<Any> = (0..total_slots).map(|_| Any::from(false)).collect();
                        card_map.insert(&mut txn, "slots", ArrayPrelim::from(bools));
                    }
                }
                // Ensure wasted key exists
                if card_map.get(&txn, "wasted").is_none() {
                    card_map.insert(&mut txn, "wasted", Any::from(false));
                }
            }
            _ => {
                // Card doesn't exist — create it
                let bools: Vec<Any> = (0..total_slots).map(|_| Any::from(false)).collect();
                let card_map = MapPrelim::from([
                    ("wasted".to_string(), Any::from(false)),
                ]);
                cards.insert(&mut txn, slug, card_map);
                // Now get the created map and add the slots array
                if let Some(yrs::Out::YMap(map_ref)) = cards.get(&txn, slug) {
                    map_ref.insert(&mut txn, "slots", ArrayPrelim::from(bools));
                }
            }
        }
    });
}

/// Read a single slot value (1-based). Returns false if not found.
pub fn get_slot(slug: &str, slot: u8) -> bool {
    if slot == 0 {
        return false;
    }
    let idx = (slot - 1) as u32;
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let cards = doc.get_or_insert_map("cards");
        let txn = doc.transact();
        match cards.get(&txn, slug) {
            Some(yrs::Out::YMap(card_map)) => {
                match card_map.get(&txn, "slots") {
                    Some(yrs::Out::YArray(slots_arr)) => {
                        match slots_arr.get(&txn, idx) {
                            Some(yrs::Out::Any(Any::Bool(b))) => b,
                            _ => false,
                        }
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    })
}

/// Toggle a specific damage slot (1-based). Returns the new checked state.
pub fn toggle_slot(slug: &str, slot: u8) -> bool {
    if slot == 0 {
        return false;
    }
    let idx = (slot - 1) as u32;
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let cards = doc.get_or_insert_map("cards");
        let mut txn = doc.transact_mut();
        match cards.get(&txn, slug) {
            Some(yrs::Out::YMap(card_map)) => {
                match card_map.get(&txn, "slots") {
                    Some(yrs::Out::YArray(slots_arr)) => {
                        let current = match slots_arr.get(&txn, idx) {
                            Some(yrs::Out::Any(Any::Bool(b))) => b,
                            _ => false,
                        };
                        let new_val = !current;
                        // YArray doesn't have a direct "set" — remove and insert
                        if idx < slots_arr.len(&txn) {
                            slots_arr.remove(&mut txn, idx);
                            slots_arr.insert(&mut txn, idx, Any::from(new_val));
                        }
                        new_val
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    })
}

/// Read the wasted state for a card.
pub fn is_wasted(slug: &str) -> bool {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let cards = doc.get_or_insert_map("cards");
        let txn = doc.transact();
        match cards.get(&txn, slug) {
            Some(yrs::Out::YMap(card_map)) => {
                match card_map.get(&txn, "wasted") {
                    Some(yrs::Out::Any(Any::Bool(b))) => b,
                    _ => false,
                }
            }
            _ => false,
        }
    })
}

/// Toggle the wasted state for a card. Returns the new wasted state.
pub fn toggle_wasted(slug: &str) -> bool {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let cards = doc.get_or_insert_map("cards");
        let mut txn = doc.transact_mut();
        match cards.get(&txn, slug) {
            Some(yrs::Out::YMap(card_map)) => {
                let current = match card_map.get(&txn, "wasted") {
                    Some(yrs::Out::Any(Any::Bool(b))) => b,
                    _ => false,
                };
                let new_val = !current;
                card_map.insert(&mut txn, "wasted", Any::from(new_val));
                new_val
            }
            _ => {
                // Card doesn't exist — create minimal entry with wasted=true
                let card_map = MapPrelim::from([
                    ("wasted".to_string(), Any::from(true)),
                ]);
                cards.insert(&mut txn, slug, card_map);
                true
            }
        }
    })
}

/// Clear damage state for a specific card.
pub fn clear_card(slug: &str) {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let cards = doc.get_or_insert_map("cards");
        let mut txn = doc.transact_mut();
        cards.remove(&mut txn, slug);
    });
}

/// Clear damage state for ALL cards.
pub fn clear_all() {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let cards = doc.get_or_insert_map("cards");
        let mut txn = doc.transact_mut();
        // Collect keys first, then remove
        let keys: Vec<String> = cards.keys(&txn).map(|k| k.to_string()).collect();
        for key in keys {
            cards.remove(&mut txn, &key);
        }
    });
}

/// Check if a card has any damage state stored.
pub fn has_card_state(slug: &str) -> bool {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let cards = doc.get_or_insert_map("cards");
        let txn = doc.transact();
        cards.get(&txn, slug).is_some()
    })
}

// ── Alarm accessors ────────────────────────────────────────────────

/// Validate color set, defaulting to "red" if invalid.
fn validate_color_set(color: &str) -> &str {
    match color {
        "red" | "green" | "blue" | "yellow" | "pink" => color,
        _ => "red",
    }
}

/// Add a new alarm.
pub fn add_alarm(turns: i32, name: &str, color_set: &str) {
    let color = validate_color_set(color_set);
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let alarms = doc.get_or_insert_array("alarms");
        let mut txn = doc.transact_mut();
        let alarm_map = MapPrelim::from([
            ("remaining".to_string(), Any::from(turns as f64)),
            ("name".to_string(), Any::from(name.to_string())),
            ("color_set".to_string(), Any::from(color.to_string())),
        ]);
        let len = alarms.len(&txn);
        alarms.insert(&mut txn, len, alarm_map);
    });
}

/// Tick all alarms: decrement by 1, remove completed (were already at 0).
pub fn tick_alarms() {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let alarms = doc.get_or_insert_array("alarms");
        let mut txn = doc.transact_mut();
        let len = alarms.len(&txn);

        // First pass: collect indices to remove (were at 0)
        let mut to_remove: Vec<u32> = Vec::new();
        for i in 0..len {
            if let Some(yrs::Out::YMap(map)) = alarms.get(&txn, i) {
                if let Some(yrs::Out::Any(Any::Number(r))) = map.get(&txn, "remaining") {
                    if (r as i32) <= 0 {
                        to_remove.push(i);
                    }
                }
            }
        }

        // Remove expired (reverse order to preserve indices)
        for &idx in to_remove.iter().rev() {
            alarms.remove(&mut txn, idx);
        }

        // Second pass: decrement remaining alarms
        let new_len = alarms.len(&txn);
        for i in 0..new_len {
            if let Some(yrs::Out::YMap(map)) = alarms.get(&txn, i) {
                if let Some(yrs::Out::Any(Any::Number(r))) = map.get(&txn, "remaining") {
                    let remaining = r as i32;
                    map.insert(&mut txn, "remaining", Any::from((remaining - 1) as f64));
                }
            }
        }
    });
}

/// Remove an alarm by index.
pub fn remove_alarm(index: usize) {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let alarms = doc.get_or_insert_array("alarms");
        let mut txn = doc.transact_mut();
        let len = alarms.len(&txn);
        if (index as u32) < len {
            alarms.remove(&mut txn, index as u32);
        }
    });
}

/// Clear all alarms from the player doc.
pub fn clear_all_alarms() {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let alarms = doc.get_or_insert_array("alarms");
        let mut txn = doc.transact_mut();
        let len = alarms.len(&txn);
        // Remove in reverse order to preserve indices
        for i in (0..len).rev() {
            alarms.remove(&mut txn, i);
        }
    });
}

/// Read the current alarms as a Vec<Alarm>.
pub fn get_alarms() -> Vec<Alarm> {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let alarms = doc.get_or_insert_array("alarms");
        let txn = doc.transact();
        let len = alarms.len(&txn);
        let mut result = Vec::with_capacity(len as usize);

        for i in 0..len {
            if let Some(yrs::Out::YMap(map)) = alarms.get(&txn, i) {
                let remaining = match map.get(&txn, "remaining") {
                    Some(yrs::Out::Any(Any::Number(n))) => n as i32,
                    _ => 0,
                };
                let name = match map.get(&txn, "name") {
                    Some(yrs::Out::Any(Any::String(s))) => s.to_string(),
                    _ => String::new(),
                };
                let color_set = match map.get(&txn, "color_set") {
                    Some(yrs::Out::Any(Any::String(s))) => s.to_string(),
                    _ => "red".to_string(),
                };
                result.push(Alarm {
                    remaining,
                    name,
                    color_set,
                });
            }
        }

        result
    })
}

// ── Settings accessors ─────────────────────────────────────────────

/// Get whether the alarm panel is visible.
pub fn get_show_alarms() -> bool {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let settings = doc.get_or_insert_map("settings");
        let txn = doc.transact();
        match settings.get(&txn, "show_alarms") {
            Some(yrs::Out::Any(Any::Bool(b))) => b,
            _ => true, // default visible
        }
    })
}

/// Set whether the alarm panel is visible.
pub fn set_show_alarms(val: bool) {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let settings = doc.get_or_insert_map("settings");
        let mut txn = doc.transact_mut();
        settings.insert(&mut txn, "show_alarms", Any::from(val));
    });
}

// ── Migration from GameState JSON ──────────────────────────────────

/// One-time migration from old `kipukas_game_state` JSON into the player doc.
/// Parses the JSON, walks cards/alarms/show_alarms, writes into the yrs Doc.
pub fn migrate_from_game_state(json: &str) -> Result<(), String> {
    let gs: crate::game::state::GameState =
        serde_json::from_str(json).map_err(|e| format!("Invalid GameState JSON: {}", e))?;

    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let cards = doc.get_or_insert_map("cards");
        let alarms = doc.get_or_insert_array("alarms");
        let settings = doc.get_or_insert_map("settings");
        let mut txn = doc.transact_mut();

        // Migrate cards
        for (slug, card_state) in &gs.cards {
            // Find max slot index to determine array size
            let max_slot = card_state.slots.keys().copied().max().unwrap_or(0);
            let bools: Vec<Any> = (1..=max_slot)
                .map(|i| Any::from(card_state.slots.get(&i).copied().unwrap_or(false)))
                .collect();

            let card_map = MapPrelim::from([
                ("wasted".to_string(), Any::from(card_state.wasted)),
            ]);
            cards.insert(&mut txn, slug.as_str(), card_map);
            if let Some(yrs::Out::YMap(map_ref)) = cards.get(&txn, slug.as_str()) {
                map_ref.insert(&mut txn, "slots", ArrayPrelim::from(bools));
            }
        }

        // Migrate alarms
        for alarm in &gs.alarms {
            let alarm_map = MapPrelim::from([
                ("remaining".to_string(), Any::from(alarm.remaining as f64)),
                ("name".to_string(), Any::from(alarm.name.clone())),
                ("color_set".to_string(), Any::from(alarm.color_set.clone())),
            ]);
            let len = alarms.len(&txn);
            alarms.insert(&mut txn, len, alarm_map);
        }

        // Migrate settings
        settings.insert(&mut txn, "show_alarms", Any::from(gs.show_alarms));
    });

    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        init_player_doc();
    }

    // ── Card damage tests ──────────────────────────────────────────

    #[test]
    fn init_creates_empty_doc() {
        reset();
        assert!(!has_card_state("test"));
        assert!(get_alarms().is_empty());
        assert!(get_show_alarms());
    }

    #[test]
    fn ensure_card_state_creates_entry() {
        reset();
        ensure_card_state("test_card", 3);
        assert!(has_card_state("test_card"));
        assert!(!get_slot("test_card", 1));
        assert!(!get_slot("test_card", 2));
        assert!(!get_slot("test_card", 3));
        assert!(!is_wasted("test_card"));
    }

    #[test]
    fn toggle_slot_works() {
        reset();
        ensure_card_state("test", 3);
        assert!(toggle_slot("test", 1)); // false → true
        assert!(get_slot("test", 1));
        assert!(!toggle_slot("test", 1)); // true → false
        assert!(!get_slot("test", 1));
    }

    #[test]
    fn toggle_slot_zero_returns_false() {
        reset();
        assert!(!toggle_slot("test", 0));
    }

    #[test]
    fn toggle_wasted_works() {
        reset();
        ensure_card_state("test", 2);
        assert!(toggle_wasted("test")); // false → true
        assert!(is_wasted("test"));
        assert!(!toggle_wasted("test")); // true → false
        assert!(!is_wasted("test"));
    }

    #[test]
    fn toggle_wasted_creates_card_if_missing() {
        reset();
        assert!(toggle_wasted("new_card")); // creates card with wasted=true
        assert!(is_wasted("new_card"));
    }

    #[test]
    fn clear_card_removes_entry() {
        reset();
        ensure_card_state("card_a", 2);
        ensure_card_state("card_b", 2);
        toggle_slot("card_a", 1);
        clear_card("card_a");
        assert!(!has_card_state("card_a"));
        assert!(has_card_state("card_b"));
    }

    #[test]
    fn clear_all_removes_all_cards() {
        reset();
        ensure_card_state("card_a", 2);
        ensure_card_state("card_b", 2);
        toggle_slot("card_a", 1);
        toggle_slot("card_b", 1);
        clear_all();
        assert!(!has_card_state("card_a"));
        assert!(!has_card_state("card_b"));
    }

    #[test]
    fn get_slot_missing_card_returns_false() {
        reset();
        assert!(!get_slot("nonexistent", 1));
    }

    // ── Alarm tests ────────────────────────────────────────────────

    #[test]
    fn add_alarm_works() {
        reset();
        add_alarm(5, "Scout patrol", "green");
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].remaining, 5);
        assert_eq!(alarms[0].name, "Scout patrol");
        assert_eq!(alarms[0].color_set, "green");
    }

    #[test]
    fn add_alarm_validates_color() {
        reset();
        add_alarm(1, "", "invalid");
        let alarms = get_alarms();
        assert_eq!(alarms[0].color_set, "red");
    }

    #[test]
    fn tick_decrements_and_removes_expired() {
        reset();
        add_alarm(2, "stays", "red");
        add_alarm(1, "completes", "blue");

        tick_alarms();
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].remaining, 1);
        assert_eq!(alarms[1].remaining, 0);

        tick_alarms();
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].remaining, 0);

        tick_alarms();
        let alarms = get_alarms();
        assert!(alarms.is_empty());
    }

    #[test]
    fn remove_alarm_by_index() {
        reset();
        add_alarm(5, "first", "red");
        add_alarm(3, "second", "green");
        add_alarm(1, "third", "blue");
        remove_alarm(1);
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].remaining, 5);
        assert_eq!(alarms[1].remaining, 1);
    }

    #[test]
    fn remove_alarm_out_of_bounds_is_noop() {
        reset();
        add_alarm(5, "", "red");
        remove_alarm(99);
        assert_eq!(get_alarms().len(), 1);
    }

    // ── Settings tests ─────────────────────────────────────────────

    #[test]
    fn show_alarms_default_true() {
        reset();
        assert!(get_show_alarms());
    }

    #[test]
    fn toggle_show_alarms() {
        reset();
        set_show_alarms(false);
        assert!(!get_show_alarms());
        set_show_alarms(true);
        assert!(get_show_alarms());
    }

    // ── Persistence tests ──────────────────────────────────────────

    #[test]
    fn persist_and_restore_roundtrip() {
        reset();
        ensure_card_state("test_card", 3);
        toggle_slot("test_card", 2);
        add_alarm(5, "timer", "green");
        set_show_alarms(false);

        let state = encode_full_state();
        assert!(!state.is_empty());

        // Reset and restore
        init_player_doc();
        assert!(!has_card_state("test_card"));
        assert!(get_alarms().is_empty());

        let result = restore_from_state(&state);
        assert!(result.is_ok());

        assert!(has_card_state("test_card"));
        assert!(get_slot("test_card", 2));
        assert!(!get_slot("test_card", 1));
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].name, "timer");
        assert!(!get_show_alarms());
    }

    #[test]
    fn restore_empty_is_noop() {
        reset();
        add_alarm(1, "keep", "red");
        let result = restore_from_state("");
        assert!(result.is_ok());
        assert_eq!(get_alarms().len(), 1);
    }

    // ── Migration tests ────────────────────────────────────────────

    #[test]
    fn migrate_from_game_state_json() {
        reset();
        let json = r#"{
            "cards": {
                "brox_the_defiant": {
                    "slots": {"1": true, "2": false, "3": true},
                    "wasted": false
                }
            },
            "alarms": [
                {"remaining": 5, "name": "test timer", "color_set": "green"},
                {"remaining": 2, "name": "", "color_set": "red"}
            ],
            "show_alarms": false
        }"#;

        let result = migrate_from_game_state(json);
        assert!(result.is_ok());

        // Check cards
        assert!(has_card_state("brox_the_defiant"));
        assert!(get_slot("brox_the_defiant", 1));
        assert!(!get_slot("brox_the_defiant", 2));
        assert!(get_slot("brox_the_defiant", 3));
        assert!(!is_wasted("brox_the_defiant"));

        // Check alarms
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].remaining, 5);
        assert_eq!(alarms[0].name, "test timer");
        assert_eq!(alarms[0].color_set, "green");
        assert_eq!(alarms[1].remaining, 2);

        // Check settings
        assert!(!get_show_alarms());
    }

    #[test]
    fn migrate_invalid_json_returns_error() {
        reset();
        let result = migrate_from_game_state("not valid json {{{");
        assert!(result.is_err());
    }

    #[test]
    fn migrate_empty_state() {
        reset();
        let json = r#"{"cards":{},"alarms":[],"show_alarms":true}"#;
        let result = migrate_from_game_state(json);
        assert!(result.is_ok());
        assert!(get_alarms().is_empty());
        assert!(get_show_alarms());
    }
}
