//! Keal damage tracking — per-card damage state and HTML rendering.
//!
//! Each card with keal means has numbered slots (1-based, sequential across
//! all keal means groups). Checking all slots reveals the "Final Blows" section
//! where the wasted checkbox appears.
//!
//! State is stored in the global GameState (thread_local WASM memory).

use crate::cards_generated::CARDS;
use crate::game::state::{with_state, with_state_mut, CardDamageState};

/// Look up a card by slug from the compiled-in catalog.
fn find_card(slug: &str) -> Option<&'static crate::cards_generated::Card> {
    CARDS.iter().find(|c| c.slug == slug)
}

/// Get or initialize damage state for a card.
/// Ensures all expected slots exist in the HashMap.
fn ensure_card_state(slug: &str, total_slots: u8) {
    with_state_mut(|state| {
        let entry = state
            .cards
            .entry(slug.to_string())
            .or_insert_with(CardDamageState::default);
        // Ensure all slot keys exist
        for i in 1..=total_slots {
            entry.slots.entry(i).or_insert(false);
        }
    });
}

/// Toggle a specific damage slot for a card. Returns the new checked state.
pub fn toggle_slot(slug: &str, slot: u8) -> bool {
    with_state_mut(|state| {
        let entry = state
            .cards
            .entry(slug.to_string())
            .or_insert_with(CardDamageState::default);
        let current = entry.slots.get(&slot).copied().unwrap_or(false);
        entry.slots.insert(slot, !current);
        !current
    })
}

/// Toggle the wasted state for a card. Returns the new wasted state.
pub fn toggle_wasted(slug: &str) -> bool {
    with_state_mut(|state| {
        let entry = state
            .cards
            .entry(slug.to_string())
            .or_insert_with(CardDamageState::default);
        entry.wasted = !entry.wasted;
        entry.wasted
    })
}

/// Clear damage state for a specific card.
pub fn clear_card(slug: &str) {
    with_state_mut(|state| {
        state.cards.remove(slug);
    });
}

/// Clear damage state for ALL cards.
pub fn clear_all() {
    with_state_mut(|state| {
        state.cards.clear();
    });
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
    with_state(|state| {
        if let Some(card_state) = state.cards.get(slug) {
            for i in 1..=total {
                if !card_state.slots.get(&i).copied().unwrap_or(false) {
                    return false;
                }
            }
            true
        } else {
            false
        }
    })
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
    let is_wasted = with_state(|state| {
        state
            .cards
            .get(slug)
            .map(|c| c.wasted)
            .unwrap_or(false)
    });

    // Debug: emit slot states as HTML comment for production diagnosis
    let slot_debug = with_state(|state| {
        if let Some(card_state) = state.cards.get(slug) {
            let pairs: Vec<String> = (1..=total)
                .map(|i| format!("{}:{}", i, card_state.slots.get(&i).copied().unwrap_or(false)))
                .collect();
            format!("slots=[{}] all_checked={} wasted={}", pairs.join(","), all_checked, is_wasted)
        } else {
            format!("no_card_state total={}", total)
        }
    });

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
            let checked = with_state(|state| {
                state
                    .cards
                    .get(slug)
                    .and_then(|c| c.slots.get(&slot_idx).copied())
                    .unwrap_or(false)
            });

            let checked_attr = if checked { " checked" } else { "" };
            let disabled_attr = if is_wasted { " disabled" } else { "" };

            html.push_str(&format!(
                r#"<input type="checkbox" class="text-kip-red rounded mr-2 focus:ring-1 focus:ring-kip-red disabled:bg-slate-100" name="damageTrack" id="{slug}_slot_{slot}"{checked}{disabled} onclick="htmx.ajax('POST', '/api/game/damage', {{values: {{card: '{slug}', slot: '{slot}'}}, target: '#keal-damage-{slug}', swap: 'innerHTML'}})">"#,
                slug = slug,
                slot = slot_idx,
                checked = checked_attr,
                disabled = disabled_attr,
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

    // Final Blows section — shown when all keal means are checked
    if all_checked || is_wasted {
        html.push_str(r#"<div>"#);

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

        // Wasted checkbox
        let wasted_checked = if is_wasted { " checked" } else { "" };
        html.push_str(r#"<div class="flex">"#);
        html.push_str(r#"<p class="mr-2">Wasted: </p>"#);
        html.push_str(&format!(
            r#"<input type="checkbox" class="text-kip-red rounded mr-2 focus:ring-1 focus:ring-kip-red" name="damageTrack" id="{slug}_wasted"{checked} onclick="htmx.ajax('POST', '/api/game/damage', {{values: {{card: '{slug}', action: 'wasted'}}, target: '#keal-damage-{slug}', swap: 'innerHTML'}})">"#,
            slug = slug,
            checked = wasted_checked,
        ));
        html.push_str(r#"</div>"#);

        html.push_str(r#"</div></div>"#);
        html.push_str(r#"</div>"#);
        // Force browser reflow so Final Blows section paints after innerHTML swap.
        // Reading offsetHeight triggers synchronous layout calculation.
        html.push_str(&format!(
            r#"<script>document.getElementById('keal-damage-{}').offsetHeight;</script>"#,
            slug
        ));
    }

    html.push_str(r#"</div>"#); // close container

    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::state::replace_state;

    fn reset_state() {
        replace_state(crate::game::state::GameState::default());
    }

    #[test]
    fn toggle_slot_works() {
        reset_state();
        assert!(toggle_slot("test", 1)); // false → true
        assert!(!toggle_slot("test", 1)); // true → false
        reset_state();
    }

    #[test]
    fn toggle_wasted_works() {
        reset_state();
        assert!(toggle_wasted("test")); // false → true
        assert!(!toggle_wasted("test")); // true → false
        reset_state();
    }

    #[test]
    fn clear_card_works() {
        reset_state();
        toggle_slot("card_a", 1);
        toggle_slot("card_b", 1);
        clear_card("card_a");
        with_state(|s| {
            assert!(!s.cards.contains_key("card_a"));
            assert!(s.cards.contains_key("card_b"));
        });
        reset_state();
    }

    #[test]
    fn clear_all_works() {
        reset_state();
        toggle_slot("card_a", 1);
        toggle_slot("card_b", 1);
        clear_all();
        with_state(|s| assert!(s.cards.is_empty()));
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
    fn render_character_card_has_checkboxes() {
        reset_state();
        let html = render_damage_tracker("brox_the_defiant");
        assert!(html.contains("Crushing Hope"));
        assert!(html.contains("Chain Raid"));
        assert!(html.contains("checkbox"));
        assert!(html.contains("Combat"));
        // Final Blows should NOT show (no slots checked)
        assert!(!html.contains("Final Blows"));
        reset_state();
    }

    #[test]
    fn final_blows_appear_when_all_checked() {
        reset_state();
        // Brox has 3 keal slots (Crushing Hope: 1, Chain Raid: 2)
        toggle_slot("brox_the_defiant", 1);
        toggle_slot("brox_the_defiant", 2);
        toggle_slot("brox_the_defiant", 3);
        let html = render_damage_tracker("brox_the_defiant");
        assert!(html.contains("Final Blows"));
        assert!(html.contains("Wasted"));
        assert!(html.contains("Brutal")); // genetic_disposition
        assert!(html.contains("Service")); // motivation
        reset_state();
    }
}
