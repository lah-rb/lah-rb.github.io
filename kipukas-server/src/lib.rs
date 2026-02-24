//! Kipukas in-browser WASM server.
//!
//! Exports `handle_request(method, path, query, body)` for the Service Worker
//! bridge to call. Uses `matchit` for URL routing — the same router
//! engine that powers Axum.
//!
//! Phase 3b: Added `/api/game/*` routes for damage tracking, turn tracking,
//! and game state persistence. POST method support enabled.
//! Phase 4: Added `/api/room/*` routes for multiplayer room management
//! and fists combat. Room state is global (shared via WebRTC).

use wasm_bindgen::prelude::*;

pub mod cards_generated;
pub mod game;
pub mod routes;
pub mod typing;

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
    router.insert("/api/qr/status", "qr_status").ok();
    router.insert("/api/qr/found", "qr_found").ok();

    // Phase 3b: Game state routes
    router.insert("/api/game/damage", "game_damage").ok();
    router.insert("/api/game/turns", "game_turns").ok();
    router.insert("/api/game/state", "game_state").ok();
    router.insert("/api/game/persist", "game_persist").ok();
    router.insert("/api/game/import", "game_import").ok();

    // Phase 4: Room/multiplayer routes
    router.insert("/api/room/status", "room_status").ok();
    router.insert("/api/room/create", "room_create").ok();
    router.insert("/api/room/join", "room_join").ok();
    router.insert("/api/room/connected", "room_connected").ok();
    router.insert("/api/room/disconnect", "room_disconnect").ok();
    router.insert("/api/room/peer_left", "room_peer_left").ok();
    router.insert("/api/room/fists", "room_fists").ok();
    router.insert("/api/room/fists/sync", "room_fists_sync").ok();
    router.insert("/api/room/fists/poll", "room_fists_poll").ok();
    router.insert("/api/room/fists/reset", "room_fists_reset").ok();
    router.insert("/api/room/fists/outcome", "room_fists_outcome").ok();
    router.insert("/api/room/fists/final", "room_fists_final").ok();
    router.insert("/api/room/fists/final/sync", "room_fists_final_sync").ok();
    router.insert("/api/room/turns", "room_turns").ok();
    router.insert("/api/room/turns/sync", "room_turns_sync").ok();
    router.insert("/api/room/turns/export", "room_turns_export").ok();
    router.insert("/api/room/state", "room_state").ok();

    // Phase 5: Yrs CRDT sync routes
    router.insert("/api/room/yrs/sv", "yrs_sv").ok();
    router.insert("/api/room/yrs/diff", "yrs_diff").ok();
    router.insert("/api/room/yrs/apply", "yrs_apply").ok();
    router.insert("/api/room/yrs/alarm/add", "yrs_alarm_add").ok();
    router.insert("/api/room/yrs/alarm/tick", "yrs_alarm_tick").ok();
    router.insert("/api/room/yrs/alarm/remove", "yrs_alarm_remove").ok();

    match router.at(path) {
        Ok(matched) => match (*matched.value, method) {
            // GET routes
            ("type_matchup", "GET") => routes::type_matchup::handle(query),
            ("cards", "GET") => routes::cards::handle(query),
            ("qr_status", "GET") => routes::qr::handle_status(query),
            ("qr_found", "GET") => routes::qr::handle_found(query),
            ("game_damage", "GET") => routes::game::handle_damage_get(query),
            ("game_turns", "GET") => routes::game::handle_turns_get(query),
            ("game_state", "GET") => routes::game::handle_state_get(query),

            // POST routes (Phase 3b)
            ("game_damage", "POST") => routes::game::handle_damage_post(body),
            ("game_turns", "POST") => routes::game::handle_turns_post(body),
            ("game_persist", "POST") => routes::game::handle_persist_post(body),
            ("game_import", "POST") => routes::game::handle_import_post(body),

            // Phase 4: Room/multiplayer routes
            ("room_status", "GET") => routes::room::handle_status_get(query),
            ("room_fists", "GET") => routes::room::handle_fists_get(query),
            ("room_fists_poll", "GET") => routes::room::handle_fists_poll_get(query),
            ("room_turns", "GET") => routes::room::handle_room_turns_get(query),
            ("room_turns_export", "GET") => routes::room::handle_room_turns_export_get(query),
            ("room_turns_sync", "POST") => routes::room::handle_room_turns_sync_post(body),
            ("room_state", "GET") => routes::room::handle_room_state_get(query),
            ("room_create", "POST") => routes::room::handle_create_post(body),
            ("room_join", "POST") => routes::room::handle_join_post(body),
            ("room_connected", "POST") => routes::room::handle_connected_post(body),
            ("room_disconnect", "POST") => routes::room::handle_disconnect_post(body),
            ("room_peer_left", "POST") => routes::room::handle_peer_left_post(body),
            ("room_fists", "POST") => routes::room::handle_fists_post(body),
            ("room_fists_sync", "POST") => routes::room::handle_fists_sync_post(body),
            ("room_fists_reset", "POST") => routes::room::handle_fists_reset_post(body),
            ("room_fists_outcome", "POST") => routes::room::handle_fists_outcome_post(body),
            ("room_fists_final", "POST") => routes::room::handle_final_blows_post(body),
            ("room_fists_final_sync", "POST") => routes::room::handle_final_blows_sync_post(body),

            // Phase 5: Yrs CRDT sync routes
            ("yrs_sv", "GET") => routes::room::handle_yrs_sv_get(query),
            ("yrs_diff", "POST") => routes::room::handle_yrs_diff_post(body),
            ("yrs_apply", "POST") => routes::room::handle_yrs_apply_post(body),
            ("yrs_alarm_add", "POST") => routes::room::handle_yrs_alarm_add_post(body),
            ("yrs_alarm_tick", "POST") => routes::room::handle_yrs_alarm_tick_post(body),
            ("yrs_alarm_remove", "POST") => routes::room::handle_yrs_alarm_remove_post(body),

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
        let html = handle_request("POST", "/api/type-matchup", "", "");
        assert!(html.contains("405"));
    }

    #[test]
    fn routes_qr_status() {
        let html = handle_request("GET", "/api/qr/status", "?action=open&privacy=false", "");
        assert!(html.contains("Privacy Notice"));
    }

    #[test]
    fn routes_qr_found() {
        let html = handle_request("GET", "/api/qr/found", "?url=kpks.us%2Ftest", "");
        assert!(html.contains("kipukas.cards/test"));
    }

    #[test]
    fn routes_cards() {
        let html = handle_request("GET", "/api/cards", "?page=0&per=4&all=true", "");
        assert!(html.contains("<a href="));
        let card_count = html.matches("<a href=").count();
        assert_eq!(card_count, 4);
    }

    // Phase 3b route tests

    #[test]
    fn routes_game_damage_get() {
        game::state::replace_state(game::state::GameState::default());
        let html = handle_request("GET", "/api/game/damage", "?card=brox_the_defiant", "");
        assert!(html.contains("Crushing Hope"));
        game::state::replace_state(game::state::GameState::default());
    }

    #[test]
    fn routes_game_damage_post() {
        game::state::replace_state(game::state::GameState::default());
        let html = handle_request("POST", "/api/game/damage", "", "card=brox_the_defiant&slot=1");
        assert!(html.contains("checked"));
        game::state::replace_state(game::state::GameState::default());
    }

    #[test]
    fn routes_game_turns_get() {
        let html = handle_request("GET", "/api/game/turns", "", "");
        assert!(html.contains("New Timer"));
    }

    #[test]
    fn routes_game_turns_post() {
        game::state::replace_state(game::state::GameState::default());
        let html = handle_request("POST", "/api/game/turns", "", "action=add&turns=5");
        assert!(html.contains("Turns to Alarm"));
        game::state::replace_state(game::state::GameState::default());
    }

    #[test]
    fn routes_game_state_get() {
        game::state::replace_state(game::state::GameState::default());
        let json = handle_request("GET", "/api/game/state", "", "");
        assert!(json.contains("cards"));
        assert!(json.contains("alarms"));
        game::state::replace_state(game::state::GameState::default());
    }

    #[test]
    fn routes_game_persist_post() {
        game::state::replace_state(game::state::GameState::default());
        let html = handle_request("POST", "/api/game/persist", "", "");
        assert!(html.contains("localStorage.setItem"));
        game::state::replace_state(game::state::GameState::default());
    }

    #[test]
    fn routes_game_import_post() {
        game::state::replace_state(game::state::GameState::default());
        let html = handle_request(
            "POST",
            "/api/game/import",
            "",
            r#"{"cards":{},"alarms":[],"show_alarms":true}"#,
        );
        assert!(html.contains("successfully"));
        game::state::replace_state(game::state::GameState::default());
    }

    #[test]
    fn game_damage_get_returns_405_for_post_on_state() {
        let html = handle_request("POST", "/api/game/state", "", "");
        assert!(html.contains("405"));
    }
}
