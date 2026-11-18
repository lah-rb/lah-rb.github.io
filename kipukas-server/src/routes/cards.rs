//! `/api/cards`: the binder's grid of cards.
//!
//! The grid is an engine-owned view (Pattern 13). The engine holds what the
//! binder is showing ([`Binder`]): which cards (all of them, those matching
//! some filters, or those matching a search) and how many of them are on the
//! page so far. The view is drawn from that:
//!
//!   GET  /api/cards?columns=<columns in the page's grid>
//!
//! renders the tiles on the page so far, at least a first batch of [`ROWS`]
//! rows, and after them, if there are more cards, a marker. When the marker
//! scrolls into view the page posts what it says (`shown=<n>`), the engine
//! shows that many, and the view is drawn again: the new tiles are added
//! after those already there.
//!
//!   POST /api/cards  shown=<n>                           show at least n tiles
//!   POST /api/cards  all=true | filter=a,b | search=x    which cards, from the top
//!
//! A tile belongs to the page once drawn (`data-engine-keep`, and an id): its
//! art has loaded and faded in, and later drawings leave it alone. So adding
//! a batch touches nothing above it, and a tile that stays through a change
//! of filter does not flash.
//!
//! What the binder shows lasts as long as the engine does: until the page is
//! left. A fresh page shows every card.

use std::cell::RefCell;

use crate::catalog::{Card, CARDS};
use crate::routes::util::{get_param, parse_form_body, parse_query};

/// How many rows of tiles are added at a time.
pub const ROWS: usize = 3;

/// The columns assumed when the page does not say how many its grid has.
const DEFAULT_COLUMNS: usize = 4;

/// The most tiles added at a time, however wide the page.
const LARGEST_BATCH: usize = 100;

/// What the binder is showing.
#[derive(Debug, Clone, PartialEq)]
struct Binder {
    /// Every card, unless a search is in progress.
    all: bool,
    /// When not `all`: cards matching any of these (a type, an archetype, a
    /// motivation or a habitat). None at all shows nothing.
    filters: Vec<String>,
    /// When not empty: cards matching this, whatever `all` and `filters` say.
    search: String,
    /// How many tiles are on the page so far. 0 until the first batch has
    /// been added to.
    shown: usize,
}

impl Default for Binder {
    fn default() -> Self {
        Binder { all: true, filters: Vec::new(), search: String::new(), shown: 0 }
    }
}

thread_local! {
    static BINDER: RefCell<Binder> = RefCell::new(Binder::default());
}

/// Show every card from the top, as a fresh page does.
pub fn reset() {
    BINDER.with(|binder| *binder.borrow_mut() = Binder::default());
}

/// The cards the binder is showing, in catalog order. Hidden cards (the 404
/// Glitch's two pages) are never among them.
fn selected(binder: &Binder) -> Vec<&'static Card> {
    CARDS
        .iter()
        .filter(|card| {
            if card.hidden {
                return false;
            }
            // A search in progress matches against it, whatever else is set.
            if !binder.search.is_empty() {
                return card_matches_search(card, &binder.search);
            }
            if binder.all {
                return true;
            }
            // No search and not all: at least one filter is needed.
            !binder.filters.is_empty() && card_matches_filters(card, &binder.filters)
        })
        .collect()
}

/// Check if a card matches any of the given filter values.
/// Filters match against: layout, genetic_disposition, motivation, habitat.
fn card_matches_filters(card: &Card, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    for f in filters {
        if card.layout.eq_ignore_ascii_case(f) {
            return true;
        }
        if let Some(gd) = card.genetic_disposition {
            if gd.eq_ignore_ascii_case(f) {
                return true;
            }
        }
        if let Some(mot) = card.motivation {
            if mot.eq_ignore_ascii_case(f) {
                return true;
            }
        }
        if let Some(hab) = card.habitat {
            if hab.eq_ignore_ascii_case(f) {
                return true;
            }
        }
    }
    false
}

/// Check if a card matches a search query (case-insensitive substring match on tags + slug + title).
fn card_matches_search(card: &Card, search: &str) -> bool {
    if search.is_empty() {
        return true;
    }
    let search_lower = search.to_ascii_lowercase();
    // Match against tags + slug + title (same fields the Alpine regex searched)
    let haystack = format!("{}{}{}", card.tags, card.slug, card.title).to_ascii_lowercase();
    haystack.contains(&search_lower)
}

