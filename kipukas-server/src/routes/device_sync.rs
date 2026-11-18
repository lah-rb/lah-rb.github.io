//! `/api/player/sync` — the Sync Devices dialog, an engine-owned view
//! (Pattern 13 in CONTRIBUTING.md) of a pairing of two devices
//! (`game::device_sync`).
//!
//! The dialog's forms and buttons post back to this route. What the relay
//! and the other device then say changes what it shows, and the engine has
//! it drawn again.

use crate::game::device_sync::{self, Dialog, Screen};
use crate::routes::util::{escape_html, get_param, parse_form_body};

const RED_BUTTON: &str =
    "bg-kip-red text-amber-50 font-bold py-2 px-3 rounded text-sm hover:bg-red-700 active:bg-red-800";

/// GET /api/player/sync — what the dialog holds.
pub fn handle_sync_get(_query: &str) -> String {
    render(&device_sync::dialog())
}

/// POST /api/player/sync — a form or button of the dialog.
///
/// Body:
///   - `passphrase=<passphrase>&action=create`: create a room for the other
///     device to join
///   - `passphrase=<passphrase>&action=code`: go on to ask for the other
///     device's room code
///   - `action=join&code=<room code>`: join the other device's room
///   - `action=back`: back to the first screen
///   - `action=close`: the dialog was closed (sent by the page), which ends
///     the pairing
pub fn handle_sync_post(body: &str) -> String {
    let params = parse_form_body(body);
    let passphrase = get_param(&params, "passphrase").unwrap_or("");
    match get_param(&params, "action").unwrap_or("") {
        "create" => device_sync::create(passphrase),
        "code" => device_sync::ask_for_code(passphrase),
        "join" => device_sync::join(get_param(&params, "code").unwrap_or("")),
        "back" => device_sync::back(),
        "close" => device_sync::close(),
        _ => {}
    }
    String::new()
}

