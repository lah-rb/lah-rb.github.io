//! Keal damage tracking — per-card damage state and HTML rendering.
//!
//! Each card with keal means has numbered slots (1-based, sequential across
//! all keal means groups). Checking all slots reveals the "Final Blows" section
//! where the wasted checkbox appears.
//!
//! State is stored in the PLAYER_DOC yrs CRDT document (thread_local WASM memory).

use crate::cards_generated::CARDS;
use crate::game::player_doc;

/// Look up a card by slug from the compiled-in catalog.
fn find_card(slug: &str) -> Option<&'static crate::cards_generated::Card> {
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
fn total_slots(card: &crate::cards_generated::Card) -> u8 {
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

/// Render the keal damage tracker HTML for a specific card.
/// This replaces the Jekyll/Alpine `keal_damage_tracker.html` include.
pub fn render_damage_tracker(slug: &str) -> String {
    let card = match find_card(slug) {
        Some(c) => c,
        None => {
            return format!(
                r#"<span class="text-kip-red">Card not found: {}</span>"#,
                slug
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

    // Debug: emit slot states as HTML comment for production diagnosis
    let slot_debug = {
        let pairs: Vec<String> = (1..=total)
            .map(|i| format!("{}:{}", i, player_doc::get_slot(slug, i)))
            .collect();
        format!("slots=[{}] all_checked={} wasted={}", pairs.join(","), all_checked, is_wasted)
    };

    let mut html = String::with_capacity(2048);
    html.push_str(&format!("<!-- [kipukas-debug] {} {} -->", slug, slot_debug));

    // Container
    html.push_str(r#"<div class="w-11/12 md:w-2/3 xl:w-1/2 pb-4 place-self-center">"#);

    // Section header
    html.push_str(r#"<p class="capitalize">Combat</p>"#);
    html.push_str(
        r#"<div class="mt-1 h-0.5 w-1/2 bg-kip-red lg:bg-slate-400 justify-self-center"></div>"#,
    );
    html.push_str(r#"<div class="mt-1 h-0.5 w-full bg-slate-200 lg:bg-kip-red"></div>"#);

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

        for _ in 0..km.count {
            let checked = player_doc::get_slot(slug, slot_idx);

            if is_wasted {
                // Disabled state — greyed out, non-clickable
                let bg = if checked { "bg-slate-400" } else { "bg-transparent" };
                html.push_str(&format!(
                    r##"<span class="mr-1 opacity-40"><div class="w-5 h-5 rounded-full border-2 {bg} border-slate-400"></div></span>"##,
                    bg = bg,
                ));
            } else if checked {
                // Checked state — filled red circle, clickable to toggle off
                html.push_str(&format!(
                    r##"<button x-data="{{ on: true }}" class="mr-1 damage-slot" @click="on = !on" onclick="htmx.ajax('POST', '/api/game/damage', {{values: {{card: '{slug}', slot: '{slot}'}}, target: '#keal-damage-{slug}', swap: 'innerHTML'}})"><div class="w-5 h-5 rounded-full border-2 transition-colors duration-300 bg-red-600 border-red-600" :class="on ? 'bg-red-600 border-red-600' : 'bg-white border-emerald-600'"></div></button>"##,
                    slug = slug,
                    slot = slot_idx,
                ));
            } else {
                // Unchecked state — green border, white fill, clickable to toggle on
                html.push_str(&format!(
                    r##"<button x-data="{{ on: false }}" class="mr-1 damage-slot" @click="on = !on" onclick="htmx.ajax('POST', '/api/game/damage', {{values: {{card: '{slug}', slot: '{slot}'}}, target: '#keal-damage-{slug}', swap: 'innerHTML'}})"><div class="w-5 h-5 rounded-full border-2 transition-colors duration-300 bg-white border-emerald-600" :class="on ? 'bg-red-600 border-red-600' : 'bg-white border-emerald-600'"></div></button>"##,
                    slug = slug,
                    slot = slot_idx,
                ));
            }

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

    // Sentinel div: present when all keal means slots are checked (or wasted).
    if all_checked || is_wasted {
        html.push_str(r#"<div class="keal-all-checked hidden"></div>"#);
    }

    // Final Blows section — always in the DOM, visibility controlled by Alpine
    {
        html.push_str(r#"<div class="final-blows-section">"#);

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

        // Wasted indicator
        html.push_str(r#"<div class="flex items-center">"#);
        html.push_str(r#"<p class="mr-2">Wasted: </p>"#);
        let wasted_on = if is_wasted { "true" } else { "false" };
        let wasted_static = if is_wasted {
            "bg-red-600 border-red-600"
        } else {
            "bg-white border-emerald-600"
        };
        html.push_str(&format!(
            r##"<button x-data="{{ on: {on} }}" class="mr-1 damage-slot" @click="on = !on" onclick="htmx.ajax('POST', '/api/game/damage', {{values: {{card: '{slug}', action: 'wasted'}}, target: '#keal-damage-{slug}', swap: 'innerHTML'}})"><div class="w-5 h-5 rounded-full border-2 transition-colors duration-300 {static_cls}" :class="on ? 'bg-red-600 border-red-600' : 'bg-white border-emerald-600'"></div></button>"##,
            on = wasted_on,
            slug = slug,
            static_cls = wasted_static,
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

    #[test]
    fn render_character_card_has_damage_slots() {
        reset_state();
        let html = render_damage_tracker("brox_the_defiant");
        assert!(html.contains("Crushing Hope"));
        assert!(html.contains("Chain Raid"));
        assert!(html.contains("damage-slot")); // Alpine-driven circle buttons
        assert!(html.contains("rounded-full")); // div circles with Tailwind classes
        assert!(html.contains("border-emerald-600")); // green unchecked state
        assert!(html.contains("Combat"));
        // Final Blows section is always in the DOM (hidden by CSS via Alpine),
        // but the sentinel div should NOT be present when slots aren't all checked.
        assert!(html.contains("final-blows-section"));
        assert!(!html.contains("keal-all-checked"));
        reset_state();
    }

    #[test]
    fn final_blows_appear_when_all_checked() {
        reset_state();
        // Brox has 3 keal slots (Crushing Hope: 1, Chain Raid: 2)
        ensure_card_state("brox_the_defiant", 3);
        toggle_slot("brox_the_defiant", 1);
        toggle_slot("brox_the_defiant", 2);
        toggle_slot("brox_the_defiant", 3);
        let html = render_damage_tracker("brox_the_defiant");
        assert!(html.contains("Final Blows"));
        assert!(html.contains("Wasted"));
        assert!(html.contains("Brutal")); // genetic_disposition
        assert!(html.contains("Service")); // motivation
        // Sentinel div should be present so Alpine shows the section
        assert!(html.contains("keal-all-checked"));
        reset_state();
    }
}
