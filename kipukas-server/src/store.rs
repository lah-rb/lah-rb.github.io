//! What the engine keeps in the browser's storage.
//!
//! The engine has nowhere of its own to keep anything: every page load
//! starts it empty. What must outlive a page is kept in the browser's
//! `localStorage`, which only the page can reach. So the two share the work
//! like this, and the page decides nothing:
//!
//! - When it loads, the page hands the engine everything stored under the
//!   engine's keys, in one request ([`start`]). The engine puts itself back
//!   as it was: the player's own state, and the room they were in.
//! - After every request the engine says which keys to write and which to
//!   remove ([`take_saves`], through the handover in `crate::page`). It works
//!   that out by comparing what it holds now with what storage holds, so no
//!   route has to remember to ask for a save.
//! - The page passes on what it sees happen to storage and to itself
//!   ([`page_event`]): another tab writing one of the keys, the page coming
//!   back into view.
//!
//! ## One tab at a time
//!
//! Every tab runs an engine of its own, started from storage when its page
//! loaded. When another tab writes to storage, this tab's engine is out of
//! date (`stale`). It asks to be loaded again the next time it is shown,
//! rather than carry on from what it had.
//!
//! The room's connection belongs to one tab: the relay gives each player one
//! place, so two tabs of the same player must not both connect. The tab that
//! last started in the room, or created or joined it, holds it, and says so
//! under [`ROOM_TAB`]. A tab that sees another take it goes quiet.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::clock;
use crate::game::{connection, crdt, player_doc, room};
use crate::page;

/// The player's own state (base64): damage, timers, affinity, loyalty.
pub const PLAYER_DOC: &str = "kipukas_player_doc";
/// Which room this player is in and who they are in it (JSON, [`Session`]).
pub const ROOM: &str = "kipukas_room";
/// The room's shared document (base64): timers and fists combat.
pub const ROOM_DOC: &str = "kipukas_room_doc";
/// The id of the tab that holds the room's connection.
pub const ROOM_TAB: &str = "kipukas_room_tab";

/// The keys the engine has stored, which the page hands over at load and
/// reports changes of.
pub const KEYS: [&str; 4] = [PLAYER_DOC, ROOM, ROOM_DOC, ROOM_TAB];

/// A relay address to use in place of the engine's own, for local work. The
/// page hands it over at load with the rest; the engine never writes it.
pub const RELAY_URL: &str = "kipukas_relay_url";

/// A room not returned to for this long is forgotten.
const ROOM_TTL_MS: f64 = 12.0 * 60.0 * 60.0 * 1000.0;

/// The room as it is stored between page loads.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Session {
    v: u8,
    #[serde(default)]
    code: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    creator: bool,
    #[serde(default, rename = "playerId")]
    player_id: String,
    #[serde(default, rename = "peerId")]
    peer_id: String,
    /// When it was last saved, in milliseconds since 1970.
    #[serde(default)]
    ts: f64,
}

#[derive(Default)]
struct Store {
    /// What storage holds under each of the engine's keys, as far as this
    /// engine knows: what it was handed at load, then what it has asked to
    /// be written since.
    held: BTreeMap<&'static str, Option<String>>,
    /// This tab's id.
    tab: String,
    /// The tab [`ROOM_TAB`] names, as last seen.
    owner: Option<String>,
    /// Another tab has written to storage since this engine started.
    stale: bool,
    /// When the room was last entered or returned to.
    room_since: f64,
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::default());
}

/// Whether this tab holds the room (true when no tab has claimed it).
pub fn is_owner() -> bool {
    STORE.with(|store| {
        let store = store.borrow();
        store.owner.as_ref().is_none_or(|owner| *owner == store.tab)
    })
}

/// Whether another tab has written to storage since this engine started.
pub fn is_stale() -> bool {
    STORE.with(|store| store.borrow().stale)
}

/// This tab now holds the room, as of now. Called when the room is created,
/// joined or returned to; the next handover writes it down.
pub fn claim_room() {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        store.owner = Some(store.tab.clone());
        store.room_since = clock::now_ms();
    });
}

/// A new id for this tab: 16 hex characters.
fn new_tab_id() -> String {
    format!("{:016x}", fastrand::u64(..))
}

