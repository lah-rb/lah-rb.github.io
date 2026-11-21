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
//! HTML: which slots are marked is in each button's `aria-pressed`, not in
//! a copy of the state held by the page. A tap asks
//! the engine for a change (`data-engine-post`), the engine applies it, and
//! the page re-renders the view from the engine and patches the result in
//! place (`assets/js/kipukas-view.js`). There is nothing to keep in sync:
//! whatever changes damage — a tap, a lost round of fists, New Game, another
//! device — the tracker is redrawn from the same state.
//!
//! **A slot is a heart.** Whole and red while the keal means stands, broken
//! and pale once it is marked. Both drawings are in every slot, one over the
//! other, and the stylesheet shows the one `aria-pressed` calls for
//! (`.damage-heart` in `assets/css/input.css`), so marking a slot changes
//! one attribute and the change is a transition, not a swap. The shapes are
//! the owner's drawings, read from `assets/utility_images/` when the engine
//! is compiled: redraw a heart there and it is the heart here. Only the
//! outlines are taken; line weight and colours are the site's.

use std::sync::OnceLock;

use crate::catalog::{Card, CARDS};
use crate::game::control::{self, Control, Standing};
use crate::game::copies;
use crate::game::player_doc;
use crate::routes::util::escape_html;

/// The heart of a keal means that stands: one outline.
const WHOLE_HEART: &str = include_str!("../../../assets/utility_images/whole_heart.svg");
/// The heart of one that is marked: two halves, parted along a crack.
const BROKEN_HEART: &str = include_str!("../../../assets/utility_images/broken_heart.svg");

/// The value of an attribute in a drawing, from `from` on.
fn attribute<'a>(svg: &'a str, name: &str) -> Option<&'a str> {
    let start = svg.find(&format!(" {}=\"", name))? + name.len() + 3;
    let end = start + svg[start..].find('"')?;
    Some(&svg[start..end])
}

/// The outline (`d`) of every path in a drawing, in the drawing's order.
/// One that holds anything but path data is left out, as what is taken goes
/// into the page's markup.
fn outlines(svg: &str) -> Vec<&str> {
    svg.split("<path")
        .skip(1)
        .filter_map(|path| attribute(path.split('>').next().unwrap_or(""), "d"))
        .filter(|d| d.chars().all(|c| c.is_ascii_digit() || " ,.-+eEMmLlHhVvCcSsQqTtAaZz".contains(c)))
        .collect()
}

/// Where a shape lies across the drawing: the middle of the x coordinates
/// its outline names. (The drawings use absolute coordinates in pairs, which
/// is what a CAD export writes.)
fn middle_x(outline: &str) -> f64 {
    let numbers: Vec<f64> = outline
        .split(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | 'e' | 'E')))
        .filter_map(|n| n.parse().ok())
        .collect();
    let xs: Vec<f64> = numbers.iter().step_by(2).copied().collect();
    xs.iter().sum::<f64>() / xs.len().max(1) as f64
}

