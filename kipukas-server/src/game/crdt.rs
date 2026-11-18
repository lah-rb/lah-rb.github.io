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
//! └── "timers" (MapRef) — shared turn timers, keyed by a stable timer id
//!     ├── "<id>" → JSON string { "remaining": i32, "name": String,
//!     │                          "color_set": String, "seq": u32 }
//!     └── ...
//! ```
//!
//! Timers are keyed by id rather than held in a list for two reasons:
//!
//! - Both players may bring the same timer into a room (after an earlier
//!   session each keeps a copy). Under the same key the two copies converge
//!   to one entry; in a list they appeared twice.
//! - "Remove" targets a timer, not a position, so it cannot hit the wrong
//!   one when the other player adds or removes at the same moment.
//!
//! Each value is one JSON string, so a timer is always replaced as a whole.
//! `seq` gives display order; ties are broken by id so both players sort
//! identically.
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
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Any, Doc, Map, MapRef, ReadTxn, StateVector, Transact, TransactionMut, Update, WriteTxn};

use crate::game::fists::FISTS;
use crate::game::player_doc;
use crate::game::state::{is_valid_alarm_id, new_alarm_id, Alarm};

/// Root map holding the shared timers.
const TIMERS: &str = "timers";

/// One shared timer as stored in the room document (a JSON string under its id).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TimerEntry {
    remaining: i32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    color_set: String,
    /// Display order. Ties are broken by id.
    #[serde(default)]
    seq: u32,
}

thread_local! {
    static CRDT_DOC: RefCell<Doc> = RefCell::new(Doc::new());
}

// ── Doc lifecycle ──────────────────────────────────────────────────

/// Initialize a fresh yrs Doc for a new multiplayer session.
/// Call on room create/join.
pub fn init_doc() {
    CRDT_DOC.with(|cell| {
        let doc = Doc::new();
        // Pre-create the root maps so both peers share the same root types.
        {
            let mut txn = doc.transact_mut();
            txn.get_or_insert_map(TIMERS);
            txn.get_or_insert_map(FISTS);
        }
        *cell.borrow_mut() = doc;
    });
}

/// Reset the Doc (on disconnect). Replaces with a fresh empty Doc.
pub fn reset_doc() {
    init_doc();
}

/// Run `f` with the room doc. Other shared state (fists combat) lives in the
/// same doc and reads or writes it through this.
pub fn with_doc<R>(f: impl FnOnce(&Doc) -> R) -> R {
    CRDT_DOC.with(|cell| f(&cell.borrow()))
}

