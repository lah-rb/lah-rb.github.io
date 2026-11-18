//! Keal damage tracking — per-card damage state and HTML rendering.
//!
//! Each card with keal means has numbered slots (1-based, sequential across
//! all keal means groups). Checking all slots reveals the "Final Blows" section
//! where the wasted checkbox appears.
//!
//! State is stored in the PLAYER_DOC yrs CRDT document (thread_local WASM memory).
//!
//! **The tracker is an engine-owned view.** The engine is the only owner of
//! damage state, and `render_damage_tracker` renders that state as plain
//! HTML: which slots are marked is in each button's classes and
//! `aria-pressed`, not in a copy of the state held by the page. A tap asks
//! the engine for a change (`data-engine-post`), the engine applies it, and
//! the page re-renders the view from the engine and patches the result in
//! place (`assets/js/kipukas-view.js`). There is nothing to keep in sync:
//! whatever changes damage — a tap, a lost round of fists, New Game, another
//! device — the tracker is redrawn from the same state.

use crate::catalog::CARDS;
use crate::game::player_doc;
use crate::routes::util::escape_html;

/// Look up a card by slug from the compiled-in catalog.
fn find_card(slug: &str) -> Option<&'static crate::catalog::Card> {
    CARDS.iter().find(|c| c.slug == slug)
}

/// Get or initialize damage state for a card.
/// Ensures all expected slots exist in the PLAYER_DOC.
pub fn ensure_card_state(slug: &str, total_slots: u8) {
    player_doc::ensure_card_state(slug, total_slots);
}

/// Toggle a specific damage slot for a card. Returns the new checked state.
/// Auto-ensures card state exists in PLAYER_DOC before toggling.
pub fn toggle_slot(slug: &str, slot: u8) -> bool {
    // Ensure the card entry exists with the correct number of slots
    let card = find_card(slug);
    if let Some(c) = card {
        let total = total_slots(c);
        ensure_card_state(slug, total);
    }
    player_doc::toggle_slot(slug, slot)
}

/// Toggle the wasted state for a card. Returns the new wasted state.
/// Auto-ensures card state exists in PLAYER_DOC before toggling.
pub fn toggle_wasted(slug: &str) -> bool {
    let card = find_card(slug);
    if let Some(c) = card {
        let total = total_slots(c);
        ensure_card_state(slug, total);
    }
    player_doc::toggle_wasted(slug)
}

/// Clear damage state for a specific card.
pub fn clear_card(slug: &str) {
    player_doc::clear_card(slug);
}

/// Clear damage state for ALL cards.
pub fn clear_all() {
    player_doc::clear_all();
}

/// Count total keal means slots for a card.
fn total_slots(card: &crate::catalog::Card) -> u8 {
    card.keal_means.iter().map(|km| km.count).sum()
}

/// Check if all keal means slots are checked for a card.
fn all_slots_checked(slug: &str, total: u8) -> bool {
    if total == 0 {
        return false;
    }
    for i in 1..=total {
        if !player_doc::get_slot(slug, i) {
            return false;
        }
    }
    true
}

/// Classes for a damage slot's circle.
fn slot_classes(checked: bool, wasted: bool) -> &'static str {
    match (wasted, checked) {
        // A wasted card's slots are greyed out and no longer take taps.
        (true, true) => "bg-slate-400 border-slate-400",
        (true, false) => "bg-transparent border-slate-400",
        (false, true) => "bg-red-600 border-red-600",
        (false, false) => "bg-white border-emerald-600",
    }
}

