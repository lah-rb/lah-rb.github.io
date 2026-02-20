//! `/api/game/*` routes — game state management for damage tracking,
//! turn tracking, and state persistence/import.
//!
//! Phase 3b: Single-player state management via HTMX + WASM.
//! Phase 4 prep: `/api/game/state` returns JSON for WebRTC sync.

use crate::game::{damage, turns};
use crate::game::state;

/// Parse URL-encoded form body into key-value pairs.
/// Handles `key=value&key2=value2` format (from HTMX POST bodies).
fn parse_form_body(body: &str) -> Vec<(String, String)> {
    if body.is_empty() {
        return Vec::new();
    }
    body.split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?;
            let val = parts.next().unwrap_or("");
            Some((
                percent_decode(key),
                percent_decode(val),
            ))
        })
        .collect()
}

/// Percent-decode a URL-encoded value.
fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(b'0');
            let lo = chars.next().unwrap_or(b'0');
            let hex = [hi, lo];
            if let Ok(s) = core::str::from_utf8(&hex) {
                if let Ok(val) = u8::from_str_radix(s, 16) {
                    result.push(val as char);
                    continue;
                }
            }
            result.push('%');
            result.push(hi as char);
            result.push(lo as char);
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
}

/// Parse a query string into key-value pairs.
fn parse_query(query: &str) -> Vec<(String, String)> {
    let q = query.strip_prefix('?').unwrap_or(query);
    parse_form_body(q)
}

/// Helper to get a value by key from a list of key-value pairs.
fn get_param<'a>(params: &'a [(String, String)], key: &str) -> Option<&'a str> {
    params.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

// ── GET /api/game/damage ───────────────────────────────────────────

/// Handle GET /api/game/damage?card={slug}
/// Returns the keal damage tracker HTML for the specified card.
pub fn handle_damage_get(query: &str) -> String {
    let params = parse_query(query);
    let slug = match get_param(&params, "card") {
        Some(s) if !s.is_empty() => s,
        _ => return r#"<span class="text-kip-red">Missing card parameter</span>"#.to_string(),
    };
    damage::render_damage_tracker(slug)
}

// ── POST /api/game/damage ──────────────────────────────────────────

/// Handle POST /api/game/damage
/// Body params:
///   - card={slug}&slot={n}     → toggle a damage slot
///   - card={slug}&action=wasted → toggle wasted state
///   - card={slug}&action=clear  → clear damage for one card
///   - action=clear_all          → clear ALL card damage
///
/// Returns updated damage tracker HTML for the affected card.
pub fn handle_damage_post(body: &str) -> String {
    let params = parse_form_body(body);
    let action = get_param(&params, "action").unwrap_or("");
    let card = get_param(&params, "card").unwrap_or("");

    match action {
        "clear_all" => {
            damage::clear_all();
            // Return a centered confirmation message as fallback — the toolbar JS
            // chains a GET to immediately re-render the damage tracker after this.
            r#"<div class="w-full text-center"><span class="text-emerald-600">All damage cleared. Please reload the page.</span></div>"#.to_string()
        }
        "clear" if !card.is_empty() => {
            damage::clear_card(card);
            damage::render_damage_tracker(card)
        }
        "wasted" if !card.is_empty() => {
            damage::toggle_wasted(card);
            damage::render_damage_tracker(card)
        }
        _ => {
            // Default: toggle a specific slot
            if card.is_empty() {
                return r#"<span class="text-kip-red">Missing card parameter</span>"#.to_string();
            }
            let slot: u8 = get_param(&params, "slot")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if slot == 0 {
                return r#"<span class="text-kip-red">Missing or invalid slot parameter</span>"#
                    .to_string();
            }
            damage::toggle_slot(card, slot);
            damage::render_damage_tracker(card)
        }
    }
}

// ── GET /api/game/turns ────────────────────────────────────────────

/// Handle GET /api/game/turns
/// Returns the turn tracker panel HTML (timer creation form),
/// or the alarm list if `?display=alarms` is specified.
pub fn handle_turns_get(query: &str) -> String {
    let params = parse_query(query);
    let display = get_param(&params, "display").unwrap_or("");
    if display == "alarms" {
        turns::render_alarm_list()
    } else {
        turns::render_turn_panel()
    }
}

// ── POST /api/game/turns ───────────────────────────────────────────

/// Handle POST /api/game/turns
/// Body params:
///   - action=add&turns={n}         → add new alarm
///   - action=tick                   → decrement all alarms
///   - action=remove&index={n}       → remove alarm at index
///   - action=toggle_visibility      → toggle alarm panel visibility
///
/// Returns updated alarm list HTML.
pub fn handle_turns_post(body: &str) -> String {
    let params = parse_form_body(body);
    let action = get_param(&params, "action").unwrap_or("");

    match action {
        "add" => {
            let t: i32 = get_param(&params, "turns")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);
            let clamped = t.clamp(1, 99);
            turns::add_alarm(clamped);
        }
        "tick" => {
            turns::tick_alarms();
        }
        "remove" => {
            let idx: usize = get_param(&params, "index")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            turns::remove_alarm(idx);
        }
        "toggle_visibility" => {
            turns::toggle_alarms_visibility();
        }
        _ => {}
    }

    turns::render_alarm_list()
}