/// Run `f`, which may change the room doc, and return everything it changed
/// as one base64 update for the peer. `f` returns whether it changed
/// anything; if not, the result is an empty string and nothing needs sending.
pub fn capture_update(f: impl FnOnce() -> bool) -> String {
    let before = with_doc(|doc| doc.transact().state_vector());
    if !f() {
        return String::new();
    }
    with_doc(|doc| encode_diff_since(doc, &before))
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

// ── Timers on a document ───────────────────────────────────────────
//
// These take the document as an argument so tests can run two of them and
// sync between them. The public functions below apply them to the room doc.

/// All timers in display order: by `seq`, then by id.
fn read_timers<T: ReadTxn>(timers: &MapRef, txn: &T) -> Vec<(String, TimerEntry)> {
    let mut out: Vec<(String, TimerEntry)> = Vec::new();
    for (id, value) in timers.iter(txn) {
        // Ids and values come from the other player too: skip anything that is
        // not a well-formed timer rather than trusting it.
        if !is_valid_alarm_id(id) {
            continue;
        }
        if let yrs::Out::Any(Any::String(json)) = value {
            if let Ok(entry) = serde_json::from_str::<TimerEntry>(&json) {
                out.push((id.to_string(), entry));
            }
        }
    }
    out.sort_by(|a, b| a.1.seq.cmp(&b.1.seq).then_with(|| a.0.cmp(&b.0)));
    out
}

fn write_timer(timers: &MapRef, txn: &mut TransactionMut, id: &str, entry: &TimerEntry) {
    let json = serde_json::to_string(entry).unwrap_or_else(|_| "{}".to_string());
    timers.insert(txn, id.to_string(), Any::from(json));
}

/// The `seq` for a timer added now: one past the highest in use.
fn next_seq<T: ReadTxn>(timers: &MapRef, txn: &T) -> u32 {
    read_timers(timers, txn)
        .iter()
        .map(|(_, e)| e.seq)
        .max()
        .map_or(1, |m| m.saturating_add(1))
}

fn add_timer_in(doc: &Doc, id: &str, turns: i32, name: &str, color_set: &str) {
    if !is_valid_alarm_id(id) {
        return;
    }
    let timers = doc.get_or_insert_map(TIMERS);
    let mut txn = doc.transact_mut();
    let entry = TimerEntry {
        remaining: turns,
        name: name.to_string(),
        color_set: validate_color_set(color_set).to_string(),
        seq: next_seq(&timers, &txn),
    };
    write_timer(&timers, &mut txn, id, &entry);
}

/// Decrement every timer by 1; remove those that were already at 0.
fn tick_timers_in(doc: &Doc) {
    let timers = doc.get_or_insert_map(TIMERS);
    let mut txn = doc.transact_mut();
    for (id, mut entry) in read_timers(&timers, &txn) {
        if entry.remaining <= 0 {
            timers.remove(&mut txn, &id);
        } else {
            entry.remaining -= 1;
            write_timer(&timers, &mut txn, &id, &entry);
        }
    }
}

fn remove_timer_in(doc: &Doc, id: &str) {
    let timers = doc.get_or_insert_map(TIMERS);
    let mut txn = doc.transact_mut();
    timers.remove(&mut txn, id);
}

fn alarms_in(doc: &Doc) -> Vec<Alarm> {
    let timers = doc.get_or_insert_map(TIMERS);
    let txn = doc.transact();
    read_timers(&timers, &txn)
        .into_iter()
        .map(|(id, e)| Alarm {
            id,
            remaining: e.remaining,
            name: e.name,
            color_set: validate_color_set(&e.color_set).to_string(),
        })
        .collect()
}

// ── Updates for the other player ───────────────────────────────────
//
// A change this player makes to the room document has to reach the other
// player. The engine cannot send anything: the page holds the connection.
// So a change made through an engine route is queued here, and after every
// post the engine's worker takes what is queued (see `crate::page`) and hands
// it to the page, which sends it (`ROOM_UPDATES` in kipukas-worker.js and
// kipukas-multiplayer.js). An update that is lost on the way (the other
// player is away) is made up by the exchange both players do when they are
// next connected.

thread_local! {
    static OUTBOX: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Queue an update of the room document to be sent to the other player. An
/// empty update (the change changed nothing) is not queued.
pub fn queue_for_peer(update: String) {
    if !update.is_empty() {
        OUTBOX.with(|outbox| outbox.borrow_mut().push(update));
    }
}

/// Take the updates queued for the other player, oldest first. They are
/// taken once: what is returned is no longer queued.
pub fn take_outbox() -> Vec<String> {
    OUTBOX.with(|outbox| std::mem::take(&mut *outbox.borrow_mut()))
}

/// Run a mutation on the room doc and return the resulting update as base64.
fn mutate_room_doc(f: impl FnOnce(&Doc)) -> String {
    CRDT_DOC.with(|cell| {
        let doc = cell.borrow();
        let sv = doc.transact().state_vector();
        f(&doc);
        encode_diff_since(&doc, &sv)
    })
}

// ── Alarm mutations (return update bytes as base64) ────────────────

/// Add a new alarm to the room doc under a fresh id. Returns the update as a
/// base64 string.
pub fn add_alarm(turns: i32, name: &str, color_set: &str) -> String {
    add_alarm_with_id(&new_alarm_id(), turns, name, color_set)
}

/// Add an alarm that already has an id (a player's own timer being shared).
/// Returns the update as a base64 string.
pub fn add_alarm_with_id(id: &str, turns: i32, name: &str, color_set: &str) -> String {
    mutate_room_doc(|doc| add_timer_in(doc, id, turns, name, color_set))
}

/// Tick all alarms: decrement by 1, remove completed (were already at 0).
/// Returns the update as a base64 string.
pub fn tick_alarms() -> String {
    mutate_room_doc(tick_timers_in)
}

/// Remove an alarm by id. Returns the update as a base64 string.
pub fn remove_alarm(id: &str) -> String {
    mutate_room_doc(|doc| remove_timer_in(doc, id))
}

// ── Read state ─────────────────────────────────────────────────────

/// Read the current alarms from the room doc, in display order.
/// Used for rendering the alarm list HTML.
pub fn get_alarms() -> Vec<Alarm> {
    CRDT_DOC.with(|cell| alarms_in(&cell.borrow()))
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

// ── Player doc ↔ room doc bridge ───────────────────────────────────

/// Move this player's own timers into the room doc.
///
/// Called on room create and on room join, so both players' pre-existing
/// timers become shared. Each timer goes in under its own id: when both
/// players hold a copy of the same timer (each kept one after an earlier
/// session), the copies land on the same key and converge to a single timer.
pub fn seed_from_local() {
    let local_alarms: Vec<Alarm> = player_doc::get_alarms();
    if local_alarms.is_empty() {
        return;
    }
    CRDT_DOC.with(|cell| {
        let doc = cell.borrow();
        let timers = doc.get_or_insert_map(TIMERS);
        let mut txn = doc.transact_mut();
        let mut seq = next_seq(&timers, &txn);
        for alarm in &local_alarms {
            // Timers saved before ids existed get one now.
            let id = if is_valid_alarm_id(&alarm.id) {
                alarm.id.clone()
            } else {
                new_alarm_id()
            };
            // Already in the room under this id: keep the room's copy.
            if timers.get(&txn, &id).is_some() {
                continue;
            }
            let entry = TimerEntry {
                remaining: alarm.remaining,
                name: alarm.name.clone(),
                color_set: validate_color_set(&alarm.color_set).to_string(),
                seq,
            };
            write_timer(&timers, &mut txn, &id, &entry);
            seq = seq.saturating_add(1);
        }
    });
    // The timers now live in the room doc. The room routes call
    // export_to_local() after each change to keep PLAYER_DOC in step, and on
    // disconnect before the room doc is reset.
    player_doc::clear_alarms();
}

/// Copy the room's timers back into this player's own timers, ids included.
/// Called after each change and on disconnect, so shared timers survive as
/// the player's own once the session ends.
pub fn export_to_local() {
    let crdt_alarms = get_alarms();
    player_doc::clear_alarms();
    for alarm in &crdt_alarms {
        player_doc::add_alarm_with_id(&alarm.id, alarm.remaining, &alarm.name, &alarm.color_set);
    }
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
    fn remove_alarm_by_id() {
        reset();
        add_alarm(5, "first", "red");
        add_alarm(3, "second", "green");
        add_alarm(1, "third", "blue");
        let second = get_alarms()[1].id.clone();
        remove_alarm(&second);
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].remaining, 5);
        assert_eq!(alarms[1].remaining, 1);
    }

    #[test]
    fn remove_alarm_unknown_id_is_noop() {
        reset();
        add_alarm(5, "", "red");
        remove_alarm("nope");
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
    }

    #[test]
    fn alarms_have_distinct_valid_ids_and_keep_insertion_order() {
        reset();
        for name in ["a", "b", "c", "d", "e"] {
            add_alarm(1, name, "red");
        }
        let alarms = get_alarms();
        let names: Vec<&str> = alarms.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c", "d", "e"]);
        let mut ids: Vec<&str> = alarms.iter().map(|a| a.id.as_str()).collect();
        assert!(ids.iter().all(|id| is_valid_alarm_id(id)));
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 5);
    }

    // ── Two players' documents ─────────────────────────────────────

    /// A second player's room document.
    fn peer_doc() -> Doc {
        let doc = Doc::new();
        {
            let mut txn = doc.transact_mut();
            txn.get_or_insert_map(TIMERS);
        }
        doc
    }

    /// Bring `to` up to date with everything `from` has (one direction of the
    /// state-vector handshake the players run on every reconnect).
    fn sync(from: &Doc, to: &Doc) {
        let sv = to.transact().state_vector();
        let diff = from.transact().encode_diff_v1(&sv);
        to.transact_mut()
            .apply_update(Update::decode_v1(&diff).unwrap())
            .unwrap();
    }

    fn sync_both(a: &Doc, b: &Doc) {
        sync(a, b);
        sync(b, a);
    }

    fn names(doc: &Doc) -> Vec<String> {
        alarms_in(doc).into_iter().map(|a| a.name).collect()
    }

    #[test]
    fn the_same_timer_seeded_by_both_players_appears_once() {
        // After an earlier session both players kept a copy of each shared
        // timer, with the same ids. Each seeds its copy into a new room.
        let (a, b) = (peer_doc(), peer_doc());
        for doc in [&a, &b] {
            add_timer_in(doc, "t1", 3, "alpha", "red");
            add_timer_in(doc, "t2", 5, "beta", "blue");
        }
        sync_both(&a, &b);
        assert_eq!(names(&a), ["alpha", "beta"]);
        assert_eq!(names(&b), ["alpha", "beta"]);
    }

    #[test]
    fn different_timers_from_each_player_merge_to_the_union() {
        let (a, b) = (peer_doc(), peer_doc());
        add_timer_in(&a, "a1", 3, "from a", "red");
        add_timer_in(&b, "b1", 5, "from b", "blue");
        sync_both(&a, &b);
        let mut got = names(&a);
        assert_eq!(got, names(&b), "both players show the same order");
        got.sort();
        assert_eq!(got, ["from a", "from b"]);
    }

    #[test]
    fn remove_targets_the_timer_even_if_the_peer_adds_concurrently() {
        let (a, b) = (peer_doc(), peer_doc());
        add_timer_in(&a, "t1", 3, "keep", "red");
        add_timer_in(&a, "t2", 5, "remove me", "blue");
        sync_both(&a, &b);

        // At the same moment: A removes t2, B adds a new timer.
        remove_timer_in(&a, "t2");
        add_timer_in(&b, "t3", 7, "new", "green");
        sync_both(&a, &b);

        let mut got = names(&a);
        got.sort();
        assert_eq!(got, ["keep", "new"]);
        let mut got_b = names(&b);
        got_b.sort();
        assert_eq!(got_b, ["keep", "new"]);
    }

    #[test]
    fn a_missed_update_is_repaired_by_the_handshake() {
        let (a, b) = (peer_doc(), peer_doc());
        add_timer_in(&a, "t1", 3, "alpha", "red");
        sync_both(&a, &b);

        // B is away: A's tick is never delivered as a live update.
        tick_timers_in(&a);
        assert_eq!(alarms_in(&b)[0].remaining, 3);

        // B comes back and the two exchange state vectors.
        sync_both(&a, &b);
        assert_eq!(alarms_in(&b)[0].remaining, 2);
    }

    #[test]
    fn malformed_entries_from_a_peer_are_ignored() {
        let doc = peer_doc();
        add_timer_in(&doc, "good", 3, "ok", "red");
        {
            let timers = doc.get_or_insert_map(TIMERS);
            let mut txn = doc.transact_mut();
            timers.insert(&mut txn, "x');alert(1);('", Any::from("{\"remaining\":1}".to_string()));
            timers.insert(&mut txn, "notjson", Any::from("oops".to_string()));
            timers.insert(&mut txn, "notstring", Any::from(5.0));
        }
        assert_eq!(names(&doc), ["ok"]);
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

        // Peer B starts empty and asks for everything it is missing.
        let doc_b = peer_doc();
        let sv_b = doc_b.transact().state_vector().encode_v1();
        let sv_b_b64 = URL_SAFE_NO_PAD.encode(&sv_b);

        let diff_for_b = encode_diff(&sv_b_b64).unwrap();
        let diff_bytes = URL_SAFE_NO_PAD.decode(&diff_for_b).unwrap();
        let update = Update::decode_v1(&diff_bytes).unwrap();
        doc_b.transact_mut().apply_update(update).unwrap();

        let alarms_b = alarms_in(&doc_b);
        assert_eq!(alarms_b.len(), 1);
        assert_eq!(alarms_b[0].name, "peer_a_timer");
        assert_eq!(alarms_b[0].remaining, 5);
        assert_eq!(alarms_b[0].color_set, "green");
    }

    #[test]
    fn apply_update_from_remote() {
        reset();

        let remote_doc = peer_doc();
        let sv_before = remote_doc.transact().state_vector();
        add_timer_in(&remote_doc, "r1", 7, "remote_timer", "pink");
        let update_bytes = remote_doc.transact().encode_diff_v1(&sv_before);
        let update_b64 = URL_SAFE_NO_PAD.encode(&update_bytes);

        let result = apply_update(&update_b64);
        assert!(result.is_ok());

        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].id, "r1");
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

        let doc_b = peer_doc();
        let sv_b_before = doc_b.transact().state_vector();
        add_timer_in(&doc_b, "b1", 3, "from_b", "blue");
        let update_b_bytes = doc_b.transact().encode_diff_v1(&sv_b_before);
        let update_b = URL_SAFE_NO_PAD.encode(&update_b_bytes);

        apply_update(&update_b).unwrap();

        let update_a_bytes = URL_SAFE_NO_PAD.decode(&update_a).unwrap();
        doc_b
            .transact_mut()
            .apply_update(Update::decode_v1(&update_a_bytes).unwrap())
            .unwrap();

        assert_eq!(get_alarms().len(), 2);
        assert_eq!(alarms_in(&doc_b).len(), 2);
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
    fn seed_from_local_copies_alarms_and_clears_local() {
        // Set up local PLAYER_DOC with alarms
        player_doc::init_player_doc();
        player_doc::add_alarm(5, "local timer", "green");
        player_doc::add_alarm(2, "another", "blue");

        // Fresh CRDT Doc
        reset();
        assert!(get_alarms().is_empty());

        seed_from_local();
        // CRDT Doc now has the alarms
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].name, "local timer");
        assert_eq!(alarms[0].remaining, 5);
        assert_eq!(alarms[0].color_set, "green");
        assert_eq!(alarms[1].name, "another");

        // Local PLAYER_DOC alarms should be cleared after seeding
        let local_alarms = player_doc::get_alarms();
        assert!(local_alarms.is_empty());

        player_doc::init_player_doc();
    }

    #[test]
    fn seed_from_local_adds_to_timers_already_in_the_room() {
        player_doc::init_player_doc();
        player_doc::add_alarm(5, "local", "red");

        reset();
        add_alarm(3, "existing", "pink");

        seed_from_local();
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].name, "existing");
        assert_eq!(alarms[1].name, "local");

        player_doc::init_player_doc();
    }

    #[test]
    fn seed_from_local_keeps_ids_and_does_not_duplicate_a_timer_already_shared() {
        player_doc::init_player_doc();
        player_doc::add_alarm_with_id("t1", 5, "local copy", "red");

        // The room already holds this timer under the same id.
        reset();
        add_alarm_with_id("t1", 2, "room copy", "blue");

        seed_from_local();
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].id, "t1");
        assert_eq!(alarms[0].name, "room copy");
        assert!(player_doc::get_alarms().is_empty());

        player_doc::init_player_doc();
    }

    #[test]
    fn a_second_session_between_the_same_players_does_not_double_timers() {
        // Session 1: one player creates two shared timers; the session ends
        // and each player keeps a copy (export_to_local on disconnect).
        player_doc::init_player_doc();
        reset();
        add_alarm(3, "alpha", "red");
        add_alarm(5, "beta", "blue");
        let session_one = get_alarms();
        export_to_local();
        reset_doc();

        // Session 2: this player seeds its copy into a fresh room doc...
        seed_from_local();
        // ...and so does the other player, whose copy has the same ids.
        let other = peer_doc();
        for (i, t) in session_one.iter().enumerate() {
            add_timer_in(&other, &t.id, t.remaining, &t.name, &t.color_set);
            assert_eq!(alarms_in(&other)[i].id, t.id);
        }

        // The two documents sync both ways, as on peer_joined.
        CRDT_DOC.with(|cell| sync_both(&cell.borrow(), &other));

        let merged: Vec<String> = get_alarms().into_iter().map(|a| a.name).collect();
        assert_eq!(merged, ["alpha", "beta"]);
        assert_eq!(names(&other), ["alpha", "beta"]);

        player_doc::init_player_doc();
    }

    #[test]
    fn timers_saved_before_ids_existed_get_one_when_shared() {
        player_doc::init_player_doc();
        player_doc::add_alarm_with_id("", 4, "legacy", "green");
        assert_eq!(player_doc::get_alarms()[0].id, "");

        reset();
        seed_from_local();
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].name, "legacy");
        assert!(is_valid_alarm_id(&alarms[0].id));

        player_doc::init_player_doc();
    }

    #[test]
    fn export_to_local_keeps_ids() {
        player_doc::init_player_doc();
        reset();
        add_alarm_with_id("keep-me", 7, "shared timer", "yellow");
        export_to_local();
        let local = player_doc::get_alarms();
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].id, "keep-me");
        player_doc::init_player_doc();
    }

    #[test]
    fn seed_from_local_skips_when_no_local_alarms() {
        player_doc::init_player_doc();

        reset();
        seed_from_local();
        assert!(get_alarms().is_empty());
    }

    #[test]
    fn export_to_local_copies_crdt_to_player_doc() {
        player_doc::init_player_doc();

        reset();
        add_alarm(7, "shared timer", "yellow");
        add_alarm(1, "quick one", "pink");

        export_to_local();
        let alarms = player_doc::get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].name, "shared timer");
        assert_eq!(alarms[0].remaining, 7);
        assert_eq!(alarms[0].color_set, "yellow");
        assert_eq!(alarms[1].name, "quick one");

        player_doc::init_player_doc();
    }

    #[test]
    fn export_to_local_replaces_existing_alarms() {
        player_doc::init_player_doc();
        player_doc::add_alarm(99, "old local", "red");

        reset();
        add_alarm(3, "from crdt", "blue");

        export_to_local();
        let alarms = player_doc::get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].name, "from crdt");

        player_doc::init_player_doc();
    }

    #[test]
    fn seed_then_disconnect_no_mutations_roundtrips_alarms() {
        // Simulate: local alarms → seed into CRDT → disconnect (no mutations).
        // Disconnect calls export_to_local() before reset, so PLAYER_DOC
        // gets the CRDT alarms back. The worker's PERSIST_STATE message
        // ensures this is saved to localStorage, preventing ghost timers.
        player_doc::init_player_doc();
        player_doc::add_alarm(5, "round trip", "green");
        player_doc::add_alarm(2, "also round trip", "pink");

        // Room create: seed local → CRDT (clears local)
        reset();
        seed_from_local();
        assert_eq!(get_alarms().len(), 2);
        assert!(player_doc::get_alarms().is_empty());

        // Disconnect: export CRDT → local, then reset CRDT
        export_to_local();
        reset_doc();

        // CRDT is empty after reset
        assert!(get_alarms().is_empty());
        // Local PLAYER_DOC has the alarms back from the roundtrip
        let local = player_doc::get_alarms();
        assert_eq!(local.len(), 2);
        assert_eq!(local[0].name, "round trip");
        assert_eq!(local[1].name, "also round trip");

        player_doc::init_player_doc();
    }

    #[test]
    fn seed_then_mutate_then_disconnect_preserves_mutations() {
        // Simulate: local alarms → seed → mutation (exports to local) → disconnect
        // The mutation route's export_to_local() keeps PLAYER_DOC in sync.
        player_doc::init_player_doc();
        player_doc::add_alarm(5, "original", "green");

        // Room create: seed local → CRDT (clears local)
        reset();
        seed_from_local();
        assert_eq!(get_alarms().len(), 1);
        assert!(player_doc::get_alarms().is_empty());

        // Simulate a CRDT mutation during the session — the mutation route
        // calls export_to_local() after each mutation to keep PLAYER_DOC in sync.
        add_alarm(1, "shared new", "blue");
        export_to_local(); // This is what the mutation routes do
        assert_eq!(get_alarms().len(), 2);
        assert_eq!(player_doc::get_alarms().len(), 2);

        // Disconnect: just reset, no export_to_local()
        reset_doc();

        // CRDT is empty after reset
        assert!(get_alarms().is_empty());
        // Local PLAYER_DOC has the state from the last mutation export
        let local = player_doc::get_alarms();
        assert_eq!(local.len(), 2);
        assert_eq!(local[0].name, "original");
        assert_eq!(local[1].name, "shared new");

        player_doc::init_player_doc();
    }
}
