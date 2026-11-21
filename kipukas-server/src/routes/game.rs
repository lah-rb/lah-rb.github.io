//! `/api/game/*` routes — game state management for damage tracking and
//! turn tracking.
//!
//! All game state is stored in PLAYER_DOC (yrs CRDT document).

use crate::clock;
use crate::game::{copies, damage, player_doc, room, turns};
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
///   - card={slug}&action=effects&by={1|-1} → one more, or one less, from
///     card effects in play towards controlling a species
///   - card={slug}&action=clear  → clear damage for one card
///   - action=new_game           → New Game: clear ALL damage and affinity, and the
///     player's own timers (a room's timers stay)
///
/// Returns updated damage tracker HTML for the affected card. The page does
/// not need it: after a change it re-renders each affected view from its own
/// route (`assets/js/kipukas-view.js`). It is returned for direct callers.
pub fn handle_damage_post(body: &str) -> String {
    let params = parse_form_body(body);
    let action = get_param(&params, "action").unwrap_or("");
    let card = get_param(&params, "card").unwrap_or("");
    // `card` is a card's name, or the key of a further copy of it in play
    // (`game::copies`). One that is of no copy in play changes nothing.
    if !card.is_empty() && !copies::in_play(card) {
        return r#"<span class="text-kip-red">No such copy of this card is in play</span>"#.to_string();
    }

    match action {
        "new_game" => {
            damage::clear_all();
            player_doc::clear_affinity();
            // The player's own timers go, and are remembered as removed (a
            // copy the other player still holds does not bring them back to
            // the next room). A room's timers are both players': they stay.
            if !room::in_room() {
                player_doc::remove_all_alarms(clock::now_ms());
            }
            // If a card slug is provided, return re-rendered tracker for that card.
            // This eliminates the need for a chained GET request from the toolbar JS.
            if !card.is_empty() {
                damage::render_damage_tracker(card)
            } else {
                r#"<div class="w-full text-center"><span class="text-emerald-600">New game started. All state reset.</span></div>"#.to_string()
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
        // One more, or one less, from card effects in play towards
        // controlling a species: `by=1` or `by=-1`.
        "effects" if !card.is_empty() => {
            let by: i32 = get_param(&params, "by").and_then(|by| by.parse().ok()).unwrap_or(0);
            damage::adjust_effects(card, by.clamp(-1, 1));
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

/// Handle GET /api/game/turns/form
/// Returns the form that makes a timer. Outside a room it is an engine-owned
/// view of this route; a route of its own keeps a tap in the list of timers
/// (a view of `/api/game/turns`) from drawing the form again.
pub fn handle_turns_form_get(_query: &str) -> String {
    turns::render_turn_panel()
}

/// Handle GET /api/game/turns
/// Returns the alarm list if `?display=alarms` is specified, which is what
/// the list's view asks for. Without it, the timer creation form: the room's
/// timer dialog loads it this way.
/// Multiplayer mode is auto-detected via room::in_room().
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
///
/// In a room these act on the room's shared timers and the change is queued
/// for the other player; otherwise on the player's own (`game::turns`).
///
/// Body params:
///   - action=add&turns={n}         → add new alarm
///   - action=tick                   → decrement all alarms
///   - action=remove&index={n}       → remove the player's own alarm at index
///   - action=remove&id={id}         → remove the room's alarm of that id
///   - action=toggle_visibility      → toggle alarm panel visibility
///   - action=expire                 → drop timers that ran out and have faded
///     (the list asks for this by itself, see `turns::render_alarm_list_at`)
///
/// Returns updated alarm list HTML.
pub fn handle_turns_post(body: &str) -> String {
    let params = parse_form_body(body);
    let action = get_param(&params, "action").unwrap_or("");
    // Each change to a timer is dated: the later of two made apart wins.
    let now = clock::now_ms();

    match action {
        "add" => {
            let t: i32 = get_param(&params, "turns")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);
            let name = get_param(&params, "name").unwrap_or("");
            let color_set = get_param(&params, "color_set").unwrap_or("red");
            turns::add(now, t, name, color_set);
        }
        "tick" => {
            turns::advance(now);
        }
        "remove" => {
            let place = get_param(&params, "index").and_then(|s| s.parse().ok());
            turns::remove(now, get_param(&params, "id"), place);
        }
        "toggle_visibility" => {
            turns::toggle_alarms_visibility();
        }
        "expire" => {
            turns::expire(now);
        }
        _ => {}
    }

    turns::render_alarm_list()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// When the timers in these tests are made and changed, where the test
    /// does not care when.
    const NOW: f64 = 1_800_000_000_000.0;
    use crate::game::crdt;

    fn reset_state() {
        crate::game::player_doc::init_player_doc();
        room::reset_room();
    }

    #[test]
    fn the_card_effects_buttons_add_and_take_a_point_and_new_game_clears_them() {
        reset_state();
        let card = "knightsoul_of_binding_time";
        let html = handle_damage_post(&format!("card={}&action=effects&by=1", card));
        assert!(html.contains(r#"data-stack="effects">1</span>"#), "{}", html);
        // Whatever is asked for, a tap is one point.
        let html = handle_damage_post(&format!("card={}&action=effects&by=7", card));
        assert!(html.contains(r#"data-stack="effects">2</span>"#));
        let html = handle_damage_post(&format!("card={}&action=effects&by=-1", card));
        assert!(html.contains(r#"data-stack="effects">1</span>"#));
        let html = handle_damage_post(&format!("card={}&action=effects", card));
        assert!(html.contains(r#"data-stack="effects">1</span>"#), "no step asked for, none taken");
        // A character has no stack to add to.
        handle_damage_post("card=brox_the_defiant&action=effects&by=1");
        assert_eq!(crate::game::player_doc::get_effects("brox_the_defiant"), 0);

        let html = handle_damage_post(&format!("action=new_game&card={}", card));
        assert!(html.contains(r#"data-stack="effects">0</span>"#));
        reset_state();
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
        // Slot 1 is now marked, and the others are not.
        assert!(html.contains(r#"data-slot="1" aria-pressed="true""#));
        assert!(html.contains(r#"data-slot="2" aria-pressed="false""#));
        assert_eq!(html.matches(r#"aria-pressed="true""#).count(), 1);
        // The tracker read back from its own route shows the same thing.
        assert_eq!(handle_damage_get("?card=brox_the_defiant"), html);
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
        assert!(html.contains(r#"data-wasted aria-pressed="true""#));
        reset_state();
    }

    #[test]
    fn damage_post_new_game() {
        reset_state();
        handle_damage_post("card=brox_the_defiant&slot=1");
        player_doc::declare_affinity("Brutal", "2026-02-25").unwrap();
        player_doc::add_alarm(NOW, 5, "timer", "red");

        let html = handle_damage_post("action=new_game");
        assert!(html.contains("New game started"));

        // Verify everything was cleared
        assert!(!player_doc::has_card_state("brox_the_defiant"));
        assert!(player_doc::get_active_affinity().is_none());
        assert!(player_doc::get_alarms().is_empty());
        reset_state();
    }

    #[test]
    fn a_new_game_alone_remembers_the_players_timers_as_removed() {
        reset_state();
        player_doc::add_alarm_with_id("mine", 5, "timer", "red", NOW);
        handle_damage_post("action=new_game");
        assert!(player_doc::get_alarms().is_empty());
        let removed = player_doc::retired();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].0, "mine");
        assert!(removed[0].1 > NOW, "dated after the timer's last change");
        reset_state();
    }

    #[test]
    fn a_new_game_in_a_room_leaves_the_rooms_timers() {
        reset_state();
        crdt::reset_doc();
        room::with_room_mut(|r| r.room_code = "ABCD".to_string());
        handle_turns_post("action=add&turns=4&name=shared&color_set=red");
        handle_damage_post("card=brox_the_defiant&slot=1");

        handle_damage_post("action=new_game");
        assert!(!player_doc::has_card_state("brox_the_defiant"));
        assert_eq!(crdt::get_alarms().len(), 1, "the room's timer stays");
        assert_eq!(player_doc::get_alarms().len(), 1, "and so does the player's copy of it");
        assert!(player_doc::retired().is_empty(), "nothing is remembered as removed");
        crdt::reset_doc();
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
        player_doc::add_alarm(NOW, 5, "", "red");
        handle_turns_post("action=toggle_visibility");
        let html = turns::render_alarm_list();
        assert!(html.contains("hidden"));
        reset_state();
    }

    /// The player's state survives a page load: what the engine hands over
    /// to be stored is what the next page starts it with.
    #[test]
    fn player_state_roundtrip() {
        reset_state();
        crate::store::handle_start_post("{}");
        // Set some state
        crate::game::player_doc::add_alarm(NOW, 5, "test", "green");
        crate::game::player_doc::ensure_card_state("test_card", 3);
        crate::game::player_doc::toggle_slot("test_card", 1);

        // What the engine asks to be stored
        let saves = crate::store::take_saves();
        let b64 = saves[crate::store::PLAYER_DOC].clone().unwrap();
        assert!(!b64.is_empty());

        // A new page: the engine starts empty and is handed what was stored
        crate::game::player_doc::init_player_doc();
        let stored = serde_json::json!({ crate::store::PLAYER_DOC: b64 });
        assert_eq!(crate::store::handle_start_post(&stored.to_string()), "ok");

        // Verify state was restored
        assert!(crate::game::player_doc::get_slot("test_card", 1));
        let alarms = crate::game::player_doc::get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].name, "test");
        crate::store::reset();
        reset_state();
    }
}
