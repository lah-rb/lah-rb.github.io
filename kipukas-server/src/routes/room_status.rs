//! `/api/room/status` and `/api/room/pill` — the room dialog and the status
//! pill, two engine-owned views (Pattern 13 in CONTRIBUTING.md) of the
//! room's connection (`game::connection`).
//!
//! The dialog is where a room is created, joined and left. Its forms and
//! buttons post back to its own route; what the relay then says changes what
//! it shows, and the engine has it drawn again.
//!
//! The pill is a small mark, bottom left, on every page while the player is
//! in a room:
//!
//! - a green dot: connected, opponent present
//! - "Waiting for opponent": connected to the relay; the opponent has not
//!   joined yet, or their connection has closed (a page change, a locked
//!   phone)
//! - "Reconnecting…": this player's own connection is down and being retried
//! - "Opponent needs to update": the opponent is on an older copy of the
//!   site; timers and combat will not sync until they update
//! - "No longer in the room": this player's place was taken while they were
//!   away (someone else joined with the room's name)
//! - "Open in another tab": this player has the room open in a newer tab,
//!   which holds the connection; a tap loads this tab again, which takes the
//!   room back

use crate::game::connection::{self, Dialog, Face, Stage};
use crate::page;
use crate::routes::util::{escape_html, get_param, parse_form_body};

const FIELD: &str = "w-full border rounded px-2 py-1 text-sm text-kip-drk-sienna border-kip-drk-sienna focus:border-kip-red focus:ring-kip-red";
const GREY_BUTTON: &str = "w-full bg-slate-400 hover:bg-slate-500 text-amber-50 font-bold py-2 px-4 rounded text-sm";

// ── The room dialog ────────────────────────────────────────────────

/// GET /api/room/status — what the room dialog holds.
pub fn handle_status_get(_query: &str) -> String {
    render_dialog(&connection::dialog())
}

/// POST /api/room/status — a form or button of the dialog.
///
/// Body:
///   - `action=create&name=<room name>`
///   - `action=join&code=<room code>&name=<room name>`
///   - `action=leave` (also calls off a create or join still on its way)
///   - `action=close` (the dialog was closed; sent by the page, not a button)
pub fn handle_status_post(body: &str) -> String {
    let params = parse_form_body(body);
    let name = get_param(&params, "name").unwrap_or("");
    match get_param(&params, "action").unwrap_or("") {
        "create" => connection::create(name),
        "join" => connection::join(get_param(&params, "code").unwrap_or(""), name),
        "leave" => connection::leave(),
        "close" => connection::dialog_closed(),
        _ => {}
    }
    String::new()
}

