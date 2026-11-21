//! `/api/card/copies` — the copies of a card in play, on the card's page:
//! an engine-owned view (Pattern 13 in CONTRIBUTING.md) at the lower left
//! of the page, over the art. The toolbar places it
//! (`_includes/toolbar.html`), so its tiles fade and wake with the
//! toolbar's.
//!
//! With one copy in play it is a single button, which puts another in
//! play. With more it also says which copy the page shows and turns to the
//! next, and can take the copy shown out of play, which it asks about
//! first: the copy's damage goes with it.
//!
//! The rules allow a deck one of a character and two of a species. The
//! binder does not hold a player to that (a soul can be stolen or crafted
//! in play, and a table may have its own rules), but once more copies are
//! in play than that, the button that adds one is red and says so.
//!
//! Which copy the page shows is in the page's address, after the `#`
//! (`game::copies`). The view's route carries it
//! (`/api/card/copies?card=<slug>&copy={hash}`), as the card panel's and
//! the fists dialog's do, so turning to another copy is a link and nothing
//! is kept of it. Adding a copy and taking one out are posts: the engine
//! changes the player's document, then asks the page for the address of
//! the copy to show.

use std::cell::RefCell;

use crate::catalog::{Card, CARDS};
use crate::game::copies;
use crate::page;
use crate::routes::util::{get_param, parse_form_body, parse_query};

const ROUTE: &str = "/api/card/copies";

/// The tiles, one above another. (Where they stand is the toolbar's to say.)
const COLUMN: &str = "flex flex-col items-start gap-2 text-kip-drk-sienna";
const TILE: &str = "toolbar-tile text-3xl font-bold leading-none";
/// The button that adds a copy, once the player has more in play than the
/// rules allow a deck.
const OVER_TILE: &str = "toolbar-tile flex-col leading-none !bg-kip-red !border-kip-red text-amber-50";

thread_local! {
    /// The copy the player has asked to take out of play and not yet said
    /// yes or no to: its key.
    static ASKED: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn find_card(key: &str) -> Option<&'static Card> {
    let slug = copies::slug_of(key);
    CARDS.iter().find(|card| card.slug == slug)
}

/// The views that show a copy of a card: all are drawn again when the
/// copies in play change.
fn redraw() {
    page::redraw("/api/card/panel");
    page::redraw(ROUTE);
    page::redraw("/api/room/fists");
}

/// Ask the page to show a copy of a card: the card's address, with the
/// copy's number after the `#`.
fn go_to(card: &Card, copy: u8) {
    page::go(&format!("{}#{}", card.url, copy));
}

/// GET /api/card/copies?card=<slug>&copy=<what follows the # of the
/// page's address> — the copies of the page's card.
pub fn handle_copies_get(query: &str) -> String {
    let params = parse_query(query);
    let Some(card) = find_card(get_param(&params, "card").unwrap_or("")) else {
        return String::new();
    };
    render(card, copies::shown(card.slug, get_param(&params, "copy").unwrap_or("")))
}

/// POST /api/card/copies — a button of the view.
///
/// Body:
///   - `action=add&card=<slug>`: put another copy of the card in play
///   - `action=ask&card=<key>`: the player wants this copy out of play
///   - `action=remove&card=<key>`: and has said yes
///   - `action=keep`: or no
pub fn handle_copies_post(body: &str) -> String {
    let params = parse_form_body(body);
    let key = get_param(&params, "card").unwrap_or("");
    let card = find_card(key);
    let asked = ASKED.with(|asked| asked.borrow_mut().take());
    match (get_param(&params, "action").unwrap_or(""), card) {
        ("add", Some(card)) => {
            if let Some(copy) = copies::add(card) {
                go_to(card, copy);
                redraw();
            }
        }
        ("ask", Some(card)) if copies::in_play(key) && copies::count(card.slug) > 1 => {
            ASKED.with(|asking| *asking.borrow_mut() = Some(key.to_string()));
        }
        // Only the copy that was asked about is taken out.
        ("remove", Some(card)) if asked.as_deref() == Some(key) => {
            let copy = copies::split(key).1;
            if copies::remove(card, copy) {
                go_to(card, copy.min(copies::count(card.slug)));
                redraw();
            }
        }
        _ => {}
    }
    String::new()
}

