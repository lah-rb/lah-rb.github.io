//! Turn/alarm tracking — diel cycle countdown timers.
//!
//! Players set alarms for N diel cycles. Each "tick" (click) decrements
//! all active alarms by 1. Alarms at 0 show "Complete". Expired alarms
//! (negative) are removed on the next tick.
//!
//! Phase 5: Extended with name, color_set fields and cleaner UI.
//! Multiplayer sync broadcasts timer mutations to the connected peer.

use crate::game::crdt;
use crate::game::player_doc;
use crate::game::room;

/// Add a new alarm with the given number of diel cycles, optional name, and color set.
pub fn add_alarm(turns: i32, name: &str, color_set: &str) {
    player_doc::add_alarm(turns, name, color_set);
}

/// Tick all alarms: decrement by 1, remove any that went below 0.
pub fn tick_alarms() {
    player_doc::tick_alarms();
}

/// Remove a specific alarm by index.
pub fn remove_alarm(index: usize) {
    player_doc::remove_alarm(index);
}

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

/// Render the turn tracker panel HTML.
/// This is the content inside the turn tracker popover — includes
/// the number input, name field, color picker, and "New Timer" button.
///
/// Automatically detects multiplayer mode via `room::is_peer_connected()`.
/// When multiplayer, the submit button routes through the multiplayer
/// sync path; otherwise it uses the local HTMX path.
pub fn render_turn_panel() -> String {
    let multiplayer = room::is_peer_connected();
    let mut html = String::with_capacity(2048);

    html.push_str(r#"<div class="p-3 text-kip-drk-sienna">"#);

    // Timer creation form
    html.push_str(r#"<div class="grid grid-cols-1 gap-2" x-data="{ selectedColor: 'red' }">"#);

    // Name field (optional)
    html.push_str(
        r#"<div><label class="block text-xs font-bold mb-1" for="timerName">Timer Name</label>"#,
    );
    html.push_str(
        r#"<input type="text" id="timerName" placeholder="(optional)" maxlength="30" class="w-full border rounded px-2 py-1 text-sm text-kip-drk-sienna border-kip-drk-sienna focus:border-kip-red focus:ring-kip-red"></div>"#,
    );

    // Diel cycles input (number, brings up numpad on mobile)
    html.push_str(
        r#"<div><label class="block text-xs font-bold mb-1" for="turnsSelector">Diel Cycles</label>"#,
    );
    html.push_str(
        r#"<input type="number" inputmode="numeric" pattern="[0-9]*" id="turnsSelector" min="1" max="99" value="1" class="w-full border rounded px-2 py-1 text-sm text-kip-drk-sienna border-kip-drk-sienna focus:border-kip-red focus:ring-kip-red"></div>"#,
    );

    // Hidden input to reliably track selected color (Alpine x-model)
    html.push_str(
        r#"<input type="hidden" id="timerColorSet" x-model="selectedColor">"#,
    );

    // Color picker
    html.push_str(r#"<div><label class="block text-xs font-bold mb-1">Color</label>"#);
    html.push_str(r#"<div class="flex gap-2">"#);
    for (color, bg_class) in &[
        ("red", "bg-red-400"),
        ("green", "bg-emerald-400"),
        ("blue", "bg-blue-400"),
        ("yellow", "bg-yellow-400"),
        ("pink", "bg-pink-400"),
    ] {
        html.push_str(&format!(
            r#"<button type="button" @click="selectedColor = '{}'" :class="selectedColor === '{}' ? 'ring-2 ring-kip-drk-sienna ring-offset-1 scale-110' : 'opacity-60'" class="{} w-7 h-7 rounded-full transition-all cursor-pointer" aria-label="{} color"></button>"#,
            color, color, bg_class, color
        ));
    }
    html.push_str(r#"</div></div>"#);

    // Submit button — reads color from hidden input (reliable across all Alpine contexts)
    if multiplayer {
        html.push_str(
            r#"<button aria-label="Submit turn timer" class="bg-kip-red hover:bg-emerald-600 text-amber-50 font-bold py-2 px-4 rounded text-sm" onclick="kipukasMultiplayer.addTurn(document.getElementById('turnsSelector').value, document.getElementById('timerName').value, document.getElementById('timerColorSet').value || 'red')">New Timer</button>"#,
        );
    } else {
        html.push_str(
            r#"<button aria-label="Submit turn timer" class="bg-kip-red hover:bg-emerald-600 text-amber-50 font-bold py-2 px-4 rounded text-sm" onclick="htmx.ajax('POST', '/api/game/turns', {values: {action: 'add', turns: document.getElementById('turnsSelector').value, name: document.getElementById('timerName').value, color_set: document.getElementById('timerColorSet').value || 'red'}, target: '#turn-alarms', swap: 'innerHTML'})">New Timer</button>"#,
        );
    }

    html.push_str(r#"</div>"#); // close x-data grid
    html.push_str(r#"</div>"#); // close p-3

    html
}

/// Render the alarm list HTML.
/// This is the floating alarm display in the top-left corner.
///
/// Automatically detects multiplayer mode via `room::is_peer_connected()`.
/// When multiplayer, reads alarms from the yrs CRDT Doc (synced between peers)
/// and renders multiplayer sync buttons. Otherwise reads from PLAYER_DOC.
pub fn render_alarm_list() -> String {
    let multiplayer = room::is_peer_connected();
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
            r#"<button class="py-2 px-4 h-fit w-fit bg-amber-50 rounded-lg text-kip-drk-goldenrod mb-2 hover:bg-amber-100 select-none cursor-pointer font-bold text-sm" onclick="htmx.ajax('POST', '/api/game/turns', {values: {action: 'tick'}, target: '#turn-alarms', swap: 'innerHTML'})">&#x23E9; Advance on Diel Roll</button>"#,
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

        // Name + countdown
        if alarm.remaining == 0 {
            if alarm.name.is_empty() {
                html.push_str(r#"<span class="text-sm font-bold">Complete!</span>"#);
            } else {
                html.push_str(&format!(
                    r#"<span class="text-sm"><strong>{}</strong> — Complete!</span>"#,
                    alarm.name
                ));
            }
        } else if alarm.name.is_empty() {
            html.push_str(&format!(
                r#"<span class="text-sm">Turns to Alarm: <strong>{}</strong></span>"#,
                alarm.remaining
            ));
        } else {
            html.push_str(&format!(
                r#"<span class="text-sm"><strong>{}</strong> — {}</span>"#,
                alarm.name, alarm.remaining
            ));
        }

        // Remove button
        if multiplayer {
            html.push_str(&format!(
                r#"<button class="ml-auto text-xs opacity-60 hover:opacity-100 cursor-pointer" onclick="kipukasMultiplayer.removeTurn({})" aria-label="Remove timer">&#x2715;</button>"#,
                i
            ));
        } else {
            html.push_str(&format!(
                r#"<button class="ml-auto text-xs opacity-60 hover:opacity-100 cursor-pointer" onclick="htmx.ajax('POST', '/api/game/turns', {{values: {{action: 'remove', index: '{}'}}, target: '#turn-alarms', swap: 'innerHTML'}})" aria-label="Remove timer">&#x2715;</button>"#,
                i
            ));
        }

        html.push_str(r#"</div>"#);
    }

    html.push_str(r#"</div>"#); // close alarm-list-inner

    // Toggle visibility button — routes through multiplayer-aware path when synced
    let rotate_class = if show_alarms { "" } else { " rotate-180" };
    let toggle_action = if multiplayer {
        "htmx.ajax('POST', '/api/room/yrs/alarm/toggle', {target: '#turn-alarms', swap: 'innerHTML'})"
    } else {
        "htmx.ajax('POST', '/api/game/turns', {values: {action: 'toggle_visibility'}, target: '#turn-alarms', swap: 'innerHTML'})"
    };
    html.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" alt="tools toggle" onclick="{}" class="fill-none stroke-2 z-50 stroke-kip-drk-goldenrod w-6 h-6 mb-2 justify-left cursor-pointer{}">"#,
        toggle_action, rotate_class
    ));
    html.push_str(
        r#"<path stroke-linecap="round" stroke-linejoin="round" d="m4.5 15.75 7.5-7.5 7.5 7.5" />"#,
    );
    html.push_str(r#"</svg>"#);

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
        add_alarm(5, "Scout patrol", "green");
        add_alarm(3, "", "red");
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
        add_alarm(1, "", "invalid");
        let alarms = player_doc::get_alarms();
        assert_eq!(alarms[0].color_set, "red"); // defaults to red
        reset_state();
    }

    #[test]
    fn tick_decrements_and_removes_expired() {
        reset_state();
        add_alarm(2, "", "red");
        add_alarm(1, "", "blue");

        tick_alarms();
        let alarms = player_doc::get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].remaining, 1);
        assert_eq!(alarms[1].remaining, 0); // complete

        tick_alarms();
        let alarms = player_doc::get_alarms();
        assert_eq!(alarms.len(), 1); // 0→-1 removed
        assert_eq!(alarms[0].remaining, 0); // 1→0 complete

        tick_alarms();
        let alarms = player_doc::get_alarms();
        assert!(alarms.is_empty()); // all removed

        reset_state();
    }

    #[test]
    fn remove_alarm_by_index() {
        reset_state();
        add_alarm(5, "first", "red");
        add_alarm(3, "second", "green");
        add_alarm(1, "third", "blue");
        remove_alarm(1); // remove the 3-turn alarm
        let alarms = player_doc::get_alarms();
        assert_eq!(alarms.len(), 2);
        assert_eq!(alarms[0].remaining, 5);
        assert_eq!(alarms[1].remaining, 1);
        reset_state();
    }

    #[test]
    fn remove_alarm_out_of_bounds_is_noop() {
        reset_state();
        add_alarm(5, "", "red");
        remove_alarm(99);
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
        add_alarm(5, "Dragon siege", "green");
        add_alarm(0, "", "red");
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
        // Set room as connected so render_alarm_list detects multiplayer
        room::with_room_mut(|r| r.connected = true);
        crdt::init_doc();
        crdt::add_alarm(3, "", "blue");
        let html = render_alarm_list();
        assert!(html.contains("kipukasMultiplayer.tickTurns()"));
        assert!(html.contains("kipukasMultiplayer.removeTurn(0)"));
        assert!(!html.contains("action: 'tick'")); // no direct HTMX tick calls
        crdt::reset_doc();
        reset_state();
    }

    #[test]
    fn render_turn_panel_has_controls() {
        reset_state();
        // room disconnected → local mode
        let html = render_turn_panel();
        assert!(html.contains("turnsSelector"));
        assert!(html.contains("timerName"));
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
        // Set room as connected so render_turn_panel detects multiplayer
        room::with_room_mut(|r| r.connected = true);
        let html = render_turn_panel();
        assert!(html.contains("kipukasMultiplayer.addTurn"));
        assert!(!html.contains("htmx.ajax")); // multiplayer routes through JS
        reset_state();
    }

    #[test]
    fn add_alarm_validates_all_colors() {
        reset_state();
        // Valid colors stored as-is
        add_alarm(1, "", "red");
        add_alarm(1, "", "green");
        add_alarm(1, "", "blue");
        add_alarm(1, "", "yellow");
        add_alarm(1, "", "pink");
        // Invalid colors default to red
        add_alarm(1, "", "invalid");
        add_alarm(1, "", "");
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
