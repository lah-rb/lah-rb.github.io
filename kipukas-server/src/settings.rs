//! `/api/settings` — the settings page, an engine-owned view (Pattern 13 in
//! CONTRIBUTING.md).
//!
//! A setting is a choice the player makes about how the site behaves on
//! this device, on or off. The engine holds them and draws them
//! (`sections/settings.html` holds one view of this route), and a tap on a
//! switch is a post here.
//!
//! Each is kept in the browser's storage under a key of its own, `true`
//! while it is on, apart from the engine's documents: it is a choice about
//! this device, so it is not synced to another device. The page hands the
//! keys over at load with the rest (`store`), and the engine has one
//! written when it is turned (`store::put`). Every setting is off until
//! the player turns it on.
//!
//! A backup of the player's data carries the settings as well
//! (`game::backup`), and restoring from one puts them as the backup has
//! them: a player who moves to another device by a backup takes their
//! choices along.
//!
//! What a setting changes is the business of the module it is about, which
//! asks [`is_on`], and may turn it by a control of its own ([`set`]: the
//! scanner's privacy notice does). To add one: a [`Setting`] here, listed
//! in [`ALL`]; its key in `READ_AT_START` in kipukas-api.js; and the
//! asking.

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::routes::util::{get_param, parse_form_body};
use crate::store;

/// A choice of the player's about this device, on or off.
pub struct Setting {
    /// Storage key: `true` while the setting is on. It is also the name a
    /// switch posts.
    pub key: &'static str,
    /// What the settings page calls it.
    title: &'static str,
    /// What it does, in the page's words to the player. Markup: it may
    /// hold a link.
    says: &'static str,
}

/// The player has accepted the privacy policy for the QR scanner's camera:
/// in the scanner's own notice, which is shown until they have, or here.
/// Turned off, the scanner shows its notice again before it uses the camera
/// (`routes::qr`). The privacy policy page's Accept link writes the key
/// too.
pub const CAMERA_PRIVACY: Setting = Setting {
    key: "qr-privacy-accepted",
    title: "Camera privacy notice",
    says: "On once you have accepted our \
           <a href=\"/privacy_policy\" class=\"text-kip-red hover:text-emerald-600 underline\">privacy policy</a> \
           for the QR scanner's camera, here or in the scanner's own notice. \
           Turn it off to take that back: the scanner will show its notice again before it next uses the camera. \
           The browser's own camera permission is set in the browser.",
};

/// The QR scanner writes over the camera's picture what its frames take
/// (`routes::qr`).
pub const SCANNER_STATUS: Setting = Setting {
    key: "kipukas-scanner-status",
    title: "Scanner status",
    says: "Show over the camera's picture how long the QR scanner spends on each frame, in milliseconds: \
           the game engine's part, the QR reader's and, with computer vision on, the model's. \
           The average since the scanner opened follows each in brackets. \
           Above that are the size of the camera's picture and how many frames a second the scanner looks at.",
};

/// Every setting, in the order the page lists them.
const ALL: [&Setting; 2] = [&CAMERA_PRIVACY, &SCANNER_STATUS];