// ── GET /api/game/state ────────────────────────────────────────────

/// Handle GET /api/game/state
/// Returns the full game state as JSON (for multiplayer prep / debugging).
pub fn handle_state_get(_query: &str) -> String {
    state::export_state_json()
}

// ── POST /api/game/persist ─────────────────────────────────────────

/// Handle POST /api/game/persist
/// Serializes current game state and returns a <script> tag that
/// writes it to localStorage.
pub fn handle_persist_post(_body: &str) -> String {
    let json = state::export_state_json();
    // Escape for embedding in a JS string literal
    let escaped = json.replace('\\', "\\\\").replace('\'', "\\'");
    format!(
        r#"<script>localStorage.setItem('kipukas_game_state', '{}'); console.log('[kipukas] Game state persisted to localStorage');</script>"#,
        escaped
    )
}

// ── POST /api/game/import ──────────────────────────────────────────

/// Handle POST /api/game/import
/// Accepts JSON body and imports it as the game state.
pub fn handle_import_post(body: &str) -> String {
    match state::import_state_json(body) {
        Ok(()) => {
            r#"<span class="text-emerald-600">Game state imported successfully</span>"#.to_string()
        }
        Err(e) => {
            format!(r#"<span class="text-kip-red">Import failed: {}</span>"#, e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::state::{replace_state, GameState};

    fn reset_state() {
        replace_state(GameState::default());
    }

    #[test]
    fn parse_form_body_works() {
        let pairs = parse_form_body("card=brox&slot=2&action=toggle");
        assert_eq!(pairs.len(), 3);
        assert_eq!(get_param(&pairs, "card"), Some("brox"));
        assert_eq!(get_param(&pairs, "slot"), Some("2"));
    }

    #[test]
    fn parse_form_body_empty() {
        let pairs = parse_form_body("");
        assert!(pairs.is_empty());
    }

    #[test]
    fn damage_get_missing_card() {
        let html = handle_damage_get("?foo=bar");
        assert!(html.contains("Missing card parameter"));
    }

    #[test]
    fn damage_get_renders_tracker() {
        reset_state();
        let html = handle_damage_get("?card=brox_the_defiant");
        assert!(html.contains("Crushing Hope"));
        assert!(html.contains("checkbox"));
        reset_state();
    }

    #[test]
    fn damage_post_toggle_slot() {
        reset_state();
        let html = handle_damage_post("card=brox_the_defiant&slot=1");
        assert!(html.contains("checked")); // slot 1 should now be checked
        reset_state();
    }

    #[test]
    fn damage_post_toggle_wasted() {
        reset_state();
        // Check all slots first so Final Blows appears
        handle_damage_post("card=brox_the_defiant&slot=1");
        handle_damage_post("card=brox_the_defiant&slot=2");
        handle_damage_post("card=brox_the_defiant&slot=3");
        let html = handle_damage_post("card=brox_the_defiant&action=wasted");
        assert!(html.contains("Final Blows"));
        assert!(html.contains("checked")); // wasted should be checked
        reset_state();
    }

    #[test]
    fn damage_post_clear_all() {
        reset_state();
        handle_damage_post("card=brox_the_defiant&slot=1");
        let html = handle_damage_post("action=clear_all");
        assert!(html.contains("All damage cleared"));
        reset_state();
    }

    #[test]
    fn turns_post_add_and_tick() {
        reset_state();
        handle_turns_post("action=add&turns=3");
        let html = turns::render_alarm_list();
        assert!(html.contains("Turns to Alarm: <strong>3</strong>"));

        let html2 = handle_turns_post("action=tick");
        assert!(html2.contains("Turns to Alarm: <strong>2</strong>"));
        reset_state();
    }

    #[test]
    fn turns_post_toggle_visibility() {
        reset_state();
        turns::add_alarm(5);
        handle_turns_post("action=toggle_visibility");
        let html = turns::render_alarm_list();
        assert!(html.contains("hidden"));
        reset_state();
    }

    #[test]
    fn state_get_returns_json() {
        reset_state();
        let json = handle_state_get("");
        assert!(json.contains("cards"));
        assert!(json.contains("alarms"));
        reset_state();
    }

    #[test]
    fn persist_returns_script() {
        reset_state();
        let html = handle_persist_post("");
        assert!(html.contains("<script>"));
        assert!(html.contains("localStorage.setItem"));
        assert!(html.contains("kipukas_game_state"));
        reset_state();
    }

    #[test]
    fn import_valid_json() {
        reset_state();
        let html = handle_import_post(r#"{"cards":{},"alarms":[{"remaining":5}],"show_alarms":true}"#);
        assert!(html.contains("successfully"));
        crate::game::state::with_state(|s| {
            assert_eq!(s.alarms.len(), 1);
            assert_eq!(s.alarms[0].remaining, 5);
        });
        reset_state();
    }

    #[test]
    fn import_invalid_json() {
        let html = handle_import_post("not json");
        assert!(html.contains("Import failed"));
    }
}
