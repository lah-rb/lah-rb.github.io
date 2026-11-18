//! Route handlers for `/api/type-matchup`, the type matchup calculator.
//!
//! GET, as one of the calculator's three views (see `game::matchup`):
//!   part=archetypes | motivations | result
//!
//! GET, for a result worked out from the query alone, with nothing kept:
//!   atk[]  — up to 3 attacker archetypes (repeated param)
//!   def[]  — up to 3 defender archetypes (repeated param)
//!   motAtk — optional attacker motive
//!   motDef — optional defender motive
//!
//! POST, from a box in the calculator: `pick=atk|def|motAtk|motDef&type=<name>`
//! picks or unpicks that name. The views are drawn again afterwards.

use crate::game::matchup;
use crate::routes::util::{get_param, parse_form_body};

/// Simple query string parser: extracts all values for a given key.
/// Handles `key=value&key=value2` and `key[]=value&key[]=value2` patterns.
fn query_values<'a>(query: &'a str, key: &str) -> Vec<&'a str> {
    let bracket_key = format!("{}[]=", key);
    let plain_key = format!("{}=", key);

    query
        .split('&')
        .filter_map(|pair| {
            if let Some(val) = pair.strip_prefix(&bracket_key) {
                Some(val)
            } else if let Some(val) = pair.strip_prefix(&plain_key) {
                Some(val)
            } else {
                None
            }
        })
        .collect()
}

/// Extract a single query param value.
fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{}=", key);
    query.split('&').find_map(|pair| pair.strip_prefix(&prefix))
}

pub fn handle(query: &str) -> String {
    // Strip leading '?' if present
    let q = query.strip_prefix('?').unwrap_or(query);

    // One of the calculator's views, drawn from what is picked.
    match query_value(q, "part") {
        Some("archetypes") => return matchup::render_archetypes(),
        Some("motivations") => return matchup::render_motivations(),
        Some("result") => return matchup::render_result(),
        _ => {}
    }

    // Otherwise a result for the archetypes and motives the query names.
    matchup::result_html(
        &query_values(q, "atk"),
        &query_values(q, "def"),
        query_value(q, "motAtk"),
        query_value(q, "motDef"),
    )
}

/// Pick or unpick one archetype or motivation. Returns the result as it now
/// is; the calculator's views are drawn again by the page.
pub fn handle_post(body: &str) -> String {
    let params = parse_form_body(body);
    matchup::pick(
        get_param(&params, "pick").unwrap_or(""),
        get_param(&params, "type").unwrap_or(""),
    );
    matchup::render_result()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query() {
        let html = handle("");
        assert!(html.contains("Attack Die Modifier:"));
        assert!(!html.contains("<br>"));
    }

    #[test]
    fn single_matchup() {
        // Cenozoic attacks Decrepit: TABLE[Decrepit][Cenozoic] = -3
        let html = handle("atk[]=Cenozoic&def[]=Decrepit");
        assert!(html.contains("-3"));
    }

    #[test]
    fn multi_attacker() {
        // Brutal+Magic attack Avian: TABLE[Avian][Brutal]=1 + TABLE[Avian][Magic]=-2 = -1
        let html = handle("atk[]=Brutal&atk[]=Magic&def[]=Avian");
        assert!(html.contains("-1"));
    }

    #[test]
    fn with_motives() {
        let html = handle("atk[]=Cenozoic&def[]=Cenozoic&motAtk=Spirit&motDef=Survival");
        assert!(html.contains("Defender must win 2 of 3"));
        assert!(html.contains("Defender trys retreat before attack."));
    }

    #[test]
    fn query_with_question_mark() {
        // Entropic attacks Cenozoic: TABLE[Cenozoic][Entropic] = -3
        let html = handle("?atk[]=Entropic&def[]=Cenozoic");
        assert!(html.contains("-3"));
    }

    #[test]
    fn the_calculators_views_are_drawn_from_what_is_picked() {
        matchup::reset();
        assert!(handle("part=archetypes").contains(r#"data-engine-body="pick=atk&type=Brutal""#));
        assert!(handle("?part=motivations").contains(r#"data-engine-body="pick=motDef&type=Duty""#));
        assert_eq!(handle("part=result"), "<span><strong>Attack Die Modifier:</strong></span>");

        // Cenozoic attacks Decrepit: -3.
        handle_post("pick=atk&type=Cenozoic");
        let html = handle_post("pick=def&type=Decrepit");
        assert!(html.contains("-3"));
        assert_eq!(handle("part=result"), html);
        assert_eq!(handle("part=archetypes").matches(" checked").count(), 2);
        matchup::reset();
    }

    #[test]
    fn a_result_from_the_query_alone_picks_nothing() {
        matchup::reset();
        handle("atk=Cenozoic&def=Decrepit");
        assert!(!handle("part=archetypes").contains(" checked"));
    }

    #[test]
    fn plain_key_format() {
        // Cenozoic attacks Decrepit: TABLE[Decrepit][Cenozoic] = -3
        let html = handle("atk=Cenozoic&def=Decrepit");
        assert!(html.contains("-3"));
    }
}