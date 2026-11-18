//! `/api/player/backup` — the Player Data dialog, an engine-owned view
//! (Pattern 13 in CONTRIBUTING.md) of making a backup of the player's data
//! and restoring from one (`game::backup`).
//!
//! The dialog's buttons and forms post back to this route. The import
//! form's file field is sent as the chosen file's text
//! (`assets/js/kipukas-view.js`), and a backup that is made reaches the
//! player as a file through the handover (`page::download`). Neither form is
//! cleared when it is sent (`data-engine-stay`): one that is refused keeps
//! its chosen file, and the engine draws the passphrase back into its field.

use crate::clock;
use crate::game::backup::{self, Dialog, Screen};
use crate::routes::util::{escape_html, get_param, parse_form_body};

const ROUTE: &str = "/api/player/backup";
const FIELD: &str =
    "w-full border border-slate-300 rounded px-3 py-2 text-sm bg-white focus:border-kip-red focus:ring-kip-red";
const RED_BUTTON: &str =
    "w-full bg-kip-red hover:bg-red-700 active:bg-red-800 text-amber-50 font-bold py-2 px-4 rounded text-sm";
const GREY_BUTTON: &str = "w-full bg-slate-400 hover:bg-slate-500 text-amber-50 font-bold py-2 px-4 rounded text-sm";

/// GET /api/player/backup — what the dialog holds.
pub fn handle_get(_query: &str) -> String {
    render(&backup::dialog())
}

/// POST /api/player/backup — a button or form of the dialog.
///
/// Body:
///   - `action=export-screen` | `import-screen` | `menu`: go to that screen
///   - `action=export&passphrase=…&confirm=…`: make a backup
///   - `action=import&file=<the file's text>&passphrase=…`: restore from one
///   - `action=close`: the dialog was closed (sent by the page)
pub fn handle_post(body: &str) -> String {
    let params = parse_form_body(body);
    let passphrase = get_param(&params, "passphrase").unwrap_or("");
    match get_param(&params, "action").unwrap_or("") {
        "export-screen" => backup::show(Screen::Export),
        "import-screen" => backup::show(Screen::Import),
        "menu" => backup::show(Screen::Menu),
        "export" => backup::export(clock::now_ms(), passphrase, get_param(&params, "confirm").unwrap_or("")),
        "import" => backup::import(passphrase, get_param(&params, "file").unwrap_or("")),
        "close" => backup::close(),
        _ => {}
    }
    String::new()
}