fn button(body: &str, label: &str, class: &str, text: &str) -> String {
    format!(
        r#"<button type="button" data-engine-post="{}" data-engine-body="{}" aria-label="{}" class="{}">{}</button>"#,
        ROUTE, body, label, class, text
    )
}

/// The view for a card of which copy `shown` is on the page. Nothing, for a
/// card that does not fight: there is nothing to keep of a copy of it.
fn render(card: &Card, shown: u8) -> String {
    if !copies::can_have(card) {
        return String::new();
    }
    let (count, key) = (copies::count(card.slug), copies::key(card.slug, shown));
    let over = copies::over_limit(card);
    let mut h = format!(
        r#"<div class="{}" data-copies="{}" data-copy="{}"{}>"#,
        COLUMN,
        count,
        shown,
        if over { " data-over-limit" } else { "" }
    );

    // The question, in place of the buttons, while it is being asked.
    if count > 1 && ASKED.with(|asked| asked.borrow().as_deref() == Some(key.as_str())) {
        h.push_str(&format!(
            concat!(
                r#"<div role="alertdialog" aria-label="Take this copy out of play" "#,
                r#"class="bg-amber-50 border-2 border-kip-drk-sienna rounded-2xl shadow-md p-3 w-56 text-sm text-left">"#,
                r#"<p class="font-bold mb-1">Take copy {} out of play?</p>"#,
                r#"<p class="mb-2">Its damage goes with it.</p>"#,
                r#"<div class="grid grid-cols-2 gap-2">{}{}</div></div></div>"#,
            ),
            shown,
            button(
                "action=keep",
                "Keep this copy",
                "bg-slate-400 hover:bg-slate-500 text-amber-50 font-bold py-1.5 px-2 rounded",
                "Keep"
            ),
            button(
                &format!("action=remove&amp;card={}", key),
                "Take this copy out of play",
                "bg-kip-red hover:bg-red-700 text-amber-50 font-bold py-1.5 px-2 rounded",
                "Take out"
            ),
        ));
        return h;
    }

    if count > 1 {
        // Which copy this is, and a turn to the next: a link, since which
        // copy the page shows is in its address.
        let next = if shown >= count { 1 } else { shown + 1 };
        h.push_str(&format!(
            concat!(
                r##"<a href="#{}" aria-label="Copy {} of {}. Show the next copy" class="toolbar-tile flex-col leading-none">"##,
                r#"<span class="text-[0.65rem] uppercase tracking-wide">copy</span>"#,
                r#"<span class="text-lg font-bold" data-copy-shown>{}/{}</span></a>"#,
            ),
            next, shown, count, shown, count
        ));
        h.push_str(&button(
            &format!("action=ask&amp;card={}", key),
            "Take this copy out of play",
            TILE,
            "&minus;",
        ));
    }
    if count < copies::MOST {
        let add = format!("action=add&amp;card={}", card.slug);
        if over {
            // More are in play than the rules allow a deck. It is said, and
            // not enforced.
            let allowed = copies::limit(card).unwrap_or(1);
            h.push_str(&button(
                &add,
                &format!(
                    "Put another copy of this card in play. Over the limit: the rules allow a deck {} of this card",
                    allowed
                ),
                OVER_TILE,
                concat!(
                    r#"<span class="text-2xl font-bold">+</span>"#,
                    r#"<span class="text-[0.6rem] font-bold uppercase tracking-wide">over<br>limit</span>"#,
                ),
            ));
        } else {
            h.push_str(&button(&add, "Put another copy of this card in play", TILE, "+"));
        }
    }
    h.push_str("</div>");
    h
}

