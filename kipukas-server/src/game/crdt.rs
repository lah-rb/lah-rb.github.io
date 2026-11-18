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
//! └── "timers" (MapRef) — shared turn timers
//!     ├── "<timer id>:<player id>" → JSON string { "remaining": i32,
//!     │       "name": String, "color_set": String, "seq": u32,
//!     │       "at": u64, "gone": bool }
//!     └── ...
//! ```
//!
//! ## The latest change to a timer wins
//!
//! The two players are often apart: a page change, a locked phone, one of
//! them leaving the room and joining it again. Each can change a timer
//! meanwhile, and when they meet again there are two versions of it. The
//! one changed last is the one both keep.
//!
//! A yrs map cannot decide that by itself. Two values written to one key by
//! players who had not seen each other's are settled by comparing the ids
//! of their documents, which are random: either may win, and the loser is
//! gone. (A value written knowing of the other does always replace it. That
//! covers a phone that was offline and came back, but not a player who left
//! and joined again, whose timers are written into a new document.)
//!
//! So no key is ever written by two players:
//!
//! - **Each player writes a timer under a key of their own**, the timer's
//!   id and theirs. Both versions survive a merge.
//! - **Each version is dated** (`at`, milliseconds since 1970). The date is
//!   the clock's, or just after the latest version the writer could see if
//!   the clock says no later (`state::dated`). So a change made knowing of
//!   another is always the later one, whatever the two clocks say; changes
//!   made apart are ordered by the clocks, and two with the same date by
//!   the players' ids.
//! - **The timer is its latest version.** A reader takes, for each timer id,
//!   the version with the latest date. Both players see the same versions
//!   after a merge, so both take the same one.
//! - **A removal is a version too** (`gone`), so that a timer one player
//!   removed is not brought back by the other's older version of it. It
//!   stays in the document for the life of the room.
//! - **The date travels with the timer** when it leaves the room as a
//!   player's own (`Alarm::changed`) and when it is brought into a room
//!   again, and so does the removal of a timer while out of a room
//!   (`player_doc::retired`).
//!
//! Whoever writes a version removes the older ones it replaces, so a timer
//! is usually one entry.
//!
//! A key without a player id is a timer written before this (by anyone, at
//! no date): it is read as the oldest version.
//!
//! Timers are keyed by id rather than held in a list so that the same timer
//! brought into a room by both players (after an earlier session each keeps
//! a copy) is one timer, and so that "remove" targets a timer, not a
//! position.
//!
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

use crate::game::fists::{self, FISTS};
use crate::game::player_doc;
use crate::game::room;
use crate::game::state::{dated, is_valid_alarm_id, new_alarm_id, Alarm};

/// Root map holding the shared timers.
const TIMERS: &str = "timers";

/// One player's version of a shared timer, as stored in the room document
/// (a JSON string under the timer's id and the player's).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct TimerEntry {
    #[serde(default)]
    remaining: i32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    color_set: String,
    /// Display order. Ties are broken by id.
    #[serde(default)]
    seq: u32,
    /// When this version was made, in milliseconds since 1970 (see
    /// `state::dated`). The latest version of a timer is the timer.
    #[serde(default)]
    at: u64,
    /// This version is the timer's removal.
    #[serde(default)]
    gone: bool,
}

/// One version of a timer in the document: whose it is, the key it is
/// under, and what it says.
struct Version {
    id: String,
    writer: String,
    key: String,
    entry: TimerEntry,
}

impl Version {
    /// What orders two versions of one timer: the later date, then the
    /// greater writer, so that both players order them alike.
    fn rank(&self) -> (u64, &str) {
        (self.entry.at, &self.writer)
    }
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

// ── Doc persistence (the room's document outlives a page: `crate::store`) ──

/// Encode the full Doc state as a URL-safe base64 string, to be stored.
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
// These take the document, the writer and the time as arguments so tests
// can run two documents, sync between them, and say what time it is. The
// public functions below apply them to the room doc.

/// Every version of every timer in the document.
fn versions<T: ReadTxn>(timers: &MapRef, txn: &T) -> Vec<Version> {
    let mut out = Vec::new();
    for (key, value) in timers.iter(txn) {
        // A key without a player id is a timer written before versions were
        // kept apart.
        let (id, writer) = key.split_once(':').unwrap_or((key, ""));
        // Keys and values come from the other player too: skip anything that
        // is not a well-formed timer rather than trusting it.
        if !is_valid_alarm_id(id) || !(writer.is_empty() || fists::is_valid_player_id(writer)) {
            continue;
        }
        if let yrs::Out::Any(Any::String(json)) = value {
            if let Ok(entry) = serde_json::from_str::<TimerEntry>(&json) {
                out.push(Version { id: id.to_string(), writer: writer.to_string(), key: key.to_string(), entry });
            }
        }
    }
    out
}

/// The latest version of the timer `id`, if the document has any.
fn latest<'a>(versions: &'a [Version], id: &str) -> Option<&'a Version> {
    versions.iter().filter(|v| v.id == id).max_by(|a, b| a.rank().cmp(&b.rank()))
}

