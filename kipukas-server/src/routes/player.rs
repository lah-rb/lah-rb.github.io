//! `/api/player/affinity` routes — affinity tracking for archetypal adaptations.
//!
//! Players declare affinity for one of the 15 archetypes once per day.
//! Each declaration increments the level for that archetype. The most
//! recently declared archetype is the "active" affinity, granting a +1
//! roll bonus on matching cards during fists combat.

use crate::game::player_doc;
use crate::routes::util::{get_param, parse_form_body, parse_query};

// ── GET /api/player/affinity ───────────────────────────────────────

/// Handle GET /api/player/affinity?today={YYYY-MM-DD}
/// Returns the full affinity panel HTML showing all 15 archetypes.
pub fn handle_affinity_get(query: &str) -> String {
    let params = parse_query(query);
    let today = get_param(&params, "today").unwrap_or("");
    render_affinity_panel(today)
}

// ── POST /api/player/affinity ──────────────────────────────────────

/// Handle POST /api/player/affinity
/// Body: archetype={name}&today={YYYY-MM-DD}
/// Declares affinity for the given archetype. Returns re-rendered panel.
pub fn handle_affinity_post(body: &str) -> String {
    let params = parse_form_body(body);
    let archetype = get_param(&params, "archetype").unwrap_or("");
    let today = get_param(&params, "today").unwrap_or("");

    if archetype.is_empty() {
        return r#"<span class="text-kip-red">Missing archetype parameter</span>"#.to_string();
    }

    match player_doc::declare_affinity(archetype, today) {
        Ok(_) => render_affinity_panel(today),
        Err(e) => {
            // Re-render panel with error toast at top
            let mut html = String::with_capacity(4096);
            html.push_str(&format!(
                r#"<div class="text-center text-xs text-kip-red mb-2">{}</div>"#,
                e
            ));
            html.push_str(&render_affinity_panel_inner(today));
            html
        }
    }
}

// ── Panel rendering ────────────────────────────────────────────────

/// Render the full affinity panel (wrapper).
fn render_affinity_panel(today: &str) -> String {
    let mut html = String::with_capacity(4096);
    html.push_str(r#"<div class="p-4 text-kip-drk-sienna">"#);
    html.push_str(r#"<p class="text-lg font-bold text-center mb-1">Archetypal Affinity</p>"#);
    html.push_str(
        r#"<p class="text-xs text-slate-500 text-center mb-3">Declare once per day to grow your bond</p>"#,
    );
    html.push_str(&render_affinity_panel_inner(today));
    html.push_str(r#"</div>"#);
    html
}

/// Render the inner panel content (archetype list).
fn render_affinity_panel_inner(today: &str) -> String {
    let archetypes = player_doc::valid_archetypes();
    let all_affinities = player_doc::get_all_affinities();
    let active = player_doc::get_active_affinity();

    let mut html = String::with_capacity(3072);

    // Active affinity highlight
    if let Some((ref active_name, active_level)) = active {
        html.push_str(r#"<div class="bg-amber-100 border border-amber-300 rounded-lg p-2 mb-3 text-center">"#);
        html.push_str(&format!(
            r#"<p class="text-sm font-bold">Active: <span class="text-kip-red">{}</span> <span class="text-slate-500">Lv.{}</span></p>"#,
            active_name, active_level
        ));
        html.push_str(
            r#"<p class="text-xs text-slate-500">+1 roll bonus on matching cards</p>"#,
        );
        html.push_str(r#"</div>"#);
    }

    // Archetype grid
    html.push_str(r#"<div class="grid grid-cols-1 gap-1">"#);

    for &archetype in archetypes {
        // Look up current affinity data
        let aff_data = all_affinities
            .iter()
            .find(|(name, _, _)| name == archetype);
        let (level, last_declared) = match aff_data {
            Some((_, lvl, last)) => (*lvl, last.as_str()),
            None => (0, ""),
        };

        let is_active = active
            .as_ref()
            .map(|(name, _)| name == archetype)
            .unwrap_or(false);
        let declared_today = last_declared == today && !today.is_empty();

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

        // Left: name + level
        html.push_str(r#"<div class="flex items-center gap-2">"#);
        if is_active {
            html.push_str(r#"<span class="text-amber-500 text-xs">★</span>"#);
        }
        html.push_str(&format!(
            r#"<span class="text-sm font-medium">{}</span>"#,
            archetype
        ));
        if level > 0 {
            // Level bar: visual dots
            html.push_str(r#"<span class="flex gap-0.5">"#);
            let display_dots = level.min(10);
            for _ in 0..display_dots {
                html.push_str(
                    r#"<span class="w-1.5 h-1.5 rounded-full bg-kip-red inline-block"></span>"#,
                );
            }
            if level > 10 {
                html.push_str(&format!(
                    r#"<span class="text-xs text-slate-400">+{}</span>"#,
                    level - 10
                ));
            }
            html.push_str(r#"</span>"#);
        }
        html.push_str(r#"</div>"#);

        // Right: level number + declare button
        html.push_str(r#"<div class="flex items-center gap-2">"#);
        if level > 0 {
            html.push_str(&format!(
                r#"<span class="text-xs text-slate-400">Lv.{}</span>"#,
                level
            ));
        }
        if declared_today {
            html.push_str(
                r#"<span class="text-xs text-slate-400 px-2 py-0.5">✓ Today</span>"#,
            );
        } else {
            html.push_str(&format!(
                "<button class=\"text-xs bg-kip-red text-amber-50 px-2 py-0.5 rounded hover:bg-red-700 active:bg-red-800\" hx-post=\"/api/player/affinity\" hx-vals='{{\"archetype\":\"{}\",\"today\":\"{}\"}}' hx-target=\"#affinity-container\" hx-swap=\"innerHTML\">Declare</button>",
                archetype, today
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
    fn affinity_post_declares_and_rerenders() {
        reset();
        let html = handle_affinity_post("archetype=Brutal&today=2026-02-26");
        // Should show active indicator
        assert!(html.contains("Active:"));
        assert!(html.contains("Brutal"));
        assert!(html.contains("Lv.1"));
        // Brutal should show "Today" instead of declare button
        assert!(html.contains("✓ Today"));
        reset();
    }

    #[test]
    fn affinity_post_rejects_same_day() {
        reset();
        handle_affinity_post("archetype=Brutal&today=2026-02-26");
        let html = handle_affinity_post("archetype=Brutal&today=2026-02-26");
        assert!(html.contains("Already declared"));
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
    fn affinity_get_shows_active_highlight() {
        reset();
        player_doc::declare_affinity("Avian", "2026-02-25").unwrap();
        player_doc::declare_affinity("Brutal", "2026-02-26").unwrap();
        let html = handle_affinity_get("?today=2026-02-26");
        // Brutal is active (most recent)
        assert!(html.contains("Active:"));
        assert!(html.contains("★"));
        // Brutal declared today
        assert!(html.contains("✓ Today"));
        reset();
    }

    #[test]
    fn affinity_get_level_dots_render() {
        reset();
        // Declare Brutal multiple days
        player_doc::declare_affinity("Brutal", "2026-02-20").unwrap();
        player_doc::declare_affinity("Brutal", "2026-02-21").unwrap();
        player_doc::declare_affinity("Brutal", "2026-02-22").unwrap();
        let html = handle_affinity_get("?today=2026-02-23");
        // Should show Lv.3
        assert!(html.contains("Lv.3"));
        // Should show 3 dots (rounded-full circles)
        assert!(html.contains("bg-kip-red"));
        reset();
    }
}
