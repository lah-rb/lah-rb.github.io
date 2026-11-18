//! `/api/hand`: a hand of cards, side by side.
//!
//! The hand page (`hand.html`) is one engine-owned view of this route (Pattern
//! 13). A hand is named in the page's address by the cards' short codes, four
//! characters each, after the `#`: `/hand/#KVphRrDC` is two cards. The page
//! passes them on (`?codes={hash}`), and the engine, which has every card and
//! its code, renders a pane for each card it knows: the card's art, and over
//! it the card's detail panel.
//!
//! Each pane's panel is a view of its own (`/api/card/panel?card=…`), filled
//! in here so the hand arrives whole, and from then on drawn by itself: a tap
//! on a damage slot draws that one panel again, not the hand. The hand is
//! drawn again only when the address changes, and then panes already on the
//! page are left as they are: a pane's art and its panel are marked
//! `data-engine-keep`, and panes have ids so that the same card in the same
//! place is the same pane.

use crate::catalog::{Card, CARDS};
use crate::game::card_panel;
use crate::routes::util::{get_param, parse_query};

/// The length of a short code.
const CODE_LENGTH: usize = 4;

/// The short codes in a hand as written in an address: every four characters
/// is a code. Commas between codes are allowed and a leading `#` is dropped;
/// characters left over at the end are not a code.
pub fn codes_of(hand: &str) -> Vec<String> {
    let characters: Vec<char> = hand
        .trim()
        .trim_start_matches('#')
        .chars()
        .filter(|c| *c != ',')
        .collect();
    characters
        .chunks_exact(CODE_LENGTH)
        .map(|code| code.iter().collect())
        .collect()
}

/// The card with this short code, among the cards the binder shows.
fn card_with_code(code: &str) -> Option<&'static Card> {
    CARDS.iter().find(|card| !card.hidden && card.code == code)
}

/// One card of the hand: its art, filling the pane, and over it the card's
/// detail panel, which scrolls.
///
/// The art is two pictures, as on a card page: a small one that is there at
/// once and the full one, which fades in over it when it has loaded. Whether
/// it has loaded is the page's to know, so both are kept as they are when
/// the hand is drawn again.
fn render_pane(place: usize, card: &Card) -> String {
    format!(
        concat!(
            r#"<section id="hand-pane-{place}-{code}" class="hand-pane relative h-full">"#,
            r#"<img data-art-lqip data-engine-keep aria-hidden="true" alt="" src="/assets/images/{art}?d=512" "#,
            r#"class="absolute inset-0 w-full h-full object-cover">"#,
            r#"<img data-art-full data-engine-keep aria-hidden="true" alt="" src="/assets/images/{art}" "#,
            r#"onload="this.classList.remove('opacity-0')" "#,
            r#"class="absolute inset-0 w-full h-full object-cover opacity-0 transition-opacity duration-700">"#,
            r#"<div class="absolute inset-0 overflow-y-auto">"#,
            r#"<div class="min-h-full backdrop-brightness-50 backdrop-blur-sm backdrop-contrast-125 backdrop-saturate-150">"#,
            r#"<div class="flex place-content-center w-full">"#,
            r#"<div data-pane-content data-engine-view="/api/card/panel?card={slug}" "#,
            r#"class="z-20 grid grid-cols-1 bg-transparent/50 text-slate-200 text-justify leading-tight w-5/6 h-fit min-h-full scroll-smooth">"#,
            "{panel}",
            "</div></div></div></div></section>",
        ),
        place = place,
        code = card.code,
        art = card.img_name,
        slug = card.slug,
        panel = card_panel::render_card_panel(card.slug),
    )
}

/// What the page shows for a hand with no card in it.
fn render_empty() -> String {
    concat!(
        r#"<div id="hand-empty" class="flex flex-col items-center justify-center h-screen text-center px-6 text-kip-drk-goldenrod">"#,
        r#"<p class="text-2xl md:text-3xl font-semibold mb-3">No cards in your hand</p>"#,
        r#"<p class="mb-6 text-slate-500 max-w-md">Add card codes to the address "#,
        r#"(e.g. <code class="font-mono">/hand/#KVphRrDC</code>), or head back to the binder to pick some.</p>"#,
        r#"<a href="/" class="toolbar-tile w-auto px-6 text-kip-drk-goldenrod">Back to the binder</a>"#,
        "</div>",
    )
    .to_string()
}

/// Render a hand: a row of panes, one for each code that is a card's, in the
/// order given. A code that is no card's is left out. A hand with no card in
/// it says so.
///
/// The row scrolls sideways and snaps to a pane: several panes across on a
/// wide screen, one to a swipe on a phone (`.hand-pane` in `input.css`).
pub fn render_hand(codes: &[String]) -> String {
    let panes: Vec<String> = codes
        .iter()
        .filter_map(|code| card_with_code(code))
        .enumerate()
        .map(|(place, card)| render_pane(place, card))
        .collect();
    if panes.is_empty() {
        return render_empty();
    }
    format!(
        r#"<div id="hand-spread" class="flex h-screen overflow-x-auto overflow-y-hidden snap-x snap-mandatory scroll-smooth">{}</div>"#,
        panes.concat()
    )
}

