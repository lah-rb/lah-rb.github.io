//! `/api/player/*` routes — player-level features.
//!
//! ## Affinity tracking
//! Players declare affinity for one of the 15 archetypes once per day.
//! Each declaration increments the level for that archetype. The most
//! recently declared archetype is the "active" affinity, granting a +1
//! roll bonus on matching cards during fists combat.
//!
//! The panel is an engine-owned view (Pattern 13) of `/api/player/affinity`.
//! Each Declare button is an engine post; the view sends it and draws the
//! panel again. The engine dates the declaration by its own clock.

use crate::clock;
use crate::game::player_doc;
use crate::routes::util::{get_param, parse_form_body};

// ── GET /api/player/affinity ───────────────────────────────────────

/// Handle GET /api/player/affinity
/// Returns the full affinity panel HTML showing all 15 archetypes.
pub fn handle_affinity_get(_query: &str) -> String {
    render_affinity_panel()
}

// ── POST /api/player/affinity ──────────────────────────────────────

/// Handle POST /api/player/affinity
/// Body: archetype={name}
/// Declares affinity for the given archetype, dated today. Returns
/// re-rendered panel. `today={YYYY-MM-DD}` in the body names another day.
pub fn handle_affinity_post(body: &str) -> String {
    let params = parse_form_body(body);
    let archetype = get_param(&params, "archetype").unwrap_or("");
    let today = match get_param(&params, "today") {
        Some(day) if !day.is_empty() => day.to_string(),
        _ => clock::today(),
    };
    let today = today.as_str();

    if archetype.is_empty() {
        return r#"<span class="text-kip-red">Missing archetype parameter</span>"#.to_string();
    }

    match player_doc::declare_affinity(archetype, today) {
        Ok(_) => render_affinity_panel(),
        Err(e) => {
            // Re-render panel with error toast at top
            let mut html = String::with_capacity(4096);
            html.push_str(&format!(
                r#"<div class="text-center text-xs text-kip-red mb-2">{}</div>"#,
                e
            ));
            html.push_str(&render_affinity_panel_inner());
            html
        }
    }
}

// ── Panel rendering ────────────────────────────────────────────────