// ── At load ────────────────────────────────────────────────────────

/// POST /api/start — put the engine back as it was, from what is stored.
///
/// Body: a JSON object of the stored value of each of [`KEYS`] that has one,
/// and of [`RELAY_URL`].
///
/// Returns `ok`, or what could not be read (for the worker to log; the engine
/// carries on from nothing for that part).
pub fn handle_start_post(body: &str) -> String {
    let stored: BTreeMap<String, String> = serde_json::from_str(body).unwrap_or_default();
    let mut problems: Vec<String> = Vec::new();

    STORE.with(|store| {
        let mut store = store.borrow_mut();
        *store = Store::default();
        store.tab = new_tab_id();
        store.owner = stored.get(ROOM_TAB).cloned();
        for key in KEYS {
            store.held.insert(key, stored.get(key).cloned());
        }
    });

    // The player's own state.
    if let Some(state) = stored.get(PLAYER_DOC) {
        if let Err(problem) = player_doc::restore_from_state(state) {
            problems.push(format!("player state: {}", problem));
        }
    }
    // What is held now is what was stored, so nothing is written until
    // something changes. (For a first visit that is a new, empty state.)
    STORE.with(|store| {
        store.borrow_mut().held.insert(PLAYER_DOC, Some(player_doc::encode_full_state()));
    });

    // The room, if the player was in one and has been back within the day.
    let session = stored
        .get(ROOM)
        .and_then(|raw| serde_json::from_str::<Session>(raw).ok())
        .filter(|session| !session.code.is_empty())
        .filter(|session| session.ts > 0.0 && clock::now_ms() - session.ts <= ROOM_TTL_MS);
    match session {
        Some(session) => {
            let doc = stored.get(ROOM_DOC).map(String::as_str).unwrap_or("");
            if let Err(problem) = crate::routes::room::resume(
                &session.code,
                &session.name,
                &session.player_id,
                &session.peer_id,
                session.creator,
                doc,
            ) {
                problems.push(format!("room: {}", problem));
            }
            // Returning refreshes the room's age and makes this tab the one
            // that holds it: any other tab of this player goes quiet.
            claim_room();
        }
        // No room, or one left over from an earlier day: whatever is stored
        // of it goes, whichever tab it was.
        None => STORE.with(|store| store.borrow_mut().owner = None),
    }

    // The engine is back in its room, with the room's document. All that is
    // missing is the connection.
    if let Some(address) = stored.get(RELAY_URL) {
        connection::use_relay(address);
    }
    connection::resumed();

    if problems.is_empty() {
        "ok".to_string()
    } else {
        problems.join("; ")
    }
}

// ── After every request ────────────────────────────────────────────

/// What each key should hold now.
fn wanted() -> Vec<(&'static str, Option<String>)> {
    let mut wanted = vec![(PLAYER_DOC, Some(player_doc::encode_full_state()))];
    // The room's keys are the business of the tab that holds the room.
    if is_owner() {
        let session = room::with_room(|r| {
            (!r.room_code.is_empty()).then(|| Session {
                v: 2,
                code: r.room_code.clone(),
                name: r.room_name.clone(),
                creator: r.creator,
                player_id: r.player_id.clone(),
                peer_id: r.peer_id.clone(),
                ts: STORE.with(|store| store.borrow().room_since),
            })
        });
        match session {
            Some(session) => {
                let tab = STORE.with(|store| store.borrow().tab.clone());
                wanted.push((ROOM, serde_json::to_string(&session).ok()));
                wanted.push((ROOM_DOC, Some(crdt::encode_full_state())));
                wanted.push((ROOM_TAB, Some(tab)));
            }
            None => {
                wanted.push((ROOM, None));
                wanted.push((ROOM_DOC, None));
                wanted.push((ROOM_TAB, None));
            }
        }
    }
    wanted
}

/// The keys to write (`Some`) and to remove (`None`) so that storage holds
/// what the engine holds now. They are taken once: what is returned counts as
/// stored from here on.
pub fn take_saves() -> BTreeMap<&'static str, Option<String>> {
    let wanted = wanted();
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut saves = BTreeMap::new();
        for (key, value) in wanted {
            if store.held.get(key).cloned().flatten() != value {
                store.held.insert(key, value.clone());
                saves.insert(key, value);
            }
        }
        saves
    })
}

