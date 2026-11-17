//! The card detail panel: what a card shows, and how it is rendered.
//!
//! ## Schema
//!
//! There is one complete set of fields a card can show ([`Field`]). A card
//! type is not a template of its own; it is a composition of reusable groups
//! of rows drawn from that set ([`CardType`]). Every type opens with the same
//! identity group and closes with the same provenance group; Character and
//! Species share the combatant group; and so on.
//!
//! This keeps "what exists" in one place:
//!
//! - To see every field a card can have, read [`Field`].
//! - To see what a type shows and in what order, read its entry in
//!   [`CARD_TYPES`].
//! - To retire a field, delete its variant: the compiler lists every group
//!   that used it. To retire a type, delete its entry.
//!
//! A row gives a field the label one type uses for it, because the same
//! field reads differently on different cards ("game effects" on a
//! Character, "uncrafted use" on an Item).
//!
//! Two pieces of card text sit outside the schema: `promotion` and
//! `promotion_conditions`. They are small print at the foot of any card that
//! has them, whatever its type, with no label.
//!
//! ## Rendering
//!
//! [`render_card_panel`] is a pure function of the card and the player's
//! state for it. It returns plain HTML with no reactive bindings or scripts,
//! so the panel is an engine-owned view (`assets/js/kipukas-view.js`): the
//! hand view shows it under `data-engine-view="/api/card/panel?card=<slug>"`
//! and it is redrawn from here whenever its state changes.
//!
//! The damage tracker is part of the panel, rendered inline by
//! [`damage::render_damage_tracker`], not a view nested inside it. A tap on a
//! damage slot therefore redraws the panel it sits in.
//!
//! Card text comes from the card files at build time
//! (`scripts/build-card-catalog.ts`). It is written by the game's authors,
//! may contain markup (`<b>`, `<br>`), and is emitted as written.

use crate::cards_generated::{Card, CARDS};
use crate::game::damage;
use crate::routes::util::escape_html;

// ── The complete set of fields ─────────────────────────────────────

/// Every field a card can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Variation,
    Movement,
    PlayStyle,
    /// Either one die for everything, or separate attack and defence dice.
    DieRolls,
    /// Keal means with their damage slots, and Final Blows. The one field
    /// that shows player state rather than card text.
    Combat,
    BrawlPower,
    Habitat,
    Genetics,
    Motivation,
    Criteria,
    Success,
    Failure,
    Activation,
    Detection,
    Scarcity,
    History,
}

impl Field {
    /// The text `card` has for this field, or `None` if it has none.
    /// [`Field::Combat`] has no text of its own; it is rendered from state.
    pub fn text(self, card: &Card) -> Option<String> {
        let own = |value: Option<&'static str>| value.map(str::to_string);
        match self {
            Field::Variation => own(card.variation),
            Field::Movement => own(card.movement_text),
            Field::PlayStyle => own(card.play_style),
            Field::DieRolls => {
                if !card.die.is_empty() {
                    Some(format!("Always Rolls: {}", card.die))
                } else if card.atk_die.is_some() || card.def_die.is_some() {
                    Some(format!(
                        "Atk: {} Def: {}",
                        card.atk_die.unwrap_or(""),
                        card.def_die.unwrap_or("")
                    ))
                } else {
                    None
                }
            }
            Field::Combat => None,
            Field::BrawlPower => {
                if card.brawl_sequence.is_empty() {
                    None
                } else {
                    Some(card.brawl_sequence.to_string())
                }
            }
            Field::Habitat => own(card.habitat),
            Field::Genetics => own(card.genetic_disposition),
            Field::Motivation => own(card.motivation),
            Field::Criteria => own(card.criteria),
            Field::Success => own(card.success),
            Field::Failure => own(card.failure),
            Field::Activation => own(card.activation),
            Field::Detection => own(card.detection),
            Field::Scarcity => own(card.scarcity),
            Field::History => own(card.description),
        }
    }
}

