//! Turn/alarm tracking — diel cycle countdown timers.
//!
//! Players set alarms for N diel cycles. Each "tick" (click) decrements
//! all active alarms by 1. Alarms at 0 show "Complete". Expired alarms
//! (negative) are removed on the next tick.
//!
//! Phase 5: Extended with name, color_set fields and cleaner UI.
//! Multiplayer sync broadcasts timer mutations to the connected peer.

use crate::game::crdt;
use crate::game::state::{with_state, with_state_mut, Alarm};

/// Add a new alarm with the given number of diel cycles, optional name, and color set.
pub fn add_alarm(turns: i32, name: &str, color_set: &str) {
    let color = validate_color_set(color_set);
    with_state_mut(|state| {
        state.alarms.push(Alarm {
            remaining: turns,
            name: name.to_string(),
            color_set: color.to_string(),
        });
    });
}

/// Tick all alarms: decrement by 1, remove any that went below 0.
pub fn tick_alarms() {
    with_state_mut(|state| {
        // Decrement all
        for alarm in state.alarms.iter_mut() {
            alarm.remaining -= 1;
        }
        // Remove expired (below 0 means they were already shown as "Complete" at 0)
        state.alarms.retain(|a| a.remaining >= 0);
    });
}

/// Remove a specific alarm by index.
pub fn remove_alarm(index: usize) {
    with_state_mut(|state| {
        if index < state.alarms.len() {
            state.alarms.remove(index);
        }
    });
}

/// Toggle alarm panel visibility.
pub fn toggle_alarms_visibility() {
    with_state_mut(|state| {
        state.show_alarms = !state.show_alarms;
    });
}

/// Merge a set of remote alarms into local state (union — no duplicates by position).
/// Used on initial multiplayer room connect to sync both players' timers.
pub fn merge_alarms(remote_alarms: &[Alarm]) {
    with_state_mut(|state| {
        for alarm in remote_alarms {
            // Simple append — both players see all timers
            state.alarms.push(alarm.clone());
        }
    });
}

/// Export current alarms as JSON for sync.
pub fn export_alarms_json() -> String {
    with_state(|state| {
        serde_json::to_string(&state.alarms).unwrap_or_else(|_| "[]".to_string())
    })
}

