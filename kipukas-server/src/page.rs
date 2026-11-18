//! What the engine asks of the page.
//!
//! The engine only answers requests: it cannot reach the page. What it wants
//! done there is queued, and after every post the engine's worker takes what
//! is queued and hands it to the page (`handOver` in kipukas-worker.js):
//!
//! - updates of the room document for the other player, which the page sends
//!   because it holds the connection (`game::crdt::queue_for_peer`);
//! - views to draw again ([`redraw`]);
//! - what to write to the browser's storage and what to remove from it
//!   (`crate::store`);
//! - whether the player is in a room, which the toolbar shows different
//!   tools for;
//! - that the page is to be loaded again ([`reload`]).
//!
//! The page does each of these as it is told and decides none of them.

use std::cell::{Cell, RefCell};

use crate::game::{crdt, room};
use crate::store;

thread_local! {
    static REDRAW: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    static RELOAD: Cell<bool> = const { Cell::new(false) };
    static QUIET: Cell<bool> = const { Cell::new(false) };
    /// Whether the page was last told the player is in a room. `None` until
    /// it has been told anything.
    static IN_ROOM_TOLD: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Ask for the views of a route to be drawn again, whatever their query.
///
/// A tap in a view draws that view again by itself. This is for a change
/// that reaches a view some other way: the other player's move arriving, or
/// a lost round of fists marking damage on a card.
pub fn redraw(route: &'static str) {
    REDRAW.with(|queue| {
        let mut queue = queue.borrow_mut();
        if !queue.contains(&route) {
            queue.push(route);
        }
    });
}

/// Take the routes whose views are to be drawn again. They are taken once:
/// what is returned is no longer queued.
pub fn take_redraws() -> Vec<&'static str> {
    REDRAW.with(|queue| std::mem::take(&mut *queue.borrow_mut()))
}

/// Ask for the page to be loaded again. The engine does when it finds itself
/// out of date: another tab has changed what is stored since it started.
pub fn reload() {
    RELOAD.with(|reload| reload.set(true));
}

/// Whether a reload was asked for. Taken once.
pub fn take_reload() -> bool {
    RELOAD.with(|reload| reload.replace(false))
}

/// Tell the page's connection script to let go of the room: another tab
/// holds it now.
pub fn quiet() {
    QUIET.with(|quiet| quiet.set(true));
}

/// The room this player is in, for the page, if that has changed since the
/// page was last told: `Some(None)` is "not in a room".
fn take_room() -> Option<Option<serde_json::Value>> {
    let in_room = room::in_room();
    if IN_ROOM_TOLD.with(|told| told.replace(Some(in_room))) == Some(in_room) {
        return None;
    }
    Some(in_room.then(|| {
        room::with_room(|r| {
            serde_json::json!({
                "code": r.room_code,
                "name": r.room_name,
                "creator": r.creator,
                "playerId": r.player_id,
                "peerId": r.peer_id,
            })
        })
    }))
}

/// POST /api/handover — take everything the engine has queued for the page.
///
/// Returns JSON. Every part is left out when there is nothing of it:
///
///   updates  room-document updates for the other player (base64), oldest first
///   redraw   routes whose views are to be drawn again
///   save     storage keys to write (a string) or remove (null)
///   room     the room the player is in, or null: sent when they enter or
///            leave one, and once at the start
///   quiet    true: another tab holds the room now
///   reload   true: load the page again
pub fn handle_handover_post(_body: &str) -> String {
    let mut handed = serde_json::Map::new();

    let updates = crdt::take_outbox();
    if !updates.is_empty() {
        handed.insert("updates".into(), serde_json::json!(updates));
    }
    let redraws = take_redraws();
    if !redraws.is_empty() {
        handed.insert("redraw".into(), serde_json::json!(redraws));
    }
    let saves = store::take_saves();
    if !saves.is_empty() {
        handed.insert("save".into(), serde_json::json!(saves));
    }
    if let Some(room) = take_room() {
        handed.insert("room".into(), serde_json::json!(room));
    }
    if QUIET.with(|quiet| quiet.replace(false)) {
        handed.insert("quiet".into(), serde_json::json!(true));
    }
    if take_reload() {
        handed.insert("reload".into(), serde_json::json!(true));
    }
    serde_json::Value::Object(handed).to_string()
}