/// The heart every slot carries: the broken halves, and the whole heart
/// over them. Which shows is the stylesheet's doing.
fn heart() -> &'static str {
    static HEART: OnceLock<String> = OnceLock::new();
    HEART.get_or_init(|| {
        let view_box = attribute(WHOLE_HEART, "viewBox").unwrap_or("0 0 60 60");
        let mut halves = outlines(BROKEN_HEART);
        // The left half first, whichever way round they were drawn.
        halves.sort_by(|a, b| middle_x(a).total_cmp(&middle_x(b)));
        let mut svg = format!(
            r#"<svg class="damage-heart" viewBox="{}" aria-hidden="true" focusable="false">"#,
            escape_html(view_box)
        );
        for (half, side) in halves.iter().zip(["left", "right"]) {
            svg.push_str(&format!(r#"<path class="damage-heart-half damage-heart-{}" d="{}"/>"#, side, half));
        }
        for whole in outlines(WHOLE_HEART) {
            svg.push_str(&format!(r#"<path class="damage-heart-whole" d="{}"/>"#, whole));
        }
        svg.push_str("</svg>");
        svg
    })
}

/// Look up a card from the compiled-in catalog, by its name or by the key
/// of one of its copies (`game::copies`).
fn find_card(key: &str) -> Option<&'static crate::catalog::Card> {
    let slug = copies::slug_of(key);
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

/// One more, or one less, from card effects in play towards controlling a
/// species (`game::control`).
pub fn adjust_effects(slug: &str, by: i32) {
    if let Some(card) = find_card(slug) {
        control::adjust_effects(card, slug, by);
    }
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

// A card's panel lies over its own art, blurred: whatever is drawn here has
// to be read against any picture. So the standing is a filled chip, the
// buttons are filled, and the bar's colours are bright ones.
const AFFINITY_COLOUR: &str = "bg-amber-400";
const EFFECTS_COLOUR: &str = "bg-red-500";
const LOYALTY_COLOUR: &str = "bg-sky-400";
const EMPTY_COLOUR: &str = "bg-white/25";
const STEP_BUTTON: &str =
    "w-7 h-7 rounded bg-amber-50 text-kip-drk-sienna font-bold leading-none disabled:opacity-40";
const CHIP: &str = "inline-block rounded-full px-2 text-amber-50";

/// A species' standing with the player, and the stack it comes of
/// (`game::control`): a bar of ten squares, filled by affinity, card effects
/// and loyalty in turn, with a mark at the species' tameability. Card
/// effects are the one part the player sets here, a point at a time.
fn render_control(card: &Card, key: &str, control: &Control) -> String {
    let (stack, needed) = (control.stack, control.tameability);
    let numbers = format!(r#" <span class="font-normal">{} of {}</span>"#, stack.total(), needed);
    let chip = |colour: &str, said: &str| format!(r#"<span class="{} {}">{}</span>"#, CHIP, colour, said);
    let (name, said) = match control.standing {
        Standing::Free => ("free", "Needs no taming".to_string()),
        Standing::Untameable => ("untameable", chip("bg-kip-red", "Cannot be tamed: always rogue")),
        Standing::Held => ("held", format!("{}{}", chip("bg-emerald-600", "&#x2714; Under your control"), numbers)),
        Standing::Rogue => ("rogue", format!("{}{}", chip("bg-kip-red", "Rogue"), numbers)),
    };
    let mut h =
        format!(r#"<div class="mt-1" data-control="{}"><p class="text-center font-bold">{}</p>"#, name, said);
    if matches!(control.standing, Standing::Held | Standing::Rogue) {
        // The bar.
        h.push_str(&format!(
            concat!(
                r#"<div class="relative mt-1" role="img" aria-label="Affinity {}, card effects {}, loyalty {}: "#,
                r#"{} of the {} this species needs"><div class="flex gap-0.5 h-2.5">"#,
            ),
            stack.affinity,
            stack.effects,
            stack.loyalty,
            stack.total(),
            needed
        ));
        let parts =
            [(stack.affinity, AFFINITY_COLOUR), (stack.effects, EFFECTS_COLOUR), (stack.loyalty, LOYALTY_COLOUR)];
        let filled: Vec<&str> =
            parts.iter().flat_map(|(points, colour)| std::iter::repeat_n(*colour, *points as usize)).collect();
        for square in 0..control::STACK_MAX as usize {
            h.push_str(&format!(
                r#"<span class="flex-1 rounded-sm {}"></span>"#,
                filled.get(square).copied().unwrap_or(EMPTY_COLOUR)
            ));
        }
        h.push_str(&format!(
            concat!(
                r#"</div><span class="absolute -top-1 -bottom-1 w-0.5 -translate-x-1/2 bg-white" "#,
                r#"style="left: {}%"></span></div>"#,
            ),
            needed * 100 / control::STACK_MAX
        ));

        // Its three parts.
        let dot = |colour: &str| format!(r#"<span class="inline-block w-2 h-2 rounded-sm mr-1 {}"></span>"#, colour);
        let step = |by: i32, label: &str, sign: &str, off: bool| {
            format!(
                concat!(
                    r#"<button type="button" data-engine-post="/api/game/damage" "#,
                    r#"data-engine-body="card={}&amp;action=effects&amp;by={}" aria-label="{}" class="{}"{}>{}</button>"#,
                ),
                key,
                by,
                label,
                STEP_BUTTON,
                if off { " disabled" } else { "" },
                sign
            )
        };
        h.push_str(&format!(
            concat!(
                r#"<dl class="grid grid-cols-[1fr_auto] gap-x-3 gap-y-1 mt-2 text-sm text-left items-center">"#,
                r#"<dt>{}Affinity</dt><dd class="text-right" data-stack="affinity">{}</dd>"#,
                r#"<dt>{}Card effects in play</dt><dd class="flex items-center gap-2">{}"#,
                r#"<span class="w-4 text-center" data-stack="effects">{}</span>{}</dd>"#,
                r#"<dt>{}Loyalty</dt><dd class="text-right" data-stack="loyalty">{} of {}</dd>"#,
                r#"</dl>"#,
            ),
            dot(AFFINITY_COLOUR),
            stack.affinity,
            dot(EFFECTS_COLOUR),
            step(-1, "One less from card effects", "&minus;", stack.effects == 0),
            stack.effects,
            step(1, "One more from card effects", "+", stack.effects >= control::CARD_EFFECTS_MAX),
            dot(LOYALTY_COLOUR),
            stack.loyalty,
            control::LOYALTY_MAX,
        ));
        // What declaring would add, while nothing is declared.
        if let (0, None, Some(archetype)) =
            (stack.affinity, player_doc::get_active_affinity(), card.genetic_disposition)
        {
            h.push_str(&format!(
                r#"<p class="text-xs text-left mt-1 opacity-80">Declaring {} as your affinity for this game adds {}.</p>"#,
                archetype,
                control::AFFINITY_POINTS
            ));
        }
    }
    h.push_str("</div>");
    h
}

/// Render the keal damage tracker HTML for a specific card: for the copy of
/// it kept under `slug`, which is the card's name or the key of a further
/// copy (`game::copies`). Every button names that copy.
///
/// The markup carries the state itself. Each slot is a button whose
/// `aria-pressed` says whether it is marked (and so which heart shows), and
/// whose `data-engine-post` / `data-engine-body` name the change a tap asks
/// for.
/// The Final Blows section is present but `hidden` until every slot is marked
/// or the card is wasted. No script or reactive binding is emitted.
pub fn render_damage_tracker(slug: &str) -> String {
    // Drawing a card's tracker keeps a place for its damage. A copy that
    // is not in play (taken out since, or cleared by New Game) is not given
    // one: the first copy is drawn.
    let slug = if copies::in_play(slug) { slug } else { copies::slug_of(slug) };
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
    // (Loyalty is the card's, whichever copy this is.)
    if let Some((total_plays, _last)) = player_doc::get_loyalty(card.slug) {
        html.push_str(&format!(
            r#"<p class="text-center">&#x2665; {} play{}</p>"#,
            total_plays,
            if total_plays == 1 { "" } else { "s" }
        ));
    }

    // Whether a species is under the player's control, and what of.
    if let Some(control) = control::of(card, slug) {
        html.push_str(&render_control(card, slug, &control));
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

        // Name + hearts row
        html.push_str(r#"<div class="flex items-center">"#);
        html.push_str(&format!(
            r#"<p class="text-kip-red mr-2">{}: </p>"#,
            km.name
        ));

        for n in 1..=km.count {
            // A slot is a button holding a heart. Its state is in
            // aria-pressed; a tap asks the engine to toggle it. A wasted
            // card's slots are greyed out and no longer take taps.
            let checked = player_doc::get_slot(slug, slot_idx);
            html.push_str(&format!(
                concat!(
                    r#"<button type="button" class="mr-2 damage-slot{}" data-slot="{}" "#,
                    r#"aria-pressed="{}" aria-label="{} damage {} of {}" "#,
                    r#"data-engine-post="/api/game/damage" data-engine-body="card={}&slot={}">"#,
                    "{}",
                    r#"</button>"#,
                ),
                if is_wasted { " damage-slot-spent" } else { "" },
                slot_idx,
                checked,
                km.name,
                n,
                km.count,
                slug,
                slot_idx,
                heart(),
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
                r#"<button type="button" class="mr-2 damage-slot" data-wasted aria-pressed="{}" aria-label="Wasted" "#,
                r#"data-engine-post="/api/game/damage" data-engine-body="card={}&action=wasted">"#,
                "{}",
                r#"</button>"#,
            ),
            is_wasted,
            slug,
            heart(),
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

    // ── Whether a species is under control ──

    /// A species of the binder with this tameability.
    fn species(tameability: u32) -> &'static Card {
        CARDS.iter().find(|card| card.layout == "Species" && card.tamability == Some(tameability)).unwrap()
    }

    #[test]
    fn a_species_page_says_it_is_rogue_and_shows_an_empty_stack() {
        reset_state();
        let card = species(5);
        let html = render_damage_tracker(card.slug);
        assert!(html.contains(r#"data-control="rogue""#));
        assert!(html.contains(r#">Rogue</span> <span class="font-normal">0 of 5</span>"#), "{}", html);
        // Ten squares, none filled, and the mark half way along.
        assert_eq!(html.matches(r#"<span class="flex-1 rounded-sm bg-white/25"></span>"#).count(), 10);
        assert!(html.contains(r#"style="left: 50%""#));
        assert!(html.contains(r#"data-stack="affinity">0</dd>"#));
        assert!(html.contains(r#"data-stack="effects">0</span>"#));
        assert!(html.contains(r#"data-stack="loyalty">0 of 4</dd>"#));
        // Nothing declared yet: what declaring its archetype would add.
        let archetype = card.genetic_disposition.unwrap();
        assert!(html.contains(&format!("Declaring {} as your affinity for this game adds 3.", archetype)));
        // The steps are engine posts, and there is no less than none.
        assert!(html.contains(&format!(
            r#"data-engine-body="card={}&amp;action=effects&amp;by=-1" aria-label="One less from card effects" class="{}" disabled>"#,
            card.slug, STEP_BUTTON
        )));
        assert!(html.contains(&format!(
            r#"data-engine-body="card={}&amp;action=effects&amp;by=1" aria-label="One more from card effects" class="{}">+</button>"#,
            card.slug, STEP_BUTTON
        )));
        reset_state();
    }

    #[test]
    fn the_stack_fills_in_the_order_of_its_parts_and_the_species_is_held_when_it_reaches_the_mark() {
        reset_state();
        let card = species(5);
        player_doc::declare_affinity(card.genetic_disposition.unwrap(), "2026-11-21").unwrap();
        adjust_effects(card.slug, 1);
        for day in 1..=3 {
            player_doc::increment_loyalty(card.slug, &format!("2026-11-{:02}", day)).unwrap();
        }
        let html = render_damage_tracker(card.slug);
        assert!(html.contains(r#"data-control="held""#));
        assert!(html.contains("Under your control"));
        assert!(html.contains(">5 of 5</span>"));
        let bar: Vec<&str> = html
            .split(r#"<span class="flex-1 rounded-sm "#)
            .skip(1)
            .map(|rest| rest.split('"').next().unwrap())
            .collect();
        let (a, e, l, none) = (AFFINITY_COLOUR, EFFECTS_COLOUR, LOYALTY_COLOUR, EMPTY_COLOUR);
        assert_eq!(bar, [a, a, a, e, l, none, none, none, none, none]);
        assert!(html.contains("Affinity 3, card effects 1, loyalty 1: 5 of the 5 this species needs"));
        assert!(!html.contains("Declaring"), "an affinity is declared");
        // Card effects at their most: no more to add.
        adjust_effects(card.slug, 1);
        adjust_effects(card.slug, 1);
        let html = render_damage_tracker(card.slug);
        assert!(html.contains(r#"data-stack="effects">3</span>"#));
        assert!(html.contains(&format!(
            r#"aria-label="One more from card effects" class="{}" disabled>"#,
            STEP_BUTTON
        )));
        reset_state();
    }

    #[test]
    fn a_species_that_needs_no_taming_says_so_and_a_character_says_nothing() {
        reset_state();
        let html = render_damage_tracker(species(0).slug);
        assert!(html.contains(r#"data-control="free""#) && html.contains("Needs no taming"));
        assert!(!html.contains("data-stack"));
        let character = CARDS.iter().find(|card| card.layout == "Character" && !card.keal_means.is_empty()).unwrap();
        let html = render_damage_tracker(character.slug);
        assert!(!html.contains("data-control"));
        reset_state();
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
    fn the_heart_is_the_two_drawings_one_over_the_other() {
        let heart = heart();
        // The broken heart's halves, left then right, under the whole one.
        let classes: Vec<&str> = heart.split("<path class=\"").skip(1).map(|p| p.split('"').next().unwrap()).collect();
        assert_eq!(
            classes,
            ["damage-heart-half damage-heart-left", "damage-heart-half damage-heart-right", "damage-heart-whole"]
        );
        assert!(heart.starts_with(r#"<svg class="damage-heart" viewBox="0 0 60 60" aria-hidden="true""#), "{}", heart);
        // Only outlines are taken from the drawings: no line weight, colour
        // or anything else of theirs.
        for theirs in ["stroke", "fill", "#000000", "<g", "<defs", "<?xml"] {
            assert!(!heart.contains(theirs), "{}", theirs);
        }
        // The halves are told apart by where they lie, not by their order.
        let halves = outlines(BROKEN_HEART);
        assert_eq!(halves.len(), 2);
        let view: Vec<f64> = attribute(BROKEN_HEART, "viewBox").unwrap().split(' ').map(|n| n.parse().unwrap()).collect();
        let centre = view[0] + view[2] / 2.0;
        let (left, right) = (heart.find("damage-heart-left").unwrap(), heart.find("damage-heart-right").unwrap());
        let outline_after = |at: usize| attribute(&heart[at..], "d").unwrap();
        assert!(middle_x(outline_after(left)) < centre);
        assert!(middle_x(outline_after(right)) > centre);
    }

    #[test]
    fn an_outline_that_is_not_path_data_is_not_taken() {
        let drawing = r#"<svg viewBox="0 0 1 1"><path id="a" d="M0,0 L1,1 z"/><path d="M0,0&quot;><script>"/></svg>"#;
        assert_eq!(outlines(drawing), ["M0,0 L1,1 z"]);
        assert_eq!(attribute(drawing, "viewBox"), Some("0 0 1 1"));
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
        // Brox has three slots, none marked to begin with.
        assert_eq!(slots(&html), [(1, false), (2, false), (3, false)]);
        // Each holds a heart, and so does the wasted button.
        assert_eq!(html.matches(r#"<svg class="damage-heart""#).count(), 4);
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
        // The wasted button is pressed.
        let wasted = html.split("<button").find(|b| b.contains("data-wasted")).unwrap();
        assert!(wasted.contains(r#"aria-pressed="true""#));
        assert!(!wasted.contains("damage-slot-spent"), "it can still be taken back");
        // Slots keep their marks but are grey and take no taps.
        assert_eq!(slots(&html), [(1, true), (2, false), (3, false)]);
        assert_eq!(html.matches(r#"class="mr-2 damage-slot damage-slot-spent""#).count(), 3);

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