// ── Card types as compositions ─────────────────────────────────────

/// A field as one card type shows it.
#[derive(Debug, Clone, Copy)]
pub struct Row {
    pub field: Field,
    /// Shown above the value. Styled with `capitalize`, so case here only
    /// matters after the first letter of each word.
    pub label: &'static str,
}

const fn row(field: Field, label: &'static str) -> Row {
    Row { field, label }
}

/// A card type: a name (the card file's `layout`) and the groups of rows it
/// is composed of, in display order.
#[derive(Debug, Clone, Copy)]
pub struct CardType {
    pub name: &'static str,
    pub groups: &'static [&'static [Row]],
}

impl CardType {
    /// The type's rows in display order.
    pub fn rows(&self) -> impl Iterator<Item = &'static Row> {
        self.groups.iter().flat_map(|group| group.iter())
    }
}

// Groups shared between types.

/// What every card opens with.
const IDENTITY: &[Row] = &[row(Field::Variation, "card variation")];

/// What every card closes with.
const PROVENANCE: &[Row] = &[
    row(Field::Scarcity, "Scarcity"),
    row(Field::History, "History"),
];

/// A card that moves and fights: Characters and Species.
const COMBATANT: &[Row] = &[
    row(Field::Movement, "movement range"),
    row(Field::PlayStyle, "game effects"),
    row(Field::DieRolls, "die rolls"),
    row(Field::Combat, "Combat"),
    row(Field::BrawlPower, "brawl power"),
    row(Field::Habitat, "Habitat"),
];

// Groups particular to one type.

const CHARACTER_NATURE: &[Row] = &[
    row(Field::Genetics, "archetypal adaptation"),
    row(Field::Motivation, "motivation"),
];

const SPECIES_NATURE: &[Row] = &[
    row(Field::Genetics, "archetypal gene"),
    row(Field::Motivation, "motivation"),
];

const ITEM_USE: &[Row] = &[row(Field::PlayStyle, "uncrafted use")];

const PERSONAL_EFFECT: &[Row] = &[row(Field::PlayStyle, "entire game effect")];

const TERRAIN_EFFECT: &[Row] = &[row(Field::PlayStyle, "game effects")];

const SABOTAGE_PLAY: &[Row] = &[
    row(Field::Criteria, "Success Criteria"),
    row(Field::Success, "Successful Sabotage Result"),
    row(Field::Failure, "Failure Consequence"),
];

const TRAP_PLAY: &[Row] = &[
    // Optional: most traps have none, and a row without text is left out.
    row(Field::PlayStyle, "game effects"),
    row(Field::Criteria, "Activation-detection Criteria"),
    row(Field::Activation, "On Activation"),
    row(Field::Detection, "On detection"),
];

/// Every card type. `name` is the `layout` of the card files of that type.
pub const CARD_TYPES: &[CardType] = &[
    CardType {
        name: "Character",
        groups: &[IDENTITY, COMBATANT, CHARACTER_NATURE, PROVENANCE],
    },
    CardType {
        name: "Species",
        groups: &[IDENTITY, COMBATANT, SPECIES_NATURE, PROVENANCE],
    },
    CardType {
        name: "Item",
        groups: &[IDENTITY, ITEM_USE, PROVENANCE],
    },
    CardType {
        name: "PersonalEffect",
        groups: &[IDENTITY, PERSONAL_EFFECT, PROVENANCE],
    },
    CardType {
        name: "TerrainFeature",
        groups: &[IDENTITY, TERRAIN_EFFECT, PROVENANCE],
    },
    CardType {
        name: "Sabotage",
        groups: &[IDENTITY, SABOTAGE_PLAY, PROVENANCE],
    },
    CardType {
        name: "Trap",
        groups: &[IDENTITY, TRAP_PLAY, PROVENANCE],
    },
    // A card with no type of its own (the Incubation Egg): identity and
    // provenance only.
    CardType {
        name: "card",
        groups: &[IDENTITY, PROVENANCE],
    },
];