/// Forget what was asked, as before any page has loaded.
#[cfg(test)]
pub fn reset() {
    ASKED.with(|asked| *asked.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::player_doc;

    const BROX: &str = "?card=brox_the_defiant";

    fn fresh() {
        player_doc::init_player_doc();
        page::reset();
        reset();
    }

    #[test]
    fn a_card_that_fights_has_a_button_to_put_another_copy_in_play() {
        fresh();
        let html = handle_copies_get(BROX);
        assert!(html.contains(r#"data-copies="1" data-copy="1""#));
        assert!(html.contains(
            r#"data-engine-post="/api/card/copies" data-engine-body="action=add&amp;card=brox_the_defiant" aria-label="Put another copy of this card in play""#
        ));
        // One copy: nothing to turn to, and nothing to take out.
        assert!(!html.contains("Show the next copy") && !html.contains("Take this copy out of play"));
        // A card that does not fight has no copies to speak of.
        let trap = CARDS.iter().find(|card| card.keal_means.is_empty()).unwrap();
        assert_eq!(handle_copies_get(&format!("?card={}", trap.slug)), "");
        assert_eq!(handle_copies_get("?card=no_such_card"), "");
        fresh();
    }

    #[test]
    fn adding_a_copy_puts_it_in_play_and_sends_the_page_to_it() {
        fresh();
        assert_eq!(handle_copies_post("action=add&card=brox_the_defiant"), "");
        assert_eq!(copies::count("brox_the_defiant"), 2);
        assert_eq!(page::take_go().as_deref(), Some("/brox_the_defiant/#2"));
        assert_eq!(page::take_redraws(), ["/api/card/panel", "/api/card/copies", "/api/room/fists"]);

        // The page then shows the second copy: which it is, a turn to the
        // first, a button to take it out, and one to add a third.
        let html = handle_copies_get("?card=brox_the_defiant&copy=2");
        assert!(html.contains(r#"data-copies="2" data-copy="2""#));
        assert!(html.contains(r##"<a href="#1" aria-label="Copy 2 of 2. Show the next copy""##));
        assert!(html.contains(r#"data-copy-shown>2/2</span>"#));
        assert!(html.contains(r#"data-engine-body="action=ask&amp;card=brox_the_defiant~2""#));
        assert!(html.contains(r#"data-engine-body="action=add&amp;card=brox_the_defiant""#));
        // From the first copy the turn is to the second.
        assert!(handle_copies_get(BROX).contains(r##"<a href="#2" aria-label="Copy 1 of 2. Show the next copy""##));
        // An address that asks for a copy there is not shows the first.
        assert!(handle_copies_get("?card=brox_the_defiant&copy=5").contains(r#"data-copy="1""#));
        fresh();
    }

    #[test]
    fn the_button_turns_red_and_says_over_limit_once_more_are_in_play_than_the_rules_allow() {
        fresh();
        // A character: the rules allow a deck one.
        assert!(!handle_copies_get(BROX).contains("over"));
        handle_copies_post("action=add&card=brox_the_defiant");
        let html = handle_copies_get("?card=brox_the_defiant&copy=2");
        assert!(html.contains("data-over-limit"));
        assert!(html.contains("!bg-kip-red") && html.contains("over<br>limit"), "{}", html);
        assert!(html.contains(
            r#"aria-label="Put another copy of this card in play. Over the limit: the rules allow a deck 1 of this card""#
        ));
        // It still adds: nothing is enforced.
        assert!(html.contains(r#"data-engine-body="action=add&amp;card=brox_the_defiant""#));
        handle_copies_post("action=add&card=brox_the_defiant");
        assert_eq!(copies::count("brox_the_defiant"), 3);

        // A species: two are allowed, a third is over.
        let species = CARDS.iter().find(|card| card.layout == "Species" && copies::can_have(card)).unwrap();
        let page = format!("?card={}", species.slug);
        handle_copies_post(&format!("action=add&card={}", species.slug));
        assert!(!handle_copies_get(&page).contains("data-over-limit"));
        handle_copies_post(&format!("action=add&card={}", species.slug));
        let html = handle_copies_get(&page);
        assert!(html.contains("data-over-limit") && html.contains("the rules allow a deck 2 of this card"));

        // Back within the limit, the button is as it was.
        handle_copies_post(&format!("action=ask&card={}~3", species.slug));
        handle_copies_post(&format!("action=remove&card={}~3", species.slug));
        let html = handle_copies_get(&page);
        assert!(!html.contains("data-over-limit") && !html.contains("!bg-kip-red"));
        fresh();
    }

    #[test]
    fn there_is_no_button_to_add_a_copy_beyond_the_most_there_can_be() {
        fresh();
        for _ in 1..copies::MOST {
            handle_copies_post("action=add&card=brox_the_defiant");
        }
        assert_eq!(copies::count("brox_the_defiant"), copies::MOST);
        assert!(!handle_copies_get(BROX).contains("action=add"));
        handle_copies_post("action=add&card=brox_the_defiant");
        assert_eq!(copies::count("brox_the_defiant"), copies::MOST);
        fresh();
    }

    #[test]
    fn taking_a_copy_out_is_asked_about_first() {
        fresh();
        handle_copies_post("action=add&card=brox_the_defiant");
        handle_copies_post("action=add&card=brox_the_defiant");
        player_doc::toggle_slot("brox_the_defiant~3", 1);
        page::take_go();
        page::take_redraws();

        // Yes without having been asked takes nothing out.
        handle_copies_post("action=remove&card=brox_the_defiant~2");
        assert_eq!(copies::count("brox_the_defiant"), 3);

        handle_copies_post("action=ask&card=brox_the_defiant~2");
        let html = handle_copies_get("?card=brox_the_defiant&copy=2");
        assert!(html.contains(r#"role="alertdialog""#) && html.contains("Take copy 2 out of play?"));
        assert!(html.contains(r#"data-engine-body="action=remove&amp;card=brox_the_defiant~2""#));
        assert!(html.contains(r#"data-engine-body="action=keep""#));
        // Only the copy asked about shows the question.
        assert!(!handle_copies_get(BROX).contains("alertdialog"));
        assert_eq!(copies::count("brox_the_defiant"), 3, "asking takes nothing out");

        // No: the buttons are back, and yes no longer stands.
        handle_copies_post("action=keep");
        assert!(!handle_copies_get("?card=brox_the_defiant&copy=2").contains("alertdialog"));
        handle_copies_post("action=remove&card=brox_the_defiant~2");
        assert_eq!(copies::count("brox_the_defiant"), 3);

        // Asked again, and yes: the third copy is the second now, and the
        // page stays at the number it showed.
        handle_copies_post("action=ask&card=brox_the_defiant~2");
        handle_copies_post("action=remove&card=brox_the_defiant~2");
        assert_eq!(copies::count("brox_the_defiant"), 2);
        assert!(player_doc::get_slot("brox_the_defiant~2", 1));
        assert_eq!(page::take_go().as_deref(), Some("/brox_the_defiant/#2"));
        assert_eq!(page::take_redraws(), ["/api/card/panel", "/api/card/copies", "/api/room/fists"]);

        // The last of them taken out: the page goes to the one before.
        handle_copies_post("action=ask&card=brox_the_defiant~2");
        handle_copies_post("action=remove&card=brox_the_defiant~2");
        assert_eq!(page::take_go().as_deref(), Some("/brox_the_defiant/#1"));
        // The only copy left cannot be asked about.
        handle_copies_post("action=ask&card=brox_the_defiant");
        assert!(!handle_copies_get(BROX).contains("alertdialog"));
        fresh();
    }
}
