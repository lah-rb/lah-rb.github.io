//! Turn/alarm tracking — diel cycle countdown timers.
//!
//! Players set alarms for N diel cycles. Each "tick" (click) decrements
//! all active alarms by 1. Alarms at 0 show "Complete". Expired alarms
//! (negative) are removed on the next tick.

use crate::game::state::{with_state, with_state_mut, Alarm};

/// Add a new alarm with the given number of diel cycles.
pub fn add_alarm(turns: i32) {
    with_state_mut(|state| {
        state.alarms.push(Alarm { remaining: turns });
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

/// Render the turn tracker panel HTML.
/// This is the content inside the turn tracker popover — includes
/// the range slider, "New Timer" button, and active alarm list.
pub fn render_turn_panel() -> String {
    let mut html = String::with_capacity(1024);

    // Timer creation form
    html.push_str(r#"<div class="grid grid-cols-1" x-data="{ turnsToAlarm: 1 }">"#);
    html.push_str(
        r#"<input type="range" name="turnsSelector" id="turnsSelector" min="1" max="10" x-model="turnsToAlarm" class="mr-4">"#,
    );
    html.push_str(
        r#"<label for="turnsSelector"><strong x-text="turnsToAlarm"></strong> Diel Cycles</label>"#,
    );
    html.push_str(&format!(
        r#"<button aria-label="Submit turn timer" class="bg-kip-red hover:bg-emerald-600 text-amber-50 font-bold py-2 my-2 px-4 rounded mr-4" onclick="htmx.ajax('POST', '/api/game/turns', {{values: {{action: 'add', turns: document.getElementById('turnsSelector').value}}, target: '#turn-alarms', swap: 'innerHTML'}})">New Timer</button>"#,
    ));
    html.push_str(r#"</div>"#);

    html
}

/// Render the alarm list HTML.
/// This is the floating alarm display in the top-left corner.
pub fn render_alarm_list() -> String {
    let (alarms, show_alarms) =
        with_state(|state| (state.alarms.clone(), state.show_alarms));

    if alarms.is_empty() {
        return String::new();
    }

    let mut html = String::with_capacity(1024);

    // Alarm container
    let collapse_class = if show_alarms { "" } else { " hidden" };
    html.push_str(&format!(
        r#"<div id="alarm-list-inner" class="{}""#,
        collapse_class.trim()
    ));
    html.push_str(r#">"#);

    // Click to tick button
    html.push_str(&format!(
        r#"<p class="py-2 px-4 h-fit w-fit bg-amber-50 rounded-lg text-kip-drk-goldenrod mb-2 hover:bg-amber-100 select-none cursor-pointer" onclick="htmx.ajax('POST', '/api/game/turns', {{values: {{action: 'tick'}}, target: '#turn-alarms', swap: 'innerHTML'}})">Click here on each diel cycle roll</p>"#,
    ));

    // Individual alarms
    for alarm in &alarms {
        html.push_str(
            r#"<div class="py-2 px-4 h-fit w-fit bg-amber-50 rounded-lg text-kip-drk-goldenrod mb-2 flex">"#,
        );
        if alarm.remaining == 0 {
            html.push_str(r#"<p>Complete, click above to close</p>"#);
        } else {
            html.push_str(&format!(
                r#"<p>Turns to Alarm: <strong>{}</strong></p>"#,
                alarm.remaining
            ));
        }
        html.push_str(r#"</div>"#);
    }

    html.push_str(r#"</div>"#); // close alarm-list-inner

    // Toggle visibility button
    let rotate_class = if show_alarms { "" } else { " rotate-180" };
    html.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" alt="tools toggle" onclick="htmx.ajax('POST', '/api/game/turns', {{values: {{action: 'toggle_visibility'}}, target: '#turn-alarms', swap: 'innerHTML'}})" class="fill-none stroke-2 z-50 stroke-kip-drk-goldenrod w-6 h-6 mb-2 justify-left cursor-pointer{}">"#,
        rotate_class
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
        add_alarm(5);
        add_alarm(3);
        with_state(|s| {
            assert_eq!(s.alarms.len(), 2);
            assert_eq!(s.alarms[0].remaining, 5);
            assert_eq!(s.alarms[1].remaining, 3);
        });
        reset_state();
    }

    #[test]
    fn tick_decrements_and_removes_expired() {
        reset_state();
        add_alarm(2);
        add_alarm(1);

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
        add_alarm(5);
        add_alarm(3);
        add_alarm(1);
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
        add_alarm(5);
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
        let html = render_alarm_list();
        assert!(html.is_empty());
        reset_state();
    }

    #[test]
    fn render_alarm_list_shows_alarms() {
        reset_state();
        add_alarm(5);
        add_alarm(0);
        let html = render_alarm_list();
        assert!(html.contains("Turns to Alarm: <strong>5</strong>"));
        assert!(html.contains("Complete, click above to close"));
        assert!(html.contains("diel cycle roll"));
        reset_state();
    }

    #[test]
    fn render_turn_panel_has_controls() {
        let html = render_turn_panel();
        assert!(html.contains("turnsSelector"));
        assert!(html.contains("New Timer"));
        assert!(html.contains("Diel Cycles"));
    }
}