// ── What the page reports ──────────────────────────────────────────

/// What the page reports of itself and of storage.
#[derive(Debug, Deserialize)]
#[serde(tag = "event", rename_all = "lowercase")]
enum PageEvent {
    /// The page came back into view.
    Shown,
    /// The browser says the network is back.
    Online,
    /// The page was brought back whole from the browser's back/forward cache,
    /// where it saw nothing of what other tabs stored. `stored` is what
    /// storage holds now, as at load.
    Restored {
        #[serde(default)]
        stored: BTreeMap<String, String>,
    },
    /// Another tab wrote or removed `key`; `value` is what it holds now.
    Storage {
        key: String,
        #[serde(default)]
        value: Option<String>,
    },
}

/// What a report from the page leaves the room's connection to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Follows {
    Nothing,
    /// The page is back in view, or the network is, and this engine is
    /// current: a good moment to check the connection.
    CameBack,
    /// Another tab took the room: this one lets go of the connection.
    RoomMoved,
}

/// Take a report from the page (the body of POST /api/page, JSON).
pub fn page_event(body: &str) -> Follows {
    let Ok(event) = serde_json::from_str::<PageEvent>(body) else {
        return Follows::Nothing;
    };
    match event {
        PageEvent::Online => Follows::CameBack,
        PageEvent::Shown => {
            if is_stale() {
                page::reload();
                Follows::Nothing
            } else {
                Follows::CameBack
            }
        }
        PageEvent::Restored { stored } => {
            let changed = STORE.with(|store| {
                let store = store.borrow();
                KEYS.iter().any(|key| store.held.get(key).cloned().flatten() != stored.get(*key).cloned())
            });
            if changed || is_stale() {
                page::reload();
                Follows::Nothing
            } else {
                Follows::CameBack
            }
        }
        PageEvent::Storage { key, value } => {
            if !KEYS.contains(&key.as_str()) {
                return Follows::Nothing;
            }
            if key == ROOM_TAB {
                // Two tabs loading at once both write the key; the page
                // reports what it holds now, which is the write that counts.
                let was_owner = is_owner();
                STORE.with(|store| store.borrow_mut().owner = value);
                if is_owner() {
                    return Follows::Nothing;
                }
                STORE.with(|store| store.borrow_mut().stale = true);
                return if was_owner && room::in_room() { Follows::RoomMoved } else { Follows::Nothing };
            }
            STORE.with(|store| store.borrow_mut().stale = true);
            Follows::Nothing
        }
    }
}

