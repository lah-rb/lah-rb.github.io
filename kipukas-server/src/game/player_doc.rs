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
//! - **First visit:** Fresh Doc created with empty defaults
//! - **Every page load:** Restored from `kipukas_player_doc` localStorage key (base64 binary)
//! - **Every mutation:** Auto-persisted to localStorage via worker → main thread message
//! - **Multiplayer:** Independent of ROOM_DOC; alarms bridge via `crdt.rs` seed/export

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::cell::RefCell;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Any, Array, ArrayPrelim, Doc, Map, MapPrelim, ReadTxn, StateVector, Transact, Update, WriteTxn};

use crate::game::state::Alarm;

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
    let doc = Doc::new();
    {
        let mut txn = doc.transact_mut();
        txn.get_or_insert_map("cards");
        txn.get_or_insert_array("alarms");
        txn.get_or_insert_map("affinity");
        txn.get_or_insert_map("loyalty");
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
            txn.get_or_insert_map("affinity");
            txn.get_or_insert_map("loyalty");
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

/// Clear all alarms (used by "New Game" reset).
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

/// Increment loyalty for a card slug. Enforces once-per-day: if `last_played`
/// already equals `today`, returns Ok but does not increment.
/// Returns `Ok(new_total_plays)` on success.
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

        match loyalty.get(&txn, slug) {
            Some(yrs::Out::YMap(entry)) => {
                // Check once-per-day
                let last = match entry.get(&txn, "last_played") {
                    Some(yrs::Out::Any(Any::String(s))) => s.to_string(),
                    _ => String::new(),
                };
                let current = match entry.get(&txn, "total_plays") {
                    Some(yrs::Out::Any(Any::Number(n))) => n as u32,
                    _ => 0,
                };
                if last == today {
                    return Ok(current); // Already played today, no-op
                }
                let new_total = current + 1;
                entry.insert(&mut txn, "total_plays", Any::from(new_total as f64));
                entry.insert(&mut txn, "last_played", Any::from(today.to_string()));
                Ok(new_total)
            }
            _ => {
                // First play ever for this card
                let entry = MapPrelim::from([
                    ("total_plays".to_string(), Any::from(1.0_f64)),
                    ("last_played".to_string(), Any::from(today.to_string())),
                ]);
                loyalty.insert(&mut txn, slug, entry);
                Ok(1)
            }
        }
    })
}

/// Get loyalty data for a single card slug.
/// Returns `Some((total_plays, last_played))` if the card has loyalty data.
pub fn get_loyalty(slug: &str) -> Option<(u32, String)> {
    PLAYER_DOC.with(|cell| {
        let doc = cell.borrow();
        let loyalty = doc.get_or_insert_map("loyalty");
        let txn = doc.transact();
        match loyalty.get(&txn, slug) {
            Some(yrs::Out::YMap(entry)) => {
                let total = match entry.get(&txn, "total_plays") {
                    Some(yrs::Out::Any(Any::Number(n))) => n as u32,
                    _ => 0,
                };
                let last = match entry.get(&txn, "last_played") {
                    Some(yrs::Out::Any(Any::String(s))) => s.to_string(),
                    _ => String::new(),
                };
                Some((total, last))
            }
            _ => None,
        }
    })
}

/// Clear all loyalty data (used by "New Game" reset).
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
        add_alarm(5, "first", "red");
        add_alarm(3, "second", "green");
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

    // ── Sync protocol tests (Phase E) ──────────────────────────────

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
    fn encode_diff_returns_valid_update() {
        use yrs::updates::encoder::Encode;
        reset();
        add_alarm(5, "local timer", "green");

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
        add_alarm(5, "local", "red");

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
        add_alarm(5, "local_timer", "green");

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