/// All timers in display order: by `seq`, then by id. A timer is its latest
/// version; one whose latest version is its removal is not a timer.
fn read_timers<T: ReadTxn>(timers: &MapRef, txn: &T) -> Vec<(String, TimerEntry)> {
    let versions = versions(timers, txn);
    let mut out: Vec<(String, TimerEntry)> = Vec::new();
    for version in &versions {
        if out.iter().any(|(id, _)| *id == version.id) {
            continue;
        }
        if let Some(latest) = latest(&versions, &version.id) {
            if !latest.entry.gone {
                out.push((latest.id.clone(), latest.entry.clone()));
            }
        }
    }
    out.sort_by(|a, b| a.1.seq.cmp(&b.1.seq).then_with(|| a.0.cmp(&b.0)));
    out
}

/// Write `writer`'s version of the timer `id`, if it is later than every
/// version the document has, and remove the versions it replaces. Returns
/// whether it was written.
fn write_version(timers: &MapRef, txn: &mut TransactionMut, id: &str, writer: &str, entry: &TimerEntry) -> bool {
    let versions = versions(timers, txn);
    let mine = (entry.at, writer);
    if latest(&versions, id).is_some_and(|latest| latest.rank() >= mine) {
        return false;
    }
    let key = format!("{}:{}", id, writer);
    // Each of these is an older version, so removing it loses nothing. If
    // its writer makes a newer one meanwhile, that is another entry in the
    // document and is not removed by this.
    for old in versions.iter().filter(|v| v.id == id && v.key != key) {
        timers.remove(txn, &old.key);
    }
    let json = serde_json::to_string(entry).unwrap_or_else(|_| "{}".to_string());
    timers.insert(txn, key, Any::from(json));
    true
}

/// The date of a version of the timer `id` made at `now`: after every
/// version of it the document has.
fn date_for<T: ReadTxn>(timers: &MapRef, txn: &T, id: &str, now: f64) -> u64 {
    let versions = versions(timers, txn);
    let seen = latest(&versions, id).map_or(0, |latest| latest.entry.at);
    dated(now, seen as f64) as u64
}

/// The `seq` for a timer added now: one past the highest in use.
fn next_seq<T: ReadTxn>(timers: &MapRef, txn: &T) -> u32 {
    read_timers(timers, txn)
        .iter()
        .map(|(_, e)| e.seq)
        .max()
        .map_or(1, |m| m.saturating_add(1))
}

/// A timer's removal, as a version of it.
fn removal(at: u64) -> TimerEntry {
    TimerEntry { remaining: 0, name: String::new(), color_set: String::new(), seq: 0, at, gone: true }
}

/// `me` makes a timer at `now`.
fn add_timer_in(doc: &Doc, me: &str, now: f64, id: &str, turns: i32, name: &str, color_set: &str) {
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
        at: date_for(&timers, &txn, id, now),
        gone: false,
    };
    write_version(&timers, &mut txn, id, me, &entry);
}

/// `me` brings a timer of their own into the room, as it was last changed
/// (`alarm.changed`). It becomes the room's timer of that id unless the
/// room has a later version of it.
fn bring_timer_in(doc: &Doc, me: &str, alarm: &Alarm) {
    let timers = doc.get_or_insert_map(TIMERS);
    let mut txn = doc.transact_mut();
    let entry = TimerEntry {
        remaining: alarm.remaining,
        name: alarm.name.clone(),
        color_set: validate_color_set(&alarm.color_set).to_string(),
        seq: next_seq(&timers, &txn),
        at: alarm.changed.max(0.0) as u64,
        gone: false,
    };
    write_version(&timers, &mut txn, &alarm.id, me, &entry);
}

/// `me` brings into the room that they removed the timer `id` at `at`,
/// while out of it.
fn bring_removal_in(doc: &Doc, me: &str, id: &str, at: f64) {
    if !is_valid_alarm_id(id) {
        return;
    }
    let timers = doc.get_or_insert_map(TIMERS);
    let mut txn = doc.transact_mut();
    write_version(&timers, &mut txn, id, me, &removal(at.max(0.0) as u64));
}

