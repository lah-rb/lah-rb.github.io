//! What the engine asks of the page, and the handover.
//!
//! The engine only answers requests: it cannot reach the page, open a socket
//! or wait for anything. What it wants done is queued, and after every post
//! the engine's worker takes what is queued (`handOver` in
//! kipukas-worker.js). The worker does its own part and passes the page its:
//!
//! - for the worker, the sockets and waits of `crate::wire`;
//! - for the page:
//!   - views to draw again ([`redraw`]), or all of them ([`refresh`]);
//!   - what to write to the browser's storage and what to remove from it
//!     (`crate::store`);
//!   - whether the player is in a room, which the toolbar shows different
//!     tools for;
//!   - that the page is to be loaded again ([`reload`]), or another page
//!     gone to ([`go`]);
//!   - a file for the player to keep ([`download`]).
//! - for the worker again, that its QR decoder is wanted ([`qr`]).
//!
//! Both do each of these as they are told and decide none of them.

use std::cell::{Cell, RefCell};

use crate::game::{connection, room};
use crate::{store, wire};

thread_local! {
    static REDRAW: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    static RELOAD: Cell<bool> = const { Cell::new(false) };
    static REFRESH: Cell<bool> = const { Cell::new(false) };
    static GO: RefCell<Option<String>> = const { RefCell::new(None) };
    static DOWNLOAD: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    static QR: Cell<Option<bool>> = const { Cell::new(None) };
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

/// Ask for every view on the page to be drawn again. For when the engine's
/// state has changed as a whole: the player's state taken up as another
/// page left it (`crate::store`).
pub fn refresh() {
    REFRESH.with(|refresh| refresh.set(true));
}

/// Whether a redraw of everything was asked for. Taken once.
pub fn take_refresh() -> bool {
    REFRESH.with(|refresh| refresh.replace(false))
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

/// Ask for the page to go to `path`, a path on this site. The QR scanner
/// does when a code that leads somewhere is read (`routes::qr`, which is
/// where the path is checked).
pub fn go(path: &str) {
    GO.with(|go| *go.borrow_mut() = Some(path.to_string()));
}

/// Where the page is to go, if anywhere. Taken once.
pub fn take_go() -> Option<String> {
    GO.with(|go| go.borrow_mut().take())
}

/// Hand the player a file to keep: its name and its text. The browser saves
/// it where it saves downloads. A backup of the player's data is one
/// (`game::backup`).
pub fn download(name: &str, text: &str) {
    DOWNLOAD.with(|download| *download.borrow_mut() = Some((name.to_string(), text.to_string())));
}

/// The file to hand over, if there is one. Taken once.
pub fn take_download() -> Option<(String, String)> {
    DOWNLOAD.with(|download| download.borrow_mut().take())
}

/// Tell the worker its QR decoder is wanted, and whether codes are to be
/// found by computer vision first (`cv`). The scanner does when it opens
/// and when that switch is turned.
pub fn qr(cv: bool) {
    QR.with(|qr| qr.set(Some(cv)));
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
///   refresh  true: draw every view again
///   save     storage keys to write (a string) or remove (null)
///   room     whether the player is in a room: sent when they enter or
///            leave one, and once at the start
///   qr       for the worker: get the QR decoder ready; `cv` says whether
///            codes are found by computer vision first, `reader`,
///            `model` and `runner` are the addresses of the reader's file,
///            the model's and that of what runs the model, `status`
///            whether it is to report what each frame takes
///   go       a path on this site for the page to go to
///   reload   true: load the page again
///   download a file for the player to keep: `{ name, text }`
pub fn handle_handover_post(_body: &str) -> String {
    let mut handed = serde_json::Map::new();

    // What this player changed in the room goes to the opponent first,
    // because it is sent through the wire.
    connection::send_updates();
    let saves = store::take_saves();
    let wire = wire::take();
    if !wire.is_empty() {
        handed.insert("wire".into(), serde_json::json!(wire));
    }
    let redraws = take_redraws();
    if !redraws.is_empty() {
        handed.insert("redraw".into(), serde_json::json!(redraws));
    }
    if !saves.is_empty() {
        handed.insert("save".into(), serde_json::json!(saves));
    }
    if take_refresh() {
        handed.insert("refresh".into(), serde_json::json!(true));
    }
    if let Some(in_room) = take_in_room() {
        handed.insert("room".into(), serde_json::json!(in_room));
    }
    if let Some(cv) = QR.with(|qr| qr.take()) {
        use crate::routes::qr::{MODEL, READER, RUNNER};
        let status = crate::settings::is_on(&crate::settings::SCANNER_STATUS);
        handed.insert(
            "qr".into(),
            serde_json::json!({ "cv": cv, "reader": READER, "model": MODEL, "runner": RUNNER, "status": status }),
        );
    }
    if let Some(path) = take_go() {
        handed.insert("go".into(), serde_json::json!(path));
    }
    if take_reload() {
        handed.insert("reload".into(), serde_json::json!(true));
    }
    if let Some((name, text)) = take_download() {
        handed.insert("download".into(), serde_json::json!({ "name": name, "text": text }));
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
    take_refresh();
    take_go();
    take_download();
    QR.with(|qr| qr.set(None));
    wire::take();
    IN_ROOM_TOLD.with(|told| told.set(None));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// When the timers in these tests are made and changed, where the test
    /// does not care when.
    const NOW: f64 = 1_800_000_000_000.0;
    use crate::game::{crdt, device_sync, player_doc};

    fn fresh() {
        player_doc::init_player_doc();
        room::reset_room();
        crdt::reset_doc();
        crdt::take_outbox();
        store::reset();
        connection::reset();
        device_sync::reset();
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
        player_doc::add_alarm(NOW, 3, "mine", "red");
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
        // The opponent is there and has said hello in our protocol.
        report("message", r#"{"type":"peer_joined","peerId":"bbbbbbbbbbbbbbbbbbbbbbbb"}"#);
        let hello = serde_json::json!({ "type": "relay", "data": {
            "type": "hello", "proto": connection::PROTOCOL, "playerId": "bbbbbbbbbbbbbbbbbbbbbbbb",
        } });
        report("message", &hello.to_string());
        handed();

        let update = crdt::add_alarm(NOW, 2, "ours", "red");
        crdt::queue_for_peer(update.clone());
        let sent = handed();
        assert_eq!(sent["wire"][0]["send"], socket.as_str());
        assert!(sent["wire"][0]["text"].as_str().unwrap().contains(&update));
        fresh();
    }

    #[test]
    fn a_game_moving_to_another_device_is_carried_by_the_same_handover() {
        fresh();
        store::handle_start_post("{}");
        handed();
        device_sync::ask();
        device_sync::invite();
        let asked = handed();
        let ops = asked["wire"].as_array().unwrap();
        let socket = ops.iter().find_map(|op| op["open"].as_str()).unwrap().to_string();
        assert!(socket.starts_with("sync:"));
        assert!(ops.contains(&serde_json::json!({ "set": "sync:expire", "ms": device_sync::INVITE_LASTS_MS })));
        assert_eq!(asked["redraw"], serde_json::json!(["/api/player/sync"]));
        let report = |event: &str, data: &str| {
            let report = serde_json::json!({ "socket": socket, "event": event, "data": data });
            wire::handle_wire_post(&report.to_string());
        };
        report("open", "");
        report("message", r#"{"type":"room_created","code":"ABCD","peer":"none"}"#);
        report("message", r#"{"type":"peer_joined","peerId":"other"}"#);
        assert_eq!(handed()["redraw"], serde_json::json!(["/api/player/sync"]));

        // The game moves when the other device asks, not as it is played:
        // a change made meanwhile is stored and sent nowhere.
        player_doc::add_alarm(NOW, 3, "mine", "red");
        let changed = handed();
        assert!(changed["save"].as_object().unwrap().contains_key(store::PLAYER_DOC));
        assert!(changed["wire"].is_null(), "{}", changed);

        // The wait the worker was asked for ends a code nobody used.
        wire::handle_wire_post(r#"{"timer":"sync:expire"}"#);
        let ended = handed();
        assert_eq!(ended["redraw"], serde_json::json!(["/api/player/sync"]));
        assert!(ended["wire"].as_array().unwrap().contains(&serde_json::json!({ "close": socket })));
        assert!(device_sync::dialog().error.starts_with("That code was not used"));
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
