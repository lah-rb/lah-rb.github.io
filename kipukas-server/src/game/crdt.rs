//! CRDT-based shared state using Yrs (Yjs Rust port).
//!
//! Provides a `yrs::Doc` for multiplayer state that syncs automatically
//! via binary updates exchanged through the WebSocket relay. The turn
//! timer is the first feature using this module — future features
//! (decks, combat history, identity) will add more root types to the
//! same Doc.
//!
//! ## Doc Structure
//!
//! ```text
//! yrs::Doc
//! └── "alarms" (ArrayRef)
//!     ├── [0] (MapRef) { "remaining": i32, "name": String, "color_set": String }
//!     ├── [1] (MapRef) { ... }
//!     └── ...
//! ```
//!
//! ## Sync Protocol
//!
//! 1. On room connect: exchange state vectors → compute diffs → apply
//! 2. On mutation: capture update bytes → relay to peer → peer applies
//! 3. On reconnect: repeat step 1 (yrs handles deduplication)
//!
//! ## Encoding
//!
//! All base64 encoding uses URL_SAFE_NO_PAD (`-` and `_` instead of `+`
//! and `/`). This prevents `+` from being decoded as space when values
//! pass through form-encoded HTTP bodies.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::cell::RefCell;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Any, Array, Doc, Map, MapPrelim, ReadTxn, StateVector, Transact, Update, WriteTxn};

use crate::game::state::{with_state, with_state_mut, Alarm};

thread_local! {
    static CRDT_DOC: RefCell<Doc> = RefCell::new(Doc::new());
}

// ── Doc lifecycle ──────────────────────────────────────────────────

/// Initialize a fresh yrs Doc for a new multiplayer session.
/// Call on room create/join.
pub fn init_doc() {
    CRDT_DOC.with(|cell| {
        let doc = Doc::new();
        // Pre-create the "alarms" root array so both peers share the same root type.
        {
            let mut txn = doc.transact_mut();
            txn.get_or_insert_array("alarms");
        }
        *cell.borrow_mut() = doc;
    });
}

/// Reset the Doc (on disconnect). Replaces with a fresh empty Doc.
pub fn reset_doc() {
    init_doc();
}

// ── Doc persistence (survives page navigation via sessionStorage) ──

/// Encode the full Doc state as a URL-safe base64 string for persistence.
/// Stored in sessionStorage so the Doc survives page navigation.
pub fn encode_full_state() -> String {
    CRDT_DOC.with(|cell| {
        let doc = cell.borrow();
        let state = doc
            .transact()
            .encode_diff_v1(&StateVector::default());
        URL_SAFE_NO_PAD.encode(&state)
    })
}

/// Restore Doc from a previously persisted URL-safe base64 state.
/// Called on page load (before sync handshake) to recover state.
pub fn restore_from_state(state_b64: &str) -> Result<(), String> {
    if state_b64.is_empty() {
        return Ok(());
    }
    let state_bytes = URL_SAFE_NO_PAD
        .decode(state_b64)
        .map_err(|e| format!("base64 decode error: {}", e))?;
    let update = Update::decode_v1(&state_bytes)
        .map_err(|e| format!("state decode error: {}", e))?;

    CRDT_DOC.with(|cell| {
        let doc = cell.borrow();
        let mut txn = doc.transact_mut();
        txn.apply_update(update)
            .map_err(|e| format!("restore error: {}", e))
    })
}

// ── Alarm mutations (return update bytes as base64) ────────────────

/// Add a new alarm to the yrs Doc. Returns the update as a base64 string.
pub fn add_alarm(turns: i32, name: &str, color_set: &str) -> String {
    let color = validate_color_set(color_set);
    CRDT_DOC.with(|cell| {
        let doc = cell.borrow();
        let alarms = doc.get_or_insert_array("alarms");
        let sv = doc.transact().state_vector();

        {
            let mut txn = doc.transact_mut();
            let alarm_map = MapPrelim::from([
                ("remaining".to_string(), Any::from(turns as f64)),
                ("name".to_string(), Any::from(name.to_string())),
                ("color_set".to_string(), Any::from(color.to_string())),
            ]);
            let len = alarms.len(&txn);
            alarms.insert(&mut txn, len, alarm_map);
        }

        encode_diff_since(&doc, &sv)
    })
}

