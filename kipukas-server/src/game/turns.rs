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
//! The timers are shared and live in the room document. Nothing about the
//! views differs: the same form and the same list, with the same posts.
//! What differs is where a change goes ([`add`], [`advance`], [`remove`]):
//! in a room it is made to the room document and queued for the other
//! player (`crdt::queue_for_peer`), and the page sends it. A change the other
//! player makes arrives through the room's own routes, after which the page
//! draws these views again.
//!
//! Showing and hiding the list is a preference of this player alone.
//!
//! ## A timer that has run out
//!
//! It says "Complete!" for [`COMPLETE_SHOWN_MS`], fades, and is gone, without
//! anyone tapping. The engine decides this by the clock: it remembers when
//! this device first saw the timer run out ([`note_completions`]) and draws
//! the timer according to how long ago that was. The page does not count
//! anything. Each such timer carries a post for the view to send by itself
//! when the picture is next due to change (`data-engine-after`), and the view
//! is drawn again then. So a timer that ran out before a page change goes on
//! from where it was, and one whose time passed while the page was asleep is
//! simply not drawn.
//!
//! A player's own run-out timers are deleted once gone ([`expire`]). A
//! room's are only left undrawn on this device: they stay in the room
//! document until the next advance removes them, as before, so nothing is
//! written to shared state by a clock, and the two players' clocks need not
//! agree.

