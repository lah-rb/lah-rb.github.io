//! A copy of a card handed to the other player, in the room document.
//!
//! In play a soul can change sides: stolen, or tamed by the other player.
//! The copy then leaves one player's device and arrives on the other's, as
//! it is (`game::copies`). The room's document carries it between the two,
//! beside the timers and the rounds of fists, so that a copy handed over
//! while the other player is away (a page change, a locked phone) is there
//! for them when they are back, and is taken once.
//!
//! ## Doc structure
//!
//! ```text
//! yrs::Doc
//! └── "handed" (MapRef)
//!     ├── "give:<id>" → JSON string Given   written by the player who hands it over
//!     └── "took:<id>" → true                written by the player who takes it
//! ```
//!
//! Each key is written by one player only, as with fists. A copy is on its
//! way from the moment it is given until it is taken. The giver can have it
//! back while it is still on its way (`withdraw`): they do when they leave
//! the room, or the other player leaves it for good, since nobody is left
//! to take it.
//!
//! Everything here is a pure function over a `&Doc`, so tests can run two
//! documents and sync between them. The routes apply these to the room doc.

use serde::{Deserialize, Serialize};
use yrs::{Any, Doc, Map, ReadTxn, Transact};

use crate::game::player_doc::CardState;

/// Root map holding the copies handed over.
pub const HANDED: &str = "handed";

/// A copy as it travels: what is kept of it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Copy {
    #[serde(default)]
    pub slots: Vec<bool>,
    #[serde(default)]
    pub wasted: bool,
    #[serde(default)]
    pub effects: u8,
    /// The copy was a guest of the player who hands it over: the other
    /// player's own, going home.
    #[serde(default)]
    pub guest: bool,
}

impl From<&CardState> for Copy {
    fn from(state: &CardState) -> Copy {
        Copy { slots: state.slots.clone(), wasted: state.wasted, effects: state.effects, guest: state.guest }
    }
}

impl Copy {
    /// What the player who takes it keeps of it. It is a guest there,
    /// unless it was a guest of the giver: then it is their own, come home.
    pub fn arriving(&self) -> CardState {
        CardState { slots: self.slots.clone(), wasted: self.wasted, effects: self.effects, guest: !self.guest }
    }

    /// What the giver keeps of it again, having it back: as it was.
    pub fn returning(&self) -> CardState {
        CardState { slots: self.slots.clone(), wasted: self.wasted, effects: self.effects, guest: self.guest }
    }
}

/// A copy of a card on its way from one player to the other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Given {
    pub from: String,
    pub to: String,
    /// The card's name.
    pub card: String,
    pub copy: Copy,
}

fn give_key(id: &str) -> String {
    format!("give:{}", id)
}

fn took_key(id: &str) -> String {
    format!("took:{}", id)
}

/// Every copy given and not yet taken, by its id. Anything malformed is
/// skipped rather than trusted: the other player writes to this map too.
fn on_their_way(doc: &Doc) -> Vec<(String, Given)> {
    let map = doc.get_or_insert_map(HANDED);
    let txn = doc.transact();
    let mut all: Vec<(String, Given)> = map
        .iter(&txn)
        .filter_map(|(key, value)| {
            let id = key.strip_prefix("give:")?;
            if map.get(&txn, &took_key(id)).is_some() {
                return None;
            }
            match value {
                yrs::Out::Any(Any::String(json)) => Some((id.to_string(), serde_json::from_str(&json).ok()?)),
                _ => None,
            }
        })
        .collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    all
}

/// Hand a copy over: it is on its way from `from` to `to` under `id`.
pub fn give(doc: &Doc, id: &str, given: &Given) {
    let Ok(json) = serde_json::to_string(given) else {
        return;
    };
    let map = doc.get_or_insert_map(HANDED);
    map.insert(&mut doc.transact_mut(), give_key(id), Any::from(json));
}

/// The copies on their way to this player.
pub fn for_me(doc: &Doc, me: &str) -> Vec<(String, Given)> {
    on_their_way(doc).into_iter().filter(|(_, given)| given.to == me && given.from != me).collect()
}

/// The copies this player has handed over that have not been taken yet.
pub fn from_me(doc: &Doc, me: &str) -> Vec<(String, Given)> {
    on_their_way(doc).into_iter().filter(|(_, given)| given.from == me).collect()
}

/// This player has taken the copy given under `id`.
pub fn take(doc: &Doc, id: &str) {
    let map = doc.get_or_insert_map(HANDED);
    map.insert(&mut doc.transact_mut(), took_key(id), Any::from(true));
}

/// The giver has the copy given under `id` back: it is no longer on its way.
pub fn withdraw(doc: &Doc, id: &str) {
    let map = doc.get_or_insert_map(HANDED);
    map.remove(&mut doc.transact_mut(), &give_key(id));
}

#[cfg(test)]
mod tests {
    use super::*;
    use yrs::updates::decoder::Decode;
    use yrs::{StateVector, Update};

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbb";

    fn doc() -> Doc {
        let doc = Doc::new();
        doc.get_or_insert_map(HANDED);
        doc
    }

    /// Bring `to` up to date with `from`.
    fn sync(from: &Doc, to: &Doc) {
        let update = from.transact().encode_state_as_update_v1(&StateVector::default());
        to.transact_mut().apply_update(Update::decode_v1(&update).unwrap()).unwrap();
    }

    fn brox(guest: bool) -> Given {
        Given {
            from: A.to_string(),
            to: B.to_string(),
            card: "brox_the_defiant".to_string(),
            copy: Copy { slots: vec![true, false, false], wasted: false, effects: 2, guest },
        }
    }

    #[test]
    fn a_copy_given_is_on_its_way_to_the_other_player_until_they_take_it() {
        let (a, b) = (doc(), doc());
        give(&a, "one", &brox(false));
        assert_eq!(from_me(&a, A), [("one".to_string(), brox(false))]);
        assert!(for_me(&a, A).is_empty(), "it is not on its way to the one who gave it");
        assert!(for_me(&b, B).is_empty(), "the other player has not heard of it yet");

        sync(&a, &b);
        assert_eq!(for_me(&b, B), [("one".to_string(), brox(false))]);
        take(&b, "one");
        assert!(for_me(&b, B).is_empty(), "taken once");
        // The giver learns that it was taken.
        assert_eq!(from_me(&a, A).len(), 1);
        sync(&b, &a);
        assert!(from_me(&a, A).is_empty());
    }

    #[test]
    fn a_copy_arrives_as_a_guest_and_goes_home_as_the_owners_own() {
        let out = brox(false).copy;
        assert!(out.arriving().guest);
        assert_eq!((out.arriving().slots, out.arriving().effects), (vec![true, false, false], 2));
        // A guest handed back.
        assert!(!brox(true).copy.arriving().guest);
        // Had back by the giver, it is as it was.
        assert!(!out.returning().guest && brox(true).copy.returning().guest);
        let state = CardState { slots: vec![false, true], wasted: true, effects: 1, guest: true };
        assert_eq!(Copy::from(&state).returning(), state);
    }

    #[test]
    fn the_giver_can_have_back_a_copy_that_is_still_on_its_way() {
        let (a, b) = (doc(), doc());
        give(&a, "one", &brox(false));
        give(&a, "two", &brox(false));
        withdraw(&a, "one");
        let left: Vec<String> = from_me(&a, A).into_iter().map(|(id, _)| id).collect();
        assert_eq!(left, ["two"]);
        sync(&a, &b);
        let coming: Vec<String> = for_me(&b, B).into_iter().map(|(id, _)| id).collect();
        assert_eq!(coming, ["two"]);
    }

    #[test]
    fn what_is_not_a_copy_given_is_passed_over() {
        let a = doc();
        let map = a.get_or_insert_map(HANDED);
        map.insert(&mut a.transact_mut(), "give:bad", Any::from("not json"));
        map.insert(&mut a.transact_mut(), "give:number", Any::from(7.0));
        map.insert(&mut a.transact_mut(), "other", Any::from("{}"));
        give(&a, "good", &brox(false));
        assert_eq!(from_me(&a, A).len(), 1);
        // One that says little is read with the rest as nothing.
        map.insert(
            &mut a.transact_mut(),
            "give:bare",
            Any::from(format!(r#"{{"from":"{}","to":"{}","card":"cloth","copy":{{}}}}"#, A, B)),
        );
        let bare = for_me(&a, B).into_iter().find(|(id, _)| id == "bare").unwrap().1;
        assert_eq!(bare.copy, Copy::default());
    }
}