/// `me` advances every timer by 1 at `now`; those that were already at 0
/// are removed.
fn tick_timers_in(doc: &Doc, me: &str, now: f64) {
    let timers = doc.get_or_insert_map(TIMERS);
    let mut txn = doc.transact_mut();
    for (id, mut entry) in read_timers(&timers, &txn) {
        entry.at = date_for(&timers, &txn, &id, now);
        if entry.remaining <= 0 {
            entry = removal(entry.at);
        } else {
            entry.remaining -= 1;
        }
        write_version(&timers, &mut txn, &id, me, &entry);
    }
}

/// `me` removes the timer `id` at `now`.
fn remove_timer_in(doc: &Doc, me: &str, now: f64, id: &str) {
    let timers = doc.get_or_insert_map(TIMERS);
    let mut txn = doc.transact_mut();
    if !read_timers(&timers, &txn).iter().any(|(timer, _)| timer == id) {
        return;
    }
    let at = date_for(&timers, &txn, id, now);
    write_version(&timers, &mut txn, id, me, &removal(at));
}

/// The timers that were removed, each with when: those whose latest version
/// is their removal.
fn removals_in(doc: &Doc) -> Vec<(String, f64)> {
    let timers = doc.get_or_insert_map(TIMERS);
    let txn = doc.transact();
    let versions = versions(&timers, &txn);
    let mut out: Vec<(String, f64)> = Vec::new();
    for version in &versions {
        if out.iter().any(|(id, _)| *id == version.id) {
            continue;
        }
        if let Some(latest) = latest(&versions, &version.id) {
            if latest.entry.gone {
                out.push((latest.id.clone(), latest.entry.at as f64));
            }
        }
    }
    out
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
            changed: e.at as f64,
        })
        .collect()
}

// ── Updates for the other player ───────────────────────────────────
//
// A change this player makes to the room document has to reach the other
// player. A change made through an engine route is queued here, and before
// every handover the room's connection takes what is queued and sends it
// (`game::connection::send_updates`). One that cannot be sent (the other
// player is away, or this player's own connection is down) is made up for
// by the exchange both players do when they are next connected.

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

/// This player's id in the room: whose versions of the timers these are.
fn me() -> String {
    room::with_room(|r| r.player_id.clone())
}

/// Add a new alarm to the room doc under a fresh id, at `now`. Returns the
/// update as a base64 string.
pub fn add_alarm(now: f64, turns: i32, name: &str, color_set: &str) -> String {
    let me = me();
    mutate_room_doc(|doc| add_timer_in(doc, &me, now, &new_alarm_id(), turns, name, color_set))
}

/// Tick all alarms at `now`: decrement by 1, remove completed (were already
/// at 0). Returns the update as a base64 string.
pub fn tick_alarms(now: f64) -> String {
    let me = me();
    mutate_room_doc(|doc| tick_timers_in(doc, &me, now))
}

