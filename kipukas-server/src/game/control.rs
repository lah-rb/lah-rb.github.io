//! Whether a species is under its player's control.
//!
//! A species has a tameability, from 0 to 10. What the player brings to it
//! is a stack of three parts, which come to 10 at most:
//!
//! ```text
//! affinity      0 or 3   the archetype declared for this game is the species' own
//! card effects  0 to 3   what cards in play add, this game: the player says
//! loyalty       0 to 4   the days the player has played this card on
//! ```
//!
//! The species is under control when the stack comes to its tameability, and
//! rogue (under its own influence) when it does not. A tameability of 0
//! needs no taming, and one above 10 can never be met: that species is
//! always rogue. Characters need no taming and have no tameability.
//!
//! Affinity and card effects are the game's (New Game clears them). Loyalty
//! is the player's record: the only part that carries from game to game. A
//! card comes to its full loyalty over a number of days of play that the
//! card may name for itself (`loyalty_days`, which no page shows) and gains
//! a point at each quarter of it.
//!
//! The owner's framework, November 2026. The numbers are theirs and are
//! meant to be played with: each is one constant here.

use crate::catalog::Card;
use crate::game::player_doc;

/// What declaring a species' archetype adds to its stack.
pub const AFFINITY_POINTS: u8 = 3;
/// The most card effects in play can add.
pub const CARD_EFFECTS_MAX: u8 = 3;
/// The most loyalty can add.
pub const LOYALTY_MAX: u8 = 4;
/// The most a stack can come to.
pub const STACK_MAX: u32 = (AFFINITY_POINTS + CARD_EFFECTS_MAX + LOYALTY_MAX) as u32;
/// Days of play for full loyalty, where a card names no number of its own.
pub const LOYALTY_DAYS: u32 = 12;

/// What the player brings to a species.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stack {
    pub affinity: u8,
    pub effects: u8,
    pub loyalty: u8,
}

impl Stack {
    pub fn total(&self) -> u32 {
        self.affinity as u32 + self.effects as u32 + self.loyalty as u32
    }
}

/// Where a species stands with its player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// It needs no taming.
    Free,
    /// The stack comes to its tameability: it is under the player's control.
    Held,
    /// It does not: the species is under its own influence.
    Rogue,
    /// No stack can: the species is always rogue.
    Untameable,
}

/// A species, its tameability, and what the player brings to it now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Control {
    pub standing: Standing,
    pub stack: Stack,
    pub tameability: u32,
}

/// The loyalty points of a card played on `days` days, which comes to its
/// full loyalty in `full` days: a point at each quarter of them.
pub fn loyalty_points(days: u32, full: u32) -> u8 {
    let quarters = LOYALTY_MAX as u64 * days as u64 / full.max(1) as u64;
    quarters.min(LOYALTY_MAX as u64) as u8
}

/// What the player brings to this card now.
pub fn stack(card: &Card) -> Stack {
    let declared = player_doc::get_active_affinity().map(|(archetype, _)| archetype);
    let own = declared.is_some() && declared.as_deref() == card.genetic_disposition;
    let days = player_doc::get_loyalty(card.slug).map(|(days, _)| days).unwrap_or(0);
    Stack {
        affinity: if own { AFFINITY_POINTS } else { 0 },
        effects: player_doc::get_effects(card.slug).min(CARD_EFFECTS_MAX),
        loyalty: loyalty_points(days, card.loyalty_days.unwrap_or(LOYALTY_DAYS)),
    }
}

/// Where this card stands with its player. None for a card that is not a
/// species with a tameability.
pub fn of(card: &Card) -> Option<Control> {
    let tameability = card.tamability.filter(|_| card.layout == "Species")?;
    let stack = stack(card);
    let standing = match tameability {
        0 => Standing::Free,
        more if more > STACK_MAX => Standing::Untameable,
        needed if stack.total() >= needed => Standing::Held,
        _ => Standing::Rogue,
    };
    Some(Control { standing, stack, tameability })
}

/// Add `by` (1 or -1, say) to the points card effects in play give this
/// card, within what they can give. Nothing for a card that has no stack.
pub fn adjust_effects(card: &Card, by: i32) {
    if of(card).is_none() {
        return;
    }
    let now = player_doc::get_effects(card.slug).min(CARD_EFFECTS_MAX) as i32;
    let wanted = (now + by).clamp(0, CARD_EFFECTS_MAX as i32) as u8;
    if wanted != player_doc::get_effects(card.slug) {
        player_doc::set_effects(card.slug, wanted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CARDS;

    /// A species of the binder with this tameability.
    fn species(tameability: u32) -> &'static Card {
        CARDS
            .iter()
            .find(|card| card.layout == "Species" && card.tamability == Some(tameability))
            .unwrap_or_else(|| panic!("no species of tameability {}", tameability))
    }

    fn reset() {
        player_doc::init_player_doc();
    }

    #[test]
    fn the_parts_of_a_stack_come_to_ten_at_most() {
        assert_eq!(STACK_MAX, 10);
        assert_eq!((AFFINITY_POINTS, CARD_EFFECTS_MAX, LOYALTY_MAX), (3, 3, 4));
    }

    #[test]
    fn a_card_gains_a_point_of_loyalty_at_each_quarter_of_its_days() {
        let points: Vec<u8> = (0..=14).map(|days| loyalty_points(days, 12)).collect();
        assert_eq!(points, [0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4]);
        // Days that do not divide by four: a point once each quarter is passed.
        let points: Vec<u8> = (0..=10).map(|days| loyalty_points(days, 10)).collect();
        assert_eq!(points, [0, 0, 0, 1, 1, 2, 2, 2, 3, 3, 4]);
        // However long it is played, and however short its days.
        assert_eq!(loyalty_points(5000, 12), 4);
        assert_eq!(loyalty_points(1, 1), 4);
        assert_eq!(loyalty_points(0, 0), 0);
        assert_eq!(loyalty_points(u32::MAX, u32::MAX), 4);
    }

    #[test]
    fn a_species_nobody_has_played_is_rogue_until_something_is_brought_to_it() {
        reset();
        let card = species(5);
        let control = of(card).unwrap();
        assert_eq!((control.standing, control.tameability), (Standing::Rogue, 5));
        assert_eq!(control.stack, Stack::default());
        reset();
    }

    #[test]
    fn each_part_adds_to_the_stack_and_the_species_is_held_when_it_comes_to_its_tameability() {
        reset();
        let card = species(5);
        // Its own archetype declared: three.
        player_doc::declare_affinity(card.genetic_disposition.unwrap(), "2026-11-21").unwrap();
        assert_eq!(of(card).unwrap().stack, Stack { affinity: 3, effects: 0, loyalty: 0 });
        assert_eq!(of(card).unwrap().standing, Standing::Rogue);
        // A card in play adds one.
        adjust_effects(card, 1);
        assert_eq!(of(card).unwrap().stack.total(), 4);
        // A quarter of its days played: one more, and that is five.
        for day in 1..=3 {
            player_doc::increment_loyalty(card.slug, &format!("2026-11-{:02}", day)).unwrap();
        }
        let control = of(card).unwrap();
        assert_eq!(control.stack, Stack { affinity: 3, effects: 1, loyalty: 1 });
        assert_eq!(control.standing, Standing::Held, "equal is enough");
        // The card in play is lost: rogue again.
        adjust_effects(card, -1);
        assert_eq!(of(card).unwrap().standing, Standing::Rogue);
        reset();
    }

    #[test]
    fn another_archetype_declared_adds_nothing() {
        reset();
        let card = species(5);
        let other = player_doc::valid_archetypes().iter().find(|a| Some(**a) != card.genetic_disposition).unwrap();
        player_doc::declare_affinity(other, "2026-11-21").unwrap();
        assert_eq!(of(card).unwrap().stack.affinity, 0);
        reset();
    }

    #[test]
    fn card_effects_stay_within_what_they_can_give() {
        reset();
        let card = species(5);
        for _ in 0..7 {
            adjust_effects(card, 1);
        }
        assert_eq!(of(card).unwrap().stack.effects, CARD_EFFECTS_MAX);
        for _ in 0..7 {
            adjust_effects(card, -1);
        }
        assert_eq!(of(card).unwrap().stack.effects, 0);
        // More than they can give, however it came to be stored, counts as the most.
        player_doc::set_effects(card.slug, 9);
        assert_eq!(of(card).unwrap().stack.effects, CARD_EFFECTS_MAX);
        adjust_effects(card, -1);
        assert_eq!(of(card).unwrap().stack.effects, CARD_EFFECTS_MAX - 1);
        reset();
    }

    #[test]
    fn new_game_clears_affinity_and_card_effects_and_keeps_loyalty() {
        reset();
        let card = species(5);
        player_doc::declare_affinity(card.genetic_disposition.unwrap(), "2026-11-21").unwrap();
        adjust_effects(card, 2);
        for day in 1..=6 {
            player_doc::increment_loyalty(card.slug, &format!("2026-11-{:02}", day)).unwrap();
        }
        assert_eq!(of(card).unwrap().stack, Stack { affinity: 3, effects: 2, loyalty: 2 });
        // What New Game does.
        player_doc::clear_all();
        player_doc::clear_affinity();
        assert_eq!(of(card).unwrap().stack, Stack { affinity: 0, effects: 0, loyalty: 2 });
        reset();
    }

    #[test]
    fn a_species_of_no_tameability_needs_no_taming_and_one_beyond_any_stack_is_always_rogue() {
        reset();
        assert_eq!(of(species(0)).unwrap().standing, Standing::Free);
        let mut beyond = species(5).clone();
        beyond.tamability = Some(STACK_MAX + 1);
        // Everything a player can bring: still rogue.
        player_doc::declare_affinity(beyond.genetic_disposition.unwrap(), "2026-11-21").unwrap();
        player_doc::set_effects(beyond.slug, CARD_EFFECTS_MAX);
        for day in 1..=28 {
            player_doc::increment_loyalty(beyond.slug, &format!("2026-10-{:02}", day)).unwrap();
        }
        let control = of(&beyond).unwrap();
        assert_eq!(control.stack.total(), STACK_MAX);
        assert_eq!(control.standing, Standing::Untameable);
        // The hardest there can be is met by all of it.
        beyond.tamability = Some(STACK_MAX);
        assert_eq!(of(&beyond).unwrap().standing, Standing::Held);
        reset();
    }

    #[test]
    fn a_card_that_names_its_own_days_comes_to_its_loyalty_by_them() {
        reset();
        let mut card = species(5).clone();
        card.loyalty_days = Some(4);
        player_doc::increment_loyalty(card.slug, "2026-11-01").unwrap();
        assert_eq!(of(&card).unwrap().stack.loyalty, 1);
        card.loyalty_days = Some(40);
        assert_eq!(of(&card).unwrap().stack.loyalty, 0);
        reset();
    }

    #[test]
    fn characters_and_cards_that_are_not_souls_have_no_stack() {
        reset();
        for card in CARDS.iter().filter(|card| card.layout != "Species") {
            assert_eq!(of(card), None, "{}", card.slug);
            adjust_effects(card, 1);
            assert_eq!(player_doc::get_effects(card.slug), 0, "{}", card.slug);
        }
        reset();
    }
}