fn render_dialog(dialog: &Dialog) -> String {
    let mut h = String::with_capacity(2048);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna">"#);
    h.push_str(r#"<p class="text-lg font-bold mb-1">Multiplayer</p>"#);
    if !dialog.notice.is_empty() {
        h.push_str(&format!(
            r#"<p class="text-kip-red text-sm my-2" role="alert">{}</p>"#,
            escape_html(&dialog.notice)
        ));
    }
    match dialog.stage {
        Stage::Out => h.push_str(&render_forms()),
        Stage::Connecting => {
            h.push_str(r#"<p class="text-sm text-amber-600 mb-3">&#x23F3; Connecting…</p>"#);
            h.push_str(&leave_button("Cancel"));
        }
        Stage::Waiting => {
            h.push_str(r#"<p class="text-sm text-amber-600 mb-1">&#x23F3; Waiting for peer…</p>"#);
            h.push_str(&render_room(dialog));
            h.push_str(&leave_button("Cancel"));
        }
        Stage::Connected => {
            h.push_str(r#"<p class="text-sm text-emerald-600 mb-1">&#x2713; Connected</p>"#);
            h.push_str(&render_room(dialog));
            h.push_str(&leave_button("Disconnect"));
        }
    }
    h.push_str("</div>");
    h
}

/// The two forms a player outside a room is offered.
fn render_forms() -> String {
    format!(
        concat!(
            r#"<form class="mt-2" data-engine-post="/api/room/status" data-engine-body="action=create" autocomplete="off">"#,
            r#"<div class="mb-3">"#,
            r#"<label class="block text-sm mb-1" for="room-name-input">Room Name</label>"#,
            r#"<input type="text" id="room-name-input" name="name" placeholder="My Room" class="{field}">"#,
            r#"</div>"#,
            r#"<button class="w-full bg-kip-red hover:bg-kip-drk-sienna text-amber-50 font-bold py-2 px-4 rounded mb-2 text-sm">Create Room</button>"#,
            r#"</form>"#,
            r#"<div class="border-t border-slate-300 my-3"></div>"#,
            r#"<form data-engine-post="/api/room/status" data-engine-body="action=join" autocomplete="off">"#,
            r#"<label class="block text-sm mb-1" for="room-code-input">Room Code</label>"#,
            r#"<input type="text" id="room-code-input" name="code" placeholder="ABCD" maxlength="4" autocapitalize="characters" class="{field} uppercase mb-2">"#,
            r#"<label class="block text-sm mb-1" for="room-name-join-input">Room Name</label>"#,
            r#"<input type="text" id="room-name-join-input" name="name" placeholder="My Room" class="{field}">"#,
            r#"<button class="w-full bg-emerald-600 hover:bg-emerald-700 text-amber-50 font-bold py-2 px-4 rounded mt-2 text-sm">Join Room</button>"#,
            r#"</form>"#,
        ),
        field = FIELD
    )
}

/// The room's name and code. Both come from the relay, and the name from
/// whoever created the room.
fn render_room(dialog: &Dialog) -> String {
    let mut h = String::new();
    if !dialog.name.is_empty() {
        h.push_str(&format!(
            r#"<p class="text-sm mb-1">Room: <strong>{}</strong></p>"#,
            escape_html(&dialog.name)
        ));
    }
    h.push_str(&format!(
        r#"<p class="text-sm mb-3 font-mono tracking-wider">Code: <strong>{}</strong></p>"#,
        escape_html(&dialog.code)
    ));
    h
}

fn leave_button(label: &str) -> String {
    format!(
        r#"<button type="button" data-engine-post="/api/room/status" data-engine-body="action=leave" class="{}">{}</button>"#,
        GREY_BUTTON, label
    )
}

// ── The status pill ────────────────────────────────────────────────

/// GET /api/room/pill — the pill, or nothing when the player is in no room.
pub fn handle_pill_get(_query: &str) -> String {
    render_pill(connection::face())
}

/// POST /api/room/pill — a tap on the pill.
///
/// Body:
///   - `action=dismiss`: put "No longer in the room" away
///   - `action=here`: use the room in this tab. Its engine is out of date
///     (another tab has had the room since), so the page is loaded again.
pub fn handle_pill_post(body: &str) -> String {
    let params = parse_form_body(body);
    match get_param(&params, "action").unwrap_or("") {
        "dismiss" => connection::dismiss_expired(),
        "here" => page::reload(),
        _ => {}
    }
    String::new()
}

const PILL: &str = "fixed bottom-0 left-0 z-40 m-3 flex items-center gap-2 rounded-full bg-amber-50/90 px-2.5 py-1.5 text-xs font-semibold text-kip-drk-sienna shadow-md select-none";
const DOT: &str = "h-2.5 w-2.5 rounded-full";

fn render_pill(face: Face) -> String {
    let waiting = |label: &str| {
        format!(
            r#"<div id="room-status-pill" data-tone="warn" class="{}"><span class="{} bg-amber-500 animate-pulse"></span><span>{}</span></div>"#,
            PILL, DOT, label
        )
    };
    match face {
        Face::Off => String::new(),
        Face::Connected => format!(
            r#"<div id="room-status-pill" data-tone="ok" class="{}"><span class="{} bg-emerald-500"></span><span class="sr-only">Connected</span></div>"#,
            PILL, DOT
        ),
        Face::Reconnecting => waiting("Reconnecting…"),
        Face::WaitingForOpponent => waiting("Waiting for opponent"),
        Face::OpponentOutdated => waiting("Opponent needs to update"),
        Face::Expired => format!(
            concat!(
                r#"<div id="room-status-pill" data-tone="bad" class="{}"><span class="{} bg-kip-red"></span>"#,
                r#"<span>No longer in the room</span>"#,
                r#"<button type="button" aria-label="Dismiss" data-engine-post="/api/room/pill" data-engine-body="action=dismiss" class="ml-1 cursor-pointer opacity-60 hover:opacity-100">&#x2715;</button>"#,
                r#"</div>"#,
            ),
            PILL, DOT
        ),
        Face::Moved => format!(
            concat!(
                r#"<button type="button" id="room-status-pill" data-tone="moved" data-engine-post="/api/room/pill" data-engine-body="action=here" class="{} cursor-pointer">"#,
                r#"<span class="{} bg-slate-400"></span><span>Open in another tab. Tap to use here.</span>"#,
                r#"</button>"#,
            ),
            PILL, DOT
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{crdt, player_doc, room};
    use crate::store;

    fn fresh() {
        player_doc::init_player_doc();
        room::reset_room();
        crdt::reset_doc();
        crdt::take_outbox();
        store::reset();
        page::reset();
        connection::reset();
    }

    fn dialog(stage: Stage, notice: &str) -> Dialog {
        Dialog { stage, name: "Table".to_string(), code: "ABCD".to_string(), notice: notice.to_string() }
    }

    #[test]
    fn outside_a_room_the_dialog_offers_to_create_or_join_one() {
        let html = render_dialog(&dialog(Stage::Out, ""));
        assert!(html.contains(r#"<form class="mt-2" data-engine-post="/api/room/status" data-engine-body="action=create""#));
        assert!(html.contains(r#"<form data-engine-post="/api/room/status" data-engine-body="action=join""#));
        assert!(html.contains(r#"id="room-name-input" name="name""#));
        assert!(html.contains(r#"id="room-code-input" name="code""#));
        assert!(html.contains(r#"id="room-name-join-input" name="name""#));
        assert!(html.contains("Create Room"));
        assert!(html.contains("Join Room"));
        assert!(!html.contains("Code:"), "no room, no code");
    }

    #[test]
    fn the_dialog_has_nothing_for_the_page_to_run() {
        for stage in [Stage::Out, Stage::Connecting, Stage::Waiting, Stage::Connected] {
            let html = render_dialog(&dialog(stage, "said"));
            for forbidden in ["onclick", "kipukasMultiplayer", "htmx", "hx-", "x-data", "@click", "<script"] {
                assert!(!html.contains(forbidden), "{} at {:?}", forbidden, stage);
            }
        }
    }

    #[test]
    fn each_stage_says_where_the_player_stands_and_how_to_get_out() {
        let connecting = render_dialog(&dialog(Stage::Connecting, ""));
        assert!(connecting.contains("Connecting…"));
        assert!(connecting.contains(r#"data-engine-body="action=leave""#));
        assert!(connecting.contains(">Cancel</button>"));
        assert!(!connecting.contains("<form"));

        let waiting = render_dialog(&dialog(Stage::Waiting, ""));
        assert!(waiting.contains("Waiting for peer"));
        assert!(waiting.contains("Room: <strong>Table</strong>"));
        assert!(waiting.contains("Code: <strong>ABCD</strong>"));
        assert!(waiting.contains(">Cancel</button>"));

        let connected = render_dialog(&dialog(Stage::Connected, ""));
        assert!(connected.contains("&#x2713; Connected"));
        assert!(connected.contains("Code: <strong>ABCD</strong>"));
        assert!(connected.contains(">Disconnect</button>"));
    }

    #[test]
    fn what_went_wrong_is_said_above_the_forms_so_the_player_can_try_again() {
        let html = render_dialog(&dialog(Stage::Out, "Room not found"));
        assert!(html.contains(r#"role="alert">Room not found</p>"#));
        assert!(html.find("Room not found").unwrap() < html.find("<form").unwrap());
    }

    #[test]
    fn a_room_name_or_a_relay_message_cannot_carry_markup() {
        let mut shown = dialog(Stage::Connected, "<b>said</b>");
        shown.name = "<img src=x onerror=alert(1)>".to_string();
        let html = render_dialog(&shown);
        assert!(!html.contains("<img"));
        assert!(!html.contains("<b>said"));
        assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
    }

    #[test]
    fn the_pill_shows_each_face_and_nothing_outside_a_room() {
        assert_eq!(render_pill(Face::Off), "");
        let ok = render_pill(Face::Connected);
        assert!(ok.contains(r#"data-tone="ok""#));
        assert!(ok.contains(r#"<span class="sr-only">Connected</span>"#));
        for (face, label) in [
            (Face::Reconnecting, "Reconnecting…"),
            (Face::WaitingForOpponent, "Waiting for opponent"),
            (Face::OpponentOutdated, "Opponent needs to update"),
        ] {
            let html = render_pill(face);
            assert!(html.contains(r#"data-tone="warn""#), "{:?}", face);
            assert!(html.contains(&format!("<span>{}</span>", label)), "{:?}", face);
            assert!(html.contains("animate-pulse"));
        }
        let bad = render_pill(Face::Expired);
        assert!(bad.contains("No longer in the room"));
        assert!(bad.contains(r#"data-engine-post="/api/room/pill" data-engine-body="action=dismiss""#));
        let moved = render_pill(Face::Moved);
        assert!(moved.starts_with("<button"));
        assert!(moved.contains(r#"data-engine-body="action=here""#));
        assert!(moved.contains("Open in another tab. Tap to use here."));
    }

    #[test]
    fn the_dialogs_forms_reach_the_connection() {
        fresh();
        assert_eq!(handle_status_post("action=join&code=AB&name=Table"), "");
        assert!(handle_status_get("").contains("Please enter a 4-character room code."));
        handle_status_post("action=close");
        assert!(!handle_status_get("").contains("Please enter"));

        handle_status_post("action=create&name=My+Room");
        assert!(handle_status_get("").contains("Connecting…"));
        handle_status_post("action=leave");
        assert!(handle_status_get("").contains("Create Room"));
        fresh();
    }

    #[test]
    fn tapping_the_moved_pill_asks_for_the_page_to_be_loaded_again() {
        fresh();
        handle_pill_post("action=here");
        assert!(page::take_reload());
        handle_pill_post("action=dismiss");
        assert!(!page::take_reload());
        fresh();
    }
}