/// Forget everything about storage, as before any page has loaded.
#[cfg(test)]
pub fn reset() {
    STORE.with(|store| *store.borrow_mut() = Store::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::fists;

    const ME: &str = "aaaaaaaaaaaaaaaaaaaaaaaa";
    const THEM: &str = "bbbbbbbbbbbbbbbbbbbbbbbb";

    fn fresh() {
        player_doc::init_player_doc();
        room::reset_room();
        crdt::reset_doc();
        crdt::take_outbox();
        page::reset();
        connection::reset();
        reset();
    }

    fn start(stored: &[(&str, &str)]) -> String {
        let stored: BTreeMap<&str, &str> = stored.iter().copied().collect();
        handle_start_post(&serde_json::to_string(&stored).unwrap())
    }

    fn session(code: &str, age_ms: f64) -> String {
        serde_json::json!({
            "v": 2, "code": code, "name": "Table", "creator": true,
            "playerId": ME, "peerId": THEM, "ts": clock::now_ms() - age_ms,
        })
        .to_string()
    }

    fn report(event: serde_json::Value) -> Follows {
        page_event(&event.to_string())
    }

    #[test]
    fn a_first_visit_starts_empty_and_writes_nothing() {
        fresh();
        assert_eq!(start(&[]), "ok");
        assert!(take_saves().is_empty());
        assert!(!room::in_room());
        fresh();
    }

    #[test]
    fn the_players_state_comes_back_and_is_not_written_again_until_it_changes() {
        fresh();
        player_doc::add_alarm(3, "mine", "red");
        let state = player_doc::encode_full_state();
        fresh();

        assert_eq!(start(&[(PLAYER_DOC, &state)]), "ok");
        assert_eq!(player_doc::get_alarms().len(), 1);
        assert!(take_saves().is_empty(), "nothing changed, nothing to write");

        player_doc::add_alarm(5, "another", "blue");
        let saves = take_saves();
        assert_eq!(saves.keys().collect::<Vec<_>>(), [&PLAYER_DOC]);
        assert_eq!(saves[PLAYER_DOC], Some(player_doc::encode_full_state()));
        assert!(take_saves().is_empty(), "taken once");
        fresh();
    }

    #[test]
    fn stored_state_that_cannot_be_read_is_reported_and_the_engine_carries_on() {
        fresh();
        let reply = start(&[(PLAYER_DOC, "not a document")]);
        assert!(reply.contains("player state"), "{}", reply);
        assert!(player_doc::get_alarms().is_empty());
        fresh();
    }

    #[test]
    fn a_room_comes_back_with_its_document_and_this_tab_takes_it() {
        fresh();
        crdt::init_doc();
        crdt::add_alarm(4, "shared", "blue");
        let doc = crdt::encode_full_state();
        fresh();

        assert_eq!(start(&[(ROOM, &session("abcd", 60_000.0)), (ROOM_DOC, &doc), (ROOM_TAB, "another-tab")]), "ok");
        assert!(room::in_room());
        room::with_room(|r| {
            assert_eq!(r.room_code, "ABCD");
            assert_eq!(r.room_name, "Table");
            assert_eq!(r.player_id, ME);
            assert_eq!(r.peer_id, THEM);
            assert!(r.creator);
            assert!(!r.connected, "the opponent is away until the relay says otherwise");
        });
        assert_eq!(crdt::get_alarms().len(), 1);
        assert!(is_owner());

        // The session is written with a new age, and the tab's claim.
        let saves = take_saves();
        let written: Session = serde_json::from_str(saves[ROOM].as_ref().unwrap()).unwrap();
        assert_eq!(written.code, "ABCD");
        assert_eq!(written.v, 2);
        assert!(clock::now_ms() - written.ts < 5_000.0);
        let tab = saves[ROOM_TAB].clone().unwrap();
        assert_eq!(tab.len(), 16);
        assert_ne!(tab, "another-tab");
        // The document is as it was stored, so it is not written again.
        assert!(!saves.contains_key(ROOM_DOC));
        fresh();
    }

    #[test]
    fn a_room_from_an_earlier_day_is_forgotten_with_everything_stored_of_it() {
        fresh();
        let old = session("ABCD", ROOM_TTL_MS + 60_000.0);
        assert_eq!(start(&[(ROOM, &old), (ROOM_DOC, "AAA"), (ROOM_TAB, "another-tab")]), "ok");
        assert!(!room::in_room());
        let saves = take_saves();
        assert_eq!(saves.get(ROOM), Some(&None));
        assert_eq!(saves.get(ROOM_DOC), Some(&None));
        assert_eq!(saves.get(ROOM_TAB), Some(&None));
        fresh();
    }

    #[test]
    fn a_session_that_cannot_be_read_or_has_no_code_is_forgotten() {
        for stored in ["{not json", r#"{"v":2,"name":"Table","ts":1}"#] {
            fresh();
            start(&[(ROOM, stored)]);
            assert!(!room::in_room(), "{}", stored);
            assert_eq!(take_saves().get(ROOM), Some(&None), "{}", stored);
        }
        fresh();
    }

    #[test]
    fn a_session_saved_before_players_had_ids_is_given_one() {
        fresh();
        let stored = serde_json::json!({ "v": 1, "code": "ABCD", "ts": clock::now_ms() }).to_string();
        start(&[(ROOM, &stored)]);
        let id = room::with_room(|r| r.player_id.clone());
        assert!(fists::is_valid_player_id(&id), "{}", id);
        fresh();
    }

    #[test]
    fn the_rooms_document_is_written_when_it_changes_and_everything_goes_on_leaving() {
        fresh();
        start(&[]);
        crate::routes::room::enter("ABCD", "Table", ME, true);
        let saves = take_saves();
        assert!(saves[ROOM].is_some());
        assert!(saves[ROOM_DOC].is_some());
        assert!(saves[ROOM_TAB].is_some());
        assert!(take_saves().is_empty());

        crdt::add_alarm(2, "shared", "red");
        let saves = take_saves();
        assert!(saves.contains_key(ROOM_DOC));
        // The room's timers are mirrored to the player's own, hence both.
        assert!(!saves.contains_key(ROOM));

        crate::routes::room::leave();
        let saves = take_saves();
        assert_eq!(saves.get(ROOM), Some(&None));
        assert_eq!(saves.get(ROOM_DOC), Some(&None));
        assert_eq!(saves.get(ROOM_TAB), Some(&None));
        fresh();
    }

    #[test]
    fn another_tab_writing_makes_this_one_stale_and_it_reloads_when_next_shown() {
        fresh();
        start(&[]);
        assert_eq!(report(serde_json::json!({ "event": "shown" })), Follows::CameBack);
        assert!(!page::take_reload());

        report(serde_json::json!({ "event": "storage", "key": PLAYER_DOC, "value": "AAA" }));
        assert!(is_stale());
        assert!(!page::take_reload(), "not while it is out of view");
        assert_eq!(report(serde_json::json!({ "event": "shown" })), Follows::Nothing);
        assert!(page::take_reload());
        fresh();
    }

    #[test]
    fn a_key_that_is_not_the_engines_is_none_of_its_business() {
        fresh();
        start(&[]);
        report(serde_json::json!({ "event": "storage", "key": "something_else", "value": "x" }));
        assert!(!is_stale());
        fresh();
    }

    #[test]
    fn another_tab_taking_the_room_quiets_this_one_and_it_writes_no_more_of_the_room() {
        fresh();
        start(&[(ROOM, &session("ABCD", 0.0))]);
        take_saves();
        assert!(is_owner());

        let follows = report(serde_json::json!({ "event": "storage", "key": ROOM_TAB, "value": "another-tab" }));
        assert_eq!(follows, Follows::RoomMoved);
        assert!(!is_owner());
        assert!(is_stale());

        // The other tab's document is the current one.
        crdt::add_alarm(2, "late", "red");
        let saves = take_saves();
        assert!(!saves.contains_key(ROOM_DOC));
        assert!(!saves.contains_key(ROOM));
        assert!(!saves.contains_key(ROOM_TAB));
        fresh();
    }

    #[test]
    fn the_tab_key_naming_this_tab_again_or_nobody_changes_nothing() {
        fresh();
        start(&[(ROOM, &session("ABCD", 0.0))]);
        let tab = take_saves()[ROOM_TAB].clone().unwrap();
        assert_eq!(report(serde_json::json!({ "event": "storage", "key": ROOM_TAB, "value": tab })), Follows::Nothing);
        assert_eq!(report(serde_json::json!({ "event": "storage", "key": ROOM_TAB, "value": null })), Follows::Nothing);
        assert!(is_owner());
        assert!(!is_stale());
        fresh();
    }

    #[test]
    fn a_page_brought_back_from_the_cache_reloads_if_storage_moved_on_without_it() {
        fresh();
        player_doc::add_alarm(3, "mine", "red");
        let state = player_doc::encode_full_state();
        fresh();
        start(&[(PLAYER_DOC, &state)]);

        // Nothing changed while it was away.
        let same = serde_json::json!({ "event": "restored", "stored": { PLAYER_DOC: state } });
        assert_eq!(report(same), Follows::CameBack);
        assert!(!page::take_reload());

        // Another tab changed the player's state meanwhile.
        let moved = serde_json::json!({ "event": "restored", "stored": { PLAYER_DOC: "AAA" } });
        assert_eq!(report(moved), Follows::Nothing);
        assert!(page::take_reload());
        fresh();
    }

    #[test]
    fn a_report_that_cannot_be_read_is_ignored() {
        fresh();
        start(&[]);
        assert_eq!(page_event("not json"), Follows::Nothing);
        assert_eq!(page_event(r#"{"event":"unheard-of"}"#), Follows::Nothing);
        fresh();
    }
}
