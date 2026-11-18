//! Turn/alarm tracking — diel cycle countdown timers.
//!
//! Players set alarms for N diel cycles. Each "tick" (click) decrements
//! all active alarms by 1. Alarms at 0 show "Complete". Expired alarms
//! (negative) are removed on the next tick.
//!
//! ## Two views
//!
//! Outside a room the timer is two engine-owned views (Pattern 13,
//! `assets/js/kipukas-view.js`): the form that makes a timer
//! ([`render_turn_panel`], at `/api/game/turns/form`) and the list of timers
//! ([`render_alarm_list`], at `/api/game/turns?display=alarms`). The form is
//! posted to `/api/game/turns`, so the list, a view of that route, is drawn
//! again when a timer is made. The form has a route of its own so that a tap
//! in the list does not draw the form again and clear what is being typed.
//!
//! ## In a room
//!
//! The timers are shared and live in the room document. The same two
//! functions render them, with controls that go through the multiplayer
//! script (`kipukasMultiplayer`), which also sends each change to the other
//! player. Showing and hiding the list is a preference of this player alone
//! and is an engine post in both cases.

use crate::game::crdt;
use crate::game::player_doc;
use crate::game::room;
use crate::routes::util::escape_html;

/// Toggle alarm panel visibility.
pub fn toggle_alarms_visibility() {
    let current = player_doc::get_show_alarms();
    player_doc::set_show_alarms(!current);
}

/// Get Tailwind CSS classes for a given color set.
fn color_classes(color_set: &str) -> (&str, &str, &str) {
    match color_set {
        "green" => ("bg-emerald-100", "text-emerald-800", "border-emerald-400"),
        "blue" => ("bg-blue-100", "text-blue-800", "border-blue-400"),
        "yellow" => ("bg-yellow-100", "text-yellow-800", "border-yellow-400"),
        "pink" => ("bg-pink-100", "text-pink-800", "border-pink-400"),
        _ => ("bg-red-100", "text-red-800", "border-red-400"), // red default
    }
}

/// Get the dot emoji for a color set (used in compact alarm cards).
fn color_dot(color_set: &str) -> &str {
    match color_set {
        "green" => "\u{1F7E2}",  // 🟢
        "blue" => "\u{1F535}",   // 🔵
        "yellow" => "\u{1F7E1}", // 🟡
        "pink" => "\u{1F7E3}",  // 🟣 (closest to pink)
        _ => "\u{1F534}",       // 🔴 red default
    }
}

const FIELD_CLASS: &str = "w-full border rounded px-2 py-1 text-sm text-kip-drk-sienna border-kip-drk-sienna focus:border-kip-red focus:ring-kip-red";
const LABEL_CLASS: &str = "block text-xs font-bold mb-1";

/// The colors a timer can have, with the class that paints each one's dot in
/// the form. The first is the one chosen to begin with.
const COLORS: [(&str, &str); 5] = [
    ("red", "bg-red-400"),
    ("green", "bg-emerald-400"),
    ("blue", "bg-blue-400"),
    ("yellow", "bg-yellow-400"),
    ("pink", "bg-pink-400"),
];

/// Render the form that makes a timer: a name, a number of diel cycles and a
/// color.
///
/// It is a plain form. Its fields are named as `POST /api/game/turns` reads
/// them (`name`, `turns`, `color_set`), and the chosen color is whichever
/// radio button is checked, so there is no state to keep beside it.
///
/// Outside a room the form is sent to the engine by the view it is in. In a
/// room (peer present or not) it hands its fields to the multiplayer script,
/// which adds the timer to the room document and tells the other player.
/// Either way the values come from this form's own fields: the page can hold
/// two of these forms at once, one for each case.
pub fn render_turn_panel() -> String {
    let multiplayer = room::in_room();
    let mut html = String::with_capacity(2048);

    html.push_str(r#"<div class="p-3 text-kip-drk-sienna">"#);
    if multiplayer {
        html.push_str(
            r#"<form class="grid grid-cols-1 gap-2" onsubmit="kipukasMultiplayer.addTurn(this.elements.turns.value, this.elements.name.value, this.elements.color_set.value); return false">"#,
        );
    } else {
        html.push_str(
            r#"<form class="grid grid-cols-1 gap-2" data-engine-post="/api/game/turns" data-engine-body="action=add">"#,
        );
    }

    // Name (optional)
    html.push_str(&format!(
        r#"<label class="{}">Timer Name<input type="text" name="name" placeholder="(optional)" maxlength="30" autocomplete="off" class="{} mt-1 font-normal"></label>"#,
        LABEL_CLASS, FIELD_CLASS
    ));

    // Diel cycles (a number field brings up the number pad on a phone)
    html.push_str(&format!(
        r#"<label class="{}">Diel Cycles<input type="number" inputmode="numeric" pattern="[0-9]*" name="turns" min="1" max="99" value="1" required class="{} mt-1 font-normal"></label>"#,
        LABEL_CLASS, FIELD_CLASS
    ));

    // Color: one radio button per color, drawn as a dot. The checked one is
    // ringed.
    html.push_str(&format!(
        r#"<fieldset><legend class="{}">Color</legend><div class="flex gap-2">"#,
        LABEL_CLASS
    ));
    for (i, (color, dot)) in COLORS.iter().enumerate() {
        html.push_str(&format!(
            r#"<label class="cursor-pointer"><input type="radio" name="color_set" value="{color}"{checked} class="sr-only peer"><span class="{dot} block w-7 h-7 rounded-full transition-all opacity-60 peer-checked:opacity-100 peer-checked:scale-110 peer-checked:ring-2 peer-checked:ring-kip-drk-sienna peer-checked:ring-offset-1 peer-focus-visible:ring-2 peer-focus-visible:ring-kip-red"></span><span class="sr-only">{color} color</span></label>"#,
            color = color,
            checked = if i == 0 { " checked" } else { "" },
            dot = dot,
        ));
    }
    html.push_str(r#"</div></fieldset>"#);

    html.push_str(
        r#"<button type="submit" aria-label="Submit turn timer" class="bg-kip-red hover:bg-emerald-600 text-amber-50 font-bold py-2 px-4 rounded text-sm">New Timer</button>"#,
    );

    html.push_str(r#"</form>"#);
    html.push_str(r#"</div>"#); // close p-3

    html
}

/// Render the alarm list HTML.
/// This is the floating alarm display in the top-left corner.
///
/// Automatically detects multiplayer mode via `room::in_room()`.
/// When in a room (peer present or not), reads alarms from the yrs CRDT Doc
/// (synced between peers) and renders multiplayer sync buttons. Otherwise
/// reads from PLAYER_DOC, and its buttons are engine posts: the view the
/// list is in sends them and draws the list again.
pub fn render_alarm_list() -> String {
    let multiplayer = room::in_room();
    // When multiplayer, read alarms from the yrs CRDT Doc (synced between peers).
    // When local, read from PLAYER_DOC (persisted to localStorage).
    let (alarms, show_alarms) = if multiplayer {
        let crdt_alarms = crdt::get_alarms();
        let show = player_doc::get_show_alarms();
        (crdt_alarms, show)
    } else {
        (player_doc::get_alarms(), player_doc::get_show_alarms())
    };

    if alarms.is_empty() {
        return String::new();
    }

    let mut html = String::with_capacity(2048);

    // Alarm container
    let collapse_class = if show_alarms { "" } else { " hidden" };
    html.push_str(&format!(
        r#"<div id="alarm-list-inner" class="{}">"#,
        collapse_class.trim()
    ));

    // Advance all button at the top
    if multiplayer {
        html.push_str(
            r#"<button class="py-2 px-4 h-fit w-fit bg-amber-50 rounded-lg text-kip-drk-goldenrod mb-2 hover:bg-amber-100 select-none cursor-pointer font-bold text-sm" onclick="kipukasMultiplayer.tickTurns()">&#x23E9; Advance on Diel Roll</button>"#,
        );
    } else {
        html.push_str(
            r#"<button type="button" class="py-2 px-4 h-fit w-fit bg-amber-50 rounded-lg text-kip-drk-goldenrod mb-2 hover:bg-amber-100 select-none cursor-pointer font-bold text-sm" data-engine-post="/api/game/turns" data-engine-body="action=tick">&#x23E9; Advance on Diel Roll</button>"#,
        );
    }

    // Individual alarm cards
    for (i, alarm) in alarms.iter().enumerate() {
        let (bg, text, border) = color_classes(&alarm.color_set);
        let dot = color_dot(&alarm.color_set);

        html.push_str(&format!(
            r#"<div class="py-2 px-3 h-fit w-fit rounded-lg mb-2 flex items-center gap-2 border {} {} {}">"#,
            bg, text, border
        ));

        // Color dot
        html.push_str(&format!(r#"<span class="text-sm">{}</span>"#, dot));

        // Name + countdown. In a room the name may have been typed by the
        // other player, so it is escaped before it goes into markup.
        let name = escape_html(&alarm.name);
        if alarm.remaining == 0 {
            if name.is_empty() {
                html.push_str(r#"<span class="text-sm font-bold">Complete!</span>"#);
            } else {
                html.push_str(&format!(
                    r#"<span class="text-sm"><strong>{}</strong> — Complete!</span>"#,
                    name
                ));
            }
        } else if name.is_empty() {
            html.push_str(&format!(
                r#"<span class="text-sm">Turns to Alarm: <strong>{}</strong></span>"#,
                alarm.remaining
            ));
        } else {
            html.push_str(&format!(
                r#"<span class="text-sm"><strong>{}</strong> — {}</span>"#,
                name, alarm.remaining
            ));
        }

        // Remove button. Shared timers are removed by id (validated when the
        // room document is read), solo timers by position.
        if multiplayer {
            html.push_str(&format!(
                r#"<button class="ml-auto text-xs opacity-60 hover:opacity-100 cursor-pointer" onclick="kipukasMultiplayer.removeTurn('{}')" aria-label="Remove timer">&#x2715;</button>"#,
                alarm.id
            ));
        } else {
            html.push_str(&format!(
                r#"<button type="button" class="ml-auto text-xs opacity-60 hover:opacity-100 cursor-pointer" data-engine-post="/api/game/turns" data-engine-body="action=remove&index={}" aria-label="Remove timer">&#x2715;</button>"#,
                i
            ));
        }

        html.push_str(r#"</div>"#);
    }

    html.push_str(r#"</div>"#); // close alarm-list-inner

    // Show or hide the list. Whether it is shown is this player's own
    // preference, in a room or not, so it is an engine post in both cases.
    let (rotate_class, toggle_label) = if show_alarms {
        ("", "Hide timers")
    } else {
        (" rotate-180", "Show timers")
    };
    html.push_str(&format!(
        r#"<button type="button" aria-label="{}" aria-expanded="{}" data-engine-post="/api/game/turns" data-engine-body="action=toggle_visibility" class="z-50 mb-2 block w-fit cursor-pointer"><svg xmlns="http://www.w3.org/2000/svg" aria-hidden="true" class="fill-none stroke-2 stroke-kip-drk-goldenrod w-6 h-6{}">"#,
        toggle_label, show_alarms, rotate_class
    ));
    html.push_str(
        r#"<path stroke-linecap="round" stroke-linejoin="round" d="m4.5 15.75 7.5-7.5 7.5 7.5" />"#,
    );
    html.push_str(r#"</svg></button>"#);

    html
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset_state() {
        player_doc::init_player_doc();
        room::reset_room();
    }

    #[test]
    fn add_alarm_works() {
        reset_state();
        player_doc::add_alarm(5, "Scout patrol", "green");
        player_doc::add_alarm(3, "", "red");
        let alarms = player_doc::get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].remaining, 5);
        assert_eq!(alarms[0].name, "Scout patrol");
        assert_eq!(alarms[0].color_set, "green");
        assert_eq!(alarms[1].remaining, 3);
        assert_eq!(alarms[1].name, "");
        assert_eq!(alarms[1].color_set, "red");
        reset_state();
    }

    #[test]
    fn add_alarm_validates_color() {
        reset_state();
        player_doc::add_alarm(1, "", "invalid");
        let alarms = player_doc::get_alarms();
        assert_eq!(alarms[0].color_set, "red"); // defaults to red
        reset_state();
    }

    #[test]
    fn tick_decrements_and_removes_expired() {
        reset_state();
        player_doc::add_alarm(2, "", "red");
        player_doc::add_alarm(1, "", "blue");

        player_doc::tick_alarms();
        let alarms = player_doc::get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].remaining, 1);
        assert_eq!(alarms[1].remaining, 0); // complete

        player_doc::tick_alarms();
        let alarms = player_doc::get_alarms();
        assert_eq!(alarms.len(), 1); // 0→-1 removed
        assert_eq!(alarms[0].remaining, 0); // 1→0 complete

        player_doc::tick_alarms();
        let alarms = player_doc::get_alarms();
        assert!(alarms.is_empty()); // all removed

        reset_state();
    }

    #[test]
    fn remove_alarm_by_index() {
        reset_state();
        player_doc::add_alarm(5, "first", "red");
        player_doc::add_alarm(3, "second", "green");
        player_doc::add_alarm(1, "third", "blue");
        player_doc::remove_alarm(1); // remove the 3-turn alarm
        let alarms = player_doc::get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].remaining, 5);
        assert_eq!(alarms[1].remaining, 1);
        reset_state();
    }

    #[test]
    fn remove_alarm_out_of_bounds_is_noop() {
        reset_state();
        player_doc::add_alarm(5, "", "red");
        player_doc::remove_alarm(99);
        assert_eq!(player_doc::get_alarms().len(), 1);
        reset_state();
    }

    #[test]
    fn toggle_visibility() {
        reset_state();
        assert!(player_doc::get_show_alarms());
        toggle_alarms_visibility();
        assert!(!player_doc::get_show_alarms());
        toggle_alarms_visibility();
        assert!(player_doc::get_show_alarms());
        reset_state();
    }

    #[test]
    fn render_alarm_list_empty_when_no_alarms() {
        reset_state();
        // room disconnected → local mode
        let html = render_alarm_list();
        assert!(html.is_empty());
        reset_state();
    }

    #[test]
    fn render_alarm_list_shows_alarms_with_colors() {
        reset_state();
        player_doc::add_alarm(5, "Dragon siege", "green");
        player_doc::add_alarm(0, "", "red");
        // room disconnected → local mode (reads from PLAYER_DOC)
        let html = render_alarm_list();
        assert!(html.contains("Dragon siege"));
        assert!(html.contains("5")); // shown in "name — 5" format
        assert!(html.contains("Complete!"));
        assert!(html.contains("Advance on Diel Roll"));
        assert!(html.contains("bg-emerald-100")); // green color classes
        assert!(html.contains("bg-red-100")); // red color classes
        reset_state();
    }

    #[test]
    fn render_alarm_list_multiplayer_uses_sync_buttons() {
        reset_state();
        // Being in a room is what makes render_alarm_list use the shared doc
        room::with_room_mut(|r| {
            r.room_code = "ABCD".to_string();
            r.connected = true;
        });
        crdt::init_doc();
        crdt::add_alarm(3, "", "blue");
        let html = render_alarm_list();
        assert!(html.contains("kipukasMultiplayer.tickTurns()"));
        // Shared timers are removed by id, not by position.
        let id = crdt::get_alarms()[0].id.clone();
        assert!(html.contains(&format!("kipukasMultiplayer.removeTurn('{}')", id)));
        // The shared timers are not changed through the solo route.
        assert!(!html.contains("action=tick"));
        assert!(!html.contains("action=remove"));
        assert!(!html.contains("htmx.ajax"));
        crdt::reset_doc();
        reset_state();
    }

    #[test]
    fn timer_names_are_escaped() {
        reset_state();
        player_doc::add_alarm(3, "<img src=x onerror=alert(1)>", "red");
        let html = render_alarm_list();
        assert!(!html.contains("<img src=x"));
        assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
        reset_state();
    }

    #[test]
    fn render_alarm_list_stays_shared_while_peer_is_away() {
        reset_state();
        // In a room, peer not connected (mid page-load, asleep, brief drop).
        room::with_room_mut(|r| r.room_code = "ABCD".to_string());
        player_doc::add_alarm(9, "solo copy", "red");
        crdt::init_doc();
        crdt::add_alarm(3, "shared", "blue");

        let html = render_alarm_list();
        assert!(html.contains("shared"));
        assert!(!html.contains("solo copy"));
        assert!(html.contains("kipukasMultiplayer.tickTurns()"));
        assert!(!html.contains("action=tick"));

        let panel = render_turn_panel();
        assert!(panel.contains("kipukasMultiplayer.addTurn"));
        crdt::reset_doc();
        reset_state();
    }

    #[test]
    fn render_alarm_list_is_local_outside_a_room() {
        reset_state();
        player_doc::add_alarm(9, "solo", "red");
        let html = render_alarm_list();
        assert!(html.contains("solo"));
        assert!(html.contains(r#"data-engine-post="/api/game/turns" data-engine-body="action=tick""#));
        assert!(!html.contains("kipukasMultiplayer"));
        reset_state();
    }

    #[test]
    fn a_solo_lists_controls_are_engine_posts_and_nothing_else() {
        reset_state();
        player_doc::add_alarm(5, "first", "red");
        player_doc::add_alarm(3, "second", "green");
        let html = render_alarm_list();
        // Each timer is removed by its place in the list.
        assert!(html.contains(r#"data-engine-body="action=remove&index=0""#));
        assert!(html.contains(r#"data-engine-body="action=remove&index=1""#));
        assert_eq!(html.matches("data-engine-post=\"/api/game/turns\"").count(), 4);
        // No script in the markup, and nothing for Alpine or HTMX to find.
        for forbidden in ["onclick", "htmx", "hx-", "x-data", "@click"] {
            assert!(!html.contains(forbidden), "{} in the list", forbidden);
        }
        reset_state();
    }

    #[test]
    fn the_list_is_hidden_and_shown_by_a_button_that_says_which() {
        reset_state();
        player_doc::add_alarm(5, "", "red");
        let html = render_alarm_list();
        assert!(html.contains(r#"aria-label="Hide timers" aria-expanded="true" data-engine-post="/api/game/turns" data-engine-body="action=toggle_visibility""#));
        toggle_alarms_visibility();
        let html = render_alarm_list();
        assert!(html.contains(r#"aria-label="Show timers" aria-expanded="false""#));
        assert!(html.contains(r#"<div id="alarm-list-inner" class="hidden">"#));
        reset_state();
    }

    #[test]
    fn hiding_the_list_is_an_engine_post_in_a_room_too() {
        reset_state();
        room::with_room_mut(|r| r.room_code = "ABCD".to_string());
        crdt::init_doc();
        crdt::add_alarm(3, "shared", "blue");
        let html = render_alarm_list();
        assert!(html.contains(r#"data-engine-body="action=toggle_visibility""#));
        crdt::reset_doc();
        reset_state();
    }

    #[test]
    fn render_turn_panel_has_controls() {
        reset_state();
        // room disconnected → local mode
        let html = render_turn_panel();
        // The fields are named as POST /api/game/turns reads them.
        assert!(html.contains(r#"name="turns""#));
        assert!(html.contains(r#"name="name""#));
        assert_eq!(html.matches(r#"name="color_set""#).count(), 5);
        assert_eq!(html.matches(" checked").count(), 1, "one color is chosen to begin with");
        assert!(html.contains(r#"value="red" checked"#));
        assert!(html.contains(r#"<form class="grid grid-cols-1 gap-2" data-engine-post="/api/game/turns" data-engine-body="action=add">"#));
        assert!(html.contains(r#"<button type="submit""#));
        // Nothing for Alpine, HTMX or a script to do, and no ids to clash
        // with the room's copy of the form.
        for forbidden in ["onclick", "onsubmit", "htmx", "x-data", "x-model", "@click", " id="] {
            assert!(!html.contains(forbidden), "{} in the form", forbidden);
        }
        assert!(html.contains("New Timer"));
        assert!(html.contains("Diel Cycles"));
        assert!(html.contains("inputmode=\"numeric\""));
        assert!(html.contains("red color"));
        assert!(html.contains("green color"));
        assert!(html.contains("blue color"));
        assert!(html.contains("yellow color"));
        assert!(html.contains("pink color"));
        reset_state();
    }

    #[test]
    fn render_turn_panel_multiplayer_uses_sync() {
        reset_state();
        // Being in a room is what makes render_turn_panel use the sync path
        room::with_room_mut(|r| {
            r.room_code = "ABCD".to_string();
            r.connected = true;
        });
        let html = render_turn_panel();
        // The form hands its own fields to the multiplayer script.
        assert!(html.contains(
            "onsubmit=\"kipukasMultiplayer.addTurn(this.elements.turns.value, this.elements.name.value, this.elements.color_set.value); return false\""
        ));
        assert!(!html.contains("data-engine-post")); // not sent to the solo route
        assert!(!html.contains("getElementById")); // not read from elsewhere in the page
        assert!(!html.contains("htmx.ajax"));
        reset_state();
    }

    #[test]
    fn add_alarm_validates_all_colors() {
        reset_state();
        // Valid colors stored as-is
        player_doc::add_alarm(1, "", "red");
        player_doc::add_alarm(1, "", "green");
        player_doc::add_alarm(1, "", "blue");
        player_doc::add_alarm(1, "", "yellow");
        player_doc::add_alarm(1, "", "pink");
        // Invalid colors default to red
        player_doc::add_alarm(1, "", "invalid");
        player_doc::add_alarm(1, "", "");
        let alarms = player_doc::get_alarms();
        assert_eq!(alarms[0].color_set, "red");
        assert_eq!(alarms[1].color_set, "green");
        assert_eq!(alarms[2].color_set, "blue");
        assert_eq!(alarms[3].color_set, "yellow");
        assert_eq!(alarms[4].color_set, "pink");
        assert_eq!(alarms[5].color_set, "red"); // "invalid" → "red"
        assert_eq!(alarms[6].color_set, "red"); // "" → "red"
        reset_state();
    }
}