/// Tick all alarms: decrement by 1, remove completed (were already at 0).
/// Returns the update as a base64 string.
pub fn tick_alarms() -> String {
    CRDT_DOC.with(|cell| {
        let doc = cell.borrow();
        let alarms = doc.get_or_insert_array("alarms");
        let sv = doc.transact().state_vector();

        {
            let mut txn = doc.transact_mut();
            let len = alarms.len(&txn);

            // First pass: collect indices to remove (were at 0, now go negative)
            let mut to_remove: Vec<u32> = Vec::new();
            for i in 0..len {
                if let Some(yrs::Out::YMap(map)) = alarms.get(&txn, i) {
                    if let Some(yrs::Out::Any(Any::Number(r))) = map.get(&txn, "remaining") {
                        let remaining = r as i32;
                        if remaining <= 0 {
                            to_remove.push(i);
                        }
                    }
                }
            }

            // Remove expired (in reverse order to preserve indices)
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
        }

        encode_diff_since(&doc, &sv)
    })
}

/// Remove an alarm by index. Returns the update as a base64 string.
pub fn remove_alarm(index: u32) -> String {
    CRDT_DOC.with(|cell| {
        let doc = cell.borrow();
        let alarms = doc.get_or_insert_array("alarms");
        let sv = doc.transact().state_vector();

        {
            let mut txn = doc.transact_mut();
            let len = alarms.len(&txn);
            if index < len {
                alarms.remove(&mut txn, index);
            }
        }

        encode_diff_since(&doc, &sv)
    })
}

// ── Read state ─────────────────────────────────────────────────────