/// POST /api/page — the page reports something of itself or of storage
/// (see `store::page_event` for what).
pub fn handle_page_post(body: &str) -> String {
    if store::page_event(body) == store::Follows::RoomMoved {
        quiet();
    }
    "ok".to_string()
}

/// Forget everything queued and everything the page has been told.
#[cfg(test)]
pub fn reset() {
    take_redraws();
    take_reload();
    QUIET.with(|quiet| quiet.set(false));
    IN_ROOM_TOLD.with(|told| told.set(None));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::player_doc;

    fn fresh() {
        player_doc::init_player_doc();
        room::reset_room();
        crdt::reset_doc();
        crdt::take_outbox();
        store::reset();
        reset();
    }

    fn handed() -> serde_json::Value {
        serde_json::from_str(&handle_handover_post("")).unwrap()
    }

    #[test]
    fn a_route_is_queued_once_and_taken_once() {
        take_redraws();
        redraw("/api/card/panel");
        redraw("/api/room/fists");
        redraw("/api/card/panel");
        assert_eq!(take_redraws(), vec!["/api/card/panel", "/api/room/fists"]);
        assert!(take_redraws().is_empty());
    }

    #[test]
    fn the_handover_carries_what_is_queued_and_empties_it() {
        fresh();
        store::handle_start_post("{}");
        handed();
        crdt::queue_for_peer("abc".to_string());
        redraw("/api/game/turns");

        let first = handed();
        assert_eq!(first["updates"], serde_json::json!(["abc"]));
        assert_eq!(first["redraw"], serde_json::json!(["/api/game/turns"]));
        assert_eq!(handed(), serde_json::json!({}), "nothing is handed over twice");
        fresh();
    }

    #[test]
    fn the_page_is_told_of_the_room_at_the_start_and_when_it_changes() {
        fresh();
        store::handle_start_post("{}");
        assert_eq!(handed()["room"], serde_json::Value::Null);
        assert!(handed().get("room").is_none(), "told once");

        crate::routes::room::enter("ABCD", "Table", "me", true);
        let entered = handed();
        assert_eq!(entered["room"]["code"], "ABCD");
        assert_eq!(entered["room"]["playerId"], "me");
        assert!(handed().get("room").is_none());

        crate::routes::room::leave();
        let left = handed();
        assert!(left.as_object().unwrap().contains_key("room"));
        assert_eq!(left["room"], serde_json::Value::Null);
        fresh();
    }

    #[test]
    fn a_change_is_handed_over_as_a_save() {
        fresh();
        store::handle_start_post("{}");
        handed();
        player_doc::add_alarm(3, "mine", "red");
        let saved = handed();
        assert_eq!(saved["save"][store::PLAYER_DOC], player_doc::encode_full_state());
        fresh();
    }

    #[test]
    fn leaving_a_room_hands_over_its_keys_as_removals() {
        fresh();
        store::handle_start_post("{}");
        crate::routes::room::enter("ABCD", "Table", "me", true);
        handed();
        crate::routes::room::leave();
        let saved = handed();
        let save = saved["save"].as_object().unwrap();
        for key in [store::ROOM, store::ROOM_DOC, store::ROOM_TAB] {
            assert_eq!(save.get(key), Some(&serde_json::Value::Null), "{}", key);
        }
        fresh();
    }

    #[test]
    fn a_stale_page_shown_again_is_told_to_reload_and_a_taken_room_to_go_quiet() {
        fresh();
        store::handle_start_post("{}");
        crate::routes::room::enter("ABCD", "Table", "me", true);
        handed();

        let taken = serde_json::json!({ "event": "storage", "key": store::ROOM_TAB, "value": "another" });
        handle_page_post(&taken.to_string());
        assert_eq!(handed()["quiet"], true);

        handle_page_post(r#"{"event":"shown"}"#);
        assert_eq!(handed()["reload"], true);
        assert!(handed().get("reload").is_none());
        fresh();
    }
}
