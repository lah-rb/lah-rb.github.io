/// Route handler for GET /api/type-matchup
///
/// Query params:
///   atk[]  — up to 3 attacker archetypes (repeated param)
///   def[]  — up to 3 defender archetypes (repeated param)
///   motAtk — optional attacker motive
///   motDef — optional defender motive
///
/// Returns an HTML fragment for HTMX to swap into the DOM.

use crate::typing::{parse_archetype, parse_motive, type_matchup};

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

    // Parse attackers
    let atk_strs = query_values(q, "atk");
    let attackers: Vec<_> = atk_strs.iter().filter_map(|s| parse_archetype(s)).collect();

    // Parse defenders
    let def_strs = query_values(q, "def");
    let defenders: Vec<_> = def_strs.iter().filter_map(|s| parse_archetype(s)).collect();

    // Parse motives
    let atk_motive = query_value(q, "motAtk").and_then(parse_motive);
    let def_motive = query_value(q, "motDef").and_then(parse_motive);

    if attackers.is_empty() && defenders.is_empty() {
        return r#"<span><strong>Attack Die Modifier:</strong></span>"#.to_string();
    }

    let result = type_matchup(&attackers, &defenders, atk_motive, def_motive);
    let display = result.to_display_string();

    // Return HTML fragment — preserve newlines as <br> for display
    let html_display = display.replace('\n', "<br>");

    format!(
        r#"<span><strong>Attack Die Modifier:</strong> {}</span>"#,
        html_display
    )
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
        let html = handle("atk[]=Cenozoic&def[]=Decrepit");
        assert!(html.contains("3"));
    }

    #[test]
    fn multi_attacker() {
        let html = handle("atk[]=Brutal&atk[]=Magic&def[]=Avian");
        assert!(html.contains("1"));
    }

    #[test]
    fn with_motives() {
        let html = handle("atk[]=Cenozoic&def[]=Cenozoic&motAtk=Spirit&motDef=Survival");
        assert!(html.contains("Defender must win 2 of 3"));
        assert!(html.contains("Defender trys retreat before attack."));
    }

    #[test]
    fn query_with_question_mark() {
        let html = handle("?atk[]=Entropic&def[]=Cenozoic");
        assert!(html.contains("3"));
    }

    #[test]
    fn plain_key_format() {
        let html = handle("atk=Cenozoic&def=Decrepit");
        assert!(html.contains("3"));
    }
}