//! Keal damage tracking — per-card damage state and HTML rendering.
//!
//! Each card with keal means has numbered slots (1-based, sequential across
//! all keal means groups). Checking all slots reveals the "Final Blows" section
//! where the wasted checkbox appears.
//!
//! State is stored in the PLAYER_DOC yrs CRDT document (thread_local WASM memory).
//!
//! **DOM Residency Model:** The damage circles are always-in-DOM elements on
//! card pages. Alpine.js owns the reactive visual state (slot toggles, wasted,
//! Final Blows visibility). Clicks fire-and-forget to the WASM worker via
//! `kipukasWorker.postMessage()` to update authoritative state and trigger
//! PERSIST_STATE → localStorage. HTMX is only used for the initial load
//! (`hx-trigger="load"` on the container in `keal_damage_tracker.html`).

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

/// Build the Alpine.js `x-data` object string for the damage tracker.
///
/// Contains reactive state (`slots`, `wasted`) initialized from PLAYER_DOC,
/// computed helpers (`allChecked`, `slotClass`), and fire-and-forget methods
/// (`toggleSlot`, `toggleWasted`) that update Alpine state immediately and
/// send the mutation to the WASM worker for authoritative persistence.
fn build_alpine_x_data(slug: &str, total: u8, is_wasted: bool) -> String {
    // Build slots object: { 1: false, 2: true, 3: false, ... }
    let mut slots_entries: Vec<String> = Vec::with_capacity(total as usize);
    for i in 1..=total {
        let checked = player_doc::get_slot(slug, i);
        slots_entries.push(format!("{}: {}", i, checked));
    }
    let slots_obj = format!("{{{}}}", slots_entries.join(", "));

    let mut xd = String::with_capacity(512);
    xd.push_str("{ slots: ");
    xd.push_str(&slots_obj);
    xd.push_str(", wasted: ");
    xd.push_str(if is_wasted { "true" } else { "false" });

    // allChecked() — computed from Alpine reactive slots state
    xd.push_str(
        ", allChecked() { return Object.values(this.slots).every(function(v) { return v }) }",
    );

    // slotClass(n) — returns Tailwind classes based on slot + wasted state
    xd.push_str(", slotClass(n) { ");
    xd.push_str("if (this.wasted) return this.slots[n] ? ");
    xd.push_str("'bg-slate-400 border-slate-400' : 'bg-transparent border-slate-400'; ");
    xd.push_str(
        "return this.slots[n] ? 'bg-red-600 border-red-600' : 'bg-white border-emerald-600'",
    );
    xd.push_str(" }");

    // fire(body) — fire-and-forget to WASM worker (no MessageChannel, no response)
    xd.push_str(", fire(body) { kipukasWorker.postMessage(");
    xd.push_str("{method:'POST',pathname:'/api/game/damage',search:'',body:body}");
    xd.push_str(") }");

    // toggleSlot(n) — Alpine reactive toggle + fire-and-forget
    xd.push_str(", toggleSlot(n) { ");
    xd.push_str("if (this.wasted) return; ");
    xd.push_str("this.slots[n] = !this.slots[n]; ");
    xd.push_str("this.fire('card=");
    xd.push_str(slug);
    xd.push_str("&slot='+n)");
    xd.push_str(" }");

    // toggleWasted() — Alpine reactive toggle + fire-and-forget
    xd.push_str(", toggleWasted() { ");
    xd.push_str("this.wasted = !this.wasted; ");
    xd.push_str("this.fire('card=");
    xd.push_str(slug);
    xd.push_str("&action=wasted')");
    xd.push_str(" }");

    xd.push_str(" }");
    xd
}