/// Longest-side pixel cap for grid thumbnails. The Service Worker fetches the
/// whole `.jxl`, the WASM pool decodes it at full resolution, then downscales to
/// this size — keeping the decoded BMP tile-sized while staying crisp on a ~200px
/// tile at high DPI.
const GRID_THUMB_MAXDIM: u32 = 512;

/// Render a single card as an HTML fragment with skeleton placeholder and smooth image loading.
///
/// The tile has an id and is marked to be kept: once it is on the page, later
/// drawings of the grid leave it as it is, with its art loaded.
///
/// Each card has one `.jxl`; the grid requests it with `?d=<maxdim>` so the
/// decode worker pool downscales the full-resolution decode to a tile-sized,
/// fully-colored thumbnail. `object-cover` + the per-card `thumbnail` anchor
/// frames the tile.
fn render_card(card: &Card, delay_ms: usize, is_initial_load: bool) -> String {
    // Different animation delay for initial load vs scroll-loaded cards
    let stagger_delay = if is_initial_load {
        delay_ms // Keep nice stagger for initial load
    } else {
        delay_ms.min(180) // Cap at 180ms (3 cards worth) for scroll-loaded cards
    };

    let anchor = match card.thumbnail {
        "top" => "object-top",
        "bottom" => "object-bottom",
        _ => "object-center",
    };

    // Image-as-background tile: the art fills the rounded tile and the card name
    // is overlaid on a bottom gradient scrim. Skeleton sits under the image and
    // is cleared on load; a subtle hover zoom is clipped by overflow-hidden.
    format!(
        r#"<div id="tile-{slug}" data-engine-keep class="animate-card-fade-in relative w-40 h-64 lg:w-50 lg:h-68" style="animation-delay:{stagger}ms">
  <a href="{url}"
    class="group block relative w-full h-full rounded-md overflow-hidden shadow-lg active:shadow-inner bg-kip-drk-sienna"
  >
    <div class="skeleton-pulse absolute inset-0"></div>
    <img
      src="/assets/images/{img}?d={maxdim}"
      alt="{alt}"
      loading="lazy"
      decoding="async"
      fetchpriority="low"
      class="absolute inset-0 w-full h-full object-cover {anchor} opacity-0 transition duration-300 ease-out group-hover:scale-105"
      onload="this.classList.remove('opacity-0'); this.previousElementSibling.classList.remove('skeleton-pulse');"
    >
    <div class="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/85 via-black/45 to-transparent px-2 pt-8 pb-2">
      <span class="block text-center text-balance leading-tight font-semibold text-sm text-amber-50 line-clamp-2 drop-shadow-md">{title}</span>
    </div>
  </a>
</div>"#,
        slug = card.slug,
        stagger = stagger_delay,
        url = card.url,
        img = card.img_name,
        maxdim = GRID_THUMB_MAXDIM,
        anchor = anchor,
        alt = card.img_alt,
        title = card.title,
    )
}

/// Render the marker that follows the tiles when there are more cards: the
/// page posts what it says when it scrolls into view (`data-engine-seen`),
/// and the grid is drawn again with `shown` tiles.
///
/// It is as tall as a tile, so it holds a place in the grid's last row.
fn render_more(shown: usize) -> String {
    format!(
        r#"<div data-engine-post="/api/cards" data-engine-body="shown={shown}" data-engine-seen class="min-h-[16rem] md:min-h-[20rem]" data-scroll-load="true"></div>"#,
        shown = shown,
    )
}

/// Render the grid: the tiles on the page so far, at least one batch, and the
/// marker for more if there are more.
fn render_grid(batch: usize) -> String {
    let binder = BINDER.with(|binder| binder.borrow().clone());
    let cards = selected(&binder);
    let shown = binder.shown.max(batch).min(cards.len());

    let mut html = String::with_capacity(shown * 1100);
    for (place, card) in cards[..shown].iter().enumerate() {
        // Tiles fade in one after another within their batch. Batches after
        // the first arrive while scrolling, so theirs is quicker.
        let delay_ms = (place % batch) * 60;
        html.push_str(&render_card(card, delay_ms, place < batch));
    }
    if shown < cards.len() {
        html.push_str(&render_more(shown + batch));
    }
    html
}

/// Handle GET /api/cards?columns=<n>
///
/// `columns` is how many columns the page's grid has; a batch is [`ROWS`]
/// rows of it.
pub fn handle(query: &str) -> String {
    let params = parse_query(query);
    let columns = get_param(&params, "columns")
        .and_then(|n| n.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_COLUMNS);
    render_grid((columns * ROWS).min(LARGEST_BATCH))
}

