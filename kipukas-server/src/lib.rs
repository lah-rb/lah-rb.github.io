//! Kipukas in-browser WASM server.
//!
//! Exports `handle_request(method, path, query, body)` for the Service Worker
//! bridge to call. Uses `matchit` for URL routing — the same router
//! engine that powers Axum.
//!
//! Phase 3b: Added `/api/game/*` routes for damage tracking, turn tracking,
//! and game state persistence. POST method support enabled.

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
