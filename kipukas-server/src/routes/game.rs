//! `/api/game/*` routes — game state management for damage tracking,
//! turn tracking, and player document persistence.
//!
//! All game state is stored in PLAYER_DOC (yrs CRDT document).

use crate::game::{damage, player_doc, turns};
use crate::routes::util::{get_param, parse_form_body, parse_query};

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
            // If a card slug is provided, return re-rendered tracker for that card.
            // This eliminates the need for a chained GET request from the toolbar JS.
            if !card.is_empty() {
                damage::render_damage_tracker(card)
            } else {
                r#"<div class="w-full text-center"><span class="text-emerald-600">All damage cleared.</span></div>"#.to_string()
            }
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
/// Multiplayer mode is auto-detected via room::is_peer_connected().
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
            let name = get_param(&params, "name").unwrap_or("");
            let color_set = get_param(&params, "color_set").unwrap_or("red");
            player_doc::add_alarm(clamped, name, color_set);
        }
        "tick" => {
            player_doc::tick_alarms();
        }
        "remove" => {
            let idx: usize = get_param(&params, "index")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            player_doc::remove_alarm(idx);
        }
        "toggle_visibility" => {
            turns::toggle_alarms_visibility();
        }
        _ => {}
    }

    turns::render_alarm_list()
}

// ── GET /api/player/state ──────────────────────────────────────────

/// Handle GET /api/player/state
/// Returns the full PLAYER_DOC as a base64 binary string for persistence.
/// Called by kipukas-api.js on PERSIST_STATE to save to localStorage.
pub fn handle_player_state_get(_query: &str) -> String {
    player_doc::encode_full_state()
}

// ── POST /api/player/restore ───────────────────────────────────────

/// Handle POST /api/player/restore
/// Restores the PLAYER_DOC from a base64 binary string.
/// Called by kipukas-api.js on page load to restore from localStorage.
pub fn handle_player_restore_post(body: &str) -> String {
    let params = parse_form_body(body);
    let state_b64 = get_param(&params, "state").unwrap_or(body.trim());
    match player_doc::restore_from_state(state_b64) {
        Ok(()) => "ok".to_string(),
        Err(e) => format!("error: {}", e),
    }
}

// ── GET /api/player/export ─────────────────────────────────────────

/// Handle GET /api/player/export
/// Returns a <script> tag that triggers a file download of the player doc
/// as a base64 text file.
pub fn handle_player_export_get(_query: &str) -> String {
    let state = player_doc::encode_full_state();
    format!(
        r#"<script>
(function() {{
  var b = new Blob(['{state}'], {{type: 'text/plain'}});
  var a = document.createElement('a');
  a.href = URL.createObjectURL(b);
  a.download = 'kipukas-player-data.txt';
  a.click();
  URL.revokeObjectURL(a.href);
  console.log('[kipukas] Player data exported');
}})();
</script>"#,
        state = state
    )
}

// ── POST /api/player/import ────────────────────────────────────────

/// Handle POST /api/player/import
/// Accepts a base64 state string and restores it as the PLAYER_DOC.
/// Used for importing a previously exported player data file.
pub fn handle_player_import_post(body: &str) -> String {
    let params = parse_form_body(body);
    let state_b64 = get_param(&params, "state").unwrap_or(body.trim());
    match player_doc::restore_from_state(state_b64) {
        Ok(()) => {
            r#"<span class="text-emerald-600">Player data imported successfully</span>"#
                .to_string()
        }
        Err(e) => {
            format!(
                r#"<span class="text-kip-red">Import failed: {}</span>"#,
                e
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::room;

    fn reset_state() {
        crate::game::player_doc::init_player_doc();
        room::reset_room();
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
        assert!(html.contains("damage-slot"));
        reset_state();
    }

    #[test]
    fn damage_post_toggle_slot() {
        reset_state();
        let html = handle_damage_post("card=brox_the_defiant&slot=1");
        assert!(html.contains("on: true")); // slot 1 Alpine state should be checked
        assert!(html.contains("bg-red-600")); // checked slot shows red via Tailwind class
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
        assert!(html.contains("bg-red-600")); // wasted indicator shows red via Tailwind class
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
        handle_turns_post("action=add&turns=3&name=test&color_set=green");
        let html = turns::render_alarm_list();
        assert!(html.contains("test")); // named alarm: "test — 3"
        assert!(html.contains("3"));

        let html2 = handle_turns_post("action=tick");
        assert!(html2.contains("test")); // named alarm: "test — 2"
        assert!(html2.contains("2"));
        reset_state();
    }

    #[test]
    fn turns_post_add_with_name_and_color() {
        reset_state();
        handle_turns_post("action=add&turns=5&name=Dragon+siege&color_set=blue");
        let html = turns::render_alarm_list();
        assert!(html.contains("Dragon siege"));
        assert!(html.contains("bg-blue-100"));
        reset_state();
    }

    #[test]
    fn turns_post_toggle_visibility() {
        reset_state();
        player_doc::add_alarm(5, "", "red");
        handle_turns_post("action=toggle_visibility");
        let html = turns::render_alarm_list();
        assert!(html.contains("hidden"));
        reset_state();
    }

    #[test]
    fn player_state_roundtrip() {
        reset_state();
        // Set some state
        crate::game::player_doc::add_alarm(5, "test", "green");
        crate::game::player_doc::ensure_card_state("test_card", 3);
        crate::game::player_doc::toggle_slot("test_card", 1);

        // Export
        let b64 = handle_player_state_get("");
        assert!(!b64.is_empty());

        // Reset and restore
        crate::game::player_doc::init_player_doc();
        let result = handle_player_restore_post(&b64);
        assert_eq!(result, "ok");

        // Verify state was restored
        assert!(crate::game::player_doc::get_slot("test_card", 1));
        let alarms = crate::game::player_doc::get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].name, "test");
        reset_state();
    }
}
