//! The card catalog as Kippa reads it.
//!
//! Kippa (the rules assistant) is given the cards as one JSON file,
//! `kipukas-server/cards_catalog.json`, which is kept in the repository and
//! handed to it by hand. `card-pages` writes the file from the engine's
//! catalog whenever it differs. It is not part of the site.
//!
//! Build time only: left out of the browser's engine.

use serde::Serialize;

use crate::catalog::Card;
use crate::site;

#[derive(Serialize)]
struct KealMeans<'a> {
    name: &'a str,
    genetics: &'a [&'a str],
    count: u8,
}

/// One card. The order of the fields is the order they are written in.
#[derive(Serialize)]
struct Entry<'a> {
    slug: &'a str,
    title: &'a str,
    #[serde(rename = "type")]
    kind: &'a str,
    variation: Option<&'a str>,
    genetic_disposition: Option<&'a str>,
    motivation: Option<&'a str>,
    habitat: Option<&'a str>,
    injury_tolerance: u8,
    movement: u8,
    die: &'a str,
    brawl_sequence: &'a str,
    keal_means: Vec<KealMeans<'a>>,
    tamability: Option<u32>,
    scarcity: Option<&'a str>,
    tags: &'a str,
    description: Option<&'a str>,
    play_style: Option<&'a str>,
    url: String,
}

/// The catalog as JSON, one entry per card, indented. The not-found page is
/// left out: it shows the same card as `/fourohfour`.
pub fn catalog(cards: &[Card]) -> String {
    let entries: Vec<Entry> = cards
        .iter()
        .filter(|card| !site::is_not_found_page(card))
        .map(|card| Entry {
            slug: card.slug,
            title: card.title,
            kind: card.layout,
            variation: card.variation,
            genetic_disposition: card.genetic_disposition,
            motivation: card.motivation,
            habitat: card.habitat,
            injury_tolerance: card.injury_tolerance,
            movement: card.movement,
            die: card.die,
            brawl_sequence: card.brawl_sequence,
            keal_means: card
                .keal_means
                .iter()
                .map(|km| KealMeans { name: km.name, genetics: km.genetics, count: km.count })
                .collect(),
            tamability: card.tamability,
            scarcity: card.scarcity,
            tags: card.tags,
            description: card.description,
            play_style: card.play_style,
            url: site::address(card),
        })
        .collect();
    serde_json::to_string_pretty(&entries).expect("the catalog is plain data")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CARDS;
    use serde_json::Value;

    fn entries() -> Vec<Value> {
        serde_json::from_str(&catalog(&CARDS)).expect("valid JSON")
    }

    #[test]
    fn every_card_but_the_not_found_page_is_listed_once() {
        let entries = entries();
        assert_eq!(entries.len(), CARDS.len() - 1);
        assert!(entries.iter().all(|e| e["slug"] != "404"));
        assert!(entries.iter().any(|e| e["slug"] == "fourohfour"));
    }

    #[test]
    fn an_entry_says_what_the_card_is() {
        let entries = entries();
        let brox = entries.iter().find(|e| e["slug"] == "brox_the_defiant").unwrap();
        assert_eq!(brox["title"], "Brox The Defiant");
        assert_eq!(brox["type"], "Character");
        assert_eq!(brox["url"], "https://www.kipukas.cards/brox_the_defiant/");
        assert!(brox["keal_means"].as_array().unwrap().iter().any(|km| km["name"] == "Crushing Hope"));
        assert!(brox["injury_tolerance"].is_u64());
        // What a card does not have is null, not left out.
        let trap = entries.iter().find(|e| e["slug"] == "timebattle").unwrap();
        assert!(trap["tamability"].is_null() && trap["habitat"].is_null());
        assert_eq!(trap["keal_means"], Value::Array(vec![]));
    }

    #[test]
    fn the_fields_are_in_the_order_kippa_was_given() {
        let cloth = CARDS.iter().find(|c| c.slug == "cloth").expect("Cloth is in the catalog");
        let text = catalog(std::slice::from_ref(cloth));
        let order: Vec<usize> = ["\"slug\"", "\"title\"", "\"type\"", "\"variation\"", "\"keal_means\"", "\"tamability\"", "\"play_style\"", "\"url\""]
            .iter()
            .map(|field| text.find(field).unwrap_or_else(|| panic!("{} is in the entry", field)))
            .collect();
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{:?}", order);
    }
}
