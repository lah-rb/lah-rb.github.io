//! The type matchup calculator: pick the archetypes and motivations of an
//! attacker and a defender and read off the attack die modifier.
//!
//! The calculator is three engine-owned views (Pattern 13) of one route,
//! `/api/type-matchup`: the archetypes, the motivations and the result. What
//! is picked is held here. A checkbox or radio button is a post that picks or
//! unpicks one thing; the engine answers by drawing all three again, with the
//! boxes checked that are picked and the rest disabled once a side has
//! [`MOST`] archetypes. The page keeps no count and no copy of the choices.
//!
//! The picks last as long as the engine does: until the page is left. They
//! are not saved with the player's data. They are a scratch pad, not game
//! state.

use std::cell::RefCell;

use crate::typing::{parse_archetype, parse_motive, type_matchup};

/// The route the calculator's views are drawn from and its controls post to.
const ROUTE: &str = "/api/type-matchup";

/// How many archetypes an attacker or a defender can have.
pub const MOST: usize = 3;

/// The archetypes, in the order the calculator lists them.
pub const ARCHETYPES: [&str; 15] = [
    "Cenozoic",
    "Decrepit",
    "Angelic",
    "Brutal",
    "Arboreal",
    "Astral",
    "Telekinetic",
    "Glitch",
    "Magic",
    "Endothermic",
    "Avian",
    "Mechanical",
    "Algorithmic",
    "Energetic",
    "Entropic",
];

/// The motivations, in the order the calculator lists them.
pub const MOTIVATIONS: [&str; 9] = [
    "Spirit",
    "Service",
    "Survival",
    "Possessor",
    "Satisfaction",
    "Sacrifice",
    "Conscience",
    "Passion",
    "Duty",
];

/// A checkbox's or radio button's classes (`checkbox` in
/// `_data/templates.yml`, which the site's other forms use).
const BOX_CLASS: &str = "text-kip-red rounded mr-2 focus:ring-1 focus:ring-kip-red disabled:bg-slate-100";

/// What is picked.
#[derive(Debug, Default, Clone, PartialEq)]
struct Picks {
    attackers: Vec<&'static str>,
    defenders: Vec<&'static str>,
    attacker_motive: Option<&'static str>,
    defender_motive: Option<&'static str>,
}

thread_local! {
    static PICKS: RefCell<Picks> = RefCell::new(Picks::default());
}

/// Unpick everything.
pub fn reset() {
    PICKS.with(|picks| *picks.borrow_mut() = Picks::default());
}

/// The name as the calculator knows it, or `None` if it knows no such name.
fn known(names: &'static [&'static str], name: &str) -> Option<&'static str> {
    names.iter().copied().find(|known| *known == name)
}

/// Pick or unpick one thing. `side` is which list it is in: `atk` or `def`
/// for an archetype, `motAtk` or `motDef` for a motivation.
///
/// An archetype that is picked is unpicked; one that is not is picked,
/// unless its side already has [`MOST`]. A side has one motivation or none:
/// picking another replaces it, picking the same one again clears it.
/// Anything else is ignored.
pub fn pick(side: &str, name: &str) {
    PICKS.with(|picks| {
        let mut picks = picks.borrow_mut();
        match side {
            "atk" | "def" => {
                let Some(name) = known(&ARCHETYPES, name) else {
                    return;
                };
                let list = if side == "atk" { &mut picks.attackers } else { &mut picks.defenders };
                if let Some(at) = list.iter().position(|picked| *picked == name) {
                    list.remove(at);
                } else if list.len() < MOST {
                    list.push(name);
                }
            }
            "motAtk" | "motDef" => {
                let Some(name) = known(&MOTIVATIONS, name) else {
                    return;
                };
                let motive = if side == "motAtk" {
                    &mut picks.attacker_motive
                } else {
                    &mut picks.defender_motive
                };
                *motive = if *motive == Some(name) { None } else { Some(name) };
            }
            _ => {}
        }
    });
}

/// One column of boxes: a heading, then a box for each name.
fn column(heading: &str, side: &str, kind: &str, names: &[&str], picked: &[&str], full: bool) -> String {
    let mut html = format!("<div><span><b>{}</b></span>", heading);
    for name in names {
        let is_picked = picked.contains(name);
        html.push_str(&format!(
            r#"<div><label><input type="{kind}" name="{side}" class="{class}" data-engine-post="{route}" data-engine-body="pick={side}&type={name}"{checked}{disabled}>{name}</label></div>"#,
            kind = kind,
            side = side,
            class = BOX_CLASS,
            route = ROUTE,
            name = name,
            checked = if is_picked { " checked" } else { "" },
            // A side that has all it can have takes no more until one is unpicked.
            disabled = if full && !is_picked { " disabled" } else { "" },
        ));
    }
    html.push_str("</div>");
    html
}

/// The archetypes: a column of checkboxes for the attacker and one for the
/// defender.
pub fn render_archetypes() -> String {
    let picks = PICKS.with(|picks| picks.borrow().clone());
    format!(
        r#"<div class="grid grid-cols-2">{}{}</div>"#,
        column(
            "Attacker",
            "atk",
            "checkbox",
            &ARCHETYPES,
            &picks.attackers,
            picks.attackers.len() >= MOST
        ),
        column(
            "Defender",
            "def",
            "checkbox",
            &ARCHETYPES,
            &picks.defenders,
            picks.defenders.len() >= MOST
        ),
    )
}

/// The motivations: a column of radio buttons for the attacker and one for
/// the defender.
pub fn render_motivations() -> String {
    let picks = PICKS.with(|picks| picks.borrow().clone());
    let one = |motive: Option<&'static str>| motive.into_iter().collect::<Vec<_>>();
    format!(
        r#"<div class="grid grid-cols-2">{}{}</div>"#,
        column(
            "Attacker",
            "motAtk",
            "radio",
            &MOTIVATIONS,
            &one(picks.attacker_motive),
            false
        ),
        column(
            "Defender",
            "motDef",
            "radio",
            &MOTIVATIONS,
            &one(picks.defender_motive),
            false
        ),
    )
}

/// The attack die modifier for these archetypes and motivations, as the
/// calculator shows it. Names that are not archetypes or motivations are
/// left out.
pub fn result_html(attackers: &[&str], defenders: &[&str], attacker_motive: Option<&str>, defender_motive: Option<&str>) -> String {
    let attackers: Vec<_> = attackers.iter().filter_map(|name| parse_archetype(name)).collect();
    let defenders: Vec<_> = defenders.iter().filter_map(|name| parse_archetype(name)).collect();
    if attackers.is_empty() && defenders.is_empty() {
        return r#"<span><strong>Attack Die Modifier:</strong></span>"#.to_string();
    }
    let result = type_matchup(
        &attackers,
        &defenders,
        attacker_motive.and_then(parse_motive),
        defender_motive.and_then(parse_motive),
    );
    // The result is lines of text; a line break in it is a line break shown.
    format!(
        r#"<span><strong>Attack Die Modifier:</strong> {}</span>"#,
        result.to_display_string().replace('\n', "<br>")
    )
}

