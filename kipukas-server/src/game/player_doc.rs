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
//! │   └── [0] { "id": String, "remaining": i32, "name": String,
//! │             "color_set": String, "changed": f64 (ms, when last changed) }
//! ├── "retired" (YMap) — timers removed lately while out of a room:
//! │   └── "<timer id>": f64 (ms, when)
//! ├── "affinity" (YMap) — the archetype declared for this game:
//! │   └── "Brutal": { "level": 1, "last_declared": "YYYY-MM-DD" }
//! ├── "loyalty" (YMap) — the days each card was played on:
//! │   └── "brox_the_defiant": { "days": { "YYYY-MM-DD": true }, "earlier": u32,
//! │                             "total_plays": u32, "last_played": "YYYY-MM-DD" }
//! ├── "merges" (YMap) — each state taken over from another device or a backup:
//! │   └── "<id>": { "at": f64 (ms), "by": "device" | "file", "gained": u32, "held": u32 }
//! └── "settings" (YMap)
//!     └── "show_alarms" (bool)
//! ```
//!
//! Cards, timers, what is retired and the affinity are the game in progress:
//! New Game clears them. Loyalty and the merges are the player's record,
//! which outlives every game. See `take_over`.
//!
//! ## Lifecycle
//!
//! - **First visit:** Fresh Doc created with empty defaults
//! - **Every page load:** Restored from what is stored under `kipukas_player_doc` (base64 binary),
//!   which the page hands over at load (`crate::store`)
//! - **Every mutation:** Stored again: after each request the engine compares the
//!   document with what is stored and names it to be written if it differs
//! - **Multiplayer:** Independent of ROOM_DOC; alarms bridge via `crdt.rs` seed/export

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{
    Any, Array, ArrayPrelim, Doc, Map, MapPrelim, MapRef, ReadTxn, StateVector, Transact, TransactionMut, Update,
    WriteTxn,
};

use crate::game::state::{dated, is_valid_alarm_id, new_alarm_id, Alarm};

thread_local! {
    static PLAYER_DOC: RefCell<Doc> = RefCell::new(new_player_doc());
}

/// The 15 valid archetypal adaptation names.
const VALID_ARCHETYPES: &[&str] = &[
    "Cenozoic",
    "Decrepit",
    "Angelic",
    "Brutal",
    "Arboreal",
    "Astral",
    "Telekinetic",
    "Glitch",
    "Magic",
    "Endothermic",
    "Avian",
    "Mechanical",
    "Algorithmic",
    "Energetic",
    "Entropic",
];

/// Create a fresh player Doc with all root types pre-created.
fn new_player_doc() -> Doc {
    let doc = with_roots(Doc::new());
    {
        let mut txn = doc.transact_mut();
        let settings = txn.get_or_insert_map("settings");
        // Default: show_alarms = true
        settings.insert(&mut txn, "show_alarms", Any::from(true));
    }
    doc
}

/// `doc` with every root type made, so that each is there to be read.
fn with_roots(doc: Doc) -> Doc {
    {
        let mut txn = doc.transact_mut();
        txn.get_or_insert_map("cards");
        txn.get_or_insert_array("alarms");
        txn.get_or_insert_map("affinity");
        txn.get_or_insert_map("loyalty");
        txn.get_or_insert_map("merges");
        txn.get_or_insert_map("settings");
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
        let doc = with_roots(Doc::new());
        doc.transact_mut().apply_update(update).map_err(|e| format!("restore error: {}", e))?;
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

/// How long a timer that was removed is remembered as removed (see
/// [`retired`]): longer than a room is kept by anyone who might still hold
/// the timer.
const RETIRED_KEPT_MS: f64 = 24.0 * 60.0 * 60.0 * 1000.0;

/// Add a new alarm with a fresh id, made at `now`.
pub fn add_alarm(now: f64, turns: i32, name: &str, color_set: &str) {
    add_alarm_with_id(&new_alarm_id(), turns, name, color_set, dated(now, 0.0));
}

/// Add an alarm that already has an id (a shared timer coming back from a
/// room), so it keeps its identity for the next room. `changed` is when it
/// was last changed, which it keeps too.
pub fn add_alarm_with_id(id: &str, turns: i32, name: &str, color_set: &str, changed: f64) {
    let color = validate_color_set(color_set);
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let alarms = doc.get_or_insert_array("alarms");
        let mut txn = doc.transact_mut();
        let alarm_map = MapPrelim::from([
            ("id".to_string(), Any::from(id.to_string())),
            ("remaining".to_string(), Any::from(turns as f64)),
            ("name".to_string(), Any::from(name.to_string())),
            ("color_set".to_string(), Any::from(color.to_string())),
            ("changed".to_string(), Any::from(changed)),
        ]);
        let len = alarms.len(&txn);
        alarms.insert(&mut txn, len, alarm_map);
    });
}

/// What a stored alarm says of itself: its id and when it was last changed.
fn id_and_changed<T: ReadTxn>(map: &MapRef, txn: &T) -> (String, f64) {
    let id = match map.get(txn, "id") {
        Some(yrs::Out::Any(Any::String(s))) => s.to_string(),
        _ => String::new(),
    };
    let changed = match map.get(txn, "changed") {
        Some(yrs::Out::Any(Any::Number(at))) => at,
        _ => 0.0,
    };
    (id, changed)
}

/// Remember that the timer `id`, last changed at `changed`, was removed at
/// `now`, and forget removals too old to matter.
///
/// A timer the player removes while out of a room may still be in a room
/// they return to, as the other player has it. Joining offers the removal
/// along with the player's timers (`crdt::seed_from_local`), and the later
/// of the two wins: the timer stays removed unless it was changed in the
/// room after that.
fn retire(txn: &mut TransactionMut, id: &str, changed: f64, now: f64) {
    let retired = txn.get_or_insert_map("retired");
    if is_valid_alarm_id(id) {
        retired.insert(txn, id, Any::from(dated(now, changed)));
    }
    forget_old(&retired, txn, now);
}

/// Forget the removals made more than a day before `latest`.
fn forget_old(retired: &MapRef, txn: &mut TransactionMut, latest: f64) {
    let old: Vec<String> = retired
        .iter(txn)
        .filter_map(|(id, value)| match value {
            yrs::Out::Any(Any::Number(at)) if latest - at <= RETIRED_KEPT_MS => None,
            _ => Some(id.to_string()),
        })
        .collect();
    for id in old {
        retired.remove(txn, &id);
    }
}

/// Remember the timers that were removed in a room (id, and when), the same
/// as those the player removes alone.
///
/// The other player may have left the room before the removal reached them,
/// and still hold the timer. The removal has to go with this player to the
/// next room to meet it there. Called with all of a room's removals each
/// time the room's timers are copied to the player's own
/// (`crdt::export_to_local`), so it writes only what it does not hold yet,
/// and takes nothing it would at once forget as too old.
pub fn remember_removed(removed: &[(String, f64)]) {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let mut txn = doc.transact_mut();
        let retired = txn.get_or_insert_map("retired");
        let held: Vec<(String, f64)> = retired
            .iter(&txn)
            .filter_map(|(id, value)| match value {
                yrs::Out::Any(Any::Number(at)) => Some((id.to_string(), at)),
                _ => None,
            })
            .collect();
        let latest = held.iter().chain(removed).map(|(_, at)| *at).fold(0.0, f64::max);
        let mut changed = false;
        for (id, at) in removed {
            if !is_valid_alarm_id(id) || latest - at > RETIRED_KEPT_MS {
                continue;
            }
            if held.iter().any(|(known, since)| known == id && since >= at) {
                continue;
            }
            retired.insert(&mut txn, id.as_str(), Any::from(*at));
            changed = true;
        }
        if changed {
            forget_old(&retired, &mut txn, latest);
        }
    });
}

/// The timers this player removed lately while out of a room: id, and when.
pub fn retired() -> Vec<(String, f64)> {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let retired = doc.get_or_insert_map("retired");
        let txn = doc.transact();
        retired
            .iter(&txn)
            .filter_map(|(id, value)| match value {
                yrs::Out::Any(Any::Number(at)) => Some((id.to_string(), at)),
                _ => None,
            })
            .collect()
    })
}

