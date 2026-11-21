//! More than one copy of a card in play.
//!
//! A player may have the same card on the board twice. Each copy takes its
//! own damage and has its own points from card effects, so each is kept as
//! a card in play of its own: the first under the card's name, as a card
//! always was, and the others under the name and their number
//! (`brox_the_defiant~2`). That written form is a card's *key*, and it is
//! what travels wherever a card's name did: in a view's buttons, and in
//! what they post. Looking a card up reads the name out of it
//! ([`slug_of`]); the player's document keeps what is kept under the whole.
//!
//! Which copy a page shows is in its address, after the `#`
//! (`/brox_the_defiant/#2`), and nowhere else: turning to another copy is
//! following a link, and costs the player's document nothing.
//!
//! A copy's loyalty is the card's: the player has played the card, whichever
//! copy it was. Copies are the game's, like the damage they carry. New Game
//! clears them all.
//!
//! In a room a copy can be handed to the other player for the rest of the
//! game: a soul that is stolen, or tamed by the other side. It leaves this
//! player's side as it is ([`hand_out`]) and arrives on the other's as a
//! copy in play there, marked as a guest ([`arrive`]). What carries it
//! between the two is the room's document (`game::handed`). Handed back, it
//! is at home again and no longer a guest.

use crate::catalog::Card;
use crate::game::player_doc::{self, CardState};

/// The most copies of one card in play. Far more than the rules allow a
/// deck ([`limit`]): this is only where the binder stops counting.
pub const MOST: u8 = 9;

/// What parts a card's name from a copy's number in a key.
const MARK: char = '~';

/// The key of a copy of a card: the card's name for the first.
pub fn key(slug: &str, copy: u8) -> String {
    if copy <= 1 {
        slug.to_string()
    } else {
        format!("{}{}{}", slug, MARK, copy)
    }
}

/// A key's card and copy. A key with no number, or one that is not a
/// number of a copy, is the first.
pub fn split(key: &str) -> (&str, u8) {
    match key.split_once(MARK) {
        Some((slug, copy)) => (slug, copy.parse().ok().filter(|copy| (2..=MOST).contains(copy)).unwrap_or(1)),
        None => (key, 1),
    }
}

/// The card a key is of.
pub fn slug_of(key: &str) -> &str {
    split(key).0
}

/// How many slots of damage a card has.
fn slots_of(card: &Card) -> u8 {
    card.keal_means.iter().map(|means| means.count).sum()
}

/// Whether more than one copy of this card can be in play: only a card
/// that fights has anything to keep of a copy.
pub fn can_have(card: &Card) -> bool {
    slots_of(card) > 0
}

/// How many copies of a card the rules allow in a deck: one of a
/// character, two of a species (Card Copy Limits, in the rules book). None
/// for a card that has no copies in play to count.
///
/// It is not enforced. A soul can be stolen or crafted in play, and a
/// table may have rules of its own, so the binder only says when a player
/// is over it ([`over_limit`]).
pub fn limit(card: &Card) -> Option<u8> {
    match card.layout {
        "Character" => Some(1),
        "Species" => Some(2),
        _ => None,
    }
}

/// Whether more copies of this card are in play than the rules allow a deck.
pub fn over_limit(card: &Card) -> bool {
    limit(card).is_some_and(|limit| count(card.slug) > limit)
}

/// How many copies of a card are in play: one, unless more were added.
pub fn count(slug: &str) -> u8 {
    let mut copies = 1;
    while copies < MOST && player_doc::has_card_state(&key(slug, copies + 1)) {
        copies += 1;
    }
    copies
}

/// Whether a key is of a copy that is in play.
pub fn in_play(key: &str) -> bool {
    let (slug, copy) = split(key);
    key == self::key(slug, copy) && copy <= count(slug)
}

/// The copy a page's address asks for (what follows its `#`), where there
/// is such a copy in play: the first otherwise.
pub fn shown(slug: &str, asked: &str) -> u8 {
    asked.trim().parse().ok().filter(|copy| (1..=count(slug)).contains(copy)).unwrap_or(1)
}

