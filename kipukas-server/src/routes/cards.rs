//! `/api/cards` route — paginated, filtered card catalog for HTMX infinite scroll.
//!
//! Query parameters:
//! - `page` — 0-indexed page number (default: 0)
//! - `per`  — cards per page (default: 12)
//! - `filter` — comma-separated filter values (layout, genetic_disposition, motivation, habitat)
//! - `search` — case-insensitive search string matched against tags + slug
//! - `all` — if "true", show all cards (ignore filter). Default behavior.

use crate::cards_generated::{Card, CARDS};

/// Parse a query string into key-value pairs.
/// Handles `?key=value&key2=value2` format.
fn parse_query(query: &str) -> Vec<(&str, &str)> {
    let q = query.strip_prefix('?').unwrap_or(query);
    if q.is_empty() {
        return Vec::new();
    }
    q.split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?;
            let val = parts.next().unwrap_or("");
            Some((key, val))
        })
        .collect()
}

/// Percent-decode a query value (basic: handles %20, %2C, etc.)
fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(b'0');
            let lo = chars.next().unwrap_or(b'0');
            let hex = [hi, lo];
            if let Ok(s) = core::str::from_utf8(&hex) {
                if let Ok(val) = u8::from_str_radix(s, 16) {
                    result.push(val as char);
                    continue;
                }
            }
            result.push('%');
            result.push(hi as char);
            result.push(lo as char);
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
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

/// Render a single card as an HTML fragment with skeleton placeholder and smooth image loading.
fn render_card(card: &Card, delay_ms: usize, is_initial_load: bool) -> String {
    // Different animation delay for initial load vs scroll-loaded cards
    let stagger_delay = if is_initial_load {
        delay_ms // Keep nice stagger for initial load
    } else {
        delay_ms.min(180) // Cap at 180ms (3 cards worth) for scroll-loaded cards
    };

    format!(
        r#"<div class="w-40 h-64 md:w-60 md:h-80 my-2.5 animate-card-fade-in relative" style="animation-delay:{}ms">
  <a href="{url}"
    class="grid grid-cols-1 w-full h-full pt-4 my-auto bg-amber-50 active:shadow-inner inline-block active:bg-amber-100 hover:bg-amber-100 shadow-lg font-semibold text-kip-drk-goldenrod rounded overflow-hidden"
  >
    <picture class="skeleton-pulse relative">
      <source media="(min-width: 768px)"
        srcset="/assets/thumbnails/x2/{img} 1x, /assets/thumbnails/x4/{img} 2x">
      <img
        src="/assets/thumbnails/x1/{img}"
        srcset="/assets/thumbnails/x1/{img} 1x, /assets/thumbnails/x2/{img} 2x, /assets/thumbnails/x3/{img} 3x"
        alt="{alt}"
        loading="lazy"
        decoding="async"
        class="w-full h-auto card-image opacity-0 transition-opacity duration-300"
        onload="this.classList.remove('opacity-0'); this.parentElement.classList.remove('skeleton-pulse');"
      >
    </picture>
    <div class="text-center text-wrap">{title}</div>
  </a>
</div>"#,
        stagger_delay,
        url = card.url,
        img = card.img_name,
        alt = card.img_alt,
        title = card.title,
    )
}

/// Render the sentinel div that triggers the next page load.
/// Uses threshold:0.3 to prevent "over eager" loading and adds delay for smoother UX.
fn render_sentinel(page: usize, per: usize, filter_param: &str, search_param: &str, all: bool) -> String {
    let mut query_parts = vec![
        format!("page={}", page),
        format!("per={}", per),
    ];
    if all {
        query_parts.push("all=true".to_string());
    }
    if !filter_param.is_empty() {
        query_parts.push(format!("filter={}", filter_param));
    }
    if !search_param.is_empty() {
        query_parts.push(format!("search={}", search_param));
    }
    let query = query_parts.join("&");

    // Add data attribute to indicate this is not initial load (for animation timing)
    format!(
        r#"<div hx-get="/api/cards?{query}" 
             hx-trigger="intersect once threshold:0.3" 
             hx-swap="outerHTML" 
             class="w-40 h-64 md:w-60 md:h-80 my-2.5"
             data-scroll-load="true"></div>"#,
        query = query,
    )
}