/// Tick all alarms at `now`: decrement by 1, remove completed (were already
/// at 0).
pub fn tick_alarms(now: f64) {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let alarms = doc.get_or_insert_array("alarms");
        let mut txn = doc.transact_mut();
        let len = alarms.len(&txn);

        // First pass: collect indices to remove (were at 0)
        let mut to_remove: Vec<(u32, String, f64)> = Vec::new();
        for i in 0..len {
            if let Some(yrs::Out::YMap(map)) = alarms.get(&txn, i) {
                if let Some(yrs::Out::Any(Any::Number(r))) = map.get(&txn, "remaining") {
                    if (r as i32) <= 0 {
                        let (id, changed) = id_and_changed(&map, &txn);
                        to_remove.push((i, id, changed));
                    }
                }
            }
        }

        // Remove expired (reverse order to preserve indices)
        for (idx, id, changed) in to_remove.iter().rev() {
            alarms.remove(&mut txn, *idx);
            retire(&mut txn, id, *changed, now);
        }

        // Second pass: decrement remaining alarms
        let new_len = alarms.len(&txn);
        for i in 0..new_len {
            if let Some(yrs::Out::YMap(map)) = alarms.get(&txn, i) {
                if let Some(yrs::Out::Any(Any::Number(r))) = map.get(&txn, "remaining") {
                    let remaining = r as i32;
                    let (_, changed) = id_and_changed(&map, &txn);
                    map.insert(&mut txn, "remaining", Any::from((remaining - 1) as f64));
                    map.insert(&mut txn, "changed", Any::from(dated(now, changed)));
                }
            }
        }
    });
}

/// Remove an alarm by index, at `now`.
pub fn remove_alarm(now: f64, index: usize) {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let alarms = doc.get_or_insert_array("alarms");
        let mut txn = doc.transact_mut();
        let len = alarms.len(&txn);
        if (index as u32) < len {
            if let Some(yrs::Out::YMap(map)) = alarms.get(&txn, index as u32) {
                let (id, changed) = id_and_changed(&map, &txn);
                retire(&mut txn, &id, changed, now);
            }
            alarms.remove(&mut txn, index as u32);
        }
    });
}

/// Remove the alarms that have one of these ids, at `now`.
pub fn remove_alarms_with_ids(now: f64, ids: &[String]) {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let alarms = doc.get_or_insert_array("alarms");
        let mut txn = doc.transact_mut();
        // Last first, so removing one does not move those still to check.
        for i in (0..alarms.len(&txn)).rev() {
            let (id, changed) = match alarms.get(&txn, i) {
                Some(yrs::Out::YMap(map)) => id_and_changed(&map, &txn),
                _ => (String::new(), 0.0),
            };
            if !id.is_empty() && ids.contains(&id) {
                alarms.remove(&mut txn, i);
                retire(&mut txn, &id, changed, now);
            }
        }
    });
}

/// When this device first saw each run-out timer say so: timer id and the
/// moment, in milliseconds (see `turns::note_completions`). It is this
/// device's own record, for the player's timers and a room's alike, so it
/// does not depend on the other player's clock.
pub fn get_alarm_done() -> Vec<(String, f64)> {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let done = doc.get_or_insert_map("alarm_done");
        let txn = doc.transact();
        done.iter(&txn)
            .filter_map(|(id, value)| match value {
                yrs::Out::Any(Any::Number(at)) => Some((id.to_string(), at)),
                _ => None,
            })
            .collect()
    })
}

/// Make the record of run-out timers exactly `entries`. Nothing is written
/// if it already is.
pub fn set_alarm_done(entries: &[(String, f64)]) {
    let current = get_alarm_done();
    if current.len() == entries.len() && entries.iter().all(|entry| current.contains(entry)) {
        return;
    }
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let done = doc.get_or_insert_map("alarm_done");
        let mut txn = doc.transact_mut();
        for (id, _) in &current {
            if !entries.iter().any(|(wanted, _)| wanted == id) {
                done.remove(&mut txn, id);
            }
        }
        for (id, at) in entries {
            done.insert(&mut txn, id.as_str(), Any::from(*at));
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
                // Timers saved before ids existed have none; one is assigned
                // when the timer is first shared (crdt::seed_from_local).
                let (id, changed) = id_and_changed(&map, &txn);
                result.push(Alarm {
                    id,
                    remaining,
                    name,
                    color_set,
                    changed,
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

// ── Affinity accessors ─────────────────────────────────────────────

/// Validate an archetype name (case-sensitive, must match one of the 15).
fn validate_archetype(name: &str) -> bool {
    VALID_ARCHETYPES.contains(&name)
}

/// Expose the valid archetypes list for route rendering.
pub fn valid_archetypes() -> &'static [&'static str] {
    VALID_ARCHETYPES
}

/// Declare affinity for an archetype. Increments level by 1 and sets
/// `last_declared` to `today`. Enforces once-per-day: if `last_declared`
/// already equals `today`, returns Err.
///
/// Returns `Ok((new_level, archetype))` on success.
pub fn declare_affinity(archetype: &str, today: &str) -> Result<(u32, String), String> {
    if !validate_archetype(archetype) {
        return Err(format!("Unknown archetype: {}", archetype));
    }
    if today.is_empty() {
        return Err("Missing today date".to_string());
    }

    // Enforce one affinity per game: if any affinity has already been declared, reject.
    if get_active_affinity().is_some() {
        return Err("Affinity already declared for this game. Start a New Game to choose again.".to_string());
    }

    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let affinity = doc.get_or_insert_map("affinity");
        let mut txn = doc.transact_mut();

        let new_level: u32 = 1;

        // Upsert: create or update the archetype entry
        match affinity.get(&txn, archetype) {
            Some(yrs::Out::YMap(entry)) => {
                entry.insert(&mut txn, "level", Any::from(new_level as f64));
                entry.insert(
                    &mut txn,
                    "last_declared",
                    Any::from(today.to_string()),
                );
            }
            _ => {
                let entry = MapPrelim::from([
                    ("level".to_string(), Any::from(new_level as f64)),
                    (
                        "last_declared".to_string(),
                        Any::from(today.to_string()),
                    ),
                ]);
                affinity.insert(&mut txn, archetype, entry);
            }
        }

        Ok((new_level, archetype.to_string()))
    })
}

/// Get affinity data for a single archetype.
/// Returns `Some((level, last_declared))` if the archetype has been declared.
pub fn get_affinity(archetype: &str) -> Option<(u32, String)> {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let affinity = doc.get_or_insert_map("affinity");
        let txn = doc.transact();
        match affinity.get(&txn, archetype) {
            Some(yrs::Out::YMap(entry)) => {
                let level = match entry.get(&txn, "level") {
                    Some(yrs::Out::Any(Any::Number(n))) => n as u32,
                    _ => 0,
                };
                let last = match entry.get(&txn, "last_declared") {
                    Some(yrs::Out::Any(Any::String(s))) => s.to_string(),
                    _ => String::new(),
                };
                Some((level, last))
            }
            _ => None,
        }
    })
}

/// Get all declared affinities as `(archetype, level, last_declared)`.
/// Only returns archetypes that have been declared at least once.
pub fn get_all_affinities() -> Vec<(String, u32, String)> {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let affinity = doc.get_or_insert_map("affinity");
        let txn = doc.transact();
        let mut result = Vec::new();

        for key in affinity.keys(&txn) {
            if let Some(yrs::Out::YMap(entry)) = affinity.get(&txn, key) {
                let level = match entry.get(&txn, "level") {
                    Some(yrs::Out::Any(Any::Number(n))) => n as u32,
                    _ => 0,
                };
                let last = match entry.get(&txn, "last_declared") {
                    Some(yrs::Out::Any(Any::String(s))) => s.to_string(),
                    _ => String::new(),
                };
                result.push((key.to_string(), level, last));
            }
        }

        result
    })
}