thread_local! {
    /// The keys of the settings that are on.
    static ON: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

/// At load: the settings as they are stored. `stored` is everything the
/// page handed over (`store::handle_start_post`).
pub fn restore(stored: &BTreeMap<String, String>) {
    let on = ALL.iter().map(|setting| setting.key).filter(|key| stored.get(*key).is_some_and(|value| value == "true"));
    ON.with(|held| *held.borrow_mut() = on.collect());
}

/// Whether the player has turned `setting` on.
pub fn is_on(setting: &Setting) -> bool {
    ON.with(|held| held.borrow().contains(&setting.key))
}

/// Turn `setting` on or off, and have that stored. What the setting is
/// about is told.
pub fn set(setting: &'static Setting, on: bool) {
    ON.with(|held| {
        let mut held = held.borrow_mut();
        held.retain(|key| *key != setting.key);
        if on {
            held.push(setting.key);
        }
    });
    store::put(setting.key, Some(on.to_string()));
    if setting.key == SCANNER_STATUS.key {
        crate::routes::qr::status_setting_changed();
    }
    if setting.key == CAMERA_PRIVACY.key {
        crate::routes::qr::privacy_setting_changed();
    }
}

/// The settings as a backup carries them: each one's key, and whether it
/// is on.
pub fn for_backup() -> serde_json::Value {
    let carried: serde_json::Map<String, serde_json::Value> =
        ALL.iter().map(|setting| (setting.key.to_string(), is_on(setting).into())).collect();
    carried.into()
}

/// Put the settings as a backup has them (`carried`, what [`for_backup`]
/// made). A setting the backup says nothing of stays as it is: the backup
/// was made before the setting was, or before backups carried settings.
/// Anything in it that is not a setting, on or off, is passed over.
pub fn from_backup(carried: &serde_json::Value) {
    for setting in ALL {
        match carried.get(setting.key).and_then(serde_json::Value::as_bool) {
            Some(on) if on != is_on(setting) => set(setting, on),
            _ => {}
        }
    }
}

/// Every setting off, as before any page has loaded.
#[cfg(test)]
pub fn reset() {
    ON.with(|held| held.borrow_mut().clear());
}

// ── Route handlers ─────────────────────────────────────────────────

/// GET /api/settings — the settings, each with its switch.
pub fn handle_get(_query: &str) -> String {
    render()
}

/// POST /api/settings — a tap on a switch.
///
/// Body: `toggle=<key>` turns that setting on if it is off, and off if it
/// is on. A key that is no setting's changes nothing.
pub fn handle_post(body: &str) -> String {
    let params = parse_form_body(body);
    let named = get_param(&params, "toggle").unwrap_or("");
    if let Some(setting) = ALL.iter().find(|setting| setting.key == named) {
        set(setting, !is_on(setting));
    }
    String::new()
}

// ── HTML fragment builders ─────────────────────────────────────────

fn render() -> String {
    let rows: String = ALL.iter().map(|setting| row(setting, is_on(setting))).collect();
    format!(r#"<ul class="divide-y divide-kip-drk-goldenrod/30 border-y border-kip-drk-goldenrod/30">{}</ul>"#, rows)
}

/// A setting and its switch. Whether it is on is the switch's
/// `aria-checked`, which is also what colours it and moves its knob.
fn row(setting: &Setting, on: bool) -> String {
    format!(
        r#"<li class="flex items-center justify-between gap-4 py-4">
  <div class="text-left leading-normal">
    <p class="text-lg font-bold">{title}</p>
    <p class="text-sm">{says}</p>
  </div>
  <button type="button" role="switch" aria-checked="{on}" aria-label="{title}"
          data-engine-post="/api/settings" data-engine-body="toggle={key}"
          class="group inline-flex h-8 w-14 shrink-0 items-center rounded-full bg-slate-400 transition-colors aria-checked:bg-emerald-600 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-kip-red">
    <span aria-hidden="true" class="inline-block size-6 translate-x-1 rounded-full bg-amber-50 shadow transition-transform group-aria-checked:translate-x-7"></span>
  </button>
</li>"#,
        title = setting.title,
        says = setting.says,
        on = on,
        key = setting.key,
    )
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page;

    fn fresh() {
        reset();
        store::reset();
        page::reset();
        crate::routes::qr::reset();
    }

    /// A tap on a switch, and the page as it is then drawn.
    fn tap(body: &str) -> String {
        assert_eq!(handle_post(body), "");
        handle_get("")
    }

    fn stored(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
    }

    #[test]
    fn every_setting_is_off_on_a_first_visit() {
        fresh();
        restore(&stored(&[]));
        for setting in ALL {
            assert!(!is_on(setting), "{}", setting.key);
        }
        let html = handle_get("");
        assert!(html.contains("Scanner status"));
        assert!(html.contains(r#"role="switch" aria-checked="false""#));
        assert!(!html.contains(r#"aria-checked="true""#));
        fresh();
    }

    #[test]
    fn the_camera_privacy_notice_is_a_setting_with_a_link_to_the_policy() {
        fresh();
        let html = handle_get("");
        let (privacy, status) = (html.find("Camera privacy notice").unwrap(), html.find("Scanner status").unwrap());
        assert!(privacy < status, "it is listed first");
        assert!(html.contains(r#"<a href="/privacy_policy""#));
        assert!(html.contains(r#"data-engine-post="/api/settings" data-engine-body="toggle=qr-privacy-accepted""#));

        // The key is the one the scanner's notice has always written, so a
        // player who accepted it there finds the switch on.
        restore(&stored(&[("qr-privacy-accepted", "true")]));
        assert!(is_on(&CAMERA_PRIVACY));
        assert!(handle_get("").contains(r#"aria-checked="true" aria-label="Camera privacy notice""#));

        let html = tap("toggle=qr-privacy-accepted");
        assert!(html.contains(r#"aria-checked="false" aria-label="Camera privacy notice""#));
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert_eq!(handed["save"]["qr-privacy-accepted"], "false");
        fresh();
    }

    #[test]
    fn a_tap_turns_a_setting_on_and_has_it_stored_and_another_turns_it_off() {
        fresh();
        let html = tap("toggle=kipukas-scanner-status");
        assert!(is_on(&SCANNER_STATUS));
        assert!(html.contains(r#"aria-checked="true" aria-label="Scanner status""#));
        assert!(html.contains(r#"data-engine-post="/api/settings" data-engine-body="toggle=kipukas-scanner-status""#));
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert_eq!(handed["save"]["kipukas-scanner-status"], "true");

        let html = tap("toggle=kipukas-scanner-status");
        assert!(!is_on(&SCANNER_STATUS));
        assert!(html.contains(r#"aria-checked="false""#));
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert_eq!(handed["save"]["kipukas-scanner-status"], "false");
        fresh();
    }

    #[test]
    fn what_is_stored_is_what_the_next_page_starts_with() {
        fresh();
        restore(&stored(&[("kipukas-scanner-status", "true")]));
        assert!(is_on(&SCANNER_STATUS));
        assert!(handle_get("").contains(r#"aria-checked="true""#));
        // Anything but `true` is off.
        for value in ["false", "", "1", "TRUE"] {
            restore(&stored(&[("kipukas-scanner-status", value)]));
            assert!(!is_on(&SCANNER_STATUS), "{:?}", value);
        }
        fresh();
    }

    #[test]
    fn a_backup_carries_every_setting_and_puts_back_those_it_names() {
        fresh();
        tap("toggle=kipukas-scanner-status");
        let carried = for_backup();
        assert_eq!(carried, serde_json::json!({ "qr-privacy-accepted": false, "kipukas-scanner-status": true }));

        // On another device, with other choices made.
        fresh();
        tap("toggle=qr-privacy-accepted");
        page::handle_handover_post("");
        from_backup(&carried);
        assert!(is_on(&SCANNER_STATUS) && !is_on(&CAMERA_PRIVACY));
        // Both are stored as they now are.
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert_eq!(handed["save"]["kipukas-scanner-status"], "true");
        assert_eq!(handed["save"]["qr-privacy-accepted"], "false");

        // A setting it does not name stays as it is, and what is not a
        // setting, on or off, is passed over.
        for carried in [
            serde_json::json!({}),
            serde_json::Value::Null,
            serde_json::json!("on"),
            serde_json::json!({ "kipukas-scanner-status": "false", "qr-privacy-accepted": 1, "kipukas_player_doc": true }),
        ] {
            from_backup(&carried);
            assert!(is_on(&SCANNER_STATUS) && !is_on(&CAMERA_PRIVACY), "{}", carried);
        }
        fresh();
    }

    #[test]
    fn a_tap_that_names_no_setting_changes_nothing() {
        fresh();
        let before = handle_get("");
        assert_eq!(tap("toggle=kipukas_player_doc"), before);
        assert_eq!(tap("toggle="), before);
        assert_eq!(tap("other=1"), before);
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert!(handed["save"].get("kipukas-scanner-status").is_none(), "nothing is written: {}", handed);
        fresh();
    }

    #[test]
    fn the_same_settings_always_draw_the_same() {
        fresh();
        assert_eq!(handle_get(""), handle_get(""));
        let on = tap("toggle=kipukas-scanner-status");
        tap("toggle=kipukas-scanner-status");
        assert_eq!(tap("toggle=kipukas-scanner-status"), on);
        fresh();
    }
}
