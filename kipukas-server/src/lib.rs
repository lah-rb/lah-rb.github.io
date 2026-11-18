//! Kipukas in-browser WASM server.
//!
//! Exports `handle_request(method, path, query, body)` for the Service Worker
//! bridge to call. Uses `matchit` for URL routing — the same router
//! engine that powers Axum.

use wasm_bindgen::prelude::*;

pub mod catalog;
pub mod clock;
pub mod game;
pub mod image;
pub mod link;
pub mod page;
pub mod routes;
pub mod store;
pub mod typing;
pub mod wire;

/// The card pages of the built site. Used at build time only (natively, by
/// `src/bin/card-pages.rs`), so it is left out of the browser's engine.
#[cfg(not(target_arch = "wasm32"))]
pub mod site;

/// The cards' preview pictures for link shares. Build time only, like `site`.
#[cfg(not(target_arch = "wasm32"))]
pub mod preview;

/// The card catalog as Kippa reads it. Build time only, like `site`.
#[cfg(not(target_arch = "wasm32"))]
pub mod kippa;

/// Process an HTTP-like request and return an HTML fragment.
///
/// Called from JavaScript (Web Worker) via wasm-bindgen.
///
/// # Arguments
/// * `method` — HTTP method (e.g., "GET", "POST")
/// * `path`   — URL path (e.g., "/api/type-matchup")
/// * `query`  — Query string (e.g., "?atk[]=Brutal&def[]=Avian")
/// * `body`   — Request body (e.g., POST form data). Empty string for GET requests.
///
/// # Returns
/// An HTML string fragment suitable for HTMX to swap into the DOM.
#[wasm_bindgen]
pub fn handle_request(method: &str, path: &str, query: &str, body: &str) -> String {
    // Build the router. matchit compiles route patterns into a radix tree.
    let mut router = matchit::Router::new();

    // Register routes — the value is a &str tag we match on below
    router.insert("/api/type-matchup", "type_matchup").ok();
    router.insert("/api/cards", "cards").ok();
    router.insert("/api/card/panel", "card_panel").ok();
    router.insert("/api/qr/status", "qr_status").ok();
    router.insert("/api/qr/found", "qr_found").ok();
    router.insert("/api/hand", "hand").ok();

    // Game state routes (PLAYER_DOC backed)
    router.insert("/api/game/damage", "game_damage").ok();
    router.insert("/api/game/turns", "game_turns").ok();
    router.insert("/api/game/turns/form", "game_turns_form").ok();

    // The room: its dialog, the status pill, and fists
    router.insert("/api/room/status", "room_status").ok();
    router.insert("/api/room/pill", "room_pill").ok();
    router.insert("/api/room/fists", "room_fists").ok();

    // Player document routes
    router.insert("/api/player/export", "player_export").ok();
    router.insert("/api/player/import", "player_import").ok();
    router.insert("/api/player/affinity", "player_affinity").ok();
    router.insert("/api/player/export/signed", "player_export_signed").ok();
    router.insert("/api/player/import/signed", "player_import_signed").ok();

    // Cross-device sync routes (Phase E)
    router.insert("/api/player/sync/sv", "player_sync_sv").ok();
    router.insert("/api/player/sync/diff", "player_sync_diff").ok();
    router.insert("/api/player/sync/apply", "player_sync_apply").ok();
    router.insert("/api/player/sync/auth", "player_sync_auth").ok();
    router.insert("/api/player/sync/verify", "player_sync_verify").ok();

    // Between the engine and what holds it: the page (`page`, `store`) and
    // its own worker (`wire`)
    router.insert("/api/start", "start").ok();
    router.insert("/api/page", "page").ok();
    router.insert("/api/wire", "wire").ok();
    router.insert("/api/handover", "handover").ok();

    match router.at(path) {
        Ok(matched) => match (*matched.value, method) {
            // GET routes
            ("type_matchup", "GET") => routes::type_matchup::handle(query),
            ("type_matchup", "POST") => routes::type_matchup::handle_post(body),
            ("cards", "GET") => routes::cards::handle(query),
            ("cards", "POST") => routes::cards::handle_post(body),
            ("card_panel", "GET") => routes::cards::handle_panel(query),
            ("hand", "GET") => routes::hand::handle(query),
            ("qr_status", "GET") => routes::qr::handle_status(query),
            ("qr_found", "GET") => routes::qr::handle_found(query),
            ("game_damage", "GET") => routes::game::handle_damage_get(query),
            ("game_turns", "GET") => routes::game::handle_turns_get(query),
            ("game_turns_form", "GET") => routes::game::handle_turns_form_get(query),

            ("game_damage", "POST") => routes::game::handle_damage_post(body),
            ("game_turns", "POST") => routes::game::handle_turns_post(body),

            // The room: its dialog, the status pill, and fists
            ("room_status", "GET") => routes::room_status::handle_status_get(query),
            ("room_status", "POST") => routes::room_status::handle_status_post(body),
            ("room_pill", "GET") => routes::room_status::handle_pill_get(query),
            ("room_pill", "POST") => routes::room_status::handle_pill_post(body),
            ("room_fists", "GET") => routes::room::handle_fists_get(query),
            ("room_fists", "POST") => routes::room::handle_fists_post(body),

            // Player document routes
            ("player_export", "GET") => routes::game::handle_player_export_get(query),
            ("player_import", "POST") => routes::game::handle_player_import_post(body),
            ("player_affinity", "GET") => routes::player::handle_affinity_get(query),
            ("player_affinity", "POST") => routes::player::handle_affinity_post(body),
            ("player_export_signed", "POST") => routes::player::handle_export_signed_post(body),
            ("player_import_signed", "POST") => routes::player::handle_import_signed_post(body),

            // Cross-device sync routes (Phase E)
            ("player_sync_sv", "GET") => routes::player::handle_sync_sv_get(query),
            ("player_sync_diff", "POST") => routes::player::handle_sync_diff_post(body),
            ("player_sync_apply", "POST") => routes::player::handle_sync_apply_post(body),
            ("player_sync_auth", "POST") => routes::player::handle_sync_auth_post(body),
            ("player_sync_verify", "POST") => routes::player::handle_sync_verify_post(body),

            // Between the engine and what holds it
            ("start", "POST") => store::handle_start_post(body),
            ("page", "POST") => page::handle_page_post(body),
            ("wire", "POST") => wire::handle_wire_post(body),
            ("handover", "POST") => page::handle_handover_post(body),

            _ => method_not_allowed(),
        },
        Err(_) => not_found(),
    }
}