/// Put another copy of a card in play, undamaged. Its number, or none if
/// the card cannot have copies or has as many as there can be.
pub fn add(card: &Card) -> Option<u8> {
    let copies = count(card.slug);
    if !can_have(card) || copies >= MOST {
        return None;
    }
    // The first copy may never have been looked at: it is kept from here
    // on, so that the others are counted from it.
    player_doc::ensure_card_state(card.slug, slots_of(card));
    player_doc::ensure_card_state(&key(card.slug, copies + 1), slots_of(card));
    Some(copies + 1)
}

/// Take a copy of a card out of play. The copies after it move up one, so
/// that they stay numbered from one with none missing. The last copy left
/// cannot be taken out. Whether one was.
pub fn remove(card: &Card, copy: u8) -> bool {
    let copies = count(card.slug);
    if copies < 2 || !(1..=copies).contains(&copy) {
        return false;
    }
    for moved in copy..copies {
        let from = player_doc::card_state(&key(card.slug, moved + 1)).unwrap_or_default();
        player_doc::put_card_state(&key(card.slug, moved), &from);
    }
    player_doc::clear_card(&key(card.slug, copies));
    true
}

/// What is kept of a copy in play, or nothing if it is not in play.
pub fn state(key: &str) -> Option<CardState> {
    if in_play(key) {
        player_doc::card_state(key)
    } else {
        None
    }
}

/// Whether the copy kept under `key` was handed over by the other player.
pub fn is_guest(key: &str) -> bool {
    state(key).is_some_and(|state| state.guest)
}

/// `state` as this card can have it: as many slots as the card has, and no
/// more from card effects than they can give. What arrives from the other
/// player is theirs to have written.
fn fitted(card: &Card, state: &CardState) -> CardState {
    let mut slots = state.slots.clone();
    slots.resize(slots_of(card) as usize, false);
    CardState { slots, effects: state.effects.min(crate::game::control::CARD_EFFECTS_MAX), ..state.clone() }
}

/// Take a copy of a card out of play to hand to the other player: what was
/// kept of it, as it is. The only copy of a card goes too: the card is then
/// as one that was never played. None if there is no such copy, or the card
/// cannot have copies.
pub fn hand_out(card: &Card, copy: u8) -> Option<CardState> {
    let copies = count(card.slug);
    if !can_have(card) || !(1..=copies).contains(&copy) {
        return None;
    }
    let state = fitted(card, &player_doc::card_state(&key(card.slug, copy)).unwrap_or_default());
    if copies > 1 {
        remove(card, copy);
    } else {
        player_doc::clear_card(card.slug);
    }
    Some(state)
}

/// A copy of a card comes into play here as `state` has it: one the other
/// player handed over, or one of this player's own coming back. Its number,
/// or none if the card cannot have copies or has as many as there can be.
///
/// It takes the place of the card's one copy if nothing has happened to
/// that one: a card that has only been looked at is kept the same way as
/// one in play, and is far the likelier. Otherwise it is a further copy.
pub fn arrive(card: &Card, state: &CardState) -> Option<u8> {
    if !can_have(card) {
        return None;
    }
    let copies = count(card.slug);
    let untouched = CardState { slots: vec![false; slots_of(card) as usize], ..CardState::default() };
    let first = player_doc::card_state(card.slug);
    let copy = if copies == 1 && first.is_none_or(|first| fitted(card, &first) == untouched) {
        1
    } else if copies < MOST {
        copies + 1
    } else {
        return None;
    };
    player_doc::put_card_state(&key(card.slug, copy), &fitted(card, state));
    Some(copy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CARDS;

    fn brox() -> &'static Card {
        CARDS.iter().find(|card| card.slug == "brox_the_defiant").unwrap()
    }

    fn reset() {
        player_doc::init_player_doc();
    }

    #[test]
    fn the_first_copy_is_kept_under_the_cards_name_and_the_others_under_name_and_number() {
        assert_eq!(key("brox_the_defiant", 1), "brox_the_defiant");
        assert_eq!(key("brox_the_defiant", 0), "brox_the_defiant");
        assert_eq!(key("brox_the_defiant", 2), "brox_the_defiant~2");
        assert_eq!(split("brox_the_defiant"), ("brox_the_defiant", 1));
        assert_eq!(split("brox_the_defiant~3"), ("brox_the_defiant", 3));
        assert_eq!(slug_of("brox_the_defiant~3"), "brox_the_defiant");
        // What is not the number of a copy is the first.
        for odd in ["brox_the_defiant~", "brox_the_defiant~x", "brox_the_defiant~1", "brox_the_defiant~0", "brox_the_defiant~10"] {
            assert_eq!(split(odd), ("brox_the_defiant", 1), "{}", odd);
        }
    }

    #[test]
    fn a_card_is_one_copy_until_another_is_added() {
        reset();
        assert_eq!(count("brox_the_defiant"), 1);
        assert_eq!(add(brox()), Some(2));
        assert_eq!(add(brox()), Some(3));
        assert_eq!(count("brox_the_defiant"), 3);
        // Each is a card in play of its own, undamaged.
        let slots: u8 = brox().keal_means.iter().map(|means| means.count).sum();
        for copy in 1..=3 {
            let state = player_doc::card_state(&key("brox_the_defiant", copy)).unwrap();
            assert_eq!(state, CardState { slots: vec![false; slots as usize], ..CardState::default() });
        }
        assert!(in_play("brox_the_defiant") && in_play("brox_the_defiant~3"));
        assert!(!in_play("brox_the_defiant~4") && !in_play("brox_the_defiant~1"));
        // Another card's copies are its own.
        assert_eq!(count("liliel_healing_fairy"), 1);
        reset();
    }

    #[test]
    fn there_are_at_most_nine_copies_and_only_of_a_card_that_fights() {
        reset();
        for copy in 2..=MOST {
            assert_eq!(add(brox()), Some(copy));
        }
        assert_eq!(add(brox()), None);
        assert_eq!(count("brox_the_defiant"), MOST);
        let trap = CARDS.iter().find(|card| card.keal_means.is_empty()).unwrap();
        assert!(!can_have(trap));
        assert_eq!(add(trap), None);
        assert_eq!(count(trap.slug), 1);
        reset();
    }

    #[test]
    fn a_player_is_over_the_rules_limit_with_two_of_a_character_or_three_of_a_species() {
        reset();
        let character = brox();
        let species = CARDS.iter().find(|card| card.layout == "Species" && can_have(card)).unwrap();
        assert_eq!((limit(character), limit(species)), (Some(1), Some(2)));
        assert!(!over_limit(character) && !over_limit(species));

        add(character);
        assert!(over_limit(character), "a second copy of a character");
        add(species);
        assert!(!over_limit(species), "two of a species are allowed");
        add(species);
        assert!(over_limit(species));
        // Being over it stops nothing: more can be added.
        assert_eq!(add(character), Some(3));
        // And taking copies out brings a player back within it.
        remove(species, 3);
        assert!(!over_limit(species));
        // A card that is not a soul has no limit here: it has no copies.
        let trap = CARDS.iter().find(|card| card.layout == "Trap").unwrap();
        assert_eq!(limit(trap), None);
        assert!(!over_limit(trap));
        reset();
    }

    #[test]
    fn each_copy_takes_its_own_damage() {
        reset();
        add(brox());
        player_doc::toggle_slot("brox_the_defiant~2", 1);
        player_doc::set_effects("brox_the_defiant~2", 2);
        assert!(player_doc::get_slot("brox_the_defiant~2", 1));
        assert!(!player_doc::get_slot("brox_the_defiant", 1));
        assert_eq!((player_doc::get_effects("brox_the_defiant"), player_doc::get_effects("brox_the_defiant~2")), (0, 2));
        reset();
    }

    #[test]
    fn taking_a_copy_out_moves_the_later_ones_up() {
        reset();
        add(brox());
        add(brox());
        // The first is damaged in its first slot, the second in its second,
        // the third is wasted.
        player_doc::toggle_slot("brox_the_defiant", 1);
        player_doc::toggle_slot("brox_the_defiant~2", 2);
        player_doc::toggle_wasted("brox_the_defiant~3");
        player_doc::set_effects("brox_the_defiant~3", 1);

        assert!(remove(brox(), 1));
        assert_eq!(count("brox_the_defiant"), 2);
        // What was the second is the first, and what was the third the second.
        assert!(!player_doc::get_slot("brox_the_defiant", 1) && player_doc::get_slot("brox_the_defiant", 2));
        assert!(player_doc::is_wasted("brox_the_defiant~2"));
        assert_eq!(player_doc::get_effects("brox_the_defiant~2"), 1);
        assert!(!player_doc::has_card_state("brox_the_defiant~3"));

        // The last of them: nothing moves.
        assert!(remove(brox(), 2));
        assert_eq!(count("brox_the_defiant"), 1);
        assert!(player_doc::get_slot("brox_the_defiant", 2));
        // The only copy left stays, and a copy that is not there is not taken out.
        assert!(!remove(brox(), 1));
        assert!(!remove(brox(), 2));
        assert_eq!(count("brox_the_defiant"), 1);
        reset();
    }

    /// A copy of Brox with its first slot marked and a point from card effects.
    fn a_hurt_brox() -> CardState {
        let slots: u8 = brox().keal_means.iter().map(|means| means.count).sum();
        let mut state = CardState { slots: vec![false; slots as usize], effects: 1, ..CardState::default() };
        state.slots[0] = true;
        state
    }

    #[test]
    fn handing_a_copy_out_takes_it_out_of_play_as_it_is() {
        reset();
        add(brox());
        player_doc::toggle_slot("brox_the_defiant~2", 1);
        player_doc::set_effects("brox_the_defiant~2", 1);
        assert_eq!(hand_out(brox(), 2), Some(a_hurt_brox()));
        assert_eq!(count("brox_the_defiant"), 1);
        // The only copy can go too. The card is then as one never played.
        player_doc::toggle_wasted("brox_the_defiant");
        let out = hand_out(brox(), 1).unwrap();
        assert!(out.wasted);
        assert!(!player_doc::has_card_state("brox_the_defiant"));
        // A card that was never looked at goes as a whole one.
        let whole = hand_out(brox(), 1).unwrap();
        assert_eq!(whole.slots.len(), a_hurt_brox().slots.len());
        assert!(!whole.slots.contains(&true) && !whole.wasted);
        // A copy there is not, and a card that does not fight.
        assert_eq!(hand_out(brox(), 2), None);
        let trap = CARDS.iter().find(|card| card.layout == "Trap").unwrap();
        assert_eq!(hand_out(trap, 1), None);
        reset();
    }

    #[test]
    fn a_copy_that_arrives_takes_the_place_of_a_card_nothing_has_happened_to() {
        reset();
        let guest = CardState { guest: true, ..a_hurt_brox() };
        // The card was never looked at here.
        assert_eq!(arrive(brox(), &guest), Some(1));
        assert_eq!(player_doc::card_state("brox_the_defiant"), Some(guest.clone()));
        assert!(is_guest("brox_the_defiant"));
        assert_eq!(count("brox_the_defiant"), 1);

        // It was looked at, and nothing more.
        reset();
        player_doc::ensure_card_state("brox_the_defiant", guest.slots.len() as u8);
        assert_eq!(arrive(brox(), &guest), Some(1));
        assert_eq!(count("brox_the_defiant"), 1);
        reset();
    }

    #[test]
    fn a_copy_that_arrives_beside_one_in_play_is_a_further_copy() {
        reset();
        let guest = CardState { guest: true, ..a_hurt_brox() };
        // This player's own Brox has taken damage.
        player_doc::ensure_card_state("brox_the_defiant", guest.slots.len() as u8);
        player_doc::toggle_slot("brox_the_defiant", 2);
        assert_eq!(arrive(brox(), &guest), Some(2));
        assert_eq!(count("brox_the_defiant"), 2);
        assert!(player_doc::get_slot("brox_the_defiant", 2) && !is_guest("brox_the_defiant"));
        assert_eq!(player_doc::card_state("brox_the_defiant~2"), Some(guest.clone()));
        // A second arrives: a third copy, though the first two are whole or not.
        assert_eq!(arrive(brox(), &guest), Some(3));
        // A guest that is whole is still a copy in play: another does not take its place.
        reset();
        let whole_guest = CardState { slots: vec![false; guest.slots.len()], guest: true, ..CardState::default() };
        assert_eq!(arrive(brox(), &whole_guest), Some(1));
        assert_eq!(arrive(brox(), &guest), Some(2));
        reset();
    }

    #[test]
    fn what_arrives_is_fitted_to_the_card() {
        reset();
        // Too many slots, and more from card effects than they can give.
        let odd = CardState { slots: vec![true; 40], wasted: true, effects: 200, guest: true };
        assert_eq!(arrive(brox(), &odd), Some(1));
        let kept = player_doc::card_state("brox_the_defiant").unwrap();
        assert_eq!(kept.slots.len(), a_hurt_brox().slots.len());
        assert_eq!(kept.effects, crate::game::control::CARD_EFFECTS_MAX);
        // Too few: the rest are whole.
        reset();
        arrive(brox(), &CardState { slots: vec![true], ..CardState::default() });
        let kept = player_doc::card_state("brox_the_defiant").unwrap();
        assert_eq!(kept.slots.iter().filter(|marked| **marked).count(), 1);
        assert_eq!(kept.slots.len(), a_hurt_brox().slots.len());
        // A card that does not fight takes nothing in, and nor does one at its most.
        let trap = CARDS.iter().find(|card| card.layout == "Trap").unwrap();
        assert_eq!(arrive(trap, &odd), None);
        reset();
        player_doc::toggle_slot("brox_the_defiant", 1);
        for _ in 1..MOST {
            add(brox());
        }
        assert_eq!(arrive(brox(), &odd), None);
        reset();
    }

    #[test]
    fn taking_a_copy_out_keeps_which_of_the_others_are_guests() {
        reset();
        // This player's own Brox has taken damage, and a guest is beside it.
        player_doc::ensure_card_state("brox_the_defiant", a_hurt_brox().slots.len() as u8);
        player_doc::toggle_slot("brox_the_defiant", 1);
        arrive(brox(), &CardState { guest: true, ..a_hurt_brox() });
        assert!(is_guest("brox_the_defiant~2"));
        remove(brox(), 1);
        assert!(is_guest("brox_the_defiant"), "the guest is the first copy now");
        reset();
    }

    #[test]
    fn the_copy_a_page_shows_is_the_one_its_address_asks_for_if_there_is_one() {
        reset();
        assert_eq!(shown("brox_the_defiant", ""), 1);
        assert_eq!(shown("brox_the_defiant", "2"), 1, "there is no second copy");
        add(brox());
        assert_eq!(shown("brox_the_defiant", "2"), 2);
        for other in ["3", "0", "x", "2x", "sync=ABCD", "-1"] {
            assert_eq!(shown("brox_the_defiant", other), 1, "{}", other);
        }
        reset();
    }

    #[test]
    fn new_game_leaves_one_copy_of_every_card() {
        reset();
        add(brox());
        add(brox());
        player_doc::clear_all();
        assert_eq!(count("brox_the_defiant"), 1);
        assert_eq!(state("brox_the_defiant~2"), None);
        reset();
    }
}
