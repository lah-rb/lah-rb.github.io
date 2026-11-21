//! The card catalog: every card, compiled into the engine.
//!
//! The cards themselves ([`CARDS`]) are not written here. The build script
//! (`build.rs`) reads the card files in `cards/` each time the engine is
//! compiled and writes them out as Rust, which is included at the bottom of
//! this file. Nothing is fetched or parsed when the engine runs, in the
//! browser or at build time.
//!
//! To change a card, edit its file and compile the engine again. To change
//! what a card can say, change [`Card`] here and how a card file is read in
//! the `catalog/` crate.

/// Where on a card a kind of damage is taken, and how many times.
#[derive(Debug, Clone)]
pub struct KealMeans {
    pub name: &'static str,
    pub genetics: &'static [&'static str],
    pub count: u8,
}

/// A card.
///
/// Text fields may contain markup: they are written by the game's authors
/// and compiled in, so they are trusted.
#[derive(Debug, Clone)]
pub struct Card {
    /// The card's address without its slashes: `timebattle` for
    /// `/timebattle/`. Its file is `cards/<slug>.html`.
    pub slug: &'static str,
    pub title: &'static str,
    /// The card's type, one of `game::card_panel::CARD_TYPES`.
    pub layout: &'static str,
    /// The art's file name in `assets/images/`.
    pub img_name: &'static str,
    pub img_alt: &'static str,
    /// Where the binder's grid cuts the art from: top, center or bottom.
    pub thumbnail: &'static str,
    pub tags: &'static str,
    pub genetic_disposition: Option<&'static str>,
    pub motivation: Option<&'static str>,
    pub habitat: Option<&'static str>,
    /// The card's address as written, e.g. `/timebattle/`.
    pub url: &'static str,
    /// The 4-character short code in the QR code printed on the card (its
    /// `redirect_from`). Empty for a hidden card that has none.
    pub code: &'static str,

    // What a card page shows (game::card_panel). The game logic reads none of
    // these.
    pub variation: Option<&'static str>,
    /// Movement as written. May be text, for a card that moves some other way
    /// than by a number of spaces.
    pub movement_text: Option<&'static str>,
    pub play_style: Option<&'static str>,
    pub atk_die: Option<&'static str>,
    pub def_die: Option<&'static str>,
    pub criteria: Option<&'static str>,
    pub success: Option<&'static str>,
    pub failure: Option<&'static str>,
    pub activation: Option<&'static str>,
    pub detection: Option<&'static str>,
    pub scarcity: Option<&'static str>,
    pub description: Option<&'static str>,
    pub promotion: Option<&'static str>,
    pub promotion_conditions: Option<&'static str>,

    // What the game counts with.
    pub injury_tolerance: u8,
    pub keal_means: &'static [KealMeans],
    /// Movement in spaces; 0 if the card's movement is text.
    pub movement: u8,
    pub die: &'static str,
    pub brawl_sequence: &'static str,
    /// Species cards only: what the player's stack must come to for the
    /// species to be under their control (`game::control`). More than a
    /// stack can come to is a species that cannot be tamed.
    pub tamability: Option<u32>,
    /// How many days of play bring the card to its full loyalty, where the
    /// card names its own (`game::control`). No page shows it.
    pub loyalty_days: Option<u32>,

    /// Left out of the binder (the 404 Glitch's two pages).
    pub hidden: bool,
    /// The art is wider than tall.
    pub landscape: bool,
}

// `CARD_COUNT` and `CARDS`, in the order the binder shows them: by title.
include!(concat!(env!("OUT_DIR"), "/cards.rs"));