/// Render the full affinity panel (wrapper).
fn render_affinity_panel() -> String {
    let mut html = String::with_capacity(4096);
    html.push_str(r#"<div class="p-4 text-kip-drk-sienna">"#);
    html.push_str(r#"<p class="text-lg font-bold text-center mb-1">Archetypal Affinity</p>"#);
    html.push_str(
        r#"<p class="text-xs text-slate-500 text-center mb-3">Declare once per day to grow your bond</p>"#,
    );
    html.push_str(&render_affinity_panel_inner());
    html.push_str(r#"</div>"#);
    html
}

/// Render the inner panel content (archetype list).
fn render_affinity_panel_inner() -> String {
    let archetypes = player_doc::valid_archetypes();
    let active = player_doc::get_active_affinity();
    let has_affinity = active.is_some();

    let mut html = String::with_capacity(3072);

    // Active affinity highlight
    if let Some((ref active_name, _active_level)) = active {
        html.push_str(r#"<div class="bg-amber-100 border border-amber-300 rounded-lg p-2 mb-3 text-center">"#);
        html.push_str(&format!(
            r#"<p class="text-sm font-bold">♥ <span class="text-kip-red">{}</span></p>"#,
            active_name
        ));
        html.push_str(
            r#"<p class="text-xs text-slate-500">+1 roll bonus on matching cards</p>"#,
        );
        html.push_str(r#"<p class="text-xs text-slate-400 mt-1">Start a New Game to change affinity</p>"#);
        html.push_str(r#"</div>"#);
    }

    // Archetype grid
    html.push_str(r#"<div class="grid grid-cols-1 gap-1">"#);

    for &archetype in archetypes {
        let is_active = active
            .as_ref()
            .map(|(name, _)| name == archetype)
            .unwrap_or(false);

        // Row container
        let bg = if is_active {
            "bg-amber-50 border-amber-300"
        } else {
            "bg-white border-slate-200"
        };
        html.push_str(&format!(
            r#"<div class="flex items-center justify-between border rounded px-2 py-1 {}">"#,
            bg
        ));

        // Left: name
        html.push_str(r#"<div class="flex items-center gap-2">"#);
        if is_active {
            html.push_str(r#"<span class="text-amber-500 text-xs">♥</span>"#);
        }
        html.push_str(&format!(
            r#"<span class="text-sm font-medium">{}</span>"#,
            archetype
        ));
        html.push_str(r#"</div>"#);

        // Right: declare button or locked indicator
        html.push_str(r#"<div class="flex items-center gap-2">"#);
        if is_active {
            html.push_str(
                r#"<span class="text-xs text-amber-600 px-2 py-0.5 font-bold">♥ Active</span>"#,
            );
        } else if has_affinity {
            // Another archetype is active — this one is locked
            html.push_str(
                r#"<span class="text-xs text-slate-300 px-2 py-0.5">Locked</span>"#,
            );
        } else {
            // No affinity yet — show declare button
            html.push_str(&format!(
                r#"<button type="button" class="text-xs bg-kip-red text-amber-50 px-2 py-0.5 rounded hover:bg-red-700 active:bg-red-800" data-engine-post="/api/player/affinity" data-engine-body="archetype={}">Declare</button>"#,
                archetype
            ));
        }
        html.push_str(r#"</div>"#);

        html.push_str(r#"</div>"#); // close row
    }

    html.push_str(r#"</div>"#); // close grid
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::game::player_doc;

    fn reset() {
        player_doc::init_player_doc();
    }

    #[test]
    fn affinity_get_renders_all_archetypes() {
        reset();
        let html = handle_affinity_get("?today=2026-02-26");
        assert!(html.contains("Archetypal Affinity"));
        assert!(html.contains("Brutal"));
        assert!(html.contains("Avian"));
        assert!(html.contains("Entropic"));
        assert!(html.contains("Declare"));
        // No active affinity yet
        assert!(!html.contains("Active:"));
        reset();
    }

    #[test]
    fn a_declare_button_is_an_engine_post_and_the_engine_dates_it() {
        reset();
        let html = handle_affinity_get("");
        assert!(html.contains(
            r#"data-engine-post="/api/player/affinity" data-engine-body="archetype=Brutal">Declare</button>"#
        ));
        for forbidden in ["hx-", "htmx", "onclick"] {
            assert!(!html.contains(forbidden), "{} in the panel", forbidden);
        }
        // What the button sends is what the engine reads, and the day is today.
        handle_affinity_post("archetype=Brutal");
        let (_, declared) = player_doc::get_affinity("Brutal").expect("Brutal is declared");
        assert_eq!(declared, clock::today());
        assert_eq!(player_doc::get_active_affinity().map(|(name, _)| name).as_deref(), Some("Brutal"));
        reset();
    }

    #[test]
    fn a_declaration_can_name_its_day() {
        reset();
        handle_affinity_post("archetype=Avian&today=2026-02-26");
        let (_, declared) = player_doc::get_affinity("Avian").expect("Avian is declared");
        assert_eq!(declared, "2026-02-26");
        reset();
    }

    #[test]
    fn an_archetypes_name_can_go_in_a_form_body_as_it_is() {
        for name in player_doc::valid_archetypes() {
            assert!(name.chars().all(|c| c.is_ascii_alphanumeric()), "{}", name);
        }
    }

    #[test]
    fn affinity_post_declares_and_rerenders() {
        reset();
        let html = handle_affinity_post("archetype=Brutal&today=2026-02-26");
        // Should show active indicator
        assert!(html.contains("Brutal"));
        assert!(html.contains("Active"));
        // All other archetypes should be locked
        assert!(html.contains("Locked"));
        // Should not show any Declare buttons
        assert!(!html.contains(">Declare</button>"));
        reset();
    }

    #[test]
    fn affinity_post_rejects_second_declaration() {
        reset();
        handle_affinity_post("archetype=Brutal&today=2026-02-26");
        // Any second declaration rejected — one per game
        let html = handle_affinity_post("archetype=Avian&today=2026-02-26");
        assert!(html.contains("already declared"));
        reset();
    }

    #[test]
    fn affinity_post_missing_archetype() {
        reset();
        let html = handle_affinity_post("today=2026-02-26");
        assert!(html.contains("Missing archetype"));
        reset();
    }

    #[test]
    fn affinity_get_shows_active_and_locked() {
        reset();
        player_doc::declare_affinity("Brutal", "2026-02-26").unwrap();
        let html = handle_affinity_get("?today=2026-02-26");
        // Brutal is active
        assert!(html.contains("♥ Active"));
        assert!(html.contains("♥"));
        // Other archetypes should be locked
        assert!(html.contains("Locked"));
        // No Declare buttons visible
        assert!(!html.contains(">Declare</button>"));
        reset();
    }

    #[test]
    fn affinity_get_no_affinity_shows_all_declare_buttons() {
        reset();
        let html = handle_affinity_get("?today=2026-02-26");
        // All 15 archetypes should have Declare buttons
        let count = html.matches(">Declare</button>").count();
        assert_eq!(count, 15);
        // No locked indicators
        assert!(!html.contains("Locked"));
        reset();
    }
}