fn not_found() -> String {
    r#"<span class="text-kip-red">404 — route not found</span>"#.to_string()
}

fn method_not_allowed() -> String {
    r#"<span class="text-kip-red">405 — method not allowed</span>"#.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_type_matchup() {
        let html = handle_request("GET", "/api/type-matchup", "?atk[]=Entropic&def[]=Cenozoic", "");
        assert!(html.contains("3"));
    }

    #[test]
    fn returns_404_for_unknown_route() {
        let html = handle_request("GET", "/api/nonexistent", "", "");
        assert!(html.contains("404"));
    }

    #[test]
    fn returns_405_for_wrong_method() {
        let html = handle_request("POST", "/api/card/panel", "", "");
        assert!(html.contains("405"));
    }

    #[test]
    fn routes_type_matchup_pick() {
        game::matchup::reset();
        let html = handle_request("POST", "/api/type-matchup", "", "pick=atk&type=Entropic");
        assert!(html.contains("Attack Die Modifier"));
        let boxes = handle_request("GET", "/api/type-matchup", "?part=archetypes", "");
        assert_eq!(boxes.matches(" checked").count(), 1);
        game::matchup::reset();
    }

    #[test]
    fn routes_qr_status() {
        let html = handle_request("GET", "/api/qr/status", "?action=open&privacy=false", "");
        assert!(html.contains("Privacy Notice"));
    }

    #[test]
    fn routes_qr_found() {
        let html = handle_request("GET", "/api/qr/found", "?url=kpks.us%2Ftest", "");
        assert!(html.contains("window.location.assign('/test')"));
    }

    #[test]
    fn routes_cards() {
        routes::cards::reset();
        // One column: a first batch of three tiles.
        let html = handle_request("GET", "/api/cards", "?columns=1", "");
        assert_eq!(html.matches("<a href=").count(), 3);
        // The marker's request for more, then the grid again.
        assert_eq!(handle_request("POST", "/api/cards", "", "shown=6"), "");
        let html = handle_request("GET", "/api/cards", "?columns=1", "");
        assert_eq!(html.matches("<a href=").count(), 6);
        routes::cards::reset();
    }

    #[test]
    fn routes_card_panel() {
        game::player_doc::init_player_doc();
        let html = handle_request("GET", "/api/card/panel", "?card=brox_the_defiant", "");
        assert!(html.contains("Brox The Defiant"));
        assert!(html.contains("Crushing Hope"));
        let missing = handle_request("GET", "/api/card/panel", "", "");
        assert!(missing.contains("Missing card parameter"));
        game::player_doc::init_player_doc();
    }

    #[test]
    fn routes_game_damage_get() {
        game::player_doc::init_player_doc();
        let html = handle_request("GET", "/api/game/damage", "?card=brox_the_defiant", "");
        assert!(html.contains("Crushing Hope"));
        game::player_doc::init_player_doc();
    }

    #[test]
    fn routes_game_damage_post() {
        game::player_doc::init_player_doc();
        let html = handle_request("POST", "/api/game/damage", "", "card=brox_the_defiant&slot=1");
        assert!(html.contains(r#"data-slot="1" aria-pressed="true""#));
        game::player_doc::init_player_doc();
    }

    #[test]
    fn routes_game_turns_get() {
        let html = handle_request("GET", "/api/game/turns", "", "");
        assert!(html.contains("New Timer"));
    }

    #[test]
    fn routes_game_turns_form_get() {
        let html = handle_request("GET", "/api/game/turns/form", "", "");
        assert!(html.contains("New Timer"));
        assert!(html.contains(r#"data-engine-post="/api/game/turns""#));
        // The list of timers is another view, at another address.
        assert_eq!(handle_request("GET", "/api/game/turns", "?display=alarms", ""), "");
    }

    #[test]
    fn routes_game_turns_post() {
        game::player_doc::init_player_doc();
        let html = handle_request("POST", "/api/game/turns", "", "action=add&turns=5");
        assert!(html.contains("Turns to Alarm"));
        game::player_doc::init_player_doc();
    }

    fn fresh() {
        game::player_doc::init_player_doc();
        game::room::reset_room();
        game::crdt::reset_doc();
        game::crdt::take_outbox();
        game::connection::reset();
        store::reset();
        page::reset();
    }

    fn handed() -> serde_json::Value {
        serde_json::from_str(&handle_request("POST", "/api/handover", "", "")).unwrap()
    }

    /// The worker reporting what the room's socket did.
    fn socket(name: &str, event: &str, data: &str) {
        let report = serde_json::json!({ "socket": name, "event": event, "data": data });
        assert_eq!(handle_request("POST", "/api/wire", "", &report.to_string()), "ok");
    }

    /// A page load through the route table: the page hands over what is
    /// stored, and the engine answers with where it stands.
    #[test]
    fn routes_start_and_page() {
        fresh();
        assert_eq!(handle_request("POST", "/api/start", "", "{}"), "ok");
        assert_eq!(handed()["room"], false);

        assert_eq!(handle_request("POST", "/api/page", "", r#"{"event":"shown","alive":[]}"#), "ok");
        assert_eq!(handle_request("POST", "/api/handover", "", ""), "{}");
        fresh();
    }

    /// A room through the route table, as the page and the worker drive it:
    /// the dialog's form is a post, the worker opens the socket the engine
    /// asks for and reports what it does, and the views are drawn by gets.
    #[test]
    fn routes_room_dialog_pill_and_wire() {
        fresh();
        handle_request("POST", "/api/start", "", "{}");
        assert!(handle_request("GET", "/api/room/status", "", "").contains("Create Room"));
        assert_eq!(handle_request("GET", "/api/room/pill", "", ""), "");

        assert_eq!(handle_request("POST", "/api/room/status", "", "action=create&name=Test+Table"), "");
        assert!(handle_request("GET", "/api/room/status", "", "").contains("Connecting"));
        let first = handed();
        let name = first["wire"][0]["open"].as_str().unwrap().to_string();
        assert!(first["wire"][0]["url"].as_str().unwrap().contains("action=create&name=Test+Table&player="));

        socket(&name, "open", "");
        socket(&name, "message", r#"{"type":"room_created","code":"ABCD","name":"Test Table","v":2}"#);
        let entered = handed();
        assert_eq!(entered["room"], true);
        assert!(entered["save"].as_object().unwrap().contains_key(store::ROOM));
        let status = handle_request("GET", "/api/room/status", "", "");
        assert!(status.contains("Waiting for peer"));
        assert!(status.contains("Code: <strong>ABCD</strong>"));

        socket(&name, "message", r#"{"type":"peer_joined","peerId":"bbbbbbbbbbbbbbbbbbbbbbbb"}"#);
        let greeted = handed();
        assert!(greeted["wire"].to_string().contains("hello"));
        assert!(handle_request("GET", "/api/room/status", "", "").contains("Connected"));
        socket(
            &name,
            "message",
            r#"{"type":"relay","data":{"type":"hello","proto":2,"playerId":"bbbbbbbbbbbbbbbbbbbbbbbb"}}"#,
        );
        assert!(handle_request("GET", "/api/room/pill", "", "").contains(r#"data-tone="ok""#));

        assert_eq!(handle_request("POST", "/api/room/status", "", "action=leave"), "");
        let left = handed();
        assert_eq!(left["room"], false);
        assert!(left["wire"].to_string().contains(r#"{\"type\":\"leave\"}"#), "{}", left["wire"]);
        assert_eq!(handle_request("GET", "/api/room/pill", "", ""), "");
        fresh();
    }

    /// A round of fists through the route table, as the page drives it: a
    /// tap is a post, the dialog is drawn again by a get, and what the engine
    /// wants done is taken by the worker after each post.
    #[test]
    fn routes_room_fists_and_handover() {
        fresh();
        handle_request("POST", "/api/start", "", "{}");
        routes::room::enter("ABCD", "Test", "me", true);
        handed();

        let fists = |body: &str| {
            assert_eq!(handle_request("POST", "/api/room/fists", "", body), "");
            handle_request("GET", "/api/room/fists", "?card=brox_the_defiant", "")
        };
        assert!(fists("action=choose&card=brox_the_defiant&role=attacking").contains("Lock In Choice"));
        fists("action=choose&card=brox_the_defiant&keal=1");
        assert!(fists("action=lock&card=brox_the_defiant").contains("Waiting for opponent"));

        let handed = handed();
        assert_eq!(handed["redraw"], serde_json::json!(["/api/card/panel"]));
        // The round is in the room's document, and the play it counted is in
        // the player's own state: both are to be stored.
        let saved = handed["save"].as_object().unwrap();
        assert!(saved.contains_key(store::ROOM_DOC));
        assert!(saved.contains_key(store::PLAYER_DOC));
        // The connection is down, so the round is not sent now; the exchange
        // of state on connecting makes up for it.
        assert!(handed.get("wire").is_none());
        fresh();
    }

    /// The requests the page's own scripts used to make are gone: saving and
    /// restoring, the room's life, the exchange of state.
    #[test]
    fn routes_the_page_used_to_drive_are_gone() {
        for (method, path) in [
            ("GET", "/api/player/state"),
            ("POST", "/api/player/restore"),
            ("POST", "/api/room/resume"),
            ("POST", "/api/room/create"),
            ("POST", "/api/room/join"),
            ("POST", "/api/room/peer"),
            ("POST", "/api/room/connected"),
            ("POST", "/api/room/disconnect"),
            ("POST", "/api/room/peer_left"),
            ("GET", "/api/room/state"),
            ("GET", "/api/room/yrs/sv"),
            ("POST", "/api/room/yrs/diff"),
            ("POST", "/api/room/yrs/apply"),
            ("GET", "/api/room/yrs/state"),
            ("POST", "/api/room/yrs/restore"),
            ("POST", "/api/room/outbox"),
            ("POST", "/api/room/fists/poll"),
        ] {
            assert!(handle_request(method, path, "", "").contains("404"), "{}", path);
        }
    }
}