use crate::clock;
use crate::game::crdt;
use crate::game::state::Alarm;
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
/// The form is the same in a room and out of one: the view it is in sends
/// it to the engine, which knows which timers it is adding to ([`add`]). The
/// page can hold two of these forms at once (the popover used alone and the
/// room's dialog), so it has no ids and its values are its own fields.
pub fn render_turn_panel() -> String {
    let mut html = String::with_capacity(2048);

    html.push_str(r#"<div class="p-3 text-kip-drk-sienna">"#);
    html.push_str(
        r#"<form class="grid grid-cols-1 gap-2" data-engine-post="/api/game/turns" data-engine-body="action=add">"#,
    );

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

/// How long a timer that has run out stays up saying so.
pub const COMPLETE_SHOWN_MS: f64 = 15_000.0;

/// How long it then takes to fade, before it is no longer drawn. The fade
/// itself is the `duration-700` on the timer; this leaves it time to finish.
pub const FADE_MS: f64 = 1_000.0;

/// Where a timer is in its last seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Ending {
    /// Still counting, or run out with no way to tell when (a timer saved
    /// before timers had ids): drawn as it is until the next advance.
    No,
    /// Run out and saying so, for this much longer.
    Shown { left_ms: f64 },
    /// Fading, for this much longer.
    Fading { left_ms: f64 },
    /// Not drawn any more.
    Gone,
}

/// The timers this page shows: the room's if in one, otherwise the player's.
fn current_alarms() -> (bool, Vec<Alarm>) {
    if room::in_room() {
        (true, crdt::get_alarms())
    } else {
        (false, player_doc::get_alarms())
    }
}

/// Remember, for each timer that has run out, when this device first saw it
/// so, and forget timers that are no longer run out or no longer there.
pub fn note_completions(now: f64) {
    let (_, alarms) = current_alarms();
    let known = player_doc::get_alarm_done();
    let done: Vec<(String, f64)> = alarms
        .iter()
        .filter(|alarm| alarm.remaining == 0 && !alarm.id.is_empty())
        .map(|alarm| {
            let since = known
                .iter()
                .find(|(id, _)| *id == alarm.id)
                .map(|(_, at)| *at)
                .unwrap_or(now);
            (alarm.id.clone(), since)
        })
        .collect();
    player_doc::set_alarm_done(&done);
}

fn ending(alarm: &Alarm, done: &[(String, f64)], now: f64) -> Ending {
    if alarm.remaining != 0 {
        return Ending::No;
    }
    let Some((_, since)) = done.iter().find(|(id, _)| *id == alarm.id) else {
        return Ending::No;
    };
    // A clock set back must not keep a timer up for ever, nor bring one back.
    let age = (now - since).max(0.0);
    if age < COMPLETE_SHOWN_MS {
        Ending::Shown { left_ms: COMPLETE_SHOWN_MS - age }
    } else if age < COMPLETE_SHOWN_MS + FADE_MS {
        Ending::Fading { left_ms: COMPLETE_SHOWN_MS + FADE_MS - age }
    } else {
        Ending::Gone
    }
}

/// Add a timer: to the room's timers when in a room, telling the other
/// player, otherwise to the player's own.
pub fn add(turns: i32, name: &str, color_set: &str) {
    let turns = turns.clamp(1, 99);
    if room::in_room() {
        crdt::queue_for_peer(crdt::add_alarm(turns, name, color_set));
        // Keep the player's own copy in step, so that the timers are still
        // there if the room ends without a clean goodbye.
        crdt::export_to_local();
    } else {
        player_doc::add_alarm(turns, name, color_set);
    }
}

/// Advance every timer by one diel cycle; one that had already run out goes.
pub fn advance() {
    if room::in_room() {
        crdt::queue_for_peer(crdt::tick_alarms());
        crdt::export_to_local();
    } else {
        player_doc::tick_alarms();
    }
}

/// Remove one timer. A room's timer is named by its id, which both players
/// agree on; a player's own by its place in the list.
pub fn remove(id: Option<&str>, place: Option<usize>) {
    if room::in_room() {
        if let Some(id) = id {
            crdt::queue_for_peer(crdt::remove_alarm(id));
            crdt::export_to_local();
        }
    } else if let Some(place) = place {
        player_doc::remove_alarm(place);
    }
}

/// Delete the player's own timers that have run out and faded. In a room
/// nothing is deleted: see the top of this file.
pub fn expire(now: f64) {
    note_completions(now);
    let (multiplayer, alarms) = current_alarms();
    if multiplayer {
        return;
    }
    let done = player_doc::get_alarm_done();
    let gone: Vec<String> = alarms
        .iter()
        .filter(|alarm| ending(alarm, &done, now) == Ending::Gone)
        .map(|alarm| alarm.id.clone())
        .collect();
    if !gone.is_empty() {
        player_doc::remove_alarms_with_ids(&gone);
        note_completions(now);
    }
}

/// Render the alarm list as it is now. See [`render_alarm_list_at`].
pub fn render_alarm_list() -> String {
    render_alarm_list_at(clock::now_ms())
}

/// Render the alarm list HTML as it is at `now`.
/// This is the floating alarm display in the top-left corner.
///
/// Automatically detects multiplayer mode via `room::in_room()`.
/// When in a room (peer present or not), reads alarms from the yrs CRDT Doc
/// (synced between peers). Otherwise reads from PLAYER_DOC. Its buttons are
/// engine posts either way: the view the list is in sends them and draws the
/// list again.
///
/// Drawing the list is also when a timer is first seen to have run out, so
/// this notes it ([`note_completions`]).
pub fn render_alarm_list_at(now: f64) -> String {
    note_completions(now);
    let (multiplayer, alarms) = current_alarms();
    let show_alarms = player_doc::get_show_alarms();
    let done = player_doc::get_alarm_done();

    // Each timer with its place in the list (a player's own timers are
    // removed by place) and where it is in its ending.
    let drawn: Vec<(usize, &Alarm, Ending)> = alarms
        .iter()
        .enumerate()
        .map(|(i, alarm)| (i, alarm, ending(alarm, &done, now)))
        .filter(|(_, _, ending)| *ending != Ending::Gone)
        .collect();

    if drawn.is_empty() {
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
    html.push_str(
        r#"<button type="button" class="py-2 px-4 h-fit w-fit bg-amber-50 rounded-lg text-kip-drk-goldenrod mb-2 hover:bg-amber-100 select-none cursor-pointer font-bold text-sm" data-engine-post="/api/game/turns" data-engine-body="action=tick">&#x23E9; Advance on Diel Roll</button>"#,
    );

    // Individual alarm cards
    for (i, alarm, ending) in drawn {
        let (bg, text, border) = color_classes(&alarm.color_set);
        let dot = color_dot(&alarm.color_set);

        // Every timer can fade; one that is fading has `opacity-0` and the
        // transition takes it there.
        let fading = if matches!(ending, Ending::Fading { .. }) { " opacity-0" } else { "" };
        html.push_str(&format!(
            r#"<div class="py-2 px-3 h-fit w-fit rounded-lg mb-2 flex items-center gap-2 border transition-opacity duration-700 {} {} {}{}">"#,
            bg, text, border, fading
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
        let which = if multiplayer {
            format!("id={}", alarm.id)
        } else {
            format!("index={}", i)
        };
        html.push_str(&format!(
            r#"<button type="button" class="ml-auto text-xs opacity-60 hover:opacity-100 cursor-pointer" data-engine-post="/api/game/turns" data-engine-body="action=remove&{}" aria-label="Remove timer">&#x2715;</button>"#,
            which
        ));

        // A timer in its ending: when the picture is next due to change (it
        // starts to fade, or is gone), the view sends this by itself and
        // draws the list again.
        if let Ending::Shown { left_ms } | Ending::Fading { left_ms } = ending {
            html.push_str(&format!(
                r#"<span hidden data-engine-post="/api/game/turns" data-engine-body="action=expire" data-engine-after="{}"></span>"#,
                left_ms.ceil() as u64
            ));
        }

        html.push_str(r#"</div>"#);
    }

    html.push_str(r#"</div>"#); // close alarm-list-inner

    // Show or hide the list: a pill under the timers, in the middle of the
    // block. Whether the list is shown is this player's own preference, in a
    // room or not, so it is an engine post in both cases.
    let (rotate_class, toggle_label) = if show_alarms {
        ("", "Hide timers")
    } else {
        (" rotate-180", "Show timers")
    };
    html.push_str(&format!(
        r#"<button type="button" aria-label="{}" aria-expanded="{}" data-engine-post="/api/game/turns" data-engine-body="action=toggle_visibility" class="z-50 mb-2 justify-self-center flex items-center justify-center w-12 h-7 rounded-full bg-amber-50 hover:bg-amber-100 shadow cursor-pointer"><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" aria-hidden="true" class="fill-none stroke-2 stroke-kip-drk-goldenrod w-5 h-5{}">"#,
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
    fn a_rooms_list_has_the_same_controls_and_removes_by_id() {
        reset_state();
        // Being in a room is what makes render_alarm_list use the shared doc
        room::with_room_mut(|r| {
            r.room_code = "ABCD".to_string();
            r.connected = true;
        });
        crdt::init_doc();
        crdt::add_alarm(3, "", "blue");
        let html = render_alarm_list();
        assert!(html.contains(r#"data-engine-post="/api/game/turns" data-engine-body="action=tick""#));
        // Shared timers are removed by id, not by position.
        let id = crdt::get_alarms()[0].id.clone();
        assert!(html.contains(&format!(r#"data-engine-body="action=remove&id={}""#, id)));
        assert!(!html.contains("action=remove&index="));
        // Nothing goes through a script any more.
        for forbidden in ["kipukasMultiplayer", "onclick", "htmx"] {
            assert!(!html.contains(forbidden), "{} in the room's list", forbidden);
        }
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
        assert!(html.contains("action=remove&id="));
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
    fn the_show_and_hide_button_is_a_pill_in_the_middle_of_the_block() {
        reset_state();
        player_doc::add_alarm(5, "", "red");
        let html = render_alarm_list();
        let button = &html[html.find(r#"aria-label="Hide timers""#).unwrap()..];
        let classes = &button[button.find("class=\"").unwrap()..button.find("><svg").unwrap()];
        for class in ["justify-self-center", "rounded-full", "bg-amber-50", "shadow"] {
            assert!(classes.contains(class), "{} on the button: {}", class, classes);
        }
        // It comes after the timers, outside the part that hides.
        assert!(html.contains("</div><button type=\"button\" aria-label=\"Hide timers\""));
        reset_state();
    }

    // ── A timer that has run out ───────────────────────────────────

    /// Some moment; the tests move on from it.
    const T0: f64 = 1_800_000_000_000.0;
    const EXPIRE: &str = r#"data-engine-post="/api/game/turns" data-engine-body="action=expire" data-engine-after="#;

    /// One of the player's own timers, run out.
    fn run_out(name: &str) -> String {
        player_doc::add_alarm(1, name, "red");
        player_doc::tick_alarms();
        player_doc::get_alarms().last().unwrap().id.clone()
    }

    #[test]
    fn a_timer_that_has_run_out_says_so_and_asks_to_be_looked_at_again() {
        reset_state();
        run_out("Siege");
        let html = render_alarm_list_at(T0);
        assert!(html.contains("<strong>Siege</strong> — Complete!"));
        assert!(html.contains(&format!("{}\"15000\"", EXPIRE)), "{}", html);
        assert!(!html.contains("opacity-0"));
        // The request is not something to tap.
        assert!(html.contains("<span hidden data-engine-post="));
        reset_state();
    }

    #[test]
    fn the_wait_is_counted_from_when_it_was_first_seen_not_from_each_drawing() {
        reset_state();
        let id = run_out("Siege");
        render_alarm_list_at(T0);
        // Drawn again later (a tap, or another page): the same 15 seconds.
        let html = render_alarm_list_at(T0 + 4_000.0);
        assert!(html.contains(&format!("{}\"11000\"", EXPIRE)), "{}", html);
        assert_eq!(player_doc::get_alarm_done(), vec![(id, T0)]);
        reset_state();
    }

    #[test]
    fn after_15_seconds_it_fades_and_a_second_later_it_is_not_drawn() {
        reset_state();
        run_out("Siege");
        render_alarm_list_at(T0);

        let last_moment = render_alarm_list_at(T0 + 14_999.0);
        assert!(!last_moment.contains("opacity-0"));
        assert!(last_moment.contains(&format!("{}\"1\"", EXPIRE)));

        let fading = render_alarm_list_at(T0 + 15_000.0);
        assert!(fading.contains("transition-opacity duration-700"));
        assert!(fading.contains(" opacity-0\">"), "{}", fading);
        assert!(fading.contains(&format!("{}\"1000\"", EXPIRE)));
        assert!(fading.contains("Complete!"));

        // The only timer: with it gone there is no list at all.
        assert_eq!(render_alarm_list_at(T0 + 16_000.0), "");
        reset_state();
    }

    #[test]
    fn a_timer_fades_by_changing_one_class_on_the_same_element() {
        reset_state();
        run_out("Siege");
        let shown = render_alarm_list_at(T0);
        let fading = render_alarm_list_at(T0 + 15_500.0);
        // The same markup but for the class and the wait, so the page patches
        // the element in place and the transition runs.
        let same = |html: &str| {
            let at = html.find("data-engine-after=\"").unwrap();
            let end = at + html[at..].find("\">").unwrap();
            format!("{}{}", &html[..at], &html[end..]).replace(" opacity-0", "")
        };
        assert_eq!(same(&shown), same(&fading));
        reset_state();
    }

    #[test]
    fn timers_still_counting_are_left_alone_and_keep_the_list_up() {
        reset_state();
        player_doc::add_alarm(6, "Patrol", "green");
        run_out("Siege"); // the advance that runs Siege out takes Patrol to 5
        let html = render_alarm_list_at(T0);
        assert_eq!(html.matches("data-engine-after").count(), 1);
        let later = render_alarm_list_at(T0 + 60_000.0);
        assert!(later.contains("<strong>Patrol</strong> — 5"));
        assert!(!later.contains("Complete!"));
        assert!(!later.contains("data-engine-after"));
        assert!(later.contains("Advance on Diel Roll"));
        // Patrol is still the first of the player's timers, and is removed
        // as such.
        assert!(later.contains(r#"data-engine-body="action=remove&index=0""#));
        reset_state();
    }

    #[test]
    fn each_run_out_timer_keeps_its_own_time() {
        reset_state();
        run_out("First");
        render_alarm_list_at(T0);
        run_out("Second"); // First is ticked away; Second runs out now
        player_doc::add_alarm_with_id("kept", 0, "Third", "blue");
        let html = render_alarm_list_at(T0 + 6_000.0);
        assert_eq!(html.matches("Complete!").count(), 2);
        assert_eq!(html.matches(&format!("{}\"15000\"", EXPIRE)).count(), 2);
        let html = render_alarm_list_at(T0 + 12_000.0);
        assert_eq!(html.matches(&format!("{}\"9000\"", EXPIRE)).count(), 2);
        reset_state();
    }

    #[test]
    fn the_players_own_timers_are_deleted_once_gone_and_no_sooner() {
        reset_state();
        player_doc::add_alarm(5, "Patrol", "green");
        run_out("Siege");
        render_alarm_list_at(T0);

        expire(T0 + 15_999.0);
        assert_eq!(player_doc::get_alarms().len(), 2, "still fading");

        expire(T0 + 16_000.0);
        let left = player_doc::get_alarms();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].name, "Patrol");
        // And nothing is remembered about a timer that is no more.
        assert!(player_doc::get_alarm_done().is_empty());
        reset_state();
    }

    #[test]
    fn a_timer_advanced_or_removed_is_forgotten() {
        reset_state();
        run_out("Siege");
        render_alarm_list_at(T0);
        assert_eq!(player_doc::get_alarm_done().len(), 1);
        player_doc::tick_alarms(); // a run-out timer goes on the next advance
        assert_eq!(render_alarm_list_at(T0 + 1_000.0), "");
        assert!(player_doc::get_alarm_done().is_empty());
        reset_state();
    }

    #[test]
    fn a_clock_set_back_neither_keeps_a_timer_up_nor_brings_one_back() {
        reset_state();
        run_out("Siege");
        render_alarm_list_at(T0);
        // An hour earlier by the clock: counted as no time passed.
        let html = render_alarm_list_at(T0 - 3_600_000.0);
        assert!(html.contains(&format!("{}\"15000\"", EXPIRE)));
        // And once gone it stays gone.
        expire(T0 + 20_000.0);
        assert_eq!(render_alarm_list_at(T0 - 3_600_000.0), "");
        reset_state();
    }

    #[test]
    fn a_run_out_timer_without_an_id_stays_until_the_next_advance() {
        reset_state();
        // A timer saved before timers had ids.
        player_doc::add_alarm_with_id("", 0, "Old", "red");
        let html = render_alarm_list_at(T0 + 3_600_000.0);
        assert!(html.contains("<strong>Old</strong> — Complete!"));
        assert!(!html.contains("data-engine-after"));
        expire(T0 + 7_200_000.0);
        assert_eq!(player_doc::get_alarms().len(), 1);
        reset_state();
    }

    #[test]
    fn a_rooms_run_out_timer_fades_here_and_stays_in_the_room_document() {
        reset_state();
        room::with_room_mut(|r| r.room_code = "ABCD".to_string());
        crdt::init_doc();
        crdt::add_alarm(1, "Shared", "blue");
        crdt::tick_alarms();

        let html = render_alarm_list_at(T0);
        assert!(html.contains("<strong>Shared</strong> — Complete!"));
        assert!(html.contains(&format!("{}\"15000\"", EXPIRE)));
        // It is the room's timer: removed by its id.
        assert!(html.contains("action=remove&id="));

        assert!(render_alarm_list_at(T0 + 15_200.0).contains(" opacity-0\">"));
        assert_eq!(render_alarm_list_at(T0 + 16_000.0), "");

        // Not drawn here any more, but not deleted: nothing is written to
        // shared state by a clock. The next advance removes it.
        expire(T0 + 60_000.0);
        assert_eq!(crdt::get_alarms().len(), 1);
        crdt::tick_alarms();
        assert!(crdt::get_alarms().is_empty());
        assert_eq!(render_alarm_list_at(T0 + 61_000.0), "");
        assert!(player_doc::get_alarm_done().is_empty());

        crdt::reset_doc();
        reset_state();
    }

    #[test]
    fn the_route_expires_by_the_real_clock() {
        reset_state();
        let id = run_out("Siege");
        // Seen to run out a minute ago.
        player_doc::set_alarm_done(&[(id, clock::now_ms() - 60_000.0)]);
        let html = crate::routes::game::handle_turns_post("action=expire");
        assert_eq!(html, "");
        assert!(player_doc::get_alarms().is_empty());
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
    fn the_form_is_the_same_in_a_room_as_out_of_one() {
        reset_state();
        let alone = render_turn_panel();
        room::with_room_mut(|r| {
            r.room_code = "ABCD".to_string();
            r.connected = true;
        });
        assert_eq!(render_turn_panel(), alone);
        reset_state();
    }

    // ── Where a change goes ────────────────────────────────────────

    fn in_a_room() {
        room::with_room_mut(|r| r.room_code = "ABCD".to_string());
        crdt::init_doc();
        crdt::take_outbox();
    }

    fn leave_the_room() {
        crdt::reset_doc();
        crdt::take_outbox();
        reset_state();
    }

    #[test]
    fn alone_a_change_is_to_the_players_own_timers_and_nothing_is_queued() {
        reset_state();
        crdt::take_outbox();
        add(5, "Patrol", "green");
        advance();
        assert_eq!(player_doc::get_alarms().len(), 1);
        assert_eq!(player_doc::get_alarms()[0].remaining, 4);
        remove(None, Some(0));
        assert!(player_doc::get_alarms().is_empty());
        assert!(crdt::take_outbox().is_empty());
        reset_state();
    }

    #[test]
    fn in_a_room_a_new_timer_goes_to_the_room_and_is_queued_for_the_other_player() {
        reset_state();
        in_a_room();
        add(5, "Siege", "blue");
        let shared = crdt::get_alarms();
        assert_eq!(shared.len(), 1);
        assert_eq!((shared[0].remaining, shared[0].name.as_str(), shared[0].color_set.as_str()), (5, "Siege", "blue"));
        // One update is waiting, and waits only once.
        let queued = crdt::take_outbox();
        assert_eq!(queued.len(), 1);
        assert!(!queued[0].is_empty());
        assert!(crdt::take_outbox().is_empty());
        // The player's own copy is kept in step, with the same id.
        let own = player_doc::get_alarms();
        assert_eq!(own.len(), 1);
        assert_eq!(own[0].id, shared[0].id);
        leave_the_room();
    }

    #[test]
    fn in_a_room_advancing_and_removing_change_the_room_and_are_queued() {
        reset_state();
        in_a_room();
        add(3, "Siege", "blue");
        add(7, "Patrol", "green");
        crdt::take_outbox();

        advance();
        let shared = crdt::get_alarms();
        assert_eq!(shared.iter().map(|a| a.remaining).collect::<Vec<_>>(), [2, 6]);
        assert_eq!(crdt::take_outbox().len(), 1);

        // By id; a place in the list means nothing in a room.
        remove(None, Some(0));
        assert_eq!(crdt::get_alarms().len(), 2);
        assert!(crdt::take_outbox().is_empty());
        remove(Some(&shared[0].id), None);
        let left = crdt::get_alarms();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].name, "Patrol");
        assert_eq!(crdt::take_outbox().len(), 1);
        leave_the_room();
    }

    #[test]
    fn the_route_acts_on_the_room_when_in_one() {
        reset_state();
        in_a_room();
        let html = crate::routes::game::handle_turns_post("action=add&turns=4&name=Watch&color_set=pink");
        assert!(html.contains("<strong>Watch</strong> — 4"));
        let id = crdt::get_alarms()[0].id.clone();
        crate::routes::game::handle_turns_post("action=tick");
        assert_eq!(crdt::get_alarms()[0].remaining, 3);
        crate::routes::game::handle_turns_post(&format!("action=remove&id={}", id));
        assert!(crdt::get_alarms().is_empty());
        // Three changes, three updates, handed over as a JSON list.
        let outbox = crate::routes::room::handle_outbox_post("");
        let updates: Vec<String> = serde_json::from_str(&outbox).unwrap();
        assert_eq!(updates.len(), 3);
        assert_eq!(crate::routes::room::handle_outbox_post(""), "[]");
        leave_the_room();
    }

    #[test]
    fn hiding_the_list_or_a_timer_fading_sends_nothing() {
        reset_state();
        in_a_room();
        crdt::add_alarm(1, "Shared", "blue");
        crdt::tick_alarms();
        crate::routes::game::handle_turns_post("action=toggle_visibility");
        crate::routes::game::handle_turns_post("action=expire");
        assert_eq!(crate::routes::room::handle_outbox_post(""), "[]");
        leave_the_room();
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