/// Render the keal damage tracker HTML for a specific card.
///
/// The markup carries the state itself. Each slot is a button whose
/// `aria-pressed` and circle classes say whether it is marked, and whose
/// `data-engine-post` / `data-engine-body` name the change a tap asks for.
/// The Final Blows section is present but `hidden` until every slot is marked
/// or the card is wasted. No script or reactive binding is emitted.
pub fn render_damage_tracker(slug: &str) -> String {
    let card = match find_card(slug) {
        Some(c) => c,
        None => {
            return format!(
                r#"<span class="text-kip-red">Card not found: {}</span>"#,
                escape_html(slug)
            );
        }
    };

    // Only Character and Species cards have keal means
    if card.keal_means.is_empty() {
        return String::new();
    }

    let total = total_slots(card);
    ensure_card_state(slug, total);

    let all_checked = all_slots_checked(slug, total);
    let is_wasted = player_doc::is_wasted(slug);

    let show_final_blows = all_checked || is_wasted;

    let mut html = String::with_capacity(2048);
    html.push_str(r#"<div class="w-11/12 md:w-2/3 xl:w-1/2 pb-4 place-self-center">"#);

    // Section header
    html.push_str(r#"<p class="capitalize">Combat</p>"#);
    html.push_str(
        r#"<div class="mt-1 h-0.5 w-1/2 bg-kip-red lg:bg-slate-400 justify-self-center"></div>"#,
    );
    html.push_str(r#"<div class="mt-1 h-0.5 w-full bg-slate-200 lg:bg-kip-red"></div>"#);

    // Phase C: Loyalty badge
    if let Some((total_plays, _last)) = player_doc::get_loyalty(slug) {
        html.push_str(&format!(
            r#"<p class="text-center">&#x2665; {} play{}</p>"#,
            total_plays,
            if total_plays == 1 { "" } else { "s" }
        ));
    }

    // Phase C: Tameability progress (Species cards only)
    if card.layout == "Species" {
        if let Some(threshold) = card.tamability {
            let loyalty_plays = player_doc::get_loyalty(slug).map(|(t, _)| t).unwrap_or(0);
            let affinity_level = player_doc::get_active_affinity()
                .and_then(|(name, level)| {
                    // Only count affinity if it matches the card's genetic_disposition
                    card.genetic_disposition.and_then(|gd| {
                        if gd == name { Some(level) } else { None }
                    })
                })
                .unwrap_or(0);
            let current = loyalty_plays + affinity_level;
            let tamed = current >= threshold;

            if tamed {
                html.push_str(
                    r#"<p class="text-center font-bold text-emerald-600">&#x2714; Tamed!</p>"#,
                );
            } else {
                html.push_str(&format!(
                    r#"<p class="text-center">Tame: {} / {}</p>"#,
                    current, threshold
                ));
                // Progress bar
                let pct = ((current as f64 / threshold as f64) * 100.0).min(100.0) as u32;
                html.push_str(r#"<div class="w-full bg-slate-200 rounded-full h-1.5 mt-0.5">"#);
                html.push_str(&format!(
                    r#"<div class="bg-emerald-500 h-1.5 rounded-full" style="width: {}%"></div>"#,
                    pct
                ));
                html.push_str(r#"</div>"#);
            }
        }
    }

    // Keal Means header
    html.push_str(r#"<div class="flex w-full my-4"><div class="w-1/2">"#);
    html.push_str(r#"<p>Keal Means</p>"#);
    html.push_str(
        r#"<div class="mt-1 h-0.5 w-1/2 bg-kip-red lg:bg-slate-400 justify-self-center"></div>"#,
    );
    html.push_str(r#"</div></div>"#);

    // Render each keal means group
    let mut slot_idx: u8 = 1;
    for km in card.keal_means {
        html.push_str(r#"<div class="flex mx-2 w-full place-self-center"><div>"#);

        // Name + checkboxes row
        html.push_str(r#"<div class="flex">"#);
        html.push_str(&format!(
            r#"<p class="text-kip-red mr-2">{}: </p>"#,
            km.name
        ));

        for n in 1..=km.count {
            // A slot is a button. Its state is in aria-pressed and in the
            // circle's classes; a tap asks the engine to toggle it.
            let checked = player_doc::get_slot(slug, slot_idx);
            html.push_str(&format!(
                concat!(
                    r#"<button type="button" class="mr-1 damage-slot{}" data-slot="{}" "#,
                    r#"aria-pressed="{}" aria-label="{} damage {} of {}" "#,
                    r#"data-engine-post="/api/game/damage" data-engine-body="card={}&slot={}">"#,
                    r#"<div class="w-5 h-5 rounded-full border-2 transition-colors duration-300 {}"></div>"#,
                    r#"</button>"#,
                ),
                if is_wasted { " opacity-40 pointer-events-none" } else { "" },
                slot_idx,
                checked,
                km.name,
                n,
                km.count,
                slug,
                slot_idx,
                slot_classes(checked, is_wasted),
            ));

            slot_idx += 1;
        }

        html.push_str(r#"</div>"#); // close flex row

        // Genetics info
        let genetics_str = km.genetics.join("-");
        html.push_str(&format!(
            r#"<p class="ml-4">Archetype: <br> {}</p>"#,
            genetics_str
        ));

        html.push_str(r#"</div></div><br>"#);
    }

    // Final Blows section — always in the DOM so it can be patched in and out
    // without rebuilding, shown once every slot is marked or the card is wasted.
    {
        html.push_str(if show_final_blows {
            r#"<div class="final-blows-section">"#
        } else {
            r#"<div class="final-blows-section" hidden>"#
        });

        // Final Blows header
        html.push_str(r#"<div class="flex w-full"><div class="w-1/2">"#);
        html.push_str(r#"<p>Final Blows</p>"#);
        html.push_str(
            r#"<div class="mt-1 h-0.5 w-1/2 bg-kip-red lg:bg-slate-400 justify-self-center"></div>"#,
        );
        html.push_str(r#"</div></div>"#);

        // Motive + Archetype info
        html.push_str(r#"<div class="flex mx-2 w-full place-self-center"><div>"#);
        if let Some(motivation) = card.motivation {
            html.push_str(&format!(r#"<p>Motive: {}</p>"#, motivation));
        }
        if let Some(gd) = card.genetic_disposition {
            html.push_str(&format!(r#"<p>Archetypal Adaptation: {}</p>"#, gd));
        }

        // Wasted toggle
        html.push_str(r#"<div class="flex items-center">"#);
        html.push_str(r#"<p class="mr-2">Wasted: </p>"#);
        html.push_str(&format!(
            concat!(
                r#"<button type="button" class="mr-1 damage-slot" data-wasted aria-pressed="{}" aria-label="Wasted" "#,
                r#"data-engine-post="/api/game/damage" data-engine-body="card={}&action=wasted">"#,
                r#"<div class="w-5 h-5 rounded-full border-2 transition-colors duration-300 {}"></div>"#,
                r#"</button>"#,
            ),
            is_wasted,
            slug,
            if is_wasted { "bg-red-600 border-red-600" } else { "bg-white border-emerald-600" },
        ));
        html.push_str(r#"</div>"#);

        html.push_str(r#"</div></div>"#);
        html.push_str(r#"</div>"#); // close .final-blows-section
    }

    html.push_str(r#"</div>"#); // close container

    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::player_doc;

    fn reset_state() {
        player_doc::init_player_doc();
    }

    #[test]
    fn toggle_slot_works() {
        reset_state();
        ensure_card_state("test", 3);
        assert!(toggle_slot("test", 1)); // false → true
        assert!(!toggle_slot("test", 1)); // true → false
        reset_state();
    }

    #[test]
    fn toggle_wasted_works() {
        reset_state();
        ensure_card_state("test", 2);
        assert!(toggle_wasted("test")); // false → true
        assert!(!toggle_wasted("test")); // true → false
        reset_state();
    }

    #[test]
    fn clear_card_works() {
        reset_state();
        ensure_card_state("card_a", 2);
        ensure_card_state("card_b", 2);
        toggle_slot("card_a", 1);
        toggle_slot("card_b", 1);
        clear_card("card_a");
        assert!(!player_doc::has_card_state("card_a"));
        assert!(player_doc::has_card_state("card_b"));
        reset_state();
    }

    #[test]
    fn clear_all_works() {
        reset_state();
        ensure_card_state("card_a", 2);
        ensure_card_state("card_b", 2);
        toggle_slot("card_a", 1);
        toggle_slot("card_b", 1);
        clear_all();
        assert!(!player_doc::has_card_state("card_a"));
        assert!(!player_doc::has_card_state("card_b"));
        reset_state();
    }

    #[test]
    fn render_unknown_card() {
        reset_state();
        let html = render_damage_tracker("nonexistent_slug");
        assert!(html.contains("Card not found"));
        reset_state();
    }

    #[test]
    fn render_item_card_returns_empty() {
        reset_state();
        // "cloth" is an Item card with no keal means
        let html = render_damage_tracker("cloth");
        assert!(html.is_empty());
        reset_state();
    }

    /// The slot buttons of a rendered tracker, in order, as (slot, pressed).
    fn slots(html: &str) -> Vec<(u8, bool)> {
        html.split("<button")
            .skip(1)
            .filter(|b| b.contains("data-slot="))
            .map(|b| {
                let slot = b
                    .split(r#"data-slot=""#)
                    .nth(1)
                    .and_then(|r| r.split('"').next())
                    .and_then(|n| n.parse().ok())
                    .unwrap();
                (slot, b.contains(r#"aria-pressed="true""#))
            })
            .collect()
    }

    /// The Final Blows section as rendered: its opening tag.
    fn final_blows_tag(html: &str) -> &str {
        let start = html.find(r#"<div class="final-blows-section""#).expect("section present");
        let end = start + html[start..].find('>').unwrap() + 1;
        &html[start..end]
    }

    #[test]
    fn render_character_card_has_damage_slots() {
        reset_state();
        let html = render_damage_tracker("brox_the_defiant");
        assert!(html.contains("Crushing Hope"));
        assert!(html.contains("Chain Raid"));
        assert!(html.contains("Combat"));
        assert!(html.contains("rounded-full")); // div circles with Tailwind classes
        // Brox has three slots, none marked to begin with.
        assert_eq!(slots(&html), [(1, false), (2, false), (3, false)]);
        assert!(html.contains("bg-white border-emerald-600"));
        assert!(!html.contains("bg-red-600"));
        reset_state();
    }

    #[test]
    fn the_markup_is_the_state_with_no_page_side_copy() {
        reset_state();
        let html = render_damage_tracker("brox_the_defiant");
        // No reactive bindings, inline handlers or scripts: the engine is the
        // only owner of the state and the page just shows it.
        for forbidden in ["x-data", ":class", "@click", "onclick", "<script", "hx-post", "hx-get"] {
            assert!(!html.contains(forbidden), "tracker contains {}", forbidden);
        }
        reset_state();
    }

    #[test]
    fn each_slot_names_the_change_a_tap_asks_for() {
        reset_state();
        let html = render_damage_tracker("brox_the_defiant");
        for n in 1..=3 {
            assert!(html.contains(&format!(
                r#"data-engine-post="/api/game/damage" data-engine-body="card=brox_the_defiant&slot={}""#,
                n
            )));
        }
        assert!(html.contains(
            r#"data-engine-post="/api/game/damage" data-engine-body="card=brox_the_defiant&action=wasted""#
        ));
        // Slots are labelled for assistive technology.
        assert!(html.contains(r#"aria-label="Crushing Hope damage 1 of 1""#));
        assert!(html.contains(r#"aria-label="Chain Raid damage 2 of 2""#));
        reset_state();
    }

    #[test]
    fn render_reflects_a_marked_slot() {
        reset_state();
        toggle_slot("brox_the_defiant", 2);
        let html = render_damage_tracker("brox_the_defiant");
        assert_eq!(slots(&html), [(1, false), (2, true), (3, false)]);
        assert_eq!(html.matches("bg-red-600 border-red-600").count(), 1);
        // Unmarking it puts it back.
        toggle_slot("brox_the_defiant", 2);
        assert_eq!(slots(&render_damage_tracker("brox_the_defiant")), [(1, false), (2, false), (3, false)]);
        reset_state();
    }

    #[test]
    fn final_blows_is_hidden_until_every_slot_is_marked() {
        reset_state();
        let html = render_damage_tracker("brox_the_defiant");
        assert_eq!(final_blows_tag(&html), r#"<div class="final-blows-section" hidden>"#);

        toggle_slot("brox_the_defiant", 1);
        toggle_slot("brox_the_defiant", 2);
        let html = render_damage_tracker("brox_the_defiant");
        assert!(final_blows_tag(&html).contains("hidden"), "two of three marked");

        toggle_slot("brox_the_defiant", 3);
        let html = render_damage_tracker("brox_the_defiant");
        assert_eq!(final_blows_tag(&html), r#"<div class="final-blows-section">"#);
        assert!(html.contains("Final Blows"));
        assert!(html.contains("Brutal")); // genetic_disposition
        assert!(html.contains("Service")); // motivation
        reset_state();
    }

    #[test]
    fn a_wasted_card_shows_final_blows_and_greys_out_its_slots() {
        reset_state();
        toggle_slot("brox_the_defiant", 1);
        toggle_wasted("brox_the_defiant");
        let html = render_damage_tracker("brox_the_defiant");

        // Shown even though not every slot is marked.
        assert_eq!(final_blows_tag(&html), r#"<div class="final-blows-section">"#);
        // The wasted button is pressed and red.
        let wasted = html.split("<button").find(|b| b.contains("data-wasted")).unwrap();
        assert!(wasted.contains(r#"aria-pressed="true""#));
        assert!(wasted.contains("bg-red-600 border-red-600"));
        // Slots keep their marks but are grey and take no taps.
        assert_eq!(slots(&html), [(1, true), (2, false), (3, false)]);
        assert_eq!(html.matches("opacity-40 pointer-events-none").count(), 3);
        assert_eq!(html.matches("bg-slate-400 border-slate-400").count(), 1);
        assert_eq!(html.matches("bg-transparent border-slate-400").count(), 2);

        // Un-wasting restores them.
        toggle_wasted("brox_the_defiant");
        let html = render_damage_tracker("brox_the_defiant");
        assert!(!html.contains("pointer-events-none"));
        assert!(final_blows_tag(&html).contains("hidden"));
        reset_state();
    }

    #[test]
    fn rendering_twice_gives_identical_markup() {
        // The page patches the view by comparing markup, so the same state
        // must always render the same way.
        reset_state();
        toggle_slot("brox_the_defiant", 1);
        assert_eq!(
            render_damage_tracker("brox_the_defiant"),
            render_damage_tracker("brox_the_defiant")
        );
        reset_state();
    }

    #[test]
    fn an_unknown_slug_is_escaped_when_echoed() {
        reset_state();
        let html = render_damage_tracker("<img src=x onerror=alert(1)>");
        assert!(html.contains("Card not found"));
        assert!(!html.contains("<img"));
        reset_state();
    }
}