/// Return the damage state for a card as a JSON string.
///
/// Used by `refreshKealTracker()` in `kipukas-multiplayer.js` to update
/// an existing Alpine scope's reactive properties directly, avoiding the
/// cross-browser `innerHTML` + `Alpine.initTree()` re-initialization bug.
///
/// Returns: `{"slots":{"1":true,"2":false,...},"wasted":false}`
/// Returns empty `{}` if the card has no keal means.
pub fn get_damage_state_json(slug: &str) -> String {
    let card = match find_card(slug) {
        Some(c) => c,
        None => return "{}".to_string(),
    };

    if card.keal_means.is_empty() {
        return "{}".to_string();
    }

    let total = total_slots(card);
    ensure_card_state(slug, total);

    let is_wasted = player_doc::is_wasted(slug);

    // Build slots JSON: {"1":true,"2":false,...}
    let mut slots_entries: Vec<String> = Vec::with_capacity(total as usize);
    for i in 1..=total {
        let checked = player_doc::get_slot(slug, i);
        slots_entries.push(format!(r#""{}""#, i) + ":" + if checked { "true" } else { "false" });
    }

    format!(
        r#"{{"slots":{{{}}},"wasted":{}}}"#,
        slots_entries.join(","),
        if is_wasted { "true" } else { "false" }
    )
}

/// Render the keal damage tracker HTML for a specific card.
///
/// Returns an HTML fragment with a single Alpine `x-data` scope at the
/// container level. All slot/wasted visual state is driven by Alpine's
/// reactive `:class` bindings. Clicks fire-and-forget to the WASM worker.
/// HTMX is NOT used for per-click swaps — only for the initial load.
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
        format!(
            "slots=[{}] all_checked={} wasted={}",
            pairs.join(","),
            all_checked,
            is_wasted
        )
    };

    let x_data = build_alpine_x_data(slug, total, is_wasted);

    let mut html = String::with_capacity(2048);
    html.push_str(&format!(
        "<!-- [kipukas-debug] {} {} -->",
        slug, slot_debug
    ));

    // Container — Alpine x-data scope owns all reactive state.
    // :class drives Final Blows visibility (replaces sentinel div pattern).
    // No hx-target/hx-swap — buttons don't trigger HTMX swaps.
    html.push_str(r#"<div x-data=""#);
    html.push_str(&x_data);
    html.push_str(
        r#"" :class="{ 'show-final-blows': allChecked() || wasted }" class="w-11/12 md:w-2/3 xl:w-1/2 pb-4 place-self-center">"#,
    );

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

        for _ in 0..km.count {
            // Every slot renders as a button with Alpine @click and :class.
            // No hx-post — Alpine handles the visual toggle, fire-and-forget
            // handles persistence. :class is the ONLY source of bg/border
            // classes, eliminating the CSS specificity conflict.
            html.push_str(r#"<button class="mr-1 damage-slot" @click="toggleSlot("#);
            html.push_str(&slot_idx.to_string());
            html.push_str(
                r#")" :class="{'opacity-40 pointer-events-none': wasted}"><div class="w-5 h-5 rounded-full border-2 transition-colors duration-300" :class="slotClass("#,
            );
            html.push_str(&slot_idx.to_string());
            html.push_str(r#")"></div></button>"#);

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

    // Final Blows section — always in the DOM, visibility controlled by
    // Alpine :class="{ 'show-final-blows': allChecked() || wasted }" on
    // the container. CSS rule: .final-blows-section { display: none; }
    // .show-final-blows .final-blows-section { display: block; }
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

        // Wasted toggle — Alpine @click + :class, no hx-post
        html.push_str(r#"<div class="flex items-center">"#);
        html.push_str(r#"<p class="mr-2">Wasted: </p>"#);
        html.push_str(
            r#"<button class="mr-1 damage-slot" @click="toggleWasted()"><div class="w-5 h-5 rounded-full border-2 transition-colors duration-300" :class="wasted ? 'bg-red-600 border-red-600' : 'bg-white border-emerald-600'"></div></button>"#,
        );
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
        assert!(html.contains("slotClass(")); // Alpine :class binding
        assert!(html.contains("toggleSlot(")); // Alpine @click handler
        assert!(html.contains("Combat"));
        // Final Blows section is always in the DOM (hidden by CSS via Alpine).
        // Alpine drives visibility via :class="{ 'show-final-blows': allChecked() || wasted }"
        assert!(html.contains("final-blows-section"));
        assert!(html.contains("show-final-blows"));
        // Initial unchecked state: border-emerald-600 in Alpine slotClass
        assert!(html.contains("border-emerald-600"));
        reset_state();
    }

    #[test]
    fn render_has_alpine_x_data_with_slots() {
        reset_state();
        let html = render_damage_tracker("brox_the_defiant");
        // x-data should contain slots object with 3 entries (all false initially)
        assert!(html.contains("slots: {1: false, 2: false, 3: false}"));
        assert!(html.contains("wasted: false"));
        assert!(html.contains("allChecked()"));
        assert!(html.contains("kipukasWorker.postMessage"));
        reset_state();
    }

    #[test]
    fn render_reflects_toggled_slot_in_x_data() {
        reset_state();
        ensure_card_state("brox_the_defiant", 3);
        toggle_slot("brox_the_defiant", 1);
        let html = render_damage_tracker("brox_the_defiant");
        // Slot 1 toggled on, 2 and 3 still off
        assert!(html.contains("slots: {1: true, 2: false, 3: false}"));
        reset_state();
    }

    #[test]
    fn render_reflects_wasted_in_x_data() {
        reset_state();
        ensure_card_state("brox_the_defiant", 3);
        toggle_slot("brox_the_defiant", 1);
        toggle_slot("brox_the_defiant", 2);
        toggle_slot("brox_the_defiant", 3);
        toggle_wasted("brox_the_defiant");
        let html = render_damage_tracker("brox_the_defiant");
        assert!(html.contains("wasted: true"));
        assert!(html.contains("Final Blows"));
        assert!(html.contains("Wasted"));
        assert!(html.contains("Brutal")); // genetic_disposition
        assert!(html.contains("Service")); // motivation
        reset_state();
    }

    #[test]
    fn render_no_hx_post_on_buttons() {
        reset_state();
        let html = render_damage_tracker("brox_the_defiant");
        // Buttons should NOT have hx-post (Alpine handles clicks, not HTMX)
        assert!(!html.contains("hx-post"));
        assert!(!html.contains("hx-vals"));
        assert!(!html.contains("hx-target"));
        assert!(!html.contains("hx-swap"));
        reset_state();
    }

    #[test]
    fn render_no_sentinel_div() {
        reset_state();
        ensure_card_state("brox_the_defiant", 3);
        toggle_slot("brox_the_defiant", 1);
        toggle_slot("brox_the_defiant", 2);
        toggle_slot("brox_the_defiant", 3);
        let html = render_damage_tracker("brox_the_defiant");
        // No sentinel div — Alpine computes allChecked() reactively
        assert!(!html.contains("keal-all-checked"));
        reset_state();
    }

    // ── JSON damage state tests ────────────────────────────────────

    #[test]
    fn json_state_unknown_card_returns_empty() {
        reset_state();
        let json = get_damage_state_json("nonexistent_slug");
        assert_eq!(json, "{}");
        reset_state();
    }

    #[test]
    fn json_state_item_card_returns_empty() {
        reset_state();
        let json = get_damage_state_json("cloth");
        assert_eq!(json, "{}");
        reset_state();
    }

    #[test]
    fn json_state_default_all_false() {
        reset_state();
        let json = get_damage_state_json("brox_the_defiant");
        assert!(json.contains(r#""1":false"#));
        assert!(json.contains(r#""2":false"#));
        assert!(json.contains(r#""3":false"#));
        assert!(json.contains(r#""wasted":false"#));
        reset_state();
    }

    #[test]
    fn json_state_reflects_toggled_slots() {
        reset_state();
        ensure_card_state("brox_the_defiant", 3);
        toggle_slot("brox_the_defiant", 1);
        toggle_slot("brox_the_defiant", 3);
        let json = get_damage_state_json("brox_the_defiant");
        assert!(json.contains(r#""1":true"#));
        assert!(json.contains(r#""2":false"#));
        assert!(json.contains(r#""3":true"#));
        assert!(json.contains(r#""wasted":false"#));
        reset_state();
    }

    #[test]
    fn json_state_reflects_wasted() {
        reset_state();
        ensure_card_state("brox_the_defiant", 3);
        toggle_wasted("brox_the_defiant");
        let json = get_damage_state_json("brox_the_defiant");
        assert!(json.contains(r#""wasted":true"#));
        reset_state();
    }
}