/// Handle POST /api/cards
///
/// `shown=<n>`: show at least n tiles (the marker's request for more). Asked
/// twice, it is done once.
///
/// Otherwise, which cards to show, from the top: `all=true`, or
/// `filter=<a>,<b>` (cards matching any), or `search=<text>`. What is not
/// given is off.
///
/// Returns nothing; the page draws the grid again.
pub fn handle_post(body: &str) -> String {
    let params = parse_form_body(body);
    BINDER.with(|binder| {
        let mut binder = binder.borrow_mut();
        if let Some(shown) = get_param(&params, "shown") {
            let shown = shown.parse::<usize>().unwrap_or(0).min(CARDS.len());
            binder.shown = binder.shown.max(shown);
            return;
        }
        *binder = Binder {
            all: get_param(&params, "all") == Some("true"),
            filters: get_param(&params, "filter")
                .unwrap_or("")
                .split(',')
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect(),
            search: get_param(&params, "search").unwrap_or("").trim().to_string(),
            shown: 0,
        };
    });
    String::new()
}

// ── GET /api/card/panel ────────────────────────────────────────────

/// Handle GET /api/card/panel?card={slug}
///
/// Returns one card's detail panel: its title, the rows of its type, its
/// damage tracker if it fights. An engine-owned view; see `game::card_panel`.
pub fn handle_panel(query: &str) -> String {
    let params = crate::routes::util::parse_query(query);
    match crate::routes::util::get_param(&params, "card") {
        Some(slug) if !slug.is_empty() => crate::game::card_panel::render_card_panel(slug),
        _ => r#"<span class="text-kip-red">Missing card parameter</span>"#.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The slugs of the tiles in some grid markup, in order.
    fn tiles(html: &str) -> Vec<&str> {
        html.split(r#"<div id="tile-"#)
            .skip(1)
            .map(|rest| &rest[..rest.find('"').unwrap()])
            .collect()
    }

    fn shown_cards() -> Vec<&'static Card> {
        CARDS.iter().filter(|c| !c.hidden).collect()
    }

    const MORE: &str = r#"<div data-engine-post="/api/cards" data-engine-body="shown="#;

    #[test]
    fn a_fresh_binder_shows_the_first_batch_of_every_card() {
        reset();
        // Two columns, three rows.
        let html = handle("?columns=2");
        let expected: Vec<&str> = shown_cards().iter().take(6).map(|c| c.slug).collect();
        assert_eq!(tiles(&html), expected);
        assert_eq!(html.matches("<a href=").count(), 6);
        // And asks for the next batch when its end is reached.
        assert!(html.ends_with(&format!(
            r#"{}12" data-engine-seen class="min-h-[16rem] md:min-h-[20rem]" data-scroll-load="true"></div>"#,
            MORE
        )));
        reset();
    }

    #[test]
    fn a_batch_is_three_rows_of_however_many_columns_the_page_has() {
        reset();
        assert_eq!(tiles(&handle("?columns=5")).len(), 15);
        assert_eq!(tiles(&handle("?columns=1")).len(), 3);
        // Not said, or nonsense: four columns.
        assert_eq!(tiles(&handle("")).len(), 12);
        assert_eq!(tiles(&handle("?columns=0")).len(), 12);
        assert_eq!(tiles(&handle("?columns=many")).len(), 12);
        // However wide, no more than all the cards.
        assert_eq!(tiles(&handle("?columns=500")).len(), shown_cards().len());
        reset();
    }

    #[test]
    fn more_adds_a_batch_after_the_tiles_already_there() {
        reset();
        let first = handle("?columns=2");
        handle_post("shown=12");
        let second = handle("?columns=2");
        assert_eq!(tiles(&second).len(), 12);
        assert_eq!(tiles(&second)[..6], tiles(&first)[..]);
        assert!(second.contains(&format!(r#"{}18""#, MORE)));
        // The first six tiles are rendered exactly as they were.
        let end_of_first = first.find(MORE).unwrap();
        assert!(second.starts_with(&first[..end_of_first]));
        reset();
    }

    #[test]
    fn asking_for_more_twice_adds_it_once() {
        reset();
        handle_post("shown=12");
        handle_post("shown=12");
        assert_eq!(tiles(&handle("?columns=2")).len(), 12);
        // Nor does a late request for fewer take any away.
        handle_post("shown=6");
        assert_eq!(tiles(&handle("?columns=2")).len(), 12);
        reset();
    }

    #[test]
    fn every_card_is_reached_once_and_then_there_is_no_marker() {
        reset();
        let all = shown_cards();
        let mut html = handle("?columns=2");
        let mut rounds = 0;
        while let Some(at) = html.find(MORE) {
            let rest = &html[at + MORE.len()..];
            let shown = &rest[..rest.find('"').unwrap()];
            handle_post(&format!("shown={}", shown));
            html = handle("?columns=2");
            rounds += 1;
            assert!(rounds < 100, "the marker never goes away");
        }
        let slugs: Vec<&str> = all.iter().map(|c| c.slug).collect();
        assert_eq!(tiles(&html), slugs);
        assert!(!html.contains("data-engine-seen"));
        // Hidden cards are not in the binder.
        assert!(!slugs.contains(&"fourohfour") && !slugs.contains(&"404"));
        reset();
    }

    #[test]
    fn a_tile_is_kept_by_the_page_and_can_be_told_from_the_others() {
        reset();
        let html = handle("?columns=500");
        for card in shown_cards() {
            assert_eq!(
                html.matches(&format!(r#"<div id="tile-{}" data-engine-keep "#, card.slug)).count(),
                1,
                "{}",
                card.slug
            );
        }
        reset();
    }

    #[test]
    fn tiles_fade_in_one_after_another_within_their_batch() {
        reset();
        handle_post("shown=12");
        let html = handle("?columns=2");
        let delays: Vec<&str> = html
            .split("animation-delay:")
            .skip(1)
            .map(|rest| &rest[..rest.find("ms").unwrap()])
            .collect();
        // The first batch staggers all the way; a later one is capped.
        assert_eq!(delays, ["0", "60", "120", "180", "240", "300", "0", "60", "120", "180", "180", "180"]);
        reset();
    }

    #[test]
    fn filters_show_the_cards_matching_any_of_them() {
        reset();
        handle_post("filter=Sabotage");
        let html = handle("?columns=500");
        let shown = tiles(&html);
        assert!(!shown.is_empty());
        for slug in &shown {
            let card = CARDS.iter().find(|c| c.slug == *slug).unwrap();
            assert_eq!(card.layout, "Sabotage", "{}", slug);
        }
        handle_post("filter=Sabotage,Trap");
        let both = tiles(&handle("?columns=500")).len();
        assert!(both > shown.len());
        assert!(both < shown_cards().len());
        reset();
    }

    #[test]
    fn a_search_matches_names_and_tags_whatever_the_filters_say() {
        reset();
        handle_post("search=otter");
        assert_eq!(tiles(&handle("?columns=500")), ["frost_tipped_arctic_otter"]);
        // With all on as well, the search still decides.
        handle_post("all=true&search=otter");
        assert_eq!(tiles(&handle("?columns=500")), ["frost_tipped_arctic_otter"]);
        handle_post("search=%20OTTER%20");
        assert_eq!(tiles(&handle("?columns=500")), ["frost_tipped_arctic_otter"]);
        reset();
    }

    #[test]
    fn neither_all_nor_a_filter_nor_a_search_shows_nothing() {
        reset();
        // The state the search bar opens in: blank until something is typed.
        handle_post("all=false");
        assert_eq!(handle("?columns=2"), "");
        handle_post("");
        assert_eq!(handle("?columns=2"), "");
        reset();
    }

    #[test]
    fn changing_what_is_shown_starts_from_the_top() {
        reset();
        handle_post("shown=24");
        assert_eq!(tiles(&handle("?columns=2")).len(), 24);
        handle_post("all=true");
        assert_eq!(tiles(&handle("?columns=2")).len(), 6);
        reset();
    }

    #[test]
    fn a_change_of_filter_renders_the_tiles_that_stay_exactly_as_before() {
        reset();
        let all = handle("?columns=500");
        handle_post("filter=Sabotage");
        let some = handle("?columns=500");
        for slug in tiles(&some) {
            let tile = |html: &str| {
                let start = html.find(&format!(r#"<div id="tile-{}""#, slug)).unwrap();
                let rest = &html[start..];
                // Up to the tile's link closing; what follows is its own closing div.
                rest[..rest.find("</a>").unwrap()].to_string()
            };
            // The same but for where it falls in its batch (the fade-in delay).
            let strip = |t: String| {
                let at = t.find("animation-delay:").unwrap();
                let end = at + t[at..].find("ms").unwrap();
                format!("{}{}", &t[..at], &t[end..])
            };
            assert_eq!(strip(tile(&some)), strip(tile(&all)), "{}", slug);
        }
        reset();
    }
}