/// Validate color set, defaulting to "red" if invalid.
fn validate_color_set(color: &str) -> &str {
    match color {
        "red" | "green" | "blue" | "yellow" | "pink" => color,
        _ => "red",
    }
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
/// If `multiplayer` is true, the submit button routes through the
/// multiplayer sync path instead of the local-only path.
pub fn render_turn_panel(multiplayer: bool) -> String {
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
/// If `multiplayer` is true, the advance and remove buttons route through
/// the multiplayer sync path so both peers are updated.
pub fn render_alarm_list(multiplayer: bool) -> String {
    // When multiplayer, read alarms from the yrs CRDT Doc (synced between peers).
    // When local, read from GameState.alarms (persisted to localStorage).
    let (alarms, show_alarms) = if multiplayer {
        let crdt_alarms = crdt::get_alarms();
        let show = with_state(|state| state.show_alarms);
        (crdt_alarms, show)
    } else {
        with_state(|state| (state.alarms.clone(), state.show_alarms))
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
    use crate::game::state::{replace_state, with_state, GameState};

    fn reset_state() {
        replace_state(GameState::default());
    }

    #[test]
    fn add_alarm_works() {
        reset_state();
        add_alarm(5, "Scout patrol", "green");
        add_alarm(3, "", "red");
        with_state(|s| {
            assert_eq!(s.alarms.len(), 2);
            assert_eq!(s.alarms[0].remaining, 5);
            assert_eq!(s.alarms[0].name, "Scout patrol");
            assert_eq!(s.alarms[0].color_set, "green");
            assert_eq!(s.alarms[1].remaining, 3);
            assert_eq!(s.alarms[1].name, "");
            assert_eq!(s.alarms[1].color_set, "red");
        });
        reset_state();
    }

    #[test]
    fn add_alarm_validates_color() {
        reset_state();
        add_alarm(1, "", "invalid");
        with_state(|s| {
            assert_eq!(s.alarms[0].color_set, "red"); // defaults to red
        });
        reset_state();
    }

    #[test]
    fn tick_decrements_and_removes_expired() {
        reset_state();
        add_alarm(2, "", "red");
        add_alarm(1, "", "blue");

        tick_alarms();
        with_state(|s| {
            assert_eq!(s.alarms.len(), 2);
            assert_eq!(s.alarms[0].remaining, 1);
            assert_eq!(s.alarms[1].remaining, 0); // complete
        });

        tick_alarms();
        with_state(|s| {
            assert_eq!(s.alarms.len(), 1); // 0→-1 removed
            assert_eq!(s.alarms[0].remaining, 0); // 1→0 complete
        });

        tick_alarms();
        with_state(|s| {
            assert!(s.alarms.is_empty()); // all removed
        });

        reset_state();
    }

    #[test]
    fn remove_alarm_by_index() {
        reset_state();
        add_alarm(5, "first", "red");
        add_alarm(3, "second", "green");
        add_alarm(1, "third", "blue");
        remove_alarm(1); // remove the 3-turn alarm
        with_state(|s| {
            assert_eq!(s.alarms.len(), 2);
            assert_eq!(s.alarms[0].remaining, 5);
            assert_eq!(s.alarms[1].remaining, 1);
        });
        reset_state();
    }

    #[test]
    fn remove_alarm_out_of_bounds_is_noop() {
        reset_state();
        add_alarm(5, "", "red");
        remove_alarm(99);
        with_state(|s| assert_eq!(s.alarms.len(), 1));
        reset_state();
    }

    #[test]
    fn toggle_visibility() {
        reset_state();
        with_state(|s| assert!(s.show_alarms));
        toggle_alarms_visibility();
        with_state(|s| assert!(!s.show_alarms));
        toggle_alarms_visibility();
        with_state(|s| assert!(s.show_alarms));
        reset_state();
    }

    #[test]
    fn render_alarm_list_empty_when_no_alarms() {
        reset_state();
        let html = render_alarm_list(false);
        assert!(html.is_empty());
        reset_state();
    }

    #[test]
    fn render_alarm_list_shows_alarms_with_colors() {
        reset_state();
        add_alarm(5, "Dragon siege", "green");
        add_alarm(0, "", "red");
        let html = render_alarm_list(false);
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
        // Multiplayer mode reads from the yrs CRDT Doc, not GameState
        crdt::init_doc();
        crdt::add_alarm(3, "", "blue");
        let html = render_alarm_list(true);
        assert!(html.contains("kipukasMultiplayer.tickTurns()"));
        assert!(html.contains("kipukasMultiplayer.removeTurn(0)"));
        // Toggle visibility always uses htmx.ajax (local-only), but tick/remove should use multiplayer
        assert!(!html.contains("action: 'tick'")); // no direct HTMX tick calls
        crdt::reset_doc();
        reset_state();
    }

    #[test]
    fn render_turn_panel_has_controls() {
        let html = render_turn_panel(false);
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
    }

    #[test]
    fn render_turn_panel_multiplayer_uses_sync() {
        let html = render_turn_panel(true);
        assert!(html.contains("kipukasMultiplayer.addTurn"));
        assert!(!html.contains("htmx.ajax")); // multiplayer routes through JS
    }

    #[test]
    fn merge_alarms_appends() {
        reset_state();
        add_alarm(5, "local", "red");
        let remote = vec![
            Alarm {
                remaining: 3,
                name: "remote".to_string(),
                color_set: "blue".to_string(),
            },
        ];
        merge_alarms(&remote);
        with_state(|s| {
            assert_eq!(s.alarms.len(), 2);
            assert_eq!(s.alarms[0].name, "local");
            assert_eq!(s.alarms[1].name, "remote");
        });
        reset_state();
    }

    #[test]
    fn export_alarms_json_roundtrip() {
        reset_state();
        add_alarm(5, "test", "green");
        let json = export_alarms_json();
        assert!(json.contains("test"));
        assert!(json.contains("green"));

        let parsed: Vec<Alarm> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].remaining, 5);
        reset_state();
    }

    #[test]
    fn validate_color_set_works() {
        assert_eq!(validate_color_set("red"), "red");
        assert_eq!(validate_color_set("green"), "green");
        assert_eq!(validate_color_set("blue"), "blue");
        assert_eq!(validate_color_set("yellow"), "yellow");
        assert_eq!(validate_color_set("pink"), "pink");
        assert_eq!(validate_color_set("invalid"), "red");
        assert_eq!(validate_color_set(""), "red");
    }
}
