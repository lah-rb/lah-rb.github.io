//! What the engine asks of the page, and the handover.
//!
//! The engine only answers requests: it cannot reach the page, open a socket
//! or wait for anything. What it wants done is queued, and after every post
//! the engine's worker takes what is queued (`handOver` in
//! kipukas-worker.js). The worker does its own part and passes the page its:
//!
//! - for the worker, the sockets and waits of `crate::wire`;
//! - for the page:
//!   - views to draw again ([`redraw`]);
//!   - what to write to the browser's storage and what to remove from it
//!     (`crate::store`);
//!   - whether the player is in a room, which the toolbar shows different
//!     tools for;
//!   - that the page is to be loaded again ([`reload`]).
//!
//! Both do each of these as they are told and decide none of them.

use std::cell::{Cell, RefCell};

use crate::game::{connection, room};
use crate::{store, wire};

thread_local! {
    static REDRAW: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    static RELOAD: Cell<bool> = const { Cell::new(false) };
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

/// Whether the player is in a room, if that has changed since the page was
/// last told.
fn take_in_room() -> Option<bool> {
    let in_room = room::in_room();
    (IN_ROOM_TOLD.with(|told| told.replace(Some(in_room))) != Some(in_room)).then_some(in_room)
}

/// POST /api/handover — take everything the engine has queued.
///
/// Returns JSON. Every part is left out when there is nothing of it:
///
///   wire     for the worker: sockets and waits (see `crate::wire`)
///   redraw   routes whose views are to be drawn again
///   save     storage keys to write (a string) or remove (null)
///   room     whether the player is in a room: sent when they enter or
///            leave one, and once at the start
///   reload   true: load the page again
pub fn handle_handover_post(_body: &str) -> String {
    let mut handed = serde_json::Map::new();

    // What this player changed in the room goes to the opponent first: it
    // is sent through the wire.
    connection::send_updates();
    let wire = wire::take();
    if !wire.is_empty() {
        handed.insert("wire".into(), serde_json::json!(wire));
    }
    let redraws = take_redraws();
    if !redraws.is_empty() {
        handed.insert("redraw".into(), serde_json::json!(redraws));
    }
    let saves = store::take_saves();
    if !saves.is_empty() {
        handed.insert("save".into(), serde_json::json!(saves));
    }
    if let Some(in_room) = take_in_room() {
        handed.insert("room".into(), serde_json::json!(in_room));
    }
    if take_reload() {
        handed.insert("reload".into(), serde_json::json!(true));
    }
    serde_json::Value::Object(handed).to_string()
}

/// POST /api/page — the page reports something of itself or of storage
/// (see `store::page_event` for what). The worker adds `alive`, the names
/// of the sockets it finds open.
pub fn handle_page_post(body: &str) -> String {
    match store::page_event(body) {
        store::Follows::Nothing => {}
        store::Follows::CameBack => {
            let alive: Vec<String> = serde_json::from_str::<serde_json::Value>(body)
                .ok()
                .and_then(|report| serde_json::from_value(report["alive"].clone()).ok())
                .unwrap_or_default();
            connection::came_back(&alive);
        }
        store::Follows::RoomMoved => connection::go_quiet(),
    }
    "ok".to_string()
}

/// Forget everything queued and everything the page has been told.
#[cfg(test)]
pub fn reset() {
    take_redraws();
    take_reload();
    wire::take();
    IN_ROOM_TOLD.with(|told| told.set(None));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{crdt, player_doc};

    fn fresh() {
        player_doc::init_player_doc();
        room::reset_room();
        crdt::reset_doc();
        crdt::take_outbox();
        store::reset();
        connection::reset();
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
        redraw("/api/game/turns");
        wire::set(wire::Channel::Room, "hello", 3000);

        let first = handed();
        assert_eq!(first["redraw"], serde_json::json!(["/api/game/turns"]));
        assert_eq!(first["wire"], serde_json::json!([{ "set": "room:hello", "ms": 3000 }]));
        assert_eq!(handed(), serde_json::json!({}), "nothing is handed over twice");
        fresh();
    }

    #[test]
    fn the_page_is_told_whether_the_player_is_in_a_room_at_the_start_and_when_it_changes() {
        fresh();
        store::handle_start_post("{}");
        assert_eq!(handed()["room"], false);
        assert!(handed().get("room").is_none(), "told once");

        crate::routes::room::enter("ABCD", "Table", "me", true);
        assert_eq!(handed()["room"], true);
        assert!(handed().get("room").is_none());

        crate::routes::room::leave();
        assert_eq!(handed()["room"], false);
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
    fn what_this_player_changes_in_the_room_goes_out_with_the_handover() {
        fresh();
        store::handle_start_post("{}");
        connection::create("Table");
        let socket = handed()["wire"][0]["open"].as_str().unwrap().to_string();
        let report = |event: &str, data: &str| {
            let report = serde_json::json!({ "socket": socket, "event": event, "data": data });
            wire::handle_wire_post(&report.to_string());
        };
        report("open", "");
        report("message", r#"{"type":"room_created","code":"ABCD","name":"Table"}"#);
        handed();

        let update = crdt::add_alarm(2, "ours", "red");
        crdt::queue_for_peer(update.clone());
        let sent = handed();
        assert_eq!(sent["wire"][0]["send"], socket.as_str());
        assert!(sent["wire"][0]["text"].as_str().unwrap().contains(&update));
        fresh();
    }

    #[test]
    fn a_stale_page_shown_again_is_told_to_reload_and_a_taken_room_goes_quiet() {
        fresh();
        store::handle_start_post("{}");
        connection::create("Table");
        let socket = handed()["wire"][0]["open"].as_str().unwrap().to_string();
        let open = serde_json::json!({ "socket": socket, "event": "open" });
        wire::handle_wire_post(&open.to_string());
        let created = serde_json::json!({
            "socket": socket, "event": "message",
            "data": r#"{"type":"room_created","code":"ABCD","name":"Table"}"#,
        });
        wire::handle_wire_post(&created.to_string());
        handed();

        let taken = serde_json::json!({ "event": "storage", "key": store::ROOM_TAB, "value": "another" });
        handle_page_post(&taken.to_string());
        let quiet = handed();
        assert!(quiet["wire"].as_array().unwrap().contains(&serde_json::json!({ "close": socket })));
        assert!(quiet["redraw"].as_array().unwrap().contains(&serde_json::json!("/api/room/pill")));

        handle_page_post(r#"{"event":"shown"}"#);
        assert_eq!(handed()["reload"], true);
        assert!(handed().get("reload").is_none());
        fresh();
    }

    #[test]
    fn a_page_back_in_view_has_the_connection_checked_against_what_the_worker_finds_open() {
        fresh();
        store::handle_start_post("{}");
        connection::create("Table");
        let socket = handed()["wire"][0]["open"].as_str().unwrap().to_string();
        let open = serde_json::json!({ "socket": socket, "event": "open" });
        wire::handle_wire_post(&open.to_string());
        let created = serde_json::json!({
            "socket": socket, "event": "message",
            "data": r#"{"type":"room_created","code":"ABCD","name":"Table"}"#,
        });
        wire::handle_wire_post(&created.to_string());
        handed();

        // The worker finds the socket open: a ping, nothing more.
        let shown = serde_json::json!({ "event": "shown", "alive": [socket] });
        handle_page_post(&shown.to_string());
        assert_eq!(handed()["wire"], serde_json::json!([{ "send": socket, "text": r#"{"type":"ping"}"# }]));

        // It does not: the socket is given up and a retry is due.
        handle_page_post(r#"{"event":"online","alive":[]}"#);
        let wire = handed()["wire"].clone();
        assert!(wire.as_array().unwrap().contains(&serde_json::json!({ "close": socket })), "{}", wire);
        assert!(wire.to_string().contains("room:retry"), "{}", wire);
        fresh();
    }
}