/// Remove an alarm by id, at `now`. Returns the update as a base64 string.
pub fn remove_alarm(now: f64, id: &str) -> String {
    let me = me();
    mutate_room_doc(|doc| remove_timer_in(doc, &me, now, id))
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

/// Bring this player's own timers into the room doc.
///
/// Called on room create and on room join, so both players' pre-existing
/// timers become shared. Each timer comes in under its own id and as it was
/// last changed. When both players hold a copy of the same timer (each kept
/// one after an earlier session, or one of them left this room and is
/// joining it again), the copy changed last is the room's timer; a timer
/// this player removed meanwhile comes in as removed.
pub fn seed_from_local() {
    let local_alarms: Vec<Alarm> = player_doc::get_alarms();
    let removed = player_doc::retired();
    if local_alarms.is_empty() && removed.is_empty() {
        return;
    }
    let me = me();
    CRDT_DOC.with(|cell| {
        let doc = cell.borrow();
        for alarm in &local_alarms {
            // Timers saved before ids existed get one now.
            let mut alarm = alarm.clone();
            if !is_valid_alarm_id(&alarm.id) {
                alarm.id = new_alarm_id();
            }
            bring_timer_in(&doc, &me, &alarm);
        }
        for (id, at) in &removed {
            bring_removal_in(&doc, &me, id, *at);
        }
    });
    // The timers now live in the room doc. The room routes call
    // export_to_local() after each change to keep PLAYER_DOC in step, and on
    // disconnect before the room doc is reset.
    player_doc::clear_alarms();
}

/// Copy the room's timers back into this player's own timers, with their
/// ids and when each was last changed, and the room's removals into what
/// the player remembers as removed. Called after each change and on
/// disconnect, so shared timers survive as the player's own once the
/// session ends, and a timer removed in the room stays removed in the next.
pub fn export_to_local() {
    let crdt_alarms = get_alarms();
    player_doc::clear_alarms();
    for alarm in &crdt_alarms {
        player_doc::add_alarm_with_id(&alarm.id, alarm.remaining, &alarm.name, &alarm.color_set, alarm.changed);
    }
    let removed = CRDT_DOC.with(|cell| removals_in(&cell.borrow()));
    player_doc::remember_removed(&removed);
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

    /// When the timers in these tests are made and changed, where the test
    /// does not care when.
    const NOW: f64 = 1_800_000_000_000.0;

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
        let _update = add_alarm(NOW, 5, "Scout patrol", "green");
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
    fn add_multiple_alarms() {
        reset();
        add_alarm(NOW, 5, "first", "red");
        add_alarm(NOW, 3, "second", "blue");
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].name, "first");
        assert_eq!(alarms[1].name, "second");
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
    fn remove_alarm_by_id() {
        reset();
        add_alarm(NOW, 5, "first", "red");
        add_alarm(NOW, 3, "second", "green");
        add_alarm(NOW, 1, "third", "blue");
        let second = get_alarms()[1].id.clone();
        remove_alarm(NOW, &second);
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].remaining, 5);
        assert_eq!(alarms[1].remaining, 1);
    }

    #[test]
    fn remove_alarm_unknown_id_is_noop() {
        reset();
        add_alarm(NOW, 5, "", "red");
        remove_alarm(NOW, "nope");
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
    }

    #[test]
    fn alarms_have_distinct_valid_ids_and_keep_insertion_order() {
        reset();
        for name in ["a", "b", "c", "d", "e"] {
            add_alarm(NOW, 1, name, "red");
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

    /// The two players' ids in the room.
    const A: &str = "player-a";
    const B: &str = "player-b";

    /// A player's room document.
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

    /// What is left of the first timer, or `None` if there is no timer.
    fn remaining(doc: &Doc) -> Option<i32> {
        alarms_in(doc).first().map(|alarm| alarm.remaining)
    }

    /// How many entries the document holds for timers, versions and all.
    fn entries(doc: &Doc) -> usize {
        let timers = doc.get_or_insert_map(TIMERS);
        let txn = doc.transact();
        timers.iter(&txn).count()
    }

    /// A timer as a player holds it outside a room.
    fn own(id: &str, remaining: i32, changed: f64) -> Alarm {
        Alarm { id: id.to_string(), remaining, name: "alpha".to_string(), color_set: "red".to_string(), changed }
    }

    /// Two players in a room with one timer at 3, made at NOW, in step.
    fn a_room_with_a_timer() -> (Doc, Doc) {
        let (a, b) = (peer_doc(), peer_doc());
        add_timer_in(&a, A, NOW, "t1", 3, "alpha", "red");
        sync_both(&a, &b);
        (a, b)
    }

    /// Which of two versions written apart a yrs map keeps depends on the
    /// random ids of the two documents. Anything said of such a case is
    /// checked over many pairs of documents.
    const PAIRS: usize = 60;

    #[test]
    fn the_same_timer_seeded_by_both_players_appears_once() {
        // After an earlier session both players kept a copy of each shared
        // timer, with the same ids. Each brings its copy into a new room.
        let (a, b) = (peer_doc(), peer_doc());
        for (doc, who) in [(&a, A), (&b, B)] {
            bring_timer_in(doc, who, &own("t1", 3, NOW));
            bring_timer_in(doc, who, &own("t2", 5, NOW));
        }
        sync_both(&a, &b);
        assert_eq!(alarms_in(&a).len(), 2);
        assert_eq!(alarms_in(&b).len(), 2);
    }

    #[test]
    fn different_timers_from_each_player_merge_to_the_union() {
        let (a, b) = (peer_doc(), peer_doc());
        add_timer_in(&a, A, NOW, "a1", 3, "from a", "red");
        add_timer_in(&b, B, NOW, "b1", 5, "from b", "blue");
        sync_both(&a, &b);
        let mut got = names(&a);
        assert_eq!(got, names(&b), "both players show the same order");
        got.sort();
        assert_eq!(got, ["from a", "from b"]);
    }

    #[test]
    fn remove_targets_the_timer_even_if_the_peer_adds_concurrently() {
        let (a, b) = (peer_doc(), peer_doc());
        add_timer_in(&a, A, NOW, "t1", 3, "keep", "red");
        add_timer_in(&a, A, NOW, "t2", 5, "remove me", "blue");
        sync_both(&a, &b);

        // At the same moment: A removes t2, B adds a new timer.
        remove_timer_in(&a, A, NOW + 1_000.0, "t2");
        add_timer_in(&b, B, NOW + 1_000.0, "t3", 7, "new", "green");
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
        let (a, b) = a_room_with_a_timer();

        // B is away: A's tick is never delivered as a live update.
        tick_timers_in(&a, A, NOW + 1_000.0);
        assert_eq!(remaining(&b), Some(3));

        // B comes back and the two exchange state vectors.
        sync_both(&a, &b);
        assert_eq!(remaining(&b), Some(2));
    }

    #[test]
    fn malformed_entries_from_a_peer_are_ignored() {
        let doc = peer_doc();
        add_timer_in(&doc, A, NOW, "good", 3, "ok", "red");
        {
            let timers = doc.get_or_insert_map(TIMERS);
            let mut txn = doc.transact_mut();
            timers.insert(&mut txn, "x');alert(1);('", Any::from("{\"remaining\":1}".to_string()));
            timers.insert(&mut txn, "bad:x');alert(1);('", Any::from("{\"remaining\":1}".to_string()));
            timers.insert(&mut txn, "notjson", Any::from("oops".to_string()));
            timers.insert(&mut txn, "notstring", Any::from(5.0));
        }
        assert_eq!(names(&doc), ["ok"]);
    }

    // ── The latest change wins ─────────────────────────────────────

    #[test]
    fn an_advance_made_while_offline_is_kept_when_the_player_comes_back() {
        for _ in 0..PAIRS {
            let (a, b) = a_room_with_a_timer();
            tick_timers_in(&a, A, NOW + 1_000.0);
            sync_both(&a, &b);
            assert_eq!((remaining(&a), remaining(&b)), (Some(2), Some(2)));
        }
    }

    #[test]
    fn an_advance_made_after_leaving_wins_when_the_player_joins_again() {
        // A leaves the room with its copy of the timer, advances it alone,
        // and joins again: a new document, into which the copy is brought.
        for _ in 0..PAIRS {
            let (_, b) = a_room_with_a_timer();
            let again = peer_doc();
            bring_timer_in(&again, "player-a-again", &own("t1", 2, NOW + 60_000.0));
            sync_both(&again, &b);
            assert_eq!((remaining(&again), remaining(&b)), (Some(2), Some(2)));
        }
    }

    #[test]
    fn a_copy_left_untouched_does_not_undo_an_advance_made_in_the_room_meanwhile() {
        // A leaves; B advances the room's timer; A joins again with the
        // copy it left with.
        for _ in 0..PAIRS {
            let (_, b) = a_room_with_a_timer();
            tick_timers_in(&b, B, NOW + 60_000.0);
            let again = peer_doc();
            bring_timer_in(&again, "player-a-again", &own("t1", 3, NOW));
            sync_both(&again, &b);
            assert_eq!((remaining(&again), remaining(&b)), (Some(2), Some(2)));
        }
    }

    #[test]
    fn of_two_advances_made_apart_the_later_is_kept_by_both() {
        for _ in 0..PAIRS {
            // A once, then B twice.
            let (a, b) = a_room_with_a_timer();
            tick_timers_in(&a, A, NOW + 1_000.0);
            tick_timers_in(&b, B, NOW + 2_000.0);
            tick_timers_in(&b, B, NOW + 3_000.0);
            sync_both(&a, &b);
            assert_eq!((remaining(&a), remaining(&b)), (Some(1), Some(1)));

            // B twice, then A once.
            let (a, b) = a_room_with_a_timer();
            tick_timers_in(&b, B, NOW + 1_000.0);
            tick_timers_in(&b, B, NOW + 2_000.0);
            tick_timers_in(&a, A, NOW + 3_000.0);
            sync_both(&a, &b);
            assert_eq!((remaining(&a), remaining(&b)), (Some(2), Some(2)));
        }
    }

    #[test]
    fn two_changes_dated_the_same_are_settled_the_same_way_by_both() {
        for _ in 0..PAIRS {
            let (a, b) = a_room_with_a_timer();
            tick_timers_in(&a, A, NOW + 1_000.0);
            remove_timer_in(&b, B, NOW + 1_000.0, "t1");
            sync_both(&a, &b);
            // The greater player id has it: B's removal.
            assert_eq!((remaining(&a), remaining(&b)), (None, None));
        }
    }

    #[test]
    fn a_change_made_knowing_of_another_is_the_later_one_whatever_the_clocks_say() {
        // A's clock is an hour ahead. B sees A's advance and then advances
        // too: B's is the later change, though B's clock says otherwise.
        let (a, b) = a_room_with_a_timer();
        tick_timers_in(&a, A, NOW + 3_600_000.0);
        sync_both(&a, &b);
        tick_timers_in(&b, B, NOW + 5_000.0);
        sync_both(&a, &b);
        assert_eq!((remaining(&a), remaining(&b)), (Some(1), Some(1)));

        // Two changes in the same millisecond by one player, likewise.
        let doc = peer_doc();
        add_timer_in(&doc, A, NOW, "t1", 3, "alpha", "red");
        tick_timers_in(&doc, A, NOW);
        tick_timers_in(&doc, A, NOW);
        assert_eq!(remaining(&doc), Some(1));
    }

    #[test]
    fn a_timer_removed_in_the_room_is_not_brought_back_by_an_older_copy() {
        for _ in 0..PAIRS {
            let (_, b) = a_room_with_a_timer();
            remove_timer_in(&b, B, NOW + 60_000.0, "t1");
            let again = peer_doc();
            bring_timer_in(&again, "player-a-again", &own("t1", 3, NOW));
            sync_both(&again, &b);
            assert_eq!((remaining(&again), remaining(&b)), (None, None));
        }
    }

    #[test]
    fn a_timer_that_ran_out_in_the_room_is_not_brought_back_either() {
        let (a, b) = (peer_doc(), peer_doc());
        add_timer_in(&a, A, NOW, "t1", 1, "alpha", "red");
        sync_both(&a, &b);
        // B advances it to nothing, then past: it goes.
        tick_timers_in(&b, B, NOW + 1_000.0);
        tick_timers_in(&b, B, NOW + 2_000.0);
        assert_eq!(remaining(&b), None);
        sync_both(&a, &b);
        assert_eq!(remaining(&a), None);
    }

    #[test]
    fn a_timer_removed_after_leaving_goes_from_the_room_when_the_player_joins_again() {
        for _ in 0..PAIRS {
            let (_, b) = a_room_with_a_timer();
            let again = peer_doc();
            bring_removal_in(&again, "player-a-again", "t1", NOW + 60_000.0);
            sync_both(&again, &b);
            assert_eq!((remaining(&again), remaining(&b)), (None, None));
        }
    }

    #[test]
    fn a_timer_changed_in_the_room_after_the_other_removed_theirs_stays() {
        // A leaves and removes its copy. Later still, B advances the
        // room's. A joins again: the advance is the later change.
        for _ in 0..PAIRS {
            let (_, b) = a_room_with_a_timer();
            tick_timers_in(&b, B, NOW + 120_000.0);
            let again = peer_doc();
            bring_removal_in(&again, "player-a-again", "t1", NOW + 60_000.0);
            sync_both(&again, &b);
            assert_eq!((remaining(&again), remaining(&b)), (Some(2), Some(2)));
        }
    }

    #[test]
    fn a_timer_is_one_entry_once_its_latest_version_is_seen_by_who_changes_it_next() {
        let (a, b) = a_room_with_a_timer();
        assert_eq!(entries(&a), 1);

        // Changed apart: both versions are in the document after the merge.
        tick_timers_in(&a, A, NOW + 1_000.0);
        tick_timers_in(&b, B, NOW + 2_000.0);
        sync_both(&a, &b);
        assert_eq!((entries(&a), entries(&b)), (2, 2));

        // The later of the two (B's, at 2) is the timer. The next change is
        // made to that, and replaces both.
        assert_eq!((remaining(&a), remaining(&b)), (Some(2), Some(2)));
        tick_timers_in(&a, A, NOW + 3_000.0);
        sync_both(&a, &b);
        assert_eq!((entries(&a), entries(&b)), (1, 1));
        assert_eq!((remaining(&a), remaining(&b)), (Some(1), Some(1)));

        // A removal stays, as the timer's one entry, for older copies to meet.
        tick_timers_in(&b, B, NOW + 4_000.0);
        tick_timers_in(&b, B, NOW + 5_000.0);
        sync_both(&a, &b);
        assert_eq!((entries(&a), entries(&b)), (1, 1));
        assert_eq!((remaining(&a), remaining(&b)), (None, None));
    }

    #[test]
    fn removing_an_older_version_does_not_remove_a_newer_one_its_writer_made_meanwhile() {
        for _ in 0..PAIRS {
            let (a, b) = a_room_with_a_timer();
            tick_timers_in(&b, B, NOW + 1_000.0);
            sync_both(&a, &b);
            // A replaces B's version (and removes it) while B, apart, makes a newer one.
            tick_timers_in(&a, A, NOW + 2_000.0);
            tick_timers_in(&b, B, NOW + 3_000.0);
            sync_both(&a, &b);
            // B's newest: 3 → 2 (B) → 1 (B again). A's 3 → 2 → 1 was earlier.
            assert_eq!((remaining(&a), remaining(&b)), (Some(1), Some(1)));
            let latest_is_bs = alarms_in(&a)[0].changed == NOW + 3_000.0;
            assert!(latest_is_bs);
        }
    }

    #[test]
    fn a_timer_written_before_versions_were_kept_apart_is_read_and_then_replaced() {
        let doc = peer_doc();
        {
            let timers = doc.get_or_insert_map(TIMERS);
            let mut txn = doc.transact_mut();
            let old = r#"{"remaining":4,"name":"from before","color_set":"blue","seq":1}"#;
            timers.insert(&mut txn, "t1", Any::from(old.to_string()));
        }
        let before = alarms_in(&doc);
        assert_eq!((before[0].name.as_str(), before[0].remaining, before[0].changed), ("from before", 4, 0.0));

        tick_timers_in(&doc, A, NOW);
        assert_eq!(remaining(&doc), Some(3));
        assert_eq!(entries(&doc), 1, "the old entry is replaced");
        let timers = doc.get_or_insert_map(TIMERS);
        let txn = doc.transact();
        assert!(timers.get(&txn, "t1:player-a").is_some());
    }

    #[test]
    fn add_returns_nonempty_update() {
        reset();
        let update = add_alarm(NOW, 5, "test", "red");
        assert!(!update.is_empty());
        let decoded = URL_SAFE_NO_PAD.decode(&update);
        assert!(decoded.is_ok());
    }

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
    fn sync_two_docs_converge() {
        reset();
        add_alarm(NOW, 5, "peer_a_timer", "green");

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
        add_timer_in(&remote_doc, B, NOW, "r1", 7, "remote_timer", "pink");
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
        add_alarm(NOW, 5, "test", "red");
        assert_eq!(get_alarms().len(), 1);
        reset_doc();
        assert_eq!(get_alarms().len(), 0);
    }

    #[test]
    fn concurrent_adds_both_survive() {
        reset();

        let update_a = add_alarm(NOW, 5, "from_a", "red");

        let doc_b = peer_doc();
        let sv_b_before = doc_b.transact().state_vector();
        add_timer_in(&doc_b, B, NOW, "b1", 3, "from_b", "blue");
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
        add_alarm(NOW, 5, "persisted", "green");
        add_alarm(NOW, 2, "also persisted", "pink");

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
        add_alarm(NOW, 1, "keep", "red");
        let result = restore_from_state("");
        assert!(result.is_ok());
        assert_eq!(get_alarms().len(), 1);
    }

    #[test]
    fn seed_from_local_copies_alarms_and_clears_local() {
        // Set up local PLAYER_DOC with alarms
        player_doc::init_player_doc();
        player_doc::add_alarm(NOW, 5, "local timer", "green");
        player_doc::add_alarm(NOW, 2, "another", "blue");

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
        player_doc::add_alarm(NOW, 5, "local", "red");

        reset();
        add_alarm(NOW, 3, "existing", "pink");

        seed_from_local();
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].name, "existing");
        assert_eq!(alarms[1].name, "local");

        player_doc::init_player_doc();
    }

    #[test]
    fn seed_from_local_keeps_ids_and_the_later_of_two_copies_of_a_timer() {
        // The room already holds this timer under the same id, changed
        // after the player's copy was.
        player_doc::init_player_doc();
        player_doc::add_alarm_with_id("t1", 5, "local copy", "red", NOW);
        reset();
        CRDT_DOC.with(|cell| bring_timer_in(&cell.borrow(), B, &Alarm {
            id: "t1".to_string(), remaining: 2, name: "room copy".to_string(), color_set: "blue".to_string(),
            changed: NOW + 1_000.0,
        }));

        seed_from_local();
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].id, "t1");
        assert_eq!(alarms[0].name, "room copy");
        assert!(player_doc::get_alarms().is_empty());

        // And the other way about: the player's copy was changed last.
        player_doc::init_player_doc();
        player_doc::add_alarm_with_id("t1", 5, "local copy", "red", NOW + 2_000.0);
        seed_from_local();
        let alarms = get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].name, "local copy");
        assert_eq!(alarms[0].changed, NOW + 2_000.0, "it keeps its date");

        player_doc::init_player_doc();
    }

    #[test]
    fn a_timer_removed_while_out_of_a_room_is_brought_in_as_removed() {
        // The player left a room with a timer, and it ran out and went.
        player_doc::init_player_doc();
        player_doc::add_alarm_with_id("t1", 0, "alpha", "red", NOW);
        player_doc::tick_alarms(NOW + 60_000.0);
        assert!(player_doc::get_alarms().is_empty());

        // The room they join again still has it, as the other player left it.
        reset();
        CRDT_DOC.with(|cell| bring_timer_in(&cell.borrow(), B, &Alarm {
            id: "t1".to_string(), remaining: 1, name: "alpha".to_string(), color_set: "red".to_string(),
            changed: NOW,
        }));
        assert_eq!(get_alarms().len(), 1);

        seed_from_local();
        assert!(get_alarms().is_empty(), "the removal is the later change");

        player_doc::init_player_doc();
    }

    #[test]
    fn a_second_session_between_the_same_players_does_not_double_timers() {
        // Session 1: one player creates two shared timers; the session ends
        // and each player keeps a copy (export_to_local on disconnect).
        player_doc::init_player_doc();
        reset();
        add_alarm(NOW, 3, "alpha", "red");
        add_alarm(NOW, 5, "beta", "blue");
        let session_one = get_alarms();
        export_to_local();
        reset_doc();

        // Session 2: this player seeds its copy into a fresh room doc...
        seed_from_local();
        // ...and so does the other player, whose copy has the same ids.
        let other = peer_doc();
        for (i, t) in session_one.iter().enumerate() {
            bring_timer_in(&other, B, t);
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
        player_doc::add_alarm_with_id("", 4, "legacy", "green", 0.0);
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
    fn a_timer_removed_in_the_room_stays_removed_in_the_next_room() {
        // B was cut off when A removed the timer, and left without hearing
        // of it: B still holds the timer. They meet in a new room.
        player_doc::init_player_doc();
        reset();
        CRDT_DOC.with(|cell| add_timer_in(&cell.borrow(), A, NOW, "shared", 7, "shared timer", "yellow"));
        let held_by_b = get_alarms().remove(0);
        remove_alarm(NOW + 5_000.0, "shared");
        export_to_local();
        assert!(player_doc::get_alarms().is_empty());
        assert_eq!(player_doc::retired(), [("shared".to_string(), NOW + 5_000.0)]);

        // A leaves and makes the new room; B joins it with the copy.
        reset();
        seed_from_local();
        CRDT_DOC.with(|cell| bring_timer_in(&cell.borrow(), B, &held_by_b));
        assert!(get_alarms().is_empty(), "the removal was the later change");
        player_doc::init_player_doc();
    }

    #[test]
    fn export_to_local_keeps_ids_and_when_each_timer_was_last_changed() {
        player_doc::init_player_doc();
        reset();
        CRDT_DOC.with(|cell| add_timer_in(&cell.borrow(), A, NOW, "keep-me", 7, "shared timer", "yellow"));
        tick_alarms(NOW + 5_000.0);
        export_to_local();
        let local = player_doc::get_alarms();
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].id, "keep-me");
        assert_eq!(local[0].remaining, 6);
        assert_eq!(local[0].changed, NOW + 5_000.0);
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
        add_alarm(NOW, 7, "shared timer", "yellow");
        add_alarm(NOW, 1, "quick one", "pink");

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
        player_doc::add_alarm(NOW, 99, "old local", "red");

        reset();
        add_alarm(NOW, 3, "from crdt", "blue");

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
        // gets the CRDT alarms back, and the next handover has it stored,
        // preventing ghost timers.
        player_doc::init_player_doc();
        player_doc::add_alarm(NOW, 5, "round trip", "green");
        player_doc::add_alarm(NOW, 2, "also round trip", "pink");

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
        player_doc::add_alarm(NOW, 5, "original", "green");

        // Room create: seed local → CRDT (clears local)
        reset();
        seed_from_local();
        assert_eq!(get_alarms().len(), 1);
        assert!(player_doc::get_alarms().is_empty());

        // Simulate a CRDT mutation during the session — the mutation route
        // calls export_to_local() after each mutation to keep PLAYER_DOC in sync.
        add_alarm(NOW, 1, "shared new", "blue");
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
