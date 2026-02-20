//! Kipukas in-browser WASM server.
//!
//! Exports `handle_request(method, path, query)` for the Service Worker
//! bridge to call. Uses `matchit` for URL routing — the same router
//! engine that powers Axum.

use wasm_bindgen::prelude::*;

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
    router.insert("/api/qr/status", "qr_status").ok();
    router.insert("/api/qr/found", "qr_found").ok();

    // Suppress unused-variable warning for body until Phase 3 POST routes use it
    let _ = body;

    match router.at(path) {
        Ok(matched) => match *matched.value {
            "type_matchup" if method == "GET" => routes::type_matchup::handle(query),
            "qr_status" if method == "GET" => routes::qr::handle_status(query),
            "qr_found" if method == "GET" => routes::qr::handle_found(query),
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
}