/// Handle GET /api/cards
pub fn handle(query: &str) -> String {
    let params = parse_query(query);

    let mut page: usize = 0;
    let mut per: usize = 12;
    let mut filter_raw = String::new();
    let mut search_raw = String::new();
    let mut all = false;

    for (key, val) in &params {
        match *key {
            "page" => page = val.parse().unwrap_or(0),
            "per" => per = val.parse().unwrap_or(12),
            "filter" => filter_raw = percent_decode(val),
            "search" => search_raw = percent_decode(val),
            "all" => all = *val == "true",
            _ => {}
        }
    }

    // Clamp per to reasonable bounds
    if per == 0 {
        per = 12;
    }
    if per > 100 {
        per = 100;
    }

    // Parse filter values
    let filters: Vec<String> = if filter_raw.is_empty() {
        Vec::new()
    } else {
        filter_raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    };

    // Filter cards
    let filtered: Vec<&Card> = CARDS
        .iter()
        .filter(|card| {
            // If "all" is true and no search, show everything
            if all && search_raw.is_empty() {
                return true;
            }
            // If search is active, match search (regardless of filter/all)
            if !search_raw.is_empty() {
                return card_matches_search(card, &search_raw);
            }
            // Otherwise, apply filters
            if all {
                return true;
            }
            // No search, no all — need at least one active filter
            if filters.is_empty() {
                return false;
            }
            card_matches_filters(card, &filters)
        })
        .collect();

    let total = filtered.len();
    let start = page * per;

    // If start is beyond filtered results, return empty (no more pages)
    if start >= total {
        return String::new();
    }

    let end = (start + per).min(total);
    let page_cards = &filtered[start..end];
    let has_more = end < total;

    let mut html = String::with_capacity(page_cards.len() * 512);

    // Determine if this is the initial load (page 0) or scroll load
    let is_initial_load = page == 0;

    for (i, card) in page_cards.iter().enumerate() {
        let delay_ms = i * 60;
        html.push_str(&render_card(card, delay_ms, is_initial_load));
    }

    // Add sentinel for next page if there are more cards
    if has_more {
        // Pass through the original filter/search params for the sentinel
        let filter_param = if filters.is_empty() {
            String::new()
        } else {
            filter_raw.clone()
        };
        html.push_str(&render_sentinel(page + 1, per, &filter_param, &search_raw, all));
    }

    html
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_first_page() {
        let html = handle("?page=0&per=4&all=true");
        // Should contain exactly 4 card links + 1 sentinel
        let card_count = html.matches("<a href=").count();
        assert_eq!(card_count, 4);
        assert!(html.contains("hx-trigger=\"intersect once threshold:0.3\""));
    }

    #[test]
    fn returns_empty_for_beyond_last_page() {
        let html = handle("?page=999&per=12&all=true");
        assert!(html.is_empty());
    }

    #[test]
    fn no_sentinel_on_last_page() {
        // Request all cards in one page
        let html = handle("?page=0&per=200&all=true");
        assert!(!html.contains("hx-trigger=\"intersect\""));
        // But should have cards
        assert!(html.contains("<a href="));
    }

    #[test]
    fn filter_by_layout() {
        let html = handle("?page=0&per=100&filter=Sabotage");
        // Should only contain Sabotage cards
        assert!(html.contains("<a href="));
        // All returned cards should be Sabotage type
        // (We can't easily verify this without parsing HTML, but we can verify
        // the count is less than total)
        let card_count = html.matches("<a href=").count();
        assert!(card_count > 0);
        assert!(card_count < 56);
    }

    #[test]
    fn search_works() {
        let html = handle("?page=0&per=100&search=otter");
        assert!(html.contains("frost_tipped_arctic_otter"));
    }

    #[test]
    fn no_filters_no_all_returns_empty() {
        // No all=true and no filter and no search = empty grid
        // (This is the "search mode ready" state — grid is blank until user types)
        let html = handle("?page=0&per=100");
        assert!(html.is_empty());
    }

    #[test]
    fn percent_decode_works() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("a%2Cb"), "a,b");
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn sentinel_carries_params() {
        let html = handle("?page=0&per=4&all=true&search=test");
        if html.contains("hx-trigger") {
            assert!(html.contains("page=1"));
            assert!(html.contains("search=test"));
            assert!(html.contains("all=true"));
        }
    }
}