//! The recipe lookup: pick up to three base items and read what they make.
//!
//! The lookup is one engine-owned view (Pattern 13) of `/api/recipes`: the
//! items to pick from, the recipe for what is picked, and the picked items'
//! pictures. What is picked is held here. A box is a post that picks or
//! unpicks one item; the engine answers by drawing the view again, with the
//! picked boxes checked and the rest disabled once [`MOST`] are picked. The
//! page keeps no count, no copy of the choices and no recipes.
//!
//! The picks last as long as the engine does: until the page is left. They
//! are a scratch pad, not game state, and are not saved with the player's
//! data.
//!
//! Every item's picture and the plus sign before it are always in the
//! markup, hidden unless the item is picked. The pictures are then loaded
//! once, and picking an item only shows what is already there.

use std::cell::RefCell;

use crate::catalog::CARDS;
use crate::routes::util::{BOX_CLASS, escape_html};

/// The route the lookup is drawn from and its boxes post to.
const ROUTE: &str = "/api/recipes";

/// How many items a recipe can combine.
pub const MOST: usize = 3;

/// The base items, in the order the lookup lists them. Each is an item
/// card, named here by its address.
pub const ITEMS: [&str; 5] = ["cloth", "feathers", "honey", "sticks", "string"];

/// The recipes: the items combined (in the order of [`ITEMS`]), what they
/// make, and what that does. There is one for every combination of one to
/// three items.
const RECIPES: &[(&[&str], &str, &str)] = &[
    (&["cloth"], "A \"Disguise\"", "Add 1 to escape roll"),
    (&["feathers"], "Was something killed?", "Opponent loses next turn. Trap activates on D20: 3, 5, 11, Std Detection. Two Feathers cards must be played together to activate effect."),
    (&["honey"], "Yum!", "Recover 1 keal means in 4 turns, or in 2 turns if two Honey cards are played together."),
    (&["sticks"], "Hit with it...", "Add 1 to attack roll. No dragons..."),
    (&["string"], "Tangle up the situation!", "Opponent loses 2 damage from their next attack roll. Two String cards must be played together to activate effect."),
    (&["cloth", "feathers"], "Pillow:", "Recover 1 keal means at night in a basecamp. Fixture item, can be stolen if not moved after a basecamp is lost."),
    (&["cloth", "honey"], "Bee Bomb!", "After 3 diel cycles, all souls in the territory lose a keal means."),
    (&["feathers", "honey"], "Sticky feather trap!", "Take 1 damage from target's rolls until they attempt to swim in or cross water (without a boat). Trap activates on D20: 3, 5, 11, Std Detection."),
    (&["cloth", "sticks"], "Bag on stick:", "Wielder may move 3 reasonable items or 1 reasonable trap."),
    (&["feathers", "sticks"], "Atlatl:", "Wielder now has a ranged attack and can attack 1 movement stand away with Cenozoic D6 vs defender keal means."),
    (&["honey", "sticks"], "Sticky stick fingers?", "Take 1 item card from your opponent's deck via random draw and place it in your deck."),
    (&["cloth", "string"], "Tunic:", "No damage is taken due to storm."),
    (&["feathers", "string"], "Fire Starter?", "Chance to destroy a terrain feature. Win with a D6 vs D20."),
    (&["honey", "string"], "Sweet candle:", "Prolongs day, move 1 soul at night. If movement causes contention, you attack first and add 1 to attack roll. Items are wasted after use."),
    (&["sticks", "string"], "Snare Trap:", "Immobilize target until it can be rescued. Trap activates on D20: 3, 5, 11, Std Detection."),
    (&["cloth", "feathers", "honey"], "Chicken \"Disguise\"", "Buckawk!!! An even better chance to escape. Wielder rolls with a D20 to escape."),
    (&["cloth", "feathers", "sticks"], "Lite training bag:", "Target is immobilized for 1 turn while training. Target gains 1 attack damage until wasted. After training, items are wasted."),
    (&["cloth", "honey", "sticks"], "Torch:", "Prolongs day, move up to 3 souls at night (must start and end at the same spot as the wielder). If movement causes contention, you attack first and add 1 to attack roll. Items are wasted after use."),
    (&["feathers", "honey", "sticks"], "Stick-y feather pit trap?", "Target takes 1 damage. If target is not wasted, take 1 damage from target's rolls until they attempt to swim in or cross water (without a boat). Can be reset. Trap activates on D20: 3, 5, 11, 13, Std Detection."),
    (&["cloth", "feathers", "string"], "Jacket:", "For the wielder, no damage is taken during storm. Absorbs 1 from attacking dice rolls while equipped."),
    (&["cloth", "honey", "string"], "Tough Bundles for blade training!", "The target blade wielder is immobilized for 1 turn while training. Target gains 2 attack damage until wasted. After training, items are wasted."),
    (&["feathers", "honey", "string"], "\"Advanced\" fire starter?", "Better chance to destroy a terrain feature. Win with D6 against D6-D6 with a retry."),
    (&["cloth", "sticks", "string"], "Hammock:", "Recover 1 keal means during day in a basecamp (3 total if Pillow is in the same basecamp). Fixture item, can be stolen if not moved after a basecamp is lost."),
    (&["feathers", "sticks", "string"], "Basic bow:", "Wielder now has a ranged attack and can attack 1 movement stand (or basecamp) away with Cenozoic D6-D6 (or D6 against soul in basecamp) vs defender keal means. Wielder can also launch small things (i.e. Bee Bomb)."),
    (&["honey", "sticks", "string"], "Baited snare trap:", "Immobilize target for 1 turn. Trap activates on D20: 3, 5, 11, 13, 17, 19, Std Detection. Insect/Bug appearing things become trapped as soon as they are in the same territory."),
];

thread_local! {
    /// The items picked, in the order of [`ITEMS`].
    static PICKED: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

/// Unpick everything.
pub fn reset() {
    PICKED.with(|picked| picked.borrow_mut().clear());
}

fn picked() -> Vec<&'static str> {
    PICKED.with(|picked| picked.borrow().clone())
}

/// Pick the item `name`, or unpick it if it is picked. A name that is not an
/// item is ignored, and so is one more item than a recipe can combine.
pub fn pick(name: &str) {
    let Some(item) = ITEMS.iter().copied().find(|item| *item == name) else {
        return;
    };
    PICKED.with(|picked| {
        let mut picked = picked.borrow_mut();
        if let Some(at) = picked.iter().position(|p| *p == item) {
            picked.remove(at);
        } else if picked.len() < MOST {
            picked.push(item);
            picked.sort_by_key(|p| ITEMS.iter().position(|item| item == p));
        }
    });
}

/// What the items make and what that does, if they make anything.
pub fn recipe(items: &[&str]) -> Option<(&'static str, &'static str)> {
    RECIPES
        .iter()
        .find(|(combined, _, _)| *combined == items)
        .map(|(_, makes, does)| (*makes, *does))
}

/// An item's name as the lookup shows it: its card's title.
fn title(item: &str) -> &str {
    CARDS.iter().find(|card| card.slug == item).map_or(item, |card| card.title)
}

/// The lookup as it is now.
pub fn render() -> String {
    let picked = picked();
    let full = picked.len() >= MOST;
    let mut html = String::with_capacity(2048);

    // The items to pick from, and the recipe beside them.
    html.push_str(r#"<div class="md:flex md:flex-row md:gap-x-4">"#);
    html.push_str(r#"<div class="outline outline-kip-drk-goldenrod p-4 pr-20 content-center w-fit h-fit">"#);
    for item in ITEMS {
        let is_picked = picked.contains(&item);
        html.push_str(&format!(
            concat!(
                r#"<div class="flex content-center"><label><input type="checkbox" name="items" "#,
                r#"class="{class} place-self-center" data-engine-post="{route}" data-engine-body="pick={item}"{checked}{disabled}>"#,
                r#"{title}</label></div>"#,
            ),
            class = BOX_CLASS,
            route = ROUTE,
            item = item,
            checked = if is_picked { " checked" } else { "" },
            disabled = if full && !is_picked { " disabled" } else { "" },
            title = escape_html(title(item)),
        ));
    }
    html.push_str("</div>");
    let (makes, does) = recipe(&picked).unwrap_or(("", ""));
    html.push_str(&format!(
        r#"<div class="pt-4 content-center"><p><strong class="text-xl">{}</strong><br><span>{}</span></p></div>"#,
        escape_html(makes),
        escape_html(does),
    ));
    html.push_str("</div>");

    // The picked items' pictures, with a plus between them on a wide screen.
    // The row is as tall as its pictures (they are taller than they are
    // wide), and no shorter on a wide screen than the lookup's page keeps
    // for it before it is drawn.
    html.push_str(r#"<div class="flex flex-wrap m-4 justify-center md:min-h-40 gap-4">"#);
    for item in ITEMS {
        let is_picked = picked.contains(&item);
        let follows_another = is_picked && picked.first() != Some(&item);
        html.push_str(&format!(
            concat!(
                r#"<p class="appear text-7xl content-center mx-2 hidden lg:block" aria-hidden="true"{plus}>+</p>"#,
                r#"<img class="appear rounded outline outline-1 outline-kip-drk-goldenrod" width="160" height="160" "#,
                r#"src="/assets/images/{item}.jxl?d=400" alt="{title}"{picture}>"#,
            ),
            plus = if follows_another { "" } else { " hidden" },
            item = item,
            title = escape_html(title(item)),
            picture = if is_picked { "" } else { " hidden" },
        ));
    }
    html.push_str("</div>");
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lookup's boxes as rendered, in order: (item, checked, disabled).
    fn boxes(html: &str) -> Vec<(String, bool, bool)> {
        html.split("<input")
            .skip(1)
            .map(|b| {
                let tag = b.split('>').next().unwrap();
                let item = tag.split("data-engine-body=\"pick=").nth(1).unwrap().split('"').next().unwrap();
                // The bare attributes follow the last quoted one. (The
                // box's classes name `disabled:` too.)
                let bare = tag.rsplit('"').next().unwrap();
                (item.to_string(), bare.contains(" checked"), bare.contains(" disabled"))
            })
            .collect()
    }

    /// The pictures shown, in order.
    fn shown(html: &str) -> Vec<String> {
        html.split("<img")
            .skip(1)
            .map(|i| i.split('>').next().unwrap())
            .filter(|tag| !tag.rsplit('"').next().unwrap().contains(" hidden"))
            .map(|tag| tag.split("alt=\"").nth(1).unwrap().split('"').next().unwrap().to_string())
            .collect()
    }

    /// How many plus signs are shown.
    fn pluses(html: &str) -> usize {
        html.split("<p class=\"appear")
            .skip(1)
            .filter(|p| !p.split('>').next().unwrap().rsplit('"').next().unwrap().contains(" hidden"))
            .count()
    }

    #[test]
    fn every_item_is_an_item_card() {
        for item in ITEMS {
            let card = CARDS.iter().find(|card| card.slug == item).unwrap_or_else(|| panic!("no card {}", item));
            assert_eq!(card.layout, "Item", "{}", item);
        }
    }

    #[test]
    fn there_is_one_recipe_for_every_combination_of_one_to_three_items() {
        let mut combinations = 0;
        for mask in 1u32..(1 << ITEMS.len()) {
            let items: Vec<&str> = ITEMS.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, item)| *item).collect();
            let found = RECIPES.iter().filter(|(combined, _, _)| *combined == items.as_slice()).count();
            if items.len() <= MOST {
                combinations += 1;
                assert_eq!(found, 1, "{:?}", items);
                let (makes, does) = recipe(&items).unwrap();
                assert!(!makes.is_empty() && !does.is_empty(), "{:?}", items);
            } else {
                assert_eq!(found, 0, "{:?}", items);
            }
        }
        assert_eq!(combinations, 25);
        assert_eq!(RECIPES.len(), 25);
        assert_eq!(recipe(&[]), None);
    }

    #[test]
    fn nothing_is_picked_to_begin_with() {
        reset();
        let html = render();
        assert_eq!(boxes(&html).len(), 5);
        assert!(boxes(&html).iter().all(|(_, checked, disabled)| !checked && !disabled));
        assert!(shown(&html).is_empty());
        assert_eq!(pluses(&html), 0);
        // Every picture is there all the same, to be shown when picked.
        assert_eq!(html.matches("<img").count(), 5);
        assert!(html.contains(r#"<strong class="text-xl"></strong><br><span></span>"#));
    }

    #[test]
    fn a_box_is_a_post_that_picks_its_own_item_and_carries_no_script() {
        reset();
        let html = render();
        assert!(html.contains(&format!(
            r#"<input type="checkbox" name="items" class="{} place-self-center" data-engine-post="/api/recipes" data-engine-body="pick=cloth">Cloth</label>"#,
            BOX_CLASS
        )));
        for forbidden in ["x-data", "x-show", ":checked", "@click", "onclick", "<script", " id="] {
            assert!(!html.contains(forbidden), "{}", forbidden);
        }
    }

    #[test]
    fn picking_shows_the_recipe_and_the_pictures_and_picking_again_takes_them_away() {
        reset();
        pick("sticks");
        let html = render();
        assert_eq!(boxes(&html)[3], ("sticks".to_string(), true, false));
        assert!(html.contains(r#"<strong class="text-xl">Hit with it...</strong><br><span>Add 1 to attack roll. No dragons...</span>"#));
        assert_eq!(shown(&html), ["Sticks"]);
        assert_eq!(pluses(&html), 0);

        // A second item, listed before the first: the recipe is for the two,
        // in the lookup's order, with one plus between them.
        pick("feathers");
        let html = render();
        assert!(html.contains("Atlatl:"));
        assert_eq!(shown(&html), ["Feathers", "Sticks"]);
        assert_eq!(pluses(&html), 1);

        pick("sticks");
        let html = render();
        assert!(html.contains("Was something killed?"));
        assert_eq!(shown(&html), ["Feathers"]);
        assert_eq!(pluses(&html), 0);
        reset();
    }

    #[test]
    fn with_three_picked_the_other_boxes_take_no_more_until_one_is_unpicked() {
        reset();
        for item in ["string", "cloth", "honey"] {
            pick(item);
        }
        let html = render();
        assert_eq!(
            boxes(&html),
            [
                ("cloth".to_string(), true, false),
                ("feathers".to_string(), false, true),
                ("honey".to_string(), true, false),
                ("sticks".to_string(), false, true),
                ("string".to_string(), true, false),
            ]
        );
        assert!(html.contains("Tough Bundles for blade training!"));
        assert_eq!(pluses(&html), 2);

        // A fourth is refused, even if asked for behind the disabled box.
        pick("feathers");
        assert_eq!(picked(), ["cloth", "honey", "string"]);

        pick("honey");
        assert!(boxes(&render()).iter().all(|(_, _, disabled)| !disabled));
        reset();
    }

    #[test]
    fn what_is_not_an_item_is_ignored_and_what_is_shown_is_escaped() {
        reset();
        pick("dragon");
        pick("\"><script>");
        assert!(picked().is_empty());
        // A recipe's name with quotation marks in it.
        pick("cloth");
        assert!(render().contains("A &quot;Disguise&quot;"));
        reset();
    }
}