/// Handle GET /api/hand?codes=<the codes, as in the page's address>
pub fn handle(query: &str) -> String {
    let params = parse_query(query);
    render_hand(&codes_of(get_param(&params, "codes").unwrap_or("")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::player_doc;

    fn card(slug: &str) -> &'static Card {
        CARDS.iter().find(|c| c.slug == slug).unwrap_or_else(|| panic!("{} is a card", slug))
    }

    fn codes(slugs: &[&str]) -> String {
        slugs.iter().map(|slug| card(slug).code).collect()
    }

    #[test]
    fn a_hand_is_read_four_characters_at_a_time() {
        assert_eq!(codes_of("KVphRrDC"), ["KVph", "RrDC"]);
        assert_eq!(codes_of("#KVphRrDC"), ["KVph", "RrDC"]);
        assert_eq!(codes_of("KVph,RrDC"), ["KVph", "RrDC"]);
        assert_eq!(codes_of("  KVphRrDC "), ["KVph", "RrDC"]);
        // What is left over is not a code.
        assert_eq!(codes_of("KVphRr"), ["KVph"]);
        assert!(codes_of("").is_empty());
        assert!(codes_of("#").is_empty());
        assert!(codes_of("abc").is_empty());
    }

    #[test]
    fn a_hand_has_a_pane_for_each_card_in_order() {
        player_doc::init_player_doc();
        let hand = codes(&["brox_the_defiant", "cloth", "timebattle"]);
        let html = handle(&format!("?codes={}", hand));
        assert!(html.starts_with(r#"<div id="hand-spread""#));
        assert_eq!(html.matches(r#"class="hand-pane"#).count(), 3);
        let places: Vec<usize> = ["Brox The Defiant", "Cloth", "Timebattle"]
            .iter()
            .map(|title| {
                html.find(&format!(">{}</h1>", title))
                    .unwrap_or_else(|| panic!("a pane headed {}", title))
            })
            .collect();
        assert!(places.windows(2).all(|pair| pair[0] < pair[1]), "{:?}", places);
        // Each pane has the card's art, small and full.
        let art = card("cloth").img_name;
        assert!(html.contains(&format!(r#"src="/assets/images/{}?d=512""#, art)));
        assert!(html.contains(&format!(r#"src="/assets/images/{}" onload="#, art)));
    }

    #[test]
    fn a_panes_panel_is_a_view_of_its_own_and_arrives_filled() {
        player_doc::init_player_doc();
        let html = handle(&format!("?codes={}", codes(&["brox_the_defiant"])));
        let view = r#"data-engine-view="/api/card/panel?card=brox_the_defiant""#;
        assert_eq!(html.matches(view).count(), 1);
        // What is inside it is what that view's own route renders.
        let panel = card_panel::render_card_panel("brox_the_defiant");
        assert!(html.contains(&format!("scroll-smooth\">{}</div>", panel)));
        assert!(html.contains("Crushing Hope"));
    }

    #[test]
    fn what_the_page_must_keep_is_marked_and_panes_can_be_told_apart() {
        player_doc::init_player_doc();
        let brox = card("brox_the_defiant").code;
        // The same card twice is two panes.
        let html = handle(&format!("?codes={}{}", brox, brox));
        assert!(html.contains(&format!(r#"<section id="hand-pane-0-{}""#, brox)));
        assert!(html.contains(&format!(r#"<section id="hand-pane-1-{}""#, brox)));
        // The art keeps whether it has loaded.
        assert_eq!(html.matches("<img data-art-lqip data-engine-keep").count(), 2);
        assert_eq!(html.matches("<img data-art-full data-engine-keep").count(), 2);
    }

    #[test]
    fn a_code_that_is_no_cards_is_left_out() {
        player_doc::init_player_doc();
        let brox = card("brox_the_defiant").code;
        let html = handle(&format!("?codes=zzzz{}0000", brox));
        assert_eq!(html.matches(r#"class="hand-pane"#).count(), 1);
        assert!(html.contains(&format!(r#"id="hand-pane-0-{}""#, brox)));
        // A hidden card is not in a hand either (the 404 Glitch).
        let hidden = card("fourohfour").code;
        assert!(!hidden.is_empty());
        assert!(handle(&format!("?codes={}", hidden)).contains("No cards in your hand"));
    }

    #[test]
    fn a_hand_with_no_card_in_it_says_so() {
        for query in ["", "?codes=", "?codes=zzzz", "?codes=ab", "?other=1"] {
            let html = handle(query);
            assert!(html.starts_with(r#"<div id="hand-empty""#), "{}", query);
            assert!(html.contains("No cards in your hand"));
            assert!(html.contains(r#"<a href="/""#));
            assert!(!html.contains("hand-spread"));
        }
    }

    #[test]
    fn codes_arrive_as_the_page_encodes_them() {
        player_doc::init_player_doc();
        let (brox, cloth) = (card("brox_the_defiant").code, card("cloth").code);
        // A comma between codes, percent-encoded in the query.
        let html = handle(&format!("?codes={}%2C{}", brox, cloth));
        assert_eq!(html.matches(r#"class="hand-pane"#).count(), 2);
        // Nothing in the address reaches the markup but the codes of real cards.
        let html = handle("?codes=%22%3E%3Cscript%3E");
        assert!(!html.contains("<script"));
        assert!(html.contains("No cards in your hand"));
    }

    #[test]
    fn a_panel_in_a_hand_shows_the_players_damage() {
        player_doc::init_player_doc();
        crate::game::damage::toggle_slot("brox_the_defiant", 1);
        let html = handle(&format!("?codes={}", codes(&["brox_the_defiant"])));
        assert!(html.contains(r#"data-slot="1" aria-pressed="true""#));
        player_doc::init_player_doc();
    }
}