fn render(dialog: &Dialog) -> String {
    let mut h = String::with_capacity(2048);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna">"#);
    h.push_str(r#"<p class="text-lg font-bold text-center mb-1">🔄 Sync Devices</p>"#);
    h.push_str(
        r#"<p class="text-xs text-slate-500 text-center mb-3">Merge your player data between two devices</p>"#,
    );

    // What went wrong, or else where things stand. Both are partly the
    // relay's words.
    if !dialog.error.is_empty() {
        h.push_str(&format!(
            r#"<div class="bg-red-50 border border-red-200 rounded p-2 mb-3 text-center"><p class="text-xs text-kip-red" role="alert">{}</p></div>"#,
            escape_html(&dialog.error)
        ));
    } else if !dialog.status.is_empty() {
        h.push_str(&format!(
            r#"<div class="bg-blue-50 border border-blue-200 rounded p-2 mb-3 text-center"><p class="text-xs text-blue-700">{}</p></div>"#,
            escape_html(&dialog.status)
        ));
    }

    match dialog.screen {
        Screen::Menu => h.push_str(&render_menu(dialog)),
        Screen::Join => h.push_str(&render_join()),
        Screen::Paired => h.push_str(&render_paired(dialog)),
    }
    h.push_str("</div>");
    h
}

/// The passphrase, and the choice of creating a room or joining one. The
/// button pressed says which (its `name` and `value` are sent with the form).
fn render_menu(dialog: &Dialog) -> String {
    let typed = if dialog.passphrase.is_empty() {
        String::new()
    } else {
        format!(r#" value="{}""#, escape_html(&dialog.passphrase))
    };
    format!(
        concat!(
            r#"<form data-engine-post="/api/player/sync" autocomplete="off">"#,
            r#"<div class="mb-3">"#,
            r#"<label class="block text-xs font-medium mb-1" for="sync-passphrase">Shared Passphrase</label>"#,
            r#"<input type="password" id="sync-passphrase" name="passphrase"{typed} placeholder="Same passphrase on both devices" class="w-full border border-slate-300 rounded px-2 py-1.5 text-sm bg-white">"#,
            r#"<p class="text-xs text-slate-400 mt-0.5">Both devices must use the same passphrase</p>"#,
            r#"</div>"#,
            r#"<div class="grid grid-cols-2 gap-2 mb-3">"#,
            r#"<button name="action" value="create" class="{red}">Create Room</button>"#,
            r#"<button name="action" value="code" class="bg-slate-600 text-amber-50 font-bold py-2 px-3 rounded text-sm hover:bg-slate-700 active:bg-slate-800">Join Room</button>"#,
            r#"</div>"#,
            r#"</form>"#,
            r#"<div class="text-xs text-slate-400 text-center">"#,
            r#"<p>Create a room on one device, then join from the other.</p>"#,
            r#"<p class="mt-1">Data merges using conflict-free replication (CRDT).</p>"#,
            r#"</div>"#,
        ),
        typed = typed,
        red = RED_BUTTON
    )
}

/// The other device's room code.
fn render_join() -> String {
    format!(
        concat!(
            r#"<form data-engine-post="/api/player/sync" data-engine-body="action=join" autocomplete="off">"#,
            r#"<div class="mb-3">"#,
            r#"<label class="block text-xs font-medium mb-1" for="sync-code">Room Code</label>"#,
            r#"<input type="text" id="sync-code" name="code" placeholder="4-letter code" maxlength="4" autocapitalize="characters" class="w-full border border-slate-300 rounded px-2 py-1.5 bg-white uppercase text-center font-mono text-lg tracking-widest">"#,
            r#"</div>"#,
            r#"<div class="grid grid-cols-2 gap-2 mb-3">"#,
            r#"<button type="button" data-engine-post="/api/player/sync" data-engine-body="action=back" class="bg-slate-400 text-amber-50 font-bold py-2 px-3 rounded text-sm hover:bg-slate-500">← Back</button>"#,
            r#"<button class="{red}">Join</button>"#,
            r#"</div>"#,
            r#"</form>"#,
        ),
        red = RED_BUTTON
    )
}

/// In a room: its code, and how the exchange is going. The dialog's own
/// Close button ends it.
fn render_paired(dialog: &Dialog) -> String {
    let mut h = String::new();
    if !dialog.code.is_empty() {
        h.push_str(&format!(
            concat!(
                r#"<div class="bg-white border-2 border-slate-300 rounded-lg p-3 mb-3 text-center">"#,
                r#"<p class="text-xs text-slate-500 mb-1">Room Code</p>"#,
                r#"<p class="text-3xl font-mono font-bold tracking-widest text-kip-drk-sienna">{}</p>"#,
                r#"</div>"#,
            ),
            escape_html(&dialog.code)
        ));
    }
    if dialog.syncing {
        h.push_str(concat!(
            r#"<div class="flex items-center justify-center gap-2 mb-3">"#,
            r#"<svg class="animate-spin h-4 w-4 text-kip-red" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">"#,
            r#"<circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>"#,
            r#"<path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path>"#,
            r#"</svg>"#,
            r#"<span class="text-xs text-slate-600">Syncing…</span>"#,
            r#"</div>"#,
        ));
    }
    if dialog.done {
        h.push_str(concat!(
            r#"<div class="bg-emerald-50 border border-emerald-200 rounded p-3 mb-3 text-center">"#,
            r#"<p class="text-sm font-bold text-emerald-700">✓ Sync Complete</p>"#,
            r#"<p class="text-xs text-emerald-600 mt-1">Player data has been merged on both devices.</p>"#,
            r#"</div>"#,
        ));
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog(screen: Screen) -> Dialog {
        Dialog { screen, ..Dialog::default() }
    }

    #[test]
    fn the_first_screen_asks_for_the_passphrase_and_which_way_to_pair() {
        let html = render(&dialog(Screen::Menu));
        assert!(html.contains(r#"<form data-engine-post="/api/player/sync" autocomplete="off">"#));
        assert!(html.contains(r#"type="password" id="sync-passphrase" name="passphrase" placeholder"#));
        assert!(html.contains(r#"<button name="action" value="create""#));
        assert!(html.contains(r#"<button name="action" value="code""#));
    }

    #[test]
    fn the_dialog_has_nothing_for_the_page_to_run() {
        let mut busy = dialog(Screen::Paired);
        busy.code = "ABCD".to_string();
        busy.syncing = true;
        busy.done = true;
        for shown in [dialog(Screen::Menu), dialog(Screen::Join), busy] {
            let html = render(&shown);
            for forbidden in ["onclick", "kipukasSync", "x-data", "x-model", "@click", "x-show", "<script", "<template"] {
                assert!(!html.contains(forbidden), "{} at {:?}", forbidden, shown.screen);
            }
        }
    }

    #[test]
    fn what_was_typed_is_put_back_in_its_field_and_cannot_break_out_of_it() {
        let mut shown = dialog(Screen::Menu);
        shown.passphrase = r#"a"><b>"#.to_string();
        let html = render(&shown);
        assert!(html.contains(r#"name="passphrase" value="a&quot;&gt;&lt;b&gt;" placeholder"#), "{}", html);
    }

    #[test]
    fn the_join_screen_asks_for_the_code_and_offers_the_way_back() {
        let html = render(&dialog(Screen::Join));
        assert!(html.contains(r#"<form data-engine-post="/api/player/sync" data-engine-body="action=join""#));
        assert!(html.contains(r#"id="sync-code" name="code""#));
        assert!(html.contains(r#"<button type="button" data-engine-post="/api/player/sync" data-engine-body="action=back""#));
    }

    #[test]
    fn an_error_is_shown_in_place_of_the_status_and_neither_can_carry_markup() {
        let mut shown = dialog(Screen::Menu);
        shown.status = "Joining…".to_string();
        shown.error = "<i>Room not found</i>".to_string();
        let html = render(&shown);
        assert!(html.contains("&lt;i&gt;Room not found&lt;/i&gt;"));
        assert!(!html.contains("Joining…"));

        shown.error.clear();
        assert!(render(&shown).contains("Joining…"));
    }

    #[test]
    fn a_pairing_shows_its_code_and_how_the_exchange_is_going() {
        let mut shown = dialog(Screen::Paired);
        assert!(!render(&shown).contains("Room Code"), "no code yet");
        shown.code = "ABCD".to_string();
        shown.syncing = true;
        let html = render(&shown);
        assert!(html.contains(">ABCD</p>"));
        assert!(html.contains("Syncing…"));
        assert!(!html.contains("Sync Complete"));
        shown.syncing = false;
        shown.done = true;
        let html = render(&shown);
        assert!(html.contains("✓ Sync Complete"));
        assert!(!html.contains("Syncing…"));
    }

    #[test]
    fn the_forms_reach_the_pairing() {
        device_sync::reset();
        assert_eq!(handle_sync_post("passphrase=abc&action=create"), "");
        assert!(handle_sync_get("").contains("Passphrase must be at least 4 characters"));
        handle_sync_post("passphrase=correct+horse&action=code");
        assert!(handle_sync_get("").contains(r#"name="code""#));
        handle_sync_post("action=back");
        assert!(handle_sync_get("").contains(r#"value="correct horse""#));
        handle_sync_post("action=close");
        assert!(!handle_sync_get("").contains("correct horse"));
        device_sync::reset();
    }
}