/// The result for what is picked now.
pub fn render_result() -> String {
    let picks = PICKS.with(|picks| picks.borrow().clone());
    result_html(
        &picks.attackers,
        &picks.defenders,
        picks.attacker_motive,
        picks.defender_motive,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::player_doc;

    // A box that is disabled is one whose tag ends ` disabled>`. The word alone
    // is in every box's class list (`disabled:bg-slate-100`).

    /// The box for `name` on `side`, as rendered.
    fn the_box(html: &str, side: &str, name: &str) -> String {
        let body = format!(r#"data-engine-body="pick={}&type={}""#, side, name);
        let at = html.find(&body).unwrap_or_else(|| panic!("a box for {} {}", side, name));
        let start = html[..at].rfind("<input").unwrap();
        let end = at + html[at..].find('>').unwrap();
        html[start..=end].to_string()
    }

    #[test]
    fn every_name_the_calculator_lists_is_one_the_game_knows() {
        for name in ARCHETYPES {
            assert!(parse_archetype(name).is_some(), "{}", name);
        }
        for name in MOTIVATIONS {
            assert!(parse_motive(name).is_some(), "{}", name);
        }
        // The same archetypes, in the same order, as the affinity panel.
        assert_eq!(ARCHETYPES.as_slice(), player_doc::valid_archetypes());
    }

    #[test]
    fn a_names_spelling_can_go_in_a_form_body_as_it_is() {
        for name in ARCHETYPES.iter().chain(MOTIVATIONS.iter()) {
            assert!(name.chars().all(|c| c.is_ascii_alphanumeric()), "{}", name);
        }
    }

    #[test]
    fn nothing_is_picked_to_begin_with() {
        reset();
        let archetypes = render_archetypes();
        assert_eq!(archetypes.matches(r#"type="checkbox""#).count(), 30);
        assert!(!archetypes.contains(" checked"));
        assert!(!archetypes.contains(" disabled>"));
        let motivations = render_motivations();
        assert_eq!(motivations.matches(r#"type="radio""#).count(), 18);
        assert!(!motivations.contains(" checked"));
        assert_eq!(render_result(), "<span><strong>Attack Die Modifier:</strong></span>");
    }

    #[test]
    fn a_box_is_a_post_that_picks_its_own_name_and_has_nothing_for_alpine_or_htmx() {
        reset();
        let html = format!("{}{}", render_archetypes(), render_motivations());
        assert_eq!(
            the_box(&html, "atk", "Brutal"),
            format!(
                r#"<input type="checkbox" name="atk" class="{}" data-engine-post="/api/type-matchup" data-engine-body="pick=atk&type=Brutal">"#,
                BOX_CLASS
            )
        );
        assert!(the_box(&html, "motDef", "Duty").starts_with(r#"<input type="radio" name="motDef""#));
        for forbidden in ["x-data", "@click", ":checked", "hx-", "htmx", " id="] {
            assert!(!html.contains(forbidden), "{} in the calculator", forbidden);
        }
    }

    #[test]
    fn picking_checks_the_box_and_picking_again_unchecks_it() {
        reset();
        pick("atk", "Brutal");
        let html = render_archetypes();
        assert!(the_box(&html, "atk", "Brutal").ends_with(" checked>"));
        // The same name on the other side is another box.
        assert!(!the_box(&html, "def", "Brutal").contains("checked"));
        pick("atk", "Brutal");
        assert!(!render_archetypes().contains(" checked"));
        reset();
    }

    #[test]
    fn a_side_with_three_archetypes_takes_no_more_until_one_is_unpicked() {
        reset();
        for name in ["Brutal", "Magic", "Avian"] {
            pick("atk", name);
        }
        let html = render_archetypes();
        assert_eq!(html.matches(" checked").count(), 3);
        // The other twelve attacker boxes are disabled; no defender box is.
        assert_eq!(html.matches(" disabled>").count(), 12);
        assert!(the_box(&html, "atk", "Glitch").ends_with(" disabled>"));
        assert!(!the_box(&html, "atk", "Magic").ends_with(" disabled>"));
        assert!(!the_box(&html, "def", "Glitch").ends_with(" disabled>"));

        // A fourth is not taken, even if asked for.
        pick("atk", "Glitch");
        assert_eq!(render_archetypes().matches(" checked").count(), 3);

        pick("atk", "Magic");
        let html = render_archetypes();
        assert_eq!(html.matches(" checked").count(), 2);
        assert!(!html.contains(" disabled>"));
        reset();
    }

    #[test]
    fn a_side_has_one_motivation_or_none() {
        reset();
        pick("motAtk", "Spirit");
        assert!(the_box(&render_motivations(), "motAtk", "Spirit").ends_with(" checked>"));
        pick("motAtk", "Duty"); // another replaces it
        let html = render_motivations();
        assert_eq!(html.matches(" checked").count(), 1);
        assert!(the_box(&html, "motAtk", "Duty").ends_with(" checked>"));
        pick("motAtk", "Duty"); // the same again clears it
        assert!(!render_motivations().contains(" checked"));
        // Motivations are never disabled.
        pick("motDef", "Survival");
        assert!(!render_motivations().contains(" disabled>"));
        reset();
    }

    #[test]
    fn the_result_is_for_what_is_picked() {
        reset();
        // Cenozoic attacks Decrepit: -3.
        pick("atk", "Cenozoic");
        pick("def", "Decrepit");
        assert!(render_result().contains("-3"));
        assert_eq!(render_result(), result_html(&["Cenozoic"], &["Decrepit"], None, None));

        // Motivations that interact add their lines.
        reset();
        pick("atk", "Cenozoic");
        pick("def", "Cenozoic");
        pick("motAtk", "Spirit");
        pick("motDef", "Survival");
        let html = render_result();
        assert!(html.contains("Defender must win 2 of 3"));
        assert!(html.contains("<br>"));
        reset();
    }

    #[test]
    fn a_name_or_side_the_calculator_does_not_know_is_ignored() {
        reset();
        pick("atk", "Dragon");
        pick("atk", "brutal"); // names are as listed, capitals and all
        pick("motAtk", "Brutal");
        pick("sideways", "Brutal");
        pick("atk", "\"><script>");
        assert_eq!(PICKS.with(|picks| picks.borrow().clone()), Picks::default());
    }
}
