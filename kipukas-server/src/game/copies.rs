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

use crate::catalog::Card;
use crate::game::player_doc::{self, CardState};

/// The most copies of one card in play.
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

/// What is kept of a copy in play: for handing it to another player.
pub fn state(key: &str) -> Option<CardState> {
    if in_play(key) {
        player_doc::card_state(key)
    } else {
        None
    }
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
            assert_eq!(state, CardState { slots: vec![false; slots as usize], wasted: false, effects: 0 });
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