fn render(dialog: &Dialog) -> String {
    let mut h = String::with_capacity(2048);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna">"#);
    h.push_str(r#"<p class="text-lg font-bold text-center mb-1">Player Data</p>"#);
    h.push_str(r#"<p class="text-xs text-slate-500 text-center mb-4">Backup and restore your game progress</p>"#);

    // What came of the last thing asked. Partly a file's words (a version).
    if !dialog.notice.is_empty() {
        h.push_str(&format!(
            r#"<div class="text-center text-sm mb-3 p-2 rounded {}" role="{}">{}</div>"#,
            if dialog.good { "bg-emerald-50 text-emerald-700" } else { "bg-red-50 text-kip-red" },
            if dialog.good { "status" } else { "alert" },
            escape_html(&dialog.notice)
        ));
    }

    match dialog.screen {
        Screen::Menu => h.push_str(&render_menu()),
        Screen::Export => h.push_str(&render_export(dialog)),
        Screen::Import => h.push_str(&render_import(dialog)),
    }
    h.push_str("</div>");
    h
}

/// A field's `value` attribute for what was typed, if anything was.
fn typed(text: &str) -> String {
    if text.is_empty() { String::new() } else { format!(r#" value="{}""#, escape_html(text)) }
}

fn render_menu() -> String {
    format!(
        concat!(
            r#"<div class="grid gap-2">"#,
            r#"<button type="button" data-engine-post="{route}" data-engine-body="action=export-screen" "#,
            r#"class="w-full bg-kip-drk-goldenrod hover:bg-yellow-700 active:bg-yellow-800 text-amber-50 font-bold py-3 px-4 rounded text-sm flex items-center justify-center gap-2">"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">"#,
            r#"<path stroke-linecap="round" stroke-linejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5M16.5 12L12 16.5m0 0L7.5 12m4.5 4.5V3" />"#,
            r#"</svg>Export Backup</button>"#,
            r#"<button type="button" data-engine-post="{route}" data-engine-body="action=import-screen" "#,
            r#"class="w-full bg-slate-500 hover:bg-slate-600 active:bg-slate-700 text-amber-50 font-bold py-3 px-4 rounded text-sm flex items-center justify-center gap-2">"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">"#,
            r#"<path stroke-linecap="round" stroke-linejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5m-13.5-9L12 3m0 0l4.5 4.5M12 3v13.5" />"#,
            r#"</svg>Import Backup</button>"#,
            r#"<p class="text-xs text-slate-400 text-center mt-2">Exports are signed and encrypted with your passphrase."#,
            r#"<br>Keep your passphrase safe — it cannot be recovered.</p>"#,
            r#"</div>"#,
        ),
        route = ROUTE
    )
}

/// The passphrase, twice.
fn render_export(dialog: &Dialog) -> String {
    format!(
        concat!(
            r#"<form data-engine-post="{route}" data-engine-body="action=export" data-engine-stay autocomplete="off" class="grid gap-3">"#,
            r#"<label class="grid gap-1 text-sm font-medium">Passphrase"#,
            r#"<input type="password" name="passphrase"{passphrase} placeholder="Enter a passphrase" class="{field} font-normal"></label>"#,
            r#"<label class="grid gap-1 text-sm font-medium">Confirm Passphrase"#,
            r#"<input type="password" name="confirm"{confirm} placeholder="Confirm passphrase" class="{field} font-normal"></label>"#,
            r#"<button class="{red}">Export</button>"#,
            r#"<button type="button" data-engine-post="{route}" data-engine-body="action=menu" class="{grey}">Back</button>"#,
            r#"</form>"#,
        ),
        route = ROUTE,
        passphrase = typed(&dialog.passphrase),
        confirm = typed(&dialog.confirm),
        field = FIELD,
        red = RED_BUTTON,
        grey = GREY_BUTTON,
    )
}

/// The file and its passphrase.
fn render_import(dialog: &Dialog) -> String {
    format!(
        concat!(
            r#"<form data-engine-post="{route}" data-engine-body="action=import" data-engine-stay autocomplete="off" class="grid gap-3">"#,
            r#"<label class="grid gap-1 text-sm font-medium">Select backup file"#,
            r#"<input type="file" name="file" accept=".kipukas" class="w-full text-sm font-normal text-slate-600 file:mr-2 file:py-2 file:px-3 file:rounded file:border-0 file:text-sm file:font-medium file:bg-slate-100 file:text-kip-drk-sienna hover:file:bg-slate-200"></label>"#,
            r#"<label class="grid gap-1 text-sm font-medium">Passphrase"#,
            r#"<input type="password" name="passphrase"{passphrase} placeholder="Enter backup passphrase" class="{field} font-normal"></label>"#,
            r#"<button class="{red}">Import</button>"#,
            r#"<button type="button" data-engine-post="{route}" data-engine-body="action=menu" class="{grey}">Back</button>"#,
            r#"</form>"#,
        ),
        route = ROUTE,
        passphrase = typed(&dialog.passphrase),
        field = FIELD,
        red = RED_BUTTON,
        grey = GREY_BUTTON,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::player_doc;
    use crate::page;

    fn fresh() {
        player_doc::init_player_doc();
        page::reset();
        backup::close();
    }

    #[test]
    fn the_dialog_opens_on_two_choices_and_nothing_for_alpine_or_a_script() {
        fresh();
        let html = handle_get("");
        assert!(html.contains(r#"data-engine-post="/api/player/backup" data-engine-body="action=export-screen""#));
        assert!(html.contains(r#"data-engine-post="/api/player/backup" data-engine-body="action=import-screen""#));
        assert!(!html.contains("<form"));
        for screen in ["menu", "export-screen", "import-screen"] {
            handle_post(&format!("action={}", screen));
            let html = handle_get("");
            for forbidden in ["onclick", "x-data", "x-model", "x-show", "@click", "<script", "<template", " id=", "kipukasWorker"] {
                assert!(!html.contains(forbidden), "{} on {}", forbidden, screen);
            }
        }
        fresh();
    }

    #[test]
    fn the_export_form_sends_the_passphrase_twice_and_a_refusal_keeps_what_was_typed() {
        fresh();
        handle_post("action=export-screen");
        let html = handle_get("");
        assert!(html.contains(r#"<form data-engine-post="/api/player/backup" data-engine-body="action=export" data-engine-stay autocomplete="off""#));
        assert!(html.contains(r#"<input type="password" name="passphrase" placeholder="Enter a passphrase""#));
        assert!(html.contains(r#"<input type="password" name="confirm" placeholder="Confirm passphrase""#));
        assert!(!html.contains(r#"role="alert""#));

        handle_post("action=export&passphrase=a%22b&confirm=a%22b");
        let html = handle_get("");
        assert!(html.contains(r#"role="alert">Passphrase must be at least 4 characters</div>"#));
        assert!(html.contains(r#"name="passphrase" value="a&quot;b""#));
        assert!(html.contains(r#"name="confirm" value="a&quot;b""#));
        assert!(page::take_download().is_none());
        fresh();
    }

    #[test]
    fn exporting_hands_over_a_file_and_importing_it_restores_the_player() {
        fresh();
        player_doc::add_alarm(1_795_032_930_123.0, 5, "siege", "red");
        handle_post("action=export-screen");
        handle_post("action=export&passphrase=correct+horse&confirm=correct+horse");
        let (name, file) = page::take_download().expect("a file");
        assert!(name.starts_with("kipukas-player-backup-") && name.ends_with(".kipukas"), "{}", name);
        let html = handle_get("");
        assert!(html.contains(r#"class="text-center text-sm mb-3 p-2 rounded bg-emerald-50 text-emerald-700" role="status">✓ Backup exported successfully</div>"#));
        assert!(html.contains("Import Backup"), "back on the menu");
        assert!(!html.contains("correct horse"));

        player_doc::init_player_doc();
        handle_post("action=import-screen");
        let html = handle_get("");
        assert!(html.contains(r#"<input type="file" name="file" accept=".kipukas""#));
        // The file's text arrives as a form field, as the page sends it.
        let encoded: String = file
            .bytes()
            .map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{:02X}", b) })
            .collect();
        let body = format!("action=import&file={}&passphrase=correct+horse", encoded);
        handle_post(&body);
        assert!(handle_get("").contains("✓ Player data restored"));
        assert_eq!(player_doc::get_alarms()[0].name, "siege");
        assert!(page::take_refresh());
        fresh();
    }

    #[test]
    fn a_refused_import_says_why_and_keeps_the_passphrase() {
        fresh();
        handle_post("action=import-screen");
        handle_post("action=import&file=not+a+backup&passphrase=correct+horse");
        let html = handle_get("");
        assert!(html.contains(r#"role="alert">Not a valid Kipukas backup file</div>"#));
        assert!(html.contains(r#"name="passphrase" value="correct horse""#));
        // Back to the menu, or closing, forgets it.
        handle_post("action=menu");
        handle_post("action=import-screen");
        assert!(!handle_get("").contains("correct horse"));
        handle_post("action=import&file=x&passphrase=correct+horse");
        handle_post("action=close");
        assert_eq!(backup::dialog(), Dialog::default());
        fresh();
    }
}