/// The card type for a card file's `layout`.
pub fn card_type(layout: &str) -> Option<&'static CardType> {
    CARD_TYPES.iter().find(|t| t.name == layout)
}

// ── Rendering ──────────────────────────────────────────────────────

fn find_card(slug: &str) -> Option<&'static Card> {
    CARDS.iter().find(|c| c.slug == slug)
}

/// One labelled row: the label, two rules, and the value.
fn render_row(html: &mut String, label: &str, value: &str) {
    html.push_str(r#"<div class="z-30 w-11/12 md:w-2/3 xl:w-1/2 pb-2 place-self-center">"#);
    html.push_str(&format!(r#"<p class="capitalize">{}</p>"#, label));
    html.push_str(
        r#"<div class="mt-1 h-0.5 w-1/2 bg-kip-red lg:bg-slate-400 justify-self-center"></div>"#,
    );
    html.push_str(r#"<div class="mt-1 h-0.5 w-full bg-slate-200 lg:bg-kip-red"></div>"#);
    html.push_str(&format!(r#"<p class="text-center">{}</p>"#, value));
    html.push_str("</div>");
}

/// Render the detail panel of the card with this slug.
pub fn render_card_panel(slug: &str) -> String {
    match find_card(slug) {
        Some(card) => render_panel(card),
        None => format!(
            r#"<span class="text-kip-red">Card not found: {}</span>"#,
            escape_html(slug)
        ),
    }
}

/// Render a card's detail panel: its title, the rows of its type that it has
/// text for, its damage tracker if it fights, and any promotion text.
///
/// A row whose field the card has no text for is left out.
pub fn render_panel(card: &Card) -> String {
    let Some(card_type) = card_type(card.layout) else {
        return format!(
            r#"<span class="text-kip-red">Unknown card type: {}</span>"#,
            escape_html(card.layout)
        );
    };

    let mut html = String::with_capacity(4096);

    html.push_str(&format!(
        r#"<h1 class="z-30 text-2xl relative top-0 md:text-4xl py-5 place-self-center text-center">{}</h1>"#,
        card.title
    ));

    for row in card_type.rows() {
        if row.field == Field::Combat {
            // The tracker draws its own heading and is rendered from the
            // player's damage state. Inline, so a tap redraws this panel.
            html.push_str(&format!(
                r#"<div id="keal-damage-{}" class="w-full grid grid-cols-1 place-self-center">"#,
                card.slug
            ));
            html.push_str(&damage::render_damage_tracker(card.slug));
            html.push_str("</div>");
        } else if let Some(value) = row.field.text(card) {
            render_row(&mut html, row.label, &value);
        }
    }

    html.push_str(&format!(
        r#"<p class="font-hairline text-slate-200 text-sm mt-6">{}</p>"#,
        card.promotion.unwrap_or("")
    ));
    html.push_str(&format!(
        r#"<p class="font-hairline text-slate-300 text-xs italic">{}</p>"#,
        card.promotion_conditions.unwrap_or("")
    ));

    html
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::player_doc;

    fn reset_state() {
        player_doc::init_player_doc();
    }

    fn card(slug: &str) -> &'static Card {
        find_card(slug).unwrap_or_else(|| panic!("{} is in the catalog", slug))
    }

    /// The labels of a rendered panel's rows, in order.
    fn labels(html: &str) -> Vec<String> {
        html.split(r#"<p class="capitalize">"#)
            .skip(1)
            .map(|rest| rest.split("</p>").next().unwrap().to_string())
            .collect()
    }

    /// Every field a card has text for, whether or not its type shows it.
    const ALL_TEXT_FIELDS: &[Field] = &[
        Field::Variation,
        Field::Movement,
        Field::PlayStyle,
        Field::DieRolls,
        Field::BrawlPower,
        Field::Habitat,
        Field::Genetics,
        Field::Motivation,
        Field::Criteria,
        Field::Success,
        Field::Failure,
        Field::Activation,
        Field::Detection,
        Field::Scarcity,
        Field::History,
    ];

    // ── Schema ─────────────────────────────────────────────────────

    #[test]
    fn every_card_has_a_type() {
        for c in CARDS.iter() {
            assert!(
                card_type(c.layout).is_some(),
                "{} has layout {:?}, which is not a card type",
                c.slug,
                c.layout
            );
        }
    }

    #[test]
    fn type_names_are_unique_and_every_type_is_in_use() {
        for t in CARD_TYPES {
            assert_eq!(
                CARD_TYPES.iter().filter(|o| o.name == t.name).count(),
                1,
                "{} is defined more than once",
                t.name
            );
            assert!(
                CARDS.iter().any(|c| c.layout == t.name),
                "no card has type {}: retire it or add a card",
                t.name
            );
        }
    }

    #[test]
    fn a_type_shows_a_field_at_most_once() {
        for t in CARD_TYPES {
            let fields: Vec<Field> = t.rows().map(|r| r.field).collect();
            for f in &fields {
                assert_eq!(
                    fields.iter().filter(|o| *o == f).count(),
                    1,
                    "{} shows {:?} more than once",
                    t.name,
                    f
                );
            }
        }
    }

    #[test]
    fn every_type_opens_with_identity_and_closes_with_provenance() {
        for t in CARD_TYPES {
            let fields: Vec<Field> = t.rows().map(|r| r.field).collect();
            assert_eq!(fields.first(), Some(&Field::Variation), "{}", t.name);
            assert_eq!(
                &fields[fields.len() - 2..],
                [Field::Scarcity, Field::History],
                "{}",
                t.name
            );
        }
    }

    #[test]
    fn only_types_that_fight_have_the_combat_field() {
        for t in CARD_TYPES {
            let fights = t.rows().any(|r| r.field == Field::Combat);
            assert_eq!(fights, t.name == "Character" || t.name == "Species", "{}", t.name);
        }
        // And every card with keal means is of a type that shows them.
        for c in CARDS.iter().filter(|c| !c.keal_means.is_empty()) {
            let t = card_type(c.layout).unwrap();
            assert!(t.rows().any(|r| r.field == Field::Combat), "{}", c.slug);
        }
    }

    #[test]
    fn no_card_has_text_its_type_does_not_show() {
        // Text in a card file that its type has no row for is never seen by a
        // player. Any such case fails here instead of going unnoticed: either
        // the type should show the field, or the text should come out of the
        // card file. (This is how Timebattle's `play_style` was found: no Trap
        // showed that field until it was added to the Trap's rows.)
        const KNOWN: &[(&str, Field)] = &[];

        let mut unseen: Vec<(&str, Field)> = Vec::new();
        for c in CARDS.iter() {
            let shown: Vec<Field> = card_type(c.layout).unwrap().rows().map(|r| r.field).collect();
            for f in ALL_TEXT_FIELDS {
                if f.text(c).is_some() && !shown.contains(f) {
                    unseen.push((c.slug, *f));
                }
            }
        }
        assert_eq!(unseen, KNOWN);
    }

    #[test]
    fn every_card_has_the_text_its_type_always_shows() {
        // Identity and provenance are on every card; a card missing them
        // would render with a gap.
        for c in CARDS.iter() {
            for f in [Field::Variation, Field::Scarcity, Field::History] {
                assert!(f.text(c).is_some(), "{} has no {:?}", c.slug, f);
            }
        }
    }

    // ── Rendering ──────────────────────────────────────────────────

    #[test]
    fn every_card_renders_with_its_title_and_no_error() {
        reset_state();
        for c in CARDS.iter() {
            let html = render_card_panel(c.slug);
            assert!(html.contains(&format!(">{}</h1>", c.title)), "{}", c.slug);
            assert!(!html.contains("Card not found"), "{}", c.slug);
            assert!(!html.contains("Unknown card type"), "{}", c.slug);
        }
        reset_state();
    }

    #[test]
    fn a_character_shows_its_rows_in_order() {
        reset_state();
        let html = render_card_panel("brox_the_defiant");
        assert_eq!(
            labels(&html),
            [
                "card variation",
                "movement range",
                "game effects",
                "die rolls",
                "Combat", // the tracker's own heading
                "brawl power",
                "Habitat",
                "archetypal adaptation",
                "motivation",
                "Scarcity",
                "History",
            ]
        );
        reset_state();
    }

    #[test]
    fn a_species_calls_its_genetics_a_gene() {
        reset_state();
        let species = CARDS.iter().find(|c| c.layout == "Species" && !c.hidden).unwrap();
        let html = render_card_panel(species.slug);
        assert!(labels(&html).contains(&"archetypal gene".to_string()));
        assert!(!labels(&html).contains(&"archetypal adaptation".to_string()));
        reset_state();
    }

    #[test]
    fn each_remaining_type_shows_its_own_rows() {
        reset_state();
        let first = |layout: &str| CARDS.iter().find(|c| c.layout == layout).unwrap().slug;
        assert_eq!(
            labels(&render_card_panel(first("Item"))),
            ["card variation", "uncrafted use", "Scarcity", "History"]
        );
        assert_eq!(
            labels(&render_card_panel(first("PersonalEffect"))),
            ["card variation", "entire game effect", "Scarcity", "History"]
        );
        assert_eq!(
            labels(&render_card_panel(first("TerrainFeature"))),
            ["card variation", "game effects", "Scarcity", "History"]
        );
        assert_eq!(
            labels(&render_card_panel(first("Sabotage"))),
            [
                "card variation",
                "Success Criteria",
                "Successful Sabotage Result",
                "Failure Consequence",
                "Scarcity",
                "History"
            ]
        );
        // A trap's game effects are optional: shown when the card has them.
        let plain_trap = CARDS
            .iter()
            .find(|c| c.layout == "Trap" && c.play_style.is_none())
            .expect("a trap without game effects");
        assert_eq!(
            labels(&render_card_panel(plain_trap.slug)),
            [
                "card variation",
                "Activation-detection Criteria",
                "On Activation",
                "On detection",
                "Scarcity",
                "History"
            ]
        );
        assert_eq!(
            labels(&render_card_panel("timebattle")),
            [
                "card variation",
                "game effects",
                "Activation-detection Criteria",
                "On Activation",
                "On detection",
                "Scarcity",
                "History"
            ]
        );
        assert_eq!(
            labels(&render_card_panel("incubation_egg")),
            ["card variation", "Scarcity", "History"]
        );
        reset_state();
    }

    #[test]
    fn die_rolls_shows_one_die_or_separate_attack_and_defence_dice() {
        let one = CARDS.iter().find(|c| !c.die.is_empty()).unwrap();
        assert_eq!(
            Field::DieRolls.text(one),
            Some(format!("Always Rolls: {}", one.die))
        );
        let split = CARDS.iter().find(|c| c.die.is_empty() && c.atk_die.is_some()).unwrap();
        assert_eq!(
            Field::DieRolls.text(split),
            Some(format!(
                "Atk: {} Def: {}",
                split.atk_die.unwrap(),
                split.def_die.unwrap()
            ))
        );
        // A card with no dice at all has no die rolls row.
        assert_eq!(Field::DieRolls.text(card("cloth")), None);
    }

    #[test]
    fn card_text_keeps_the_markup_its_author_wrote() {
        reset_state();
        // Some card text is written with markup; it must reach the page as
        // markup, not as escaped text.
        let marked = CARDS
            .iter()
            .find(|c| c.criteria.is_some_and(|t| t.contains("<b>")))
            .expect("a card whose criteria contains <b>");
        let html = render_card_panel(marked.slug);
        assert!(html.contains(marked.criteria.unwrap()));
        assert!(!html.contains("&lt;b&gt;"));
        reset_state();
    }

    #[test]
    fn a_fighting_card_has_its_tracker_inline_and_tappable() {
        reset_state();
        let html = render_card_panel("brox_the_defiant");
        // The tracker is inside the panel, not a view of its own.
        assert!(html.contains(r#"<div id="keal-damage-brox_the_defiant""#));
        assert!(!html.contains("data-engine-view"));
        assert!(html.contains(
            r#"data-engine-post="/api/game/damage" data-engine-body="card=brox_the_defiant&slot=1""#
        ));
        assert!(html.contains("Crushing Hope"));
        // It sits between die rolls and brawl power.
        let at = |needle: &str| html.find(needle).unwrap_or_else(|| panic!("{} present", needle));
        assert!(at(">die rolls<") < at("keal-damage-brox_the_defiant"));
        assert!(at("keal-damage-brox_the_defiant") < at(">brawl power<"));
        reset_state();
    }

    #[test]
    fn the_panel_shows_the_players_damage() {
        reset_state();
        damage::toggle_slot("brox_the_defiant", 2);
        let html = render_card_panel("brox_the_defiant");
        assert!(html.contains(r#"data-slot="2" aria-pressed="true""#));
        assert!(html.contains(r#"data-slot="1" aria-pressed="false""#));
        reset_state();
    }

    #[test]
    fn a_card_that_does_not_fight_has_no_tracker() {
        reset_state();
        let html = render_card_panel("cloth");
        assert!(!html.contains("keal-damage-"));
        assert!(!html.contains("data-engine-post"));
        reset_state();
    }

    #[test]
    fn promotion_text_is_small_print_at_the_foot_of_a_card_that_has_it() {
        reset_state();
        // No card in the catalog carries a promotion at present, so one is
        // made up here from a real card.
        let promoted = Card {
            promotion: Some("Mention this card for a gift."),
            promotion_conditions: Some("Limit 1 per customer."),
            ..card("cloth").clone()
        };
        let html = render_panel(&promoted);
        assert!(html.ends_with(concat!(
            r#"<p class="font-hairline text-slate-200 text-sm mt-6">Mention this card for a gift.</p>"#,
            r#"<p class="font-hairline text-slate-300 text-xs italic">Limit 1 per customer.</p>"#,
        )));
        // Without one, the two paragraphs are still there, empty: they carry
        // the panel's bottom spacing.
        assert!(render_card_panel("cloth").ends_with(concat!(
            r#"<p class="font-hairline text-slate-200 text-sm mt-6"></p>"#,
            r#"<p class="font-hairline text-slate-300 text-xs italic"></p>"#,
        )));
        reset_state();
    }

    #[test]
    fn a_card_of_an_unknown_type_says_so() {
        let odd = Card {
            layout: "Vehicle",
            ..card("cloth").clone()
        };
        assert!(render_panel(&odd).contains("Unknown card type: Vehicle"));
    }

    #[test]
    fn the_panel_has_no_bindings_or_scripts() {
        reset_state();
        for c in CARDS.iter() {
            let html = render_card_panel(c.slug);
            for forbidden in ["x-data", "x-show", ":class", "@click", "onclick", "<script", "hx-get", "hx-post"] {
                assert!(!html.contains(forbidden), "{} panel contains {}", c.slug, forbidden);
            }
        }
        reset_state();
    }

    #[test]
    fn the_same_state_renders_the_same_markup() {
        reset_state();
        damage::toggle_slot("brox_the_defiant", 1);
        assert_eq!(
            render_card_panel("brox_the_defiant"),
            render_card_panel("brox_the_defiant")
        );
        reset_state();
    }

    #[test]
    fn an_unknown_slug_is_escaped_when_echoed() {
        let html = render_card_panel("<img src=x onerror=alert(1)>");
        assert!(html.contains("Card not found"));
        assert!(!html.contains("<img"));
    }
}
