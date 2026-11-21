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
//!
//! ## Sync history
//! Each state this device took over from another device or a backup is kept
//! as a merge, and the merges are added up (`player_doc::take_over`).
//! Nothing uses the numbers yet. `/api/player/history` draws them on the
//! Player Data page for whoever opens it as `/player_data#history`, which
//! no link does.

use crate::clock;
use crate::game::player_doc::{self, Merge, SyncSum};
use crate::routes::util::{get_param, parse_form_body, parse_query};

// ── GET /api/player/history ────────────────────────────────────────

/// What follows the `#` of the Player Data page's address when the sync
/// history is asked for.
const HISTORY: &str = "history";

/// GET /api/player/history?show=<what follows the # of the page's address>
/// — the merges and their sum, where the address asks for them, and nothing
/// where it does not.
pub fn handle_history_get(query: &str) -> String {
    let params = parse_query(query);
    if get_param(&params, "show") != Some(HISTORY) {
        return String::new();
    }
    render_history(&player_doc::merges(), &player_doc::sync_sum())
}

fn render_history(merges: &[Merge], sum: &SyncSum) -> String {
    let mut h = String::with_capacity(2048);
    h.push_str(concat!(
        r#"<section class="mt-10 text-sm" data-sync-history>"#,
        r#"<h2 class="text-2xl font-bold mb-2">Sync history</h2>"#,
        r#"<p class="max-w-prose mb-3">Each time this device took a game from another device or a backup "#,
        r#"that held something new to it, the two histories were compared. Gained is how many changes "#,
        r#"arrived that this device lacked, held how many it had that the other lacked, shared how many "#,
        r#"both had. Distance is gained and held together, times the smaller of the two over the larger. "#,
        r#"The days of play are the same two counts of the two records alone. "#,
        r#"Nothing is decided by these numbers.</p>"#,
    ));
    h.push_str(&format!(
        concat!(
            r#"<dl class="grid grid-cols-[auto_1fr] gap-x-4 max-w-xs mb-4" data-sync-sum>"#,
            r#"<dt class="font-bold">Sync sum</dt><dd data-sum="distance">{}</dd>"#,
            r#"<dt>Merges</dt><dd data-sum="merges">{}</dd>"#,
            r#"<dt>Gained</dt><dd data-sum="gained">{}</dd>"#,
            r#"<dt>Held</dt><dd data-sum="held">{}</dd>"#,
            r#"</dl>"#,
        ),
        sum.distance, sum.merges, sum.gained, sum.held
    ));
    if merges.is_empty() {
        h.push_str(r#"<p>No merges yet.</p>"#);
    } else {
        // A line for each, of two rows, so that it fits a phone's width.
        h.push_str(r#"<ul class="max-w-sm">"#);
        for merge in merges {
            // To the minute: `YYYY-MM-DD HH:MM`.
            let when = clock::moment_of(merge.at).replace('T', " ");
            let from = if merge.by == "file" { "a backup" } else { "a device" };
            h.push_str(&format!(
                concat!(
                    r#"<li class="py-1 border-t border-kip-drk-goldenrod/30" data-merge>"#,
                    r#"<div class="font-bold">{} UTC, from {}</div>"#,
                    r#"<div>gained {}, held {}, shared {}, distance {}</div>"#,
                    r#"<div>days of play gained {}, held {}</div></li>"#,
                ),
                &when[..16.min(when.len())],
                from,
                merge.gained,
                merge.held,
                merge.shared,
                merge.distance(),
                merge.days_gained,
                merge.days_held
            ));
        }
        h.push_str("</ul>");
    }
    h.push_str("</section>");
    h
}

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
    fn the_sync_history_is_drawn_only_where_the_address_asks_for_it() {
        reset();
        assert_eq!(handle_history_get(""), "");
        assert_eq!(handle_history_get("?show="), "");
        assert_eq!(handle_history_get("?show=sync%3DABCD-EFGHJKLMNPQRSTUVWXYZ"), "");
        let html = handle_history_get("?show=history");
        assert!(html.contains("Sync history"));
        assert!(html.contains(r#"<dd data-sum="distance">0</dd>"#));
        assert!(html.contains("No merges yet."));
        reset();
    }

    #[test]
    fn the_sync_history_lists_each_merge_and_the_sum() {
        // Another device's state, with a play of its own, taken over here.
        reset();
        player_doc::increment_loyalty("brox", "2026-02-25").unwrap();
        let theirs = player_doc::encode_full_state();
        reset();
        player_doc::increment_loyalty("liliel", "2026-02-26").unwrap();
        let taken = player_doc::take_over(&theirs, 1_800_000_500_000.0, "file").unwrap();

        let html = handle_history_get("?show=history");
        let sum = player_doc::sync_sum();
        assert!(sum.distance > 0, "{:?}", sum);
        assert!(html.contains(&format!(r#"<dd data-sum="distance">{}</dd>"#, sum.distance)));
        assert!(html.contains(r#"<dd data-sum="merges">1</dd>"#));
        assert!(html.contains(&format!(r#"<dd data-sum="gained">{}</dd>"#, taken.gained)));
        assert!(html.contains(&format!(r#"<dd data-sum="held">{}</dd>"#, taken.held)));
        assert_eq!(html.matches("data-merge").count(), 1);
        assert!(html.contains(">2027-01-15 08:08 UTC, from a backup</div>"), "{}", html);
        assert!(html.contains(&format!(
            ">gained {}, held {}, shared 0, distance {}</div>",
            taken.gained, taken.held, sum.distance
        )));
        assert!(html.contains(">days of play gained 1, held 1</div>"), "{}", html);
        assert!(!html.contains("No merges yet."));
        reset();
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