/// Read the current alarms from the yrs Doc as a Vec<Alarm>.
/// Used for rendering the alarm list HTML.
pub fn get_alarms() -> Vec<Alarm> {
    CRDT_DOC.with(|cell| {
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

// ── Sync protocol ──────────────────────────────────────────────────

/// Encode the Doc's state vector as a URL-safe base64 string.
/// Used for sync handshake step 1.
pub fn encode_state_vector() -> String {
    CRDT_DOC.with(|cell| {
        let doc = cell.borrow();
        let sv = doc.transact().state_vector().encode_v1();
        URL_SAFE_NO_PAD.encode(&sv)
    })
}

/// Given a remote peer's state vector (URL-safe base64), compute the diff
/// update containing all changes they haven't seen. Returns URL-safe base64 update.
/// Used for sync handshake step 2.
pub fn encode_diff(remote_sv_b64: &str) -> Result<String, String> {
    let sv_bytes = URL_SAFE_NO_PAD
        .decode(remote_sv_b64)
        .map_err(|e| format!("base64 decode error: {}", e))?;
    let remote_sv = StateVector::decode_v1(&sv_bytes)
        .map_err(|e| format!("state vector decode error: {}", e))?;

    CRDT_DOC.with(|cell| {
        let doc = cell.borrow();
        let update = doc.transact().encode_diff_v1(&remote_sv);
        Ok(URL_SAFE_NO_PAD.encode(&update))
    })
}

/// Apply a URL-safe base64-encoded update from a remote peer.
pub fn apply_update(update_b64: &str) -> Result<(), String> {
    let update_bytes = URL_SAFE_NO_PAD
        .decode(update_b64)
        .map_err(|e| format!("base64 decode error: {}", e))?;
    let update = Update::decode_v1(&update_bytes)
        .map_err(|e| format!("update decode error: {}", e))?;

    CRDT_DOC.with(|cell| {
        let doc = cell.borrow();
        let mut txn = doc.transact_mut();
        txn.apply_update(update)
            .map_err(|e| format!("apply update error: {}", e))
    })
}

// ── GameState ↔ CRDT bridge ────────────────────────────────────────

/// Seed the CRDT Doc from local `GameState.alarms`.
/// Called on room create/join so any pre-existing local timers become
/// shared with the peer. Skips if the Doc already has alarms (e.g.,
/// restored from sessionStorage on page navigation).
pub fn seed_from_local() {
    let local_alarms: Vec<Alarm> = with_state(|state| state.alarms.clone());
    if local_alarms.is_empty() {
        return;
    }
    // Only seed into an empty Doc — avoid duplicating alarms that were
    // already restored from sessionStorage.
    let existing = get_alarms();
    if !existing.is_empty() {
        return;
    }
    for alarm in &local_alarms {
        add_alarm(alarm.remaining, &alarm.name, &alarm.color_set);
    }
}

/// Copy the current CRDT alarms back into local `GameState.alarms`.
/// Called on disconnect so shared timers survive as local timers after
/// the multiplayer session ends.
pub fn export_to_local() {
    let crdt_alarms = get_alarms();
    with_state_mut(|state| {
        state.alarms = crdt_alarms;
    });
}

// ── Internal helpers ───────────────────────────────────────────────

/// Compute the diff between the current Doc state and a previous state vector,
/// return it as a URL-safe base64 string. Used after mutations to capture the update.
fn encode_diff_since(doc: &Doc, before_sv: &StateVector) -> String {
    let update = doc.transact().encode_diff_v1(before_sv);
    URL_SAFE_NO_PAD.encode(&update)
}

/// Validate color set, defaulting to "red" if invalid.
fn validate_color_set(color: &str) -> &str {
    match color {
        "red" | "green" | "blue" | "yellow" | "pink" => color,
        _ => "red",
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        init_doc();
    }

    #[test]
    fn init_creates_empty_alarms() {
        reset();
        let alarms = get_alarms();
        assert!(alarms.is_empty());
    }

    #[test]
    fn add_alarm_works() {
        reset();
        let _update = add_alarm(5, "Scout patrol", "green");
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
    fn add_multiple_alarms() {
        reset();
        add_alarm(5, "first", "red");
        add_alarm(3, "second", "blue");
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].name, "first");
        assert_eq!(alarms[1].name, "second");
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
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
    }

    #[test]
    fn add_returns_nonempty_update() {
        reset();
        let update = add_alarm(5, "test", "red");
        assert!(!update.is_empty());
        let decoded = URL_SAFE_NO_PAD.decode(&update);
        assert!(decoded.is_ok());
    }

    #[test]
    fn state_vector_is_valid_base64() {
        reset();
        add_alarm(3, "test", "blue");
        let sv = encode_state_vector();
        assert!(!sv.is_empty());
        let decoded = URL_SAFE_NO_PAD.decode(&sv);
        assert!(decoded.is_ok());
    }

    #[test]
    fn sync_two_docs_converge() {
        reset();
        add_alarm(5, "peer_a_timer", "green");

        let doc_b = Doc::new();
        {
            let mut txn = doc_b.transact_mut();
            txn.get_or_insert_array("alarms");
        }

        let sv_b = doc_b.transact().state_vector().encode_v1();
        let sv_b_b64 = URL_SAFE_NO_PAD.encode(&sv_b);

        let diff_for_b = encode_diff(&sv_b_b64).unwrap();

        let diff_bytes = URL_SAFE_NO_PAD.decode(&diff_for_b).unwrap();
        let update = Update::decode_v1(&diff_bytes).unwrap();
        doc_b.transact_mut().apply_update(update).unwrap();

        let alarms_b = {
            let arr = doc_b.get_or_insert_array("alarms");
            let txn = doc_b.transact();
            let len = arr.len(&txn);
            let mut result = Vec::new();
            for i in 0..len {
                if let Some(yrs::Out::YMap(map)) = arr.get(&txn, i) {
                    let name = match map.get(&txn, "name") {
                        Some(yrs::Out::Any(Any::String(s))) => s.to_string(),
                        _ => String::new(),
                    };
                    result.push(name);
                }
            }
            result
        };

        assert_eq!(alarms_b.len(), 1);
        assert_eq!(alarms_b[0], "peer_a_timer");
    }

    #[test]
    fn apply_update_from_remote() {
        reset();

        let remote_doc = Doc::new();
        let remote_arr = remote_doc.get_or_insert_array("alarms");
        let sv_before = remote_doc.transact().state_vector();
        {
            let mut txn = remote_doc.transact_mut();
            let alarm_map = MapPrelim::from([
                ("remaining".to_string(), Any::from(7_f64)),
                ("name".to_string(), Any::from("remote_timer".to_string())),
                ("color_set".to_string(), Any::from("pink".to_string())),
            ]);
            remote_arr.insert(&mut txn, 0, alarm_map);
        }
        let update_bytes = remote_doc.transact().encode_diff_v1(&sv_before);
        let update_b64 = URL_SAFE_NO_PAD.encode(&update_bytes);

        let result = apply_update(&update_b64);
        assert!(result.is_ok());

        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].remaining, 7);
        assert_eq!(alarms[0].name, "remote_timer");
        assert_eq!(alarms[0].color_set, "pink");
    }

    #[test]
    fn apply_invalid_update_returns_error() {
        reset();
        let result = apply_update("not_valid_base64!!!");
        assert!(result.is_err());
    }

    #[test]
    fn reset_doc_clears_state() {
        reset();
        add_alarm(5, "test", "red");
        assert_eq!(get_alarms().len(), 1);
        reset_doc();
        assert_eq!(get_alarms().len(), 0);
    }

    #[test]
    fn concurrent_adds_both_survive() {
        reset();

        let update_a = add_alarm(5, "from_a", "red");

        let doc_b = Doc::new();
        let arr_b = doc_b.get_or_insert_array("alarms");
        let sv_b_before = doc_b.transact().state_vector();
        {
            let mut txn = doc_b.transact_mut();
            let alarm = MapPrelim::from([
                ("remaining".to_string(), Any::from(3_f64)),
                ("name".to_string(), Any::from("from_b".to_string())),
                ("color_set".to_string(), Any::from("blue".to_string())),
            ]);
            arr_b.insert(&mut txn, 0, alarm);
        }
        let update_b_bytes = doc_b.transact().encode_diff_v1(&sv_b_before);
        let update_b = URL_SAFE_NO_PAD.encode(&update_b_bytes);

        apply_update(&update_b).unwrap();

        let update_a_bytes = URL_SAFE_NO_PAD.decode(&update_a).unwrap();
        doc_b
            .transact_mut()
            .apply_update(Update::decode_v1(&update_a_bytes).unwrap())
            .unwrap();

        let alarms_a = get_alarms();
        assert_eq!(alarms_a.len(), 2);

        let alarms_b = {
            let txn = doc_b.transact();
            arr_b.len(&txn)
        };
        assert_eq!(alarms_b, 2);
    }

    #[test]
    fn persist_and_restore_roundtrip() {
        reset();
        add_alarm(5, "persisted", "green");
        add_alarm(2, "also persisted", "pink");

        let state = encode_full_state();
        assert!(!state.is_empty());

        // Reset and restore
        reset_doc();
        assert_eq!(get_alarms().len(), 0);

        let result = restore_from_state(&state);
        assert!(result.is_ok());

        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].name, "persisted");
        assert_eq!(alarms[0].color_set, "green");
        assert_eq!(alarms[1].name, "also persisted");
    }

    #[test]
    fn restore_empty_is_noop() {
        reset();
        add_alarm(1, "keep", "red");
        let result = restore_from_state("");
        assert!(result.is_ok());
        assert_eq!(get_alarms().len(), 1);
    }

    #[test]
    fn seed_from_local_copies_alarms() {
        use crate::game::state::{replace_state, GameState};
        // Set up local GameState with alarms
        let mut gs = GameState::default();
        gs.alarms.push(Alarm {
            remaining: 5,
            name: "local timer".to_string(),
            color_set: "green".to_string(),
        });
        gs.alarms.push(Alarm {
            remaining: 2,
            name: "another".to_string(),
            color_set: "blue".to_string(),
        });
        replace_state(gs);

        // Fresh CRDT Doc
        reset();
        assert!(get_alarms().is_empty());

        seed_from_local();
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].name, "local timer");
        assert_eq!(alarms[0].remaining, 5);
        assert_eq!(alarms[0].color_set, "green");
        assert_eq!(alarms[1].name, "another");

        replace_state(GameState::default());
    }

    #[test]
    fn seed_from_local_skips_when_doc_has_alarms() {
        use crate::game::state::{replace_state, GameState};
        let mut gs = GameState::default();
        gs.alarms.push(Alarm {
            remaining: 5,
            name: "local".to_string(),
            color_set: "red".to_string(),
        });
        replace_state(gs);

        // Doc already has alarms (e.g. restored from sessionStorage)
        reset();
        add_alarm(3, "existing", "pink");

        seed_from_local();
        let alarms = get_alarms();
        // Should NOT have added the local alarm — Doc wasn't empty
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].name, "existing");

        replace_state(GameState::default());
    }

    #[test]
    fn seed_from_local_skips_when_no_local_alarms() {
        use crate::game::state::{replace_state, GameState};
        replace_state(GameState::default());

        reset();
        seed_from_local();
        assert!(get_alarms().is_empty());

        replace_state(GameState::default());
    }

    #[test]
    fn export_to_local_copies_crdt_to_gamestate() {
        use crate::game::state::{replace_state, GameState};
        replace_state(GameState::default());

        reset();
        add_alarm(7, "shared timer", "yellow");
        add_alarm(1, "quick one", "pink");

        export_to_local();
        with_state(|state| {
            assert_eq!(state.alarms.len(), 2);
            assert_eq!(state.alarms[0].name, "shared timer");
            assert_eq!(state.alarms[0].remaining, 7);
            assert_eq!(state.alarms[0].color_set, "yellow");
            assert_eq!(state.alarms[1].name, "quick one");
        });

        replace_state(GameState::default());
    }

    #[test]
    fn export_to_local_replaces_existing_alarms() {
        use crate::game::state::{replace_state, GameState};
        let mut gs = GameState::default();
        gs.alarms.push(Alarm {
            remaining: 99,
            name: "old local".to_string(),
            color_set: "red".to_string(),
        });
        replace_state(gs);

        reset();
        add_alarm(3, "from crdt", "blue");

        export_to_local();
        with_state(|state| {
            assert_eq!(state.alarms.len(), 1);
            assert_eq!(state.alarms[0].name, "from crdt");
        });

        replace_state(GameState::default());
    }
}
