//! Route handlers for `/api/recipes`, the recipe lookup (see
//! `game::recipes`).
//!
//! GET draws the lookup as it is: the items, the recipe for what is picked,
//! and the picked items' pictures.
//!
//! POST, from a box in the lookup: `pick=<item>` picks or unpicks that item.
//! The view is drawn again afterwards.

use crate::game::recipes;
use crate::routes::util::{get_param, parse_form_body};

pub fn handle(_query: &str) -> String {
    recipes::render()
}

pub fn handle_post(body: &str) -> String {
    let params = parse_form_body(body);
    recipes::pick(get_param(&params, "pick").unwrap_or(""));
    recipes::render()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_post_picks_and_the_view_shows_it() {
        recipes::reset();
        let html = handle_post("pick=honey");
        assert!(html.contains("Yum!"));
        assert_eq!(handle(""), html);
        // Anything else in the body changes nothing.
        assert_eq!(handle_post("pick=&other=1"), html);
        recipes::reset();
    }
}