/// Clear all affinity data (used by "New Game" reset).
pub fn clear_affinity() {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let affinity = doc.get_or_insert_map("affinity");
        let mut txn = doc.transact_mut();
        let keys: Vec<String> = affinity.keys(&txn).map(|k| k.to_string()).collect();
        for key in keys {
            affinity.remove(&mut txn, &key);
        }
    });
}

/// Remove every alarm at `now`, each remembered as removed: "New Game"
/// while out of a room. A copy of one of them that the other player brings
/// to the next room does not bring it back.
pub fn remove_all_alarms(now: f64) {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let alarms = doc.get_or_insert_array("alarms");
        let mut txn = doc.transact_mut();
        let len = alarms.len(&txn);
        let mut removed: Vec<(String, f64)> = Vec::new();
        for i in 0..len {
            if let Some(yrs::Out::YMap(map)) = alarms.get(&txn, i) {
                removed.push(id_and_changed(&map, &txn));
            }
        }
        for _ in 0..len {
            alarms.remove(&mut txn, 0);
        }
        for (id, changed) in &removed {
            retire(&mut txn, id, *changed, now);
        }
    });
}

/// Clear all alarms without remembering any as removed: the player's timers
/// are moving into a room, or being rewritten as a copy of a room's.
pub fn clear_alarms() {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let alarms = doc.get_or_insert_array("alarms");
        let mut txn = doc.transact_mut();
        let len = alarms.len(&txn);
        for _ in 0..len {
            alarms.remove(&mut txn, 0);
        }
    });
}

/// Get the active affinity — the archetype with the most recent `last_declared`
/// date. If multiple share the same date, the first found wins.
/// Returns `Some((archetype_name, level))`.
pub fn get_active_affinity() -> Option<(String, u32)> {
    let all = get_all_affinities();
    if all.is_empty() {
        return None;
    }

    let mut best: Option<(String, u32, String)> = None;
    for (name, level, last) in all {
        match &best {
            None => best = Some((name, level, last)),
            Some((_, _, best_last)) => {
                if last > *best_last {
                    best = Some((name, level, last));
                }
            }
        }
    }

    best.map(|(name, level, _)| (name, level))
}

// ── Loyalty accessors ──────────────────────────────────────────────
//
// A card's loyalty is the days it was played on, each kept by its date, so
// that two devices' records of one card join without a day counted twice
// (`take_over`, below). An entry holds:
//
//   days         YMap: "<YYYY-MM-DD>" → true, a day the card was played
//   earlier      plays counted before the days were kept
//   total_plays  `earlier` and the days together
//   last_played  the latest of the days
//
// The last two are all an entry held before the days were kept. They are
// kept in step with the rest, for a page that still runs the engine as it
// was, and are read with the rest, for what such a page has counted since.

/// A card's loyalty: the days it was played on, and how many plays were
/// counted before the days were kept.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Loyalty {
    pub earlier: u32,
    pub days: BTreeSet<String>,
}

impl Loyalty {
    pub fn total(&self) -> u32 {
        self.earlier + self.days.len() as u32
    }

    /// The latest day played, or nothing if no day is known.
    pub fn last(&self) -> String {
        self.days.iter().next_back().cloned().unwrap_or_default()
    }

    /// Take in another device's record of the same card. A day both hold is
    /// one day. Of the plays counted before days were kept the larger number
    /// stands: there is no telling which of them the two share.
    fn join(&mut self, other: &Loyalty) {
        self.earlier = self.earlier.max(other.earlier);
        self.days.extend(other.days.iter().cloned());
    }
}

fn number_in<T: ReadTxn>(map: &MapRef, txn: &T, name: &str) -> Option<f64> {
    match map.get(txn, name) {
        Some(yrs::Out::Any(Any::Number(n))) => Some(n),
        _ => None,
    }
}

fn text_in<T: ReadTxn>(map: &MapRef, txn: &T, name: &str) -> Option<String> {
    match map.get(txn, name) {
        Some(yrs::Out::Any(Any::String(s))) => Some(s.to_string()),
        _ => None,
    }
}

fn read_loyalty<T: ReadTxn>(entry: &MapRef, txn: &T) -> Loyalty {
    // What the engine held before the days were kept: a count, and the last
    // day, which is one of those counted. An entry that holds days as well
    // may have been counted on since by a page still running that engine,
    // so these are read whatever else is there.
    let total = number_in(entry, txn, "total_plays").unwrap_or(0.0) as u32;
    let mut days: BTreeSet<String> = match entry.get(txn, "days") {
        Some(yrs::Out::YMap(days)) => days.keys(txn).map(str::to_string).collect(),
        _ => BTreeSet::new(),
    };
    days.extend(text_in(entry, txn, "last_played").filter(|last| !last.is_empty()));
    let earlier = number_in(entry, txn, "earlier").unwrap_or(0.0) as u32;
    Loyalty { earlier: earlier.max(total.saturating_sub(days.len() as u32)), days }
}

/// Every card's loyalty.
fn read_record<T: ReadTxn>(loyalty: &MapRef, txn: &T) -> BTreeMap<String, Loyalty> {
    loyalty
        .iter(txn)
        .filter_map(|(slug, entry)| match entry {
            yrs::Out::YMap(entry) => Some((slug.to_string(), read_loyalty(&entry, txn))),
            _ => None,
        })
        .collect()
}

/// Have a card's entry hold `want`. Only what differs is written.
fn write_loyalty(loyalty: &MapRef, txn: &mut TransactionMut, slug: &str, want: &Loyalty) {
    let entry = match loyalty.get(txn, slug) {
        Some(yrs::Out::YMap(entry)) => entry,
        _ => loyalty.insert(txn, slug, MapPrelim::default()),
    };
    let days = match entry.get(txn, "days") {
        Some(yrs::Out::YMap(days)) => days,
        _ => entry.insert(txn, "days", MapPrelim::default()),
    };
    for day in &want.days {
        if days.get(txn, day).is_none() {
            days.insert(txn, day.as_str(), Any::from(true));
        }
    }
    for (name, value) in [("earlier", want.earlier), ("total_plays", want.total())] {
        if number_in(&entry, txn, name) != Some(value as f64) {
            entry.insert(txn, name, Any::from(value as f64));
        }
    }
    if text_in(&entry, txn, "last_played") != Some(want.last()) {
        entry.insert(txn, "last_played", Any::from(want.last()));
    }
}

/// Count a play of a card on the day `today`. A card is counted once a day:
/// a second play on a day already counted changes nothing.
/// Returns `Ok(total_plays)`.
pub fn increment_loyalty(slug: &str, today: &str) -> Result<u32, String> {
    if slug.is_empty() {
        return Err("Missing card slug".to_string());
    }
    if today.is_empty() {
        return Err("Missing today date".to_string());
    }

    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let loyalty = doc.get_or_insert_map("loyalty");
        let mut txn = doc.transact_mut();
        let mut now = match loyalty.get(&txn, slug) {
            Some(yrs::Out::YMap(entry)) => read_loyalty(&entry, &txn),
            _ => Loyalty::default(),
        };
        if now.days.insert(today.to_string()) {
            write_loyalty(&loyalty, &mut txn, slug, &now);
        }
        Ok(now.total())
    })
}

/// Get loyalty data for a single card slug.
/// Returns `Some((total_plays, last_played))` if the card has loyalty data.
pub fn get_loyalty(slug: &str) -> Option<(u32, String)> {
    loyalty_of(slug).map(|loyalty| (loyalty.total(), loyalty.last()))
}

/// A card's loyalty as it is kept, if it has any.
pub fn loyalty_of(slug: &str) -> Option<Loyalty> {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let loyalty = doc.get_or_insert_map("loyalty");
        let txn = doc.transact();
        match loyalty.get(&txn, slug) {
            Some(yrs::Out::YMap(entry)) => Some(read_loyalty(&entry, &txn)),
            _ => None,
        }
    })
}

/// How many days of play the record holds, over every card.
pub fn days_played() -> u32 {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let loyalty = doc.get_or_insert_map("loyalty");
        read_record(&loyalty, &doc.transact()).values().map(|card| card.days.len() as u32).sum()
    })
}

/// Clear all loyalty data.
pub fn clear_loyalty() {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let loyalty = doc.get_or_insert_map("loyalty");
        let mut txn = doc.transact_mut();
        let keys: Vec<String> = loyalty.keys(&txn).map(|k| k.to_string()).collect();
        for key in keys {
            loyalty.remove(&mut txn, &key);
        }
    });
}

// ── A game taken over from another device, or from a backup ───────
//
// The player's document holds two things. One is the game in progress:
// damage, timers, the affinity declared, everything New Game clears. The
// other is the player's record, which outlives every game: each card's
// loyalty, and the merges below.
//
// Two games have no sensible union, so a game is only ever taken whole: the
// state that arrives takes the place of the one here. The record is the
// opposite. What this device knows of it is added to what arrives, so that
// nothing either holds is lost and nothing is counted twice, and taking the
// same state over again changes nothing.
//
// ## Merges
//
// Each time a state is taken over that holds something new to this device,
// the two histories are compared and the comparison is kept, under `merges`:
//
//   "<id>": { at, by, gained, held }
//
// `gained` is how many changes the state taken held that this device had
// not seen, `held` how many this device held that the state taken had not.
// (A change is one step of a device's count in the document's state
// vector.) A new device taking a game over holds next to nothing: `held` is
// near zero. So does a device that only receives. Two records that grew
// apart, or were never one player's, each hold much the other lacks. The
// numbers are kept and nothing is decided by them: no merge is refused.
// `by` is `device` (the other device sent it) or `file` (a backup).

/// One state taken over, as `merges` keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct Merge {
    pub id: String,
    /// When, in milliseconds since 1970.
    pub at: f64,
    /// `device` or `file`.
    pub by: String,
    pub gained: u32,
    pub held: u32,
}

/// What came of taking a state over.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Taken {
    /// Changes the state taken held that this device had not seen.
    pub gained: u32,
    /// Changes this device held that the state taken had not.
    pub held: u32,
    /// Days of play the state taken brought to this device's record.
    pub days_new_here: u32,
    /// Days of play this device added to the record it took.
    pub days_new_there: u32,
    /// The state vector of the state as it arrived (base64): what its owner
    /// holds, to be sent what it lacks (`encode_diff`).
    pub their_state_vector: String,
}

/// How many changes `a` holds that `b` does not.
fn ahead(a: &StateVector, b: &StateVector) -> u32 {
    a.iter().map(|(client, clock)| clock.saturating_sub(b.get(client))).fold(0, u32::saturating_add)
}

fn read_merges<T: ReadTxn>(merges: &MapRef, txn: &T) -> Vec<Merge> {
    let mut all: Vec<Merge> = merges
        .iter(txn)
        .filter_map(|(id, entry)| match entry {
            yrs::Out::YMap(entry) => Some(Merge {
                id: id.to_string(),
                at: number_in(&entry, txn, "at").unwrap_or(0.0),
                by: text_in(&entry, txn, "by").unwrap_or_default(),
                gained: number_in(&entry, txn, "gained").unwrap_or(0.0) as u32,
                held: number_in(&entry, txn, "held").unwrap_or(0.0) as u32,
            }),
            _ => None,
        })
        .collect();
    all.sort_by(|a, b| a.at.total_cmp(&b.at).then_with(|| a.id.cmp(&b.id)));
    all
}

fn write_merge(merges: &MapRef, txn: &mut TransactionMut, merge: &Merge) {
    let entry = MapPrelim::from([
        ("at".to_string(), Any::from(merge.at)),
        ("by".to_string(), Any::from(merge.by.clone())),
        ("gained".to_string(), Any::from(merge.gained as f64)),
        ("held".to_string(), Any::from(merge.held as f64)),
    ]);
    merges.insert(txn, merge.id.as_str(), entry);
}

/// The states taken over so far, the earliest first.
pub fn merges() -> Vec<Merge> {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let merges = doc.get_or_insert_map("merges");
        read_merges(&merges, &doc.transact())
    })
}

/// Take over the game of `state_b64` (a document's full state, as
/// `encode_full_state` gives it) at `now`: it takes the place of the game
/// here, and this device's record is added to the one it brings. `by` says
/// how it came: `device` or `file`.
///
/// Nothing is changed if the state cannot be read.
pub fn take_over(state_b64: &str, now: f64, by: &str) -> Result<Taken, String> {
    if state_b64.is_empty() {
        return Err("no state".to_string());
    }
    let bytes = URL_SAFE_NO_PAD.decode(state_b64).map_err(|e| format!("base64 decode error: {}", e))?;
    let update = Update::decode_v1(&bytes).map_err(|e| format!("state decode error: {}", e))?;

    PLAYER_DOC.with(|cell| {
        // What this device holds, before any of it is let go.
        let (mine, my_merges, my_state_vector) = {
            let doc = cell.borrow();
            let (loyalty, merges) = (doc.get_or_insert_map("loyalty"), doc.get_or_insert_map("merges"));
            let txn = doc.transact();
            (read_record(&loyalty, &txn), read_merges(&merges, &txn), txn.state_vector())
        };

        let doc = with_roots(Doc::new());
        doc.transact_mut().apply_update(update).map_err(|e| format!("restore error: {}", e))?;
        let (loyalty, merges) = (doc.get_or_insert_map("loyalty"), doc.get_or_insert_map("merges"));
        let mut txn = doc.transact_mut();
        let theirs = read_record(&loyalty, &txn);
        let their_state_vector = txn.state_vector();
        let mut taken = Taken {
            gained: ahead(&their_state_vector, &my_state_vector),
            held: ahead(&my_state_vector, &their_state_vector),
            their_state_vector: URL_SAFE_NO_PAD.encode(their_state_vector.encode_v1()),
            ..Taken::default()
        };

        // The record: what this device knows is added to what arrived.
        let nothing = Loyalty::default();
        for (slug, had) in &mine {
            let brought = theirs.get(slug).unwrap_or(&nothing);
            taken.days_new_there += had.days.difference(&brought.days).count() as u32;
            let mut joined = brought.clone();
            joined.join(had);
            if joined != *brought {
                write_loyalty(&loyalty, &mut txn, slug, &joined);
            }
        }
        for (slug, brought) in &theirs {
            let had = mine.get(slug).unwrap_or(&nothing);
            taken.days_new_here += brought.days.difference(&had.days).count() as u32;
        }
        let known: BTreeSet<String> = read_merges(&merges, &txn).into_iter().map(|merge| merge.id).collect();
        for merge in my_merges.iter().filter(|merge| !known.contains(&merge.id)) {
            write_merge(&merges, &mut txn, merge);
        }
        // A state that holds nothing new to this device is one it has had
        // before (the same backup again): no two histories met.
        if taken.gained > 0 {
            let merge = Merge {
                id: format!("{:016x}", fastrand::u64(..)),
                at: now,
                by: by.to_string(),
                gained: taken.gained,
                held: taken.held,
            };
            write_merge(&merges, &mut txn, &merge);
        }
        drop(txn);
        *cell.borrow_mut() = doc;
        Ok(taken)
    })
}

/// The game in progress, in numbers: what would be lost if another took
/// its place.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    /// Cards with damage marked, or wasted.
    pub damaged: usize,
    pub timers: usize,
    pub affinity: Option<String>,
}

impl Progress {
    pub fn is_empty(&self) -> bool {
        *self == Progress::default()
    }
}

/// The game in progress on this device.
pub fn progress() -> Progress {
    let damaged = PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let cards = doc.get_or_insert_map("cards");
        let txn = doc.transact();
        cards
            .iter(&txn)
            .filter(|(_, card)| match card {
                yrs::Out::YMap(card) => {
                    let wasted = matches!(card.get(&txn, "wasted"), Some(yrs::Out::Any(Any::Bool(true))));
                    let marked = match card.get(&txn, "slots") {
                        Some(yrs::Out::YArray(slots)) => {
                            slots.iter(&txn).any(|slot| matches!(slot, yrs::Out::Any(Any::Bool(true))))
                        }
                        _ => false,
                    };
                    wasted || marked
                }
                _ => false,
            })
            .count()
    });
    Progress { damaged, timers: get_alarms().len(), affinity: get_active_affinity().map(|(name, _)| name) }
}

// ── Sync protocol (Cross-Device Sync — Phase E) ───────────────────

/// Encode the PLAYER_DOC state vector as a URL-safe base64 string.
/// Used for sync handshake step 1 (exchange SVs with peer device).
pub fn encode_state_vector() -> String {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let sv = doc.transact().state_vector().encode_v1();
        URL_SAFE_NO_PAD.encode(&sv)
    })
}

/// Given a remote device's state vector (URL-safe base64), compute the diff
/// update containing all local changes they haven't seen.
/// Returns URL-safe base64 update. Used for sync handshake step 2.
pub fn encode_diff(remote_sv_b64: &str) -> Result<String, String> {
    let sv_bytes = URL_SAFE_NO_PAD
        .decode(remote_sv_b64)
        .map_err(|e| format!("base64 decode error: {}", e))?;
    let remote_sv = StateVector::decode_v1(&sv_bytes)
        .map_err(|e| format!("state vector decode error: {}", e))?;

    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let update = doc.transact().encode_diff_v1(&remote_sv);
        Ok(URL_SAFE_NO_PAD.encode(&update))
    })
}

/// Apply a URL-safe base64-encoded yrs update from a remote device.
/// Used during sync handshake and live sync to merge remote changes.
pub fn apply_update(update_b64: &str) -> Result<(), String> {
    let update_bytes = URL_SAFE_NO_PAD
        .decode(update_b64)
        .map_err(|e| format!("base64 decode error: {}", e))?;
    let update = Update::decode_v1(&update_bytes)
        .map_err(|e| format!("update decode error: {}", e))?;

    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let mut txn = doc.transact_mut();
        txn.apply_update(update)
            .map_err(|e| format!("apply update error: {}", e))
    })
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// When the timers in these tests are made and changed, where the test
    /// does not care when.
    const NOW: f64 = 1_800_000_000_000.0;

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
        add_alarm(NOW, 5, "Scout patrol", "green");
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].remaining, 5);
        assert_eq!(alarms[0].name, "Scout patrol");
        assert_eq!(alarms[0].color_set, "green");
    }

    #[test]
    fn add_alarm_validates_color() {
        reset();
        add_alarm(NOW, 1, "", "invalid");
        let alarms = get_alarms();
        assert_eq!(alarms[0].color_set, "red");
    }

    #[test]
    fn tick_decrements_and_removes_expired() {
        reset();
        add_alarm(NOW, 2, "stays", "red");
        add_alarm(NOW, 1, "completes", "blue");

        tick_alarms(NOW);
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].remaining, 1);
        assert_eq!(alarms[1].remaining, 0);

        tick_alarms(NOW);
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].remaining, 0);

        tick_alarms(NOW);
        let alarms = get_alarms();
        assert!(alarms.is_empty());
    }

    #[test]
    fn remove_alarm_by_index() {
        reset();
        add_alarm(NOW, 5, "first", "red");
        add_alarm(NOW, 3, "second", "green");
        add_alarm(NOW, 1, "third", "blue");
        remove_alarm(NOW, 1);
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].remaining, 5);
        assert_eq!(alarms[1].remaining, 1);
    }

    #[test]
    fn remove_alarm_out_of_bounds_is_noop() {
        reset();
        add_alarm(NOW, 5, "", "red");
        remove_alarm(NOW, 99);
        assert_eq!(get_alarms().len(), 1);
    }

    // ── Settings tests ─────────────────────────────────────────────

    #[test]
    fn a_timer_is_dated_when_it_is_made_and_each_time_it_is_advanced() {
        reset();
        add_alarm(NOW, 3, "alpha", "red");
        assert_eq!(get_alarms()[0].changed, NOW);

        tick_alarms(NOW + 5_000.0);
        assert_eq!(get_alarms()[0].changed, NOW + 5_000.0);

        // A clock that has not moved on, or has gone back, still dates the
        // change after the one before it.
        tick_alarms(NOW + 5_000.0);
        assert_eq!(get_alarms()[0].changed, NOW + 5_001.0);
        tick_alarms(NOW);
        assert_eq!(get_alarms()[0].changed, NOW + 5_002.0);

        // A timer brought back from a room keeps the date it had there.
        add_alarm_with_id("t1", 2, "from a room", "blue", NOW + 9_000.0);
        assert_eq!(get_alarms()[1].changed, NOW + 9_000.0);
        reset();
    }

    #[test]
    fn a_timer_the_player_removes_is_remembered_as_removed_and_when() {
        reset();
        add_alarm_with_id("ran-out", 0, "ran out", "red", NOW);
        add_alarm_with_id("by-hand", 4, "removed by hand", "blue", NOW);
        add_alarm_with_id("faded", 0, "ran out and faded", "green", NOW);
        add_alarm_with_id("stays", 4, "stays", "pink", NOW);
        assert!(retired().is_empty());

        // By hand, by its place in the list.
        remove_alarm(NOW + 1_000.0, 1);
        // By the clock, once it has faded.
        remove_alarms_with_ids(NOW + 2_000.0, &["faded".to_string()]);
        // By the next advance, having run out.
        tick_alarms(NOW + 3_000.0);

        let mut gone = retired();
        gone.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            gone,
            [
                ("by-hand".to_string(), NOW + 1_000.0),
                ("faded".to_string(), NOW + 2_000.0),
                ("ran-out".to_string(), NOW + 3_000.0),
            ]
        );
        let left: Vec<String> = get_alarms().into_iter().map(|alarm| alarm.id).collect();
        assert_eq!(left, ["stays"]);
        reset();
    }

    #[test]
    fn clearing_the_timers_remembers_none_as_removed() {
        // The player's timers moving into a room, or being rewritten as a
        // copy of a room's: neither is the player removing a timer.
        reset();
        add_alarm_with_id("t1", 3, "alpha", "red", NOW);
        clear_alarms();
        assert!(get_alarms().is_empty());
        assert!(retired().is_empty());
        reset();
    }

    #[test]
    fn removing_every_timer_remembers_each_as_removed() {
        // New Game, alone.
        reset();
        add_alarm_with_id("t1", 3, "alpha", "red", NOW);
        add_alarm_with_id("t2", 5, "beta", "blue", NOW + 9_000.0);
        add_alarm_with_id("", 2, "from before ids", "red", 0.0);
        remove_all_alarms(NOW + 1_000.0);
        assert!(get_alarms().is_empty());
        let mut removed = retired();
        removed.sort_by(|a, b| a.0.cmp(&b.0));
        // Each is dated after the timer's last change, though the clock
        // (here behind the second timer's) says earlier.
        assert_eq!(removed, [("t1".to_string(), NOW + 1_000.0), ("t2".to_string(), NOW + 9_001.0)]);
        reset();
    }

    #[test]
    fn removals_made_in_a_room_are_remembered_once_and_the_later_date_kept() {
        reset();
        let encoded = || PLAYER_DOC.with(|cell| cell.borrow().transact().state_vector());
        remember_removed(&[("t1".to_string(), NOW), ("not an id!".to_string(), NOW)]);
        assert_eq!(retired(), [("t1".to_string(), NOW)]);

        // Told the same again, or an earlier date: nothing is written.
        let before = encoded();
        remember_removed(&[("t1".to_string(), NOW)]);
        remember_removed(&[("t1".to_string(), NOW - 5_000.0)]);
        assert_eq!(encoded(), before, "nothing was written");
        assert_eq!(retired(), [("t1".to_string(), NOW)]);

        // A later date replaces it.
        remember_removed(&[("t1".to_string(), NOW + 5_000.0)]);
        assert_eq!(retired(), [("t1".to_string(), NOW + 5_000.0)]);
        reset();
    }

    #[test]
    fn removals_made_in_a_room_more_than_a_day_before_the_latest_are_not_kept() {
        reset();
        let day = RETIRED_KEPT_MS + 60_000.0;
        let room = [("old".to_string(), NOW), ("new".to_string(), NOW + day)];
        remember_removed(&room);
        assert_eq!(retired(), [("new".to_string(), NOW + day)]);

        // Told of the same room again, the old one is not taken up and
        // dropped again: nothing is written.
        let encoded = || PLAYER_DOC.with(|cell| cell.borrow().transact().state_vector());
        let before = encoded();
        remember_removed(&room);
        assert_eq!(encoded(), before, "nothing was written");

        // One held from before is forgotten when a much later one arrives.
        remember_removed(&[("later".to_string(), NOW + 3.0 * day)]);
        assert_eq!(retired(), [("later".to_string(), NOW + 3.0 * day)]);
        reset();
    }

    #[test]
    fn a_removal_is_forgotten_after_a_day_and_a_timer_without_an_id_is_not_remembered() {
        reset();
        add_alarm_with_id("old", 3, "old", "red", NOW);
        add_alarm_with_id("", 3, "from before ids", "red", 0.0);
        add_alarm_with_id("new", 3, "new", "red", NOW);
        remove_alarm(NOW, 0);
        remove_alarm(NOW, 0);
        assert_eq!(retired(), [("old".to_string(), NOW + 1.0)]);

        // The next removal, more than a day on, forgets it.
        remove_alarm(NOW + RETIRED_KEPT_MS + 60_000.0, 0);
        assert_eq!(retired(), [("new".to_string(), NOW + RETIRED_KEPT_MS + 60_000.0)]);
        reset();
    }

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
        add_alarm(NOW, 5, "timer", "green");
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
        add_alarm(NOW, 1, "keep", "red");
        let result = restore_from_state("");
        assert!(result.is_ok());
        assert_eq!(get_alarms().len(), 1);
    }

    // ── Affinity tests ─────────────────────────────────────────────

    #[test]
    fn init_has_no_affinities() {
        reset();
        assert!(get_all_affinities().is_empty());
        assert!(get_active_affinity().is_none());
        assert!(get_affinity("Brutal").is_none());
    }

    #[test]
    fn declare_affinity_creates_entry() {
        reset();
        let result = declare_affinity("Brutal", "2026-02-25");
        assert!(result.is_ok());
        let (level, name) = result.unwrap();
        assert_eq!(level, 1);
        assert_eq!(name, "Brutal");

        let aff = get_affinity("Brutal");
        assert!(aff.is_some());
        let (lvl, last) = aff.unwrap();
        assert_eq!(lvl, 1);
        assert_eq!(last, "2026-02-25");
    }

    #[test]
    fn declare_affinity_rejects_second_declaration() {
        reset();
        declare_affinity("Brutal", "2026-02-25").unwrap();
        // Second declaration (same or different archetype) rejected — one per game
        let result = declare_affinity("Brutal", "2026-02-26");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already declared"));

        let result2 = declare_affinity("Avian", "2026-02-25");
        assert!(result2.is_err());

        // Level should still be 1
        let (lvl, _) = get_affinity("Brutal").unwrap();
        assert_eq!(lvl, 1);
    }

    #[test]
    fn clear_affinity_allows_new_declaration() {
        reset();
        declare_affinity("Brutal", "2026-02-25").unwrap();
        assert!(get_active_affinity().is_some());

        clear_affinity();
        assert!(get_active_affinity().is_none());
        assert!(get_all_affinities().is_empty());

        // Can declare again after clearing
        let result = declare_affinity("Avian", "2026-02-26");
        assert!(result.is_ok());
        assert_eq!(get_active_affinity().unwrap().0, "Avian");
    }

    #[test]
    fn clear_alarms_removes_all() {
        reset();
        add_alarm(NOW, 5, "first", "red");
        add_alarm(NOW, 3, "second", "green");
        assert_eq!(get_alarms().len(), 2);
        clear_alarms();
        assert!(get_alarms().is_empty());
    }

    #[test]
    fn declare_affinity_rejects_invalid_archetype() {
        reset();
        let result = declare_affinity("FakeType", "2026-02-25");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown archetype"));
    }

    #[test]
    fn declare_affinity_rejects_empty_date() {
        reset();
        let result = declare_affinity("Brutal", "");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Missing today date"));
    }

    #[test]
    fn get_active_affinity_after_clear_and_redeclare() {
        reset();
        declare_affinity("Brutal", "2026-02-24").unwrap();
        assert_eq!(get_active_affinity().unwrap().0, "Brutal");

        // Clear and pick a different one
        clear_affinity();
        declare_affinity("Avian", "2026-02-25").unwrap();
        let (name, level) = get_active_affinity().unwrap();
        assert_eq!(name, "Avian");
        assert_eq!(level, 1);
    }

    #[test]
    fn affinity_persists_across_roundtrip() {
        reset();
        declare_affinity("Brutal", "2026-02-24").unwrap();

        let state = encode_full_state();

        // Reset and restore
        init_player_doc();
        assert!(get_all_affinities().is_empty());

        let result = restore_from_state(&state);
        assert!(result.is_ok());

        let (brutal_lvl, brutal_last) = get_affinity("Brutal").unwrap();
        assert_eq!(brutal_lvl, 1);
        assert_eq!(brutal_last, "2026-02-24");
    }

    #[test]
    fn valid_archetypes_returns_15() {
        assert_eq!(valid_archetypes().len(), 15);
        assert!(valid_archetypes().contains(&"Brutal"));
        assert!(valid_archetypes().contains(&"Entropic"));
    }

    // ── Loyalty tests ──────────────────────────────────────────────

    #[test]
    fn init_has_no_loyalty() {
        reset();
        assert!(get_loyalty("brox_the_defiant").is_none());
    }

    #[test]
    fn increment_loyalty_creates_entry() {
        reset();
        let result = increment_loyalty("brox_the_defiant", "2026-02-25");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);

        let (total, last) = get_loyalty("brox_the_defiant").unwrap();
        assert_eq!(total, 1);
        assert_eq!(last, "2026-02-25");
    }

    #[test]
    fn increment_loyalty_once_per_day() {
        reset();
        increment_loyalty("brox", "2026-02-25").unwrap();
        // Same day — should no-op, return current total
        let result = increment_loyalty("brox", "2026-02-25").unwrap();
        assert_eq!(result, 1);

        // Different day — should increment
        let result = increment_loyalty("brox", "2026-02-26").unwrap();
        assert_eq!(result, 2);

        let (total, last) = get_loyalty("brox").unwrap();
        assert_eq!(total, 2);
        assert_eq!(last, "2026-02-26");
    }

    #[test]
    fn increment_loyalty_multiple_cards() {
        reset();
        increment_loyalty("card_a", "2026-02-25").unwrap();
        increment_loyalty("card_b", "2026-02-25").unwrap();
        increment_loyalty("card_a", "2026-02-26").unwrap();

        assert_eq!(get_loyalty("card_a").unwrap().0, 2);
        assert_eq!(get_loyalty("card_b").unwrap().0, 1);
    }

    #[test]
    fn increment_loyalty_rejects_empty_slug() {
        reset();
        let result = increment_loyalty("", "2026-02-25");
        assert!(result.is_err());
    }

    #[test]
    fn increment_loyalty_rejects_empty_date() {
        reset();
        let result = increment_loyalty("brox", "");
        assert!(result.is_err());
    }

    #[test]
    fn clear_loyalty_removes_all() {
        reset();
        increment_loyalty("card_a", "2026-02-25").unwrap();
        increment_loyalty("card_b", "2026-02-25").unwrap();
        clear_loyalty();
        assert!(get_loyalty("card_a").is_none());
        assert!(get_loyalty("card_b").is_none());
    }

    #[test]
    fn loyalty_persists_across_roundtrip() {
        reset();
        increment_loyalty("brox", "2026-02-25").unwrap();
        increment_loyalty("brox", "2026-02-26").unwrap();

        let state = encode_full_state();

        init_player_doc();
        assert!(get_loyalty("brox").is_none());

        restore_from_state(&state).unwrap();
        let (total, last) = get_loyalty("brox").unwrap();
        assert_eq!(total, 2);
        assert_eq!(last, "2026-02-26");
    }

    #[test]
    fn a_day_already_counted_is_not_counted_again_whatever_order_the_days_come_in() {
        reset();
        increment_loyalty("brox", "2026-02-26").unwrap();
        // The device's clock was set back: an earlier day is still a day.
        assert_eq!(increment_loyalty("brox", "2026-02-20").unwrap(), 2);
        assert_eq!(increment_loyalty("brox", "2026-02-26").unwrap(), 2);
        assert_eq!(increment_loyalty("brox", "2026-02-20").unwrap(), 2);
        let kept = loyalty_of("brox").unwrap();
        assert_eq!(kept.days, BTreeSet::from(["2026-02-20".to_string(), "2026-02-26".to_string()]));
        assert_eq!(get_loyalty("brox").unwrap(), (2, "2026-02-26".to_string()));
    }

    /// A card's loyalty as the engine kept it before the days were: a count
    /// and the last day.
    fn loyalty_as_it_was(slug: &str, total: u32, last: &str) {
        PLAYER_DOC.with(|cell| {
            let doc = cell.borrow();
            let loyalty = doc.get_or_insert_map("loyalty");
            let mut txn = doc.transact_mut();
            let entry = MapPrelim::from([
                ("total_plays".to_string(), Any::from(total as f64)),
                ("last_played".to_string(), Any::from(last.to_string())),
            ]);
            loyalty.insert(&mut txn, slug, entry);
        });
    }

    #[test]
    fn loyalty_kept_as_a_count_is_read_and_carried_on_from() {
        reset();
        loyalty_as_it_was("brox", 5, "2026-02-25");
        assert_eq!(get_loyalty("brox").unwrap(), (5, "2026-02-25".to_string()));
        // Its last day is one of the five, so that day played again is not a sixth.
        assert_eq!(increment_loyalty("brox", "2026-02-25").unwrap(), 5);
        assert_eq!(increment_loyalty("brox", "2026-02-27").unwrap(), 6);
        let kept = loyalty_of("brox").unwrap();
        assert_eq!(kept.earlier, 4);
        assert_eq!(kept.days.len(), 2);
        assert_eq!(get_loyalty("brox").unwrap(), (6, "2026-02-27".to_string()));

        // What the engine as it was reads is kept in step.
        PLAYER_DOC.with(|cell| {
            let doc = cell.borrow();
            let loyalty = doc.get_or_insert_map("loyalty");
            let txn = doc.transact();
            let Some(yrs::Out::YMap(entry)) = loyalty.get(&txn, "brox") else { panic!("no entry") };
            assert_eq!(number_in(&entry, &txn, "total_plays"), Some(6.0));
            assert_eq!(text_in(&entry, &txn, "last_played").as_deref(), Some("2026-02-27"));
        });
    }

    #[test]
    fn a_play_counted_by_the_engine_as_it_was_on_an_entry_that_keeps_days_is_not_lost() {
        reset();
        increment_loyalty("brox", "2026-02-25").unwrap();
        increment_loyalty("brox", "2026-02-26").unwrap();
        // A page still running the engine as it was counts a third day its
        // way: the count and the last day, and no day among the days.
        PLAYER_DOC.with(|cell| {
            let doc = cell.borrow();
            let loyalty = doc.get_or_insert_map("loyalty");
            let mut txn = doc.transact_mut();
            let Some(yrs::Out::YMap(entry)) = loyalty.get(&txn, "brox") else { panic!("no entry") };
            entry.insert(&mut txn, "total_plays", Any::from(3.0));
            entry.insert(&mut txn, "last_played", Any::from("2026-02-27".to_string()));
        });
        assert_eq!(get_loyalty("brox").unwrap(), (3, "2026-02-27".to_string()));
        // The next count here puts it among the days.
        assert_eq!(increment_loyalty("brox", "2026-02-28").unwrap(), 4);
        let kept = loyalty_of("brox").unwrap();
        assert_eq!((kept.earlier, kept.days.len()), (0, 4));
    }

    // ── A game taken over ──────────────────────────────────────────

    /// When a state is taken over in these tests.
    const THEN: f64 = 1_800_000_500_000.0;

    /// A device with a game in progress and a record: damage on Brox, a
    /// timer, an affinity, and Brox played on two days. Its state.
    fn a_device_mid_game() -> String {
        reset();
        ensure_card_state("brox", 3);
        toggle_slot("brox", 2);
        add_alarm(NOW, 5, "siege", "red");
        declare_affinity("Brutal", "2026-02-25").unwrap();
        increment_loyalty("brox", "2026-02-24").unwrap();
        increment_loyalty("brox", "2026-02-25").unwrap();
        encode_full_state()
    }

    #[test]
    fn a_new_device_takes_the_game_and_the_record_whole() {
        let theirs = a_device_mid_game();
        reset();
        let taken = take_over(&theirs, THEN, "device").unwrap();

        assert!(get_slot("brox", 2));
        assert_eq!(get_alarms()[0].name, "siege");
        assert_eq!(get_active_affinity().unwrap().0, "Brutal");
        assert_eq!(get_loyalty("brox").unwrap(), (2, "2026-02-25".to_string()));
        assert_eq!((taken.days_new_here, taken.days_new_there), (2, 0));
        // All this device held was the one change every new document makes.
        assert!(taken.gained > 5, "{:?}", taken);
        assert_eq!(taken.held, 1);
    }

    #[test]
    fn the_game_here_gives_way_to_the_one_taken_and_the_record_here_is_kept() {
        let theirs = a_device_mid_game();
        // This device: another game, and plays of its own. One day is the same.
        reset();
        ensure_card_state("brox", 3);
        toggle_slot("brox", 1);
        ensure_card_state("liliel", 2);
        toggle_wasted("liliel");
        add_alarm(NOW, 2, "mine", "blue");
        declare_affinity("Avian", "2026-02-26").unwrap();
        increment_loyalty("brox", "2026-02-25").unwrap();
        increment_loyalty("brox", "2026-02-26").unwrap();
        increment_loyalty("liliel", "2026-02-26").unwrap();

        let taken = take_over(&theirs, THEN, "device").unwrap();

        // The game is theirs, whole: nothing of this device's is mixed in.
        assert!(get_slot("brox", 2) && !get_slot("brox", 1));
        assert!(!has_card_state("liliel"));
        let timers: Vec<String> = get_alarms().into_iter().map(|alarm| alarm.name).collect();
        assert_eq!(timers, ["siege"]);
        assert_eq!(get_all_affinities().len(), 1);
        assert_eq!(get_active_affinity().unwrap().0, "Brutal");

        // The record is both: three days of Brox, not four, and Liliel's one.
        assert_eq!(get_loyalty("brox").unwrap(), (3, "2026-02-26".to_string()));
        assert_eq!(get_loyalty("liliel").unwrap(), (1, "2026-02-26".to_string()));
        assert_eq!((taken.days_new_here, taken.days_new_there), (1, 2));
        assert!(taken.gained > 0 && taken.held > 0, "{:?}", taken);
    }

    #[test]
    fn what_the_other_device_lacks_is_what_this_one_added_to_the_record() {
        let theirs = a_device_mid_game();
        reset();
        increment_loyalty("liliel", "2026-02-26").unwrap();
        let taken = take_over(&theirs, THEN, "device").unwrap();
        let back = encode_diff(&taken.their_state_vector).unwrap();
        let here = encode_full_state();

        // The other device, as it was, is sent it.
        restore_from_state(&theirs).unwrap();
        apply_update(&back).unwrap();
        assert_eq!(get_loyalty("liliel").unwrap().0, 1);
        assert_eq!(get_loyalty("brox").unwrap().0, 2);
        // Its game is as it left it, and the two now hold the same.
        assert!(get_slot("brox", 2));
        assert_eq!(get_alarms()[0].name, "siege");
        assert_eq!(merges().len(), 1);
        assert_eq!(encode_state_vector(), {
            restore_from_state(&here).unwrap();
            encode_state_vector()
        });
    }

    #[test]
    fn taking_the_same_state_over_again_counts_nothing_twice() {
        let theirs = a_device_mid_game();
        reset();
        increment_loyalty("brox", "2026-02-26").unwrap();
        take_over(&theirs, THEN, "file").unwrap();
        let once = (get_loyalty("brox").unwrap(), merges());
        assert_eq!(once.0.0, 3);
        assert_eq!(once.1.len(), 1);

        let again = take_over(&theirs, THEN + 1000.0, "file").unwrap();
        assert_eq!(again.gained, 0, "it holds nothing this device has not seen");
        assert_eq!((again.days_new_here, again.days_new_there), (0, 1));
        assert_eq!((get_loyalty("brox").unwrap(), merges()), once);
    }

    #[test]
    fn plays_counted_before_the_days_were_kept_are_not_added_together() {
        // Their record of Brox is a count; so is this device's.
        reset();
        loyalty_as_it_was("brox", 5, "2026-02-25");
        let theirs = encode_full_state();
        reset();
        loyalty_as_it_was("brox", 3, "2026-02-26");
        take_over(&theirs, THEN, "device").unwrap();
        // The larger count, less its own last day, and the two last days.
        let kept = loyalty_of("brox").unwrap();
        assert_eq!(kept.earlier, 4);
        assert_eq!(kept.days.len(), 2);
        assert_eq!(get_loyalty("brox").unwrap(), (6, "2026-02-26".to_string()));
    }

    #[test]
    fn each_state_taken_over_that_holds_something_new_is_kept_as_a_merge() {
        let theirs = a_device_mid_game();
        reset();
        increment_loyalty("liliel", "2026-02-26").unwrap();
        let first = take_over(&theirs, THEN, "device").unwrap();
        let kept = merges();
        assert_eq!(kept.len(), 1);
        assert_eq!((kept[0].at, kept[0].by.as_str()), (THEN, "device"));
        assert_eq!((kept[0].gained, kept[0].held), (first.gained, first.held));
        assert_eq!(kept[0].id.len(), 16);

        // The merges this device holds go with it into the next state it takes.
        let mine = encode_full_state();
        let elsewhere = {
            reset();
            increment_loyalty("hilbert", "2026-03-01").unwrap();
            encode_full_state()
        };
        restore_from_state(&mine).unwrap();
        take_over(&elsewhere, THEN + 5000.0, "file").unwrap();
        let kept = merges();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].by, "device");
        assert_eq!(kept[1].by, "file");
        assert!(kept[1].held > kept[0].held, "this device held the whole of the first by then: {:?}", kept);
    }

    #[test]
    fn a_state_that_cannot_be_read_changes_nothing() {
        let before = a_device_mid_game();
        // Nothing, what is not base64, and a state cut short.
        for unreadable in ["", "!!!", &before[..before.len() / 2]] {
            assert!(take_over(unreadable, THEN, "file").is_err(), "{:?}", unreadable);
            assert_eq!(encode_full_state(), before);
        }
    }

    #[test]
    fn the_game_in_progress_is_counted() {
        reset();
        assert!(progress().is_empty());
        // A card that has only been looked at is not damage.
        ensure_card_state("brox", 3);
        assert!(progress().is_empty());
        toggle_slot("brox", 2);
        ensure_card_state("liliel", 2);
        toggle_wasted("liliel");
        add_alarm(NOW, 5, "siege", "red");
        declare_affinity("Brutal", "2026-02-25").unwrap();
        assert_eq!(progress(), Progress { damaged: 2, timers: 1, affinity: Some("Brutal".to_string()) });
        // The record is no part of it.
        reset();
        increment_loyalty("brox", "2026-02-25").unwrap();
        assert!(progress().is_empty());
    }

    // ── Sync protocol tests (Phase E) ──────────────────────────────

    #[test]
    fn state_vector_is_valid_base64() {
        reset();
        add_alarm(NOW, 3, "test", "blue");
        let sv = encode_state_vector();
        assert!(!sv.is_empty());
        let decoded = URL_SAFE_NO_PAD.decode(&sv);
        assert!(decoded.is_ok());
    }

    #[test]
    fn encode_diff_returns_valid_update() {
        use yrs::updates::encoder::Encode;
        reset();
        add_alarm(NOW, 5, "local timer", "green");

        // Create a separate Doc to simulate the remote device
        let remote_doc = Doc::new();
        {
            let mut txn = remote_doc.transact_mut();
            txn.get_or_insert_array("alarms");
            txn.get_or_insert_map("cards");
            txn.get_or_insert_map("affinity");
            txn.get_or_insert_map("loyalty");
            txn.get_or_insert_map("settings");
        }

        // Get remote SV
        let remote_sv = remote_doc.transact().state_vector().encode_v1();
        let remote_sv_b64 = URL_SAFE_NO_PAD.encode(&remote_sv);

        // Compute diff for remote
        let diff = encode_diff(&remote_sv_b64);
        assert!(diff.is_ok());
        let diff_b64 = diff.unwrap();
        assert!(!diff_b64.is_empty());

        // Apply diff to remote — should get our alarm
        let diff_bytes = URL_SAFE_NO_PAD.decode(&diff_b64).unwrap();
        let update = Update::decode_v1(&diff_bytes).unwrap();
        remote_doc.transact_mut().apply_update(update).unwrap();

        let arr = remote_doc.get_or_insert_array("alarms");
        let txn = remote_doc.transact();
        assert_eq!(arr.len(&txn), 1);
    }

    #[test]
    fn apply_update_merges_remote_changes() {
        reset();
        add_alarm(NOW, 5, "local", "red");

        // Create a separate Doc to simulate remote device
        let remote_doc = Doc::new();
        {
            let mut txn = remote_doc.transact_mut();
            txn.get_or_insert_map("cards");
            let alarms = txn.get_or_insert_array("alarms");
            txn.get_or_insert_map("affinity");
            txn.get_or_insert_map("loyalty");
            txn.get_or_insert_map("settings");
            let alarm_map = yrs::MapPrelim::from([
                ("remaining".to_string(), Any::from(3_f64)),
                ("name".to_string(), Any::from("remote".to_string())),
                ("color_set".to_string(), Any::from("blue".to_string())),
            ]);
            alarms.insert(&mut txn, 0, alarm_map);
        }

        // Encode the remote Doc changes as an update
        let remote_update = remote_doc
            .transact()
            .encode_diff_v1(&StateVector::default());
        let remote_update_b64 = URL_SAFE_NO_PAD.encode(&remote_update);

        // Apply the remote update to our PLAYER_DOC
        let result = apply_update(&remote_update_b64);
        assert!(result.is_ok());

        // Both local and remote alarms should exist
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
    }

    #[test]
    fn apply_update_rejects_invalid_base64() {
        reset();
        let result = apply_update("not_valid_base64!!!");
        assert!(result.is_err());
    }

    #[test]
    fn sync_encode_diff_and_apply_roundtrip() {
        use yrs::updates::encoder::Encode;
        // Verify that encode_diff → apply_update merges remote data into PLAYER_DOC
        reset();
        add_alarm(NOW, 5, "local_timer", "green");

        // Create a remote doc with different data
        let remote = Doc::new();
        {
            let mut txn = remote.transact_mut();
            txn.get_or_insert_map("cards");
            txn.get_or_insert_array("alarms");
            txn.get_or_insert_map("affinity");
            txn.get_or_insert_map("loyalty");
            txn.get_or_insert_map("settings");
        }

        // Get local SV, compute remote diff against it (empty remote → full local diff)
        let local_sv = encode_state_vector();
        let local_sv_bytes = URL_SAFE_NO_PAD.decode(&local_sv).unwrap();
        let sv = StateVector::decode_v1(&local_sv_bytes).unwrap();

        // Remote encodes its diff for us (nothing new from remote)
        let remote_diff = remote.transact().encode_diff_v1(&sv);
        let remote_diff_b64 = URL_SAFE_NO_PAD.encode(&remote_diff);

        // Apply remote diff — should not break local state
        apply_update(&remote_diff_b64).unwrap();

        // Local alarm should still be there
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].name, "local_timer");
    }

    #[test]
    fn encode_diff_rejects_invalid_sv() {
        reset();
        let result = encode_diff("not_valid_base64!!!");
        assert!(result.is_err());
    }
}
