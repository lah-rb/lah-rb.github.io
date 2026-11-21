//! `/api/player/sync` — the Player Data page's sync section, an engine-owned
//! view (Pattern 13 in CONTRIBUTING.md) of a game moving from one of the
//! player's devices to another (`game::device_sync`).
//!
//! Its buttons and its form post back to this route. What the relay and the
//! other device then say changes what it shows, and the engine has it drawn
//! again.
//!
//! The view's route carries what follows the `#` of the page's address
//! (`/api/player/sync?link={hash}`): the address in an invitation's QR code
//! opens the page with the invitation there. Drawing the view changes
//! nothing. An invitation is acted on only when the player answers it,
//! which is a post.

use qrcodegen::{QrCode, QrCodeEcc};

use crate::game::connection;
use crate::game::crypto::Side;
use crate::game::device_sync::{self, Dialog, Invite, Stage};
use crate::game::player_doc::{self, Progress, Taken};
use crate::routes::util::{escape_html, get_param, parse_form_body, parse_query};

const ROUTE: &str = "/api/player/sync";
const FIELD: &str =
    "w-full border border-slate-300 rounded px-3 py-2 text-sm bg-white focus:border-kip-red focus:ring-kip-red";
const RED_BUTTON: &str =
    "w-full bg-kip-red hover:bg-red-700 active:bg-red-800 text-amber-50 font-bold py-2 px-4 rounded text-sm";
const GREY_BUTTON: &str = "w-full bg-slate-400 hover:bg-slate-500 text-amber-50 font-bold py-2 px-4 rounded text-sm";

/// GET /api/player/sync?link=<what follows the # of the page's address> —
/// where a move stands, or an invitation the address carries.
pub fn handle_sync_get(query: &str) -> String {
    let params = parse_query(query);
    let here = Here {
        game: player_doc::progress(),
        room: connection::room_code(),
        seat: connection::seat().map(|seat| seat.code),
    };
    render(&device_sync::dialog_for(get_param(&params, "link").unwrap_or("")), &here)
}

/// What this device has that a move would change.
#[derive(Debug, Clone, Default)]
struct Here {
    /// Its game in progress.
    game: Progress,
    /// The room it is in, by its code.
    room: Option<String>,
    /// The room whose place it would send with its game: the same, unless
    /// another tab holds the room.
    seat: Option<String>,
}

/// POST /api/player/sync — a button or the form of the section.
///
/// Body:
///   - `action=ask`: the player wants to move this device's game to another
///   - `action=invite`: and has said yes: make the code to show
///   - `action=offer&invite=<code>`: a code typed on the receiving device
///   - `action=receive&invite=<code>`: the player there said yes to it
///   - `action=decline&invite=<code>`: or no
///   - `action=close`: call a move off, or put away what came of one
pub fn handle_sync_post(body: &str) -> String {
    let params = parse_form_body(body);
    let invite = get_param(&params, "invite").unwrap_or("");
    match get_param(&params, "action").unwrap_or("") {
        "ask" => device_sync::ask(),
        "invite" => device_sync::invite(),
        "offer" => device_sync::offer(invite),
        "receive" => device_sync::receive(invite),
        "decline" => device_sync::decline(invite),
        "close" => device_sync::close(),
        _ => {}
    }
    String::new()
}

fn button(class: &str, body: &str, label: &str) -> String {
    format!(
        r#"<button type="button" data-engine-post="{}" data-engine-body="{}" class="{}">{}</button>"#,
        ROUTE,
        escape_html(body),
        class,
        label
    )
}

fn render(dialog: &Dialog, here: &Here) -> String {
    let mut h = String::with_capacity(4096);
    h.push_str(r#"<div class="text-kip-drk-sienna text-left leading-normal">"#);
    if !dialog.error.is_empty() {
        h.push_str(&format!(
            r#"<div class="text-center text-sm mb-3 p-2 rounded bg-red-50 text-kip-red" role="alert">{}</div>"#,
            escape_html(&dialog.error)
        ));
    }
    match (dialog.stage, dialog.side) {
        (Stage::Inviting, _) => h.push_str(&render_inviting(dialog)),
        (Stage::Joining, _) => h.push_str(&render_busy("Finding the other device…", true)),
        (Stage::Moving, _) => h.push_str(&render_busy("Moving the game…", false)),
        (Stage::Done, Some(Side::Receiver)) => h.push_str(&render_received(dialog)),
        (Stage::Done, _) => h.push_str(&render_sent(dialog)),
        (Stage::Idle | Stage::Asked, _) => {
            h.push_str(&render_start(dialog));
            if dialog.stage == Stage::Asked {
                h.push_str(&render_asked(here));
            } else if let Some(invite) = &dialog.offered {
                h.push_str(&render_offered(invite, here));
            }
        }
    }
    h.push_str("</div>");
    h
}

/// How the section opens: the button that starts a move from this device,
/// and the field for a code shown by another. It is marked `data-sync-idle`,
/// which the page shows its own button by: the one that opens the scanner
/// to read such a code.
fn render_start(dialog: &Dialog) -> String {
    let typed =
        if dialog.typed.is_empty() { String::new() } else { format!(r#" value="{}""#, escape_html(&dialog.typed)) };
    format!(
        concat!(
            r#"<div class="grid gap-3" data-sync-idle>"#,
            r#"{start}"#,
            r#"<form data-engine-post="{route}" data-engine-body="action=offer" data-engine-stay autocomplete="off" class="grid gap-2 mt-2">"#,
            r#"<label class="grid gap-1 text-sm font-medium">To take a game from another device, type the code it shows"#,
            r#"<input type="text" name="invite"{typed} placeholder="ABCD-…" autocapitalize="characters" autocorrect="off" spellcheck="false" class="{field} font-mono font-normal uppercase"></label>"#,
            r#"<button class="{grey}">Use this code</button>"#,
            r#"</form>"#,
            r#"</div>"#,
        ),
        start = button(RED_BUTTON, "action=ask", "Sync a new device"),
        route = ROUTE,
        typed = typed,
        field = FIELD,
        grey = GREY_BUTTON,
    )
}

/// A question over the page, with its two answers.
fn render_question(label: &str, title: &str, paragraphs: &[String], no: String, yes: String) -> String {
    let mut h = format!(
        concat!(
            r#"<div class="fixed inset-0 flex items-center justify-center z-50 p-4" role="dialog" aria-modal="true" aria-label="{label}">"#,
            r#"<div class="absolute inset-0 bg-slate-300 opacity-75"></div>"#,
            r#"<div class="relative bg-amber-50 p-5 rounded-lg shadow-xl w-full max-w-sm text-kip-drk-sienna text-left leading-normal">"#,
            r#"<p class="text-lg font-bold mb-2">{title}</p>"#,
        ),
        label = label,
        title = title,
    );
    for paragraph in paragraphs {
        h.push_str(&format!(r#"<p class="text-sm mb-2">{}</p>"#, paragraph));
    }
    h.push_str(&format!(r#"<div class="grid grid-cols-2 gap-2 mt-4">{}{}</div></div></div>"#, no, yes));
    h
}

/// The sender is asked whether to go on.
fn render_asked(here: &Here) -> String {
    let mut paragraphs = vec![
        "Syncing removes the game in progress on the other device and puts this device's game in its place: its damage, its timers and its affinity.".to_string(),
    ];
    if let Some(room) = &here.seat {
        paragraphs.push(format!(
            "Your place in room {} goes with the game: the other device carries on there, and this device leaves the room.",
            escape_html(room)
        ));
    }
    paragraphs.push("Your player record is merged: each card's loyalty is added together on both devices.".to_string());
    paragraphs.push("Continue?".to_string());
    render_question(
        "Sync a new device",
        "Sync a new device?",
        &paragraphs,
        button(GREY_BUTTON, "action=close", "No"),
        button(RED_BUTTON, "action=invite", "Yes, continue"),
    )
}

/// What this device would lose, in words: its game in progress, and the
/// room it is in, which is part of that game.
fn in_words(here: &Here) -> String {
    let count = |n: usize, one: &str, many: &str| format!("{} {}", n, if n == 1 { one } else { many });
    let mut parts: Vec<String> = Vec::new();
    if here.game.damaged > 0 {
        parts.push(count(here.game.damaged, "card with damage", "cards with damage"));
    }
    if here.game.timers > 0 {
        parts.push(count(here.game.timers, "timer", "timers"));
    }
    if let Some(affinity) = &here.game.affinity {
        parts.push(format!("the {} affinity", escape_html(affinity)));
    }
    let game = if parts.is_empty() {
        "This device has no game in progress.".to_string()
    } else {
        format!("This device's game will be removed: {}.", parts.join(", "))
    };
    match &here.room {
        Some(room) => format!("{} It will leave room {}.", game, escape_html(room)),
        None => game,
    }
}

/// The receiver is asked whether to take the game an invitation offers.
fn render_offered(invite: &Invite, here: &Here) -> String {
    let answer = |action: &str| format!("action={}&invite={}", action, invite.compact());
    render_question(
        "Take the game from another device",
        "Take the game from your other device?",
        &[
            "The game in progress on the other device takes the place of the one on this device.".to_string(),
            in_words(here),
            "Your player record is merged: each card's loyalty is added together on both devices.".to_string(),
        ],
        button(GREY_BUTTON, &answer("decline"), "Not now"),
        button(RED_BUTTON, &answer("receive"), "Take the game"),
    )
}

const SPINNER: &str = concat!(
    r#"<svg class="animate-spin h-4 w-4 text-kip-red" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" aria-hidden="true">"#,
    r#"<circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>"#,
    r#"<path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path>"#,
    r#"</svg>"#,
);

fn render_busy(what: &str, can_cancel: bool) -> String {
    let mut h = format!(
        r#"<div class="flex items-center justify-center gap-2 my-3" role="status">{}<span class="text-sm">{}</span></div>"#,
        SPINNER, what
    );
    if can_cancel {
        h.push_str(&button(GREY_BUTTON, "action=close", "Cancel"));
    }
    h
}

/// A QR code of `text` as an SVG picture, with the quiet border a reader
/// needs. Each run of dark squares in a row is one rectangle of the path.
fn qr_svg(text: &str) -> Option<String> {
    const BORDER: i32 = 4;
    let code = QrCode::encode_text(text, QrCodeEcc::Medium).ok()?;
    let size = code.size();
    let mut path = String::new();
    for y in 0..size {
        let mut x = 0;
        while x < size {
            if !code.get_module(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x < size && code.get_module(x, y) {
                x += 1;
            }
            path.push_str(&format!("M{},{}h{}v1h-{}z", start + BORDER, y + BORDER, x - start, x - start));
        }
    }
    Some(format!(
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {side} {side}" class="w-60 h-60 max-w-full" role="img" aria-label="QR code for the other device to scan" shape-rendering="crispEdges">"##,
            r##"<rect width="{side}" height="{side}" fill="#fff"/><path d="{path}" fill="#000"/></svg>"##,
        ),
        side = size + 2 * BORDER,
        path = path,
    ))
}

/// The sender: its invitation, once the room has a code, for the other
/// device to scan or its player to type.
fn render_inviting(dialog: &Dialog) -> String {
    let Some(invite) = &dialog.invite else {
        return render_busy("Making a code…", true);
    };
    let mut h = String::new();
    h.push_str(r#"<p class="text-sm mb-2">On the other device, open this site and scan this with its QR scanner, or point its camera at it:</p>"#);
    if let Some(picture) = qr_svg(&invite.address()) {
        h.push_str(&format!(r#"<div class="bg-white rounded-lg p-2 w-fit max-w-full mx-auto">{}</div>"#, picture));
    }
    h.push_str(&format!(
        concat!(
            r#"<p class="text-xs text-slate-500 text-center mt-3">Or open Player Data there and type this code:</p>"#,
            r#"<p class="font-mono font-bold text-sm text-center select-all mb-3">{}</p>"#,
        ),
        invite.spelled()
    ));
    h.push_str(&format!(
        r#"<div class="flex items-center justify-center gap-2 mb-1" role="status">{}<span class="text-sm">{}</span></div>"#,
        SPINNER,
        if dialog.peer_here {
            "The other device is here. Waiting for its answer…"
        } else {
            "Waiting for the other device…"
        }
    ));
    h.push_str(r#"<p class="text-xs text-slate-500 text-center mb-3">The code works once, and for ten minutes.</p>"#);
    h.push_str(&button(GREY_BUTTON, "action=close", "Cancel"));
    h
}

fn days(n: u32) -> String {
    format!("{} day{} of play", n, if n == 1 { "" } else { "s" })
}

fn done(title: &str, lines: &[String]) -> String {
    let mut h = format!(
        r#"<div class="bg-emerald-50 border border-emerald-200 rounded p-3 mb-3" role="status"><p class="text-sm font-bold text-emerald-700">✓ {}</p>"#,
        title
    );
    for line in lines {
        h.push_str(&format!(r#"<p class="text-xs text-emerald-700 mt-1">{}</p>"#, line));
    }
    h.push_str("</div>");
    h.push_str(&button(GREY_BUTTON, "action=close", "Done"));
    h
}

/// The receiver: what came of taking the game over.
fn render_received(dialog: &Dialog) -> String {
    let none = Taken::default();
    let taken = dialog.taken.as_ref().unwrap_or(&none);
    let mut lines: Vec<String> = Vec::new();
    lines.push(match (taken.days_new_here, taken.days_new_there) {
        (0, 0) => "Your record was already the same on both.".to_string(),
        (here, 0) => format!("Your record gained {} from the other device.", days(here)),
        (0, there) => format!("This device added {} to your record.", days(there)),
        (here, there) => format!(
            "Your record gained {} from the other device, and this device added {}.",
            days(here),
            days(there)
        ),
    });
    // What this device added reaches the other only if it was heard from
    // afterwards.
    if !dialog.confirmed && taken.days_new_there > 0 {
        lines.push(
            "The other device went away before it said it had this device's part of the record. Sync from it again to be sure."
                .to_string(),
        );
    }
    if let Some(room) = &dialog.room {
        lines.push(format!("It has taken your place in room {}.", escape_html(room)));
    }
    done("This device now has the game from your other device.", &lines)
}

/// The sender: the other device has the game.
fn render_sent(dialog: &Dialog) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(if dialog.unheard {
        "It was not heard from after it took the game, so anything it added to your record has not reached this device. Sync again to bring it."
            .to_string()
    } else {
        match dialog.days_brought {
            0 => "Your record is the same on both.".to_string(),
            brought => format!("The other device added {} to your record.", days(brought)),
        }
    });
    if let Some(room) = &dialog.room {
        lines.push(format!("It has your place in room {}, and this device has left the room.", escape_html(room)));
    }
    done("The other device now has this game.", &lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HANDED: &str = "ABCD-EFGHJKLMNPQRSTUVWXYZ";

    fn invite() -> Invite {
        Invite::read(HANDED).unwrap()
    }

    fn at(stage: Stage, side: Option<Side>) -> Dialog {
        Dialog { stage, side, ..Dialog::default() }
    }

    fn draw(dialog: &Dialog) -> String {
        render(dialog, &Here::default())
    }

    fn fresh() {
        player_doc::init_player_doc();
        crate::page::reset();
        device_sync::reset();
    }

    #[test]
    fn the_section_opens_on_the_button_that_starts_a_move_and_a_field_for_a_code() {
        let html = draw(&Dialog::default());
        assert!(html.contains(r#"<button type="button" data-engine-post="/api/player/sync" data-engine-body="action=ask""#));
        assert!(html.contains(r#"<form data-engine-post="/api/player/sync" data-engine-body="action=offer" data-engine-stay"#));
        assert!(html.contains(r#"<input type="text" name="invite" placeholder="ABCD-…""#));
        assert!(html.contains(" data-sync-idle>"));
        assert!(!draw(&at(Stage::Inviting, Some(Side::Sender))).contains("data-sync-idle"));
        assert!(!html.contains(r#"role="dialog""#));
        assert!(!html.contains(r#"role="alert""#));
    }

    #[test]
    fn the_section_has_nothing_for_the_page_to_run() {
        let mut inviting = at(Stage::Inviting, Some(Side::Sender));
        inviting.invite = Some(invite());
        let mut offered = Dialog::default();
        offered.offered = Some(invite());
        let mut received = at(Stage::Done, Some(Side::Receiver));
        received.taken = Some(Taken { days_new_here: 2, days_new_there: 1, ..Taken::default() });
        for shown in [
            Dialog::default(),
            at(Stage::Asked, None),
            offered,
            inviting,
            at(Stage::Inviting, Some(Side::Sender)),
            at(Stage::Joining, Some(Side::Receiver)),
            at(Stage::Moving, Some(Side::Sender)),
            received,
            at(Stage::Done, Some(Side::Sender)),
        ] {
            let html = draw(&shown);
            for forbidden in ["onclick", "x-data", "x-model", "@click", "x-show", "<script", "<template", " id=", "href="] {
                assert!(!html.contains(forbidden), "{} at {:?}", forbidden, shown.stage);
            }
        }
    }

    #[test]
    fn a_code_that_could_not_be_read_is_said_and_stays_in_its_field_and_cannot_break_out_of_it() {
        let mut shown = Dialog::default();
        shown.typed = r#"a"><b>"#.to_string();
        shown.error = "That is <not> a sync code.".to_string();
        let html = draw(&shown);
        assert!(html.contains(r#"name="invite" value="a&quot;&gt;&lt;b&gt;" placeholder"#), "{}", html);
        assert!(html.contains(r#"role="alert">That is &lt;not&gt; a sync code.</div>"#));
    }

    #[test]
    fn the_sender_is_asked_in_a_dialog_what_syncing_will_do() {
        let html = draw(&at(Stage::Asked, None));
        assert!(html.contains(r#"role="dialog" aria-modal="true" aria-label="Sync a new device""#));
        assert!(html.contains("Syncing removes the game in progress on the other device"));
        assert!(html.contains("Your player record is merged"));
        assert!(html.contains(r#"data-engine-body="action=close" class="w-full bg-slate-400"#));
        assert!(html.contains(r#"data-engine-body="action=invite" class="w-full bg-kip-red"#));
    }

    #[test]
    fn the_invitation_is_shown_as_a_qr_code_of_its_address_and_as_a_code_to_type() {
        let mut shown = at(Stage::Inviting, Some(Side::Sender));
        assert!(draw(&shown).contains("Making a code…"), "until the room has its code");
        assert!(!draw(&shown).contains("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 "));

        shown.invite = Some(invite());
        let html = draw(&shown);
        assert!(html.contains(r#"role="img" aria-label="QR code for the other device to scan""#));
        assert!(html.contains(">ABCD-EFGHJ-KLMNP-QRSTU-VWXYZ</p>"));
        assert!(html.contains("Waiting for the other device…"));
        assert!(html.contains(r#"data-engine-body="action=close""#));
        // The address is in the picture only, never in the page as a link.
        assert!(!html.contains("kipukas.cards"));

        shown.peer_here = true;
        assert!(draw(&shown).contains("The other device is here. Waiting for its answer…"));
    }

    #[test]
    fn the_qr_code_is_the_one_for_the_address_with_a_quiet_border() {
        let address = invite().address();
        let code = QrCode::encode_text(&address, QrCodeEcc::Medium).unwrap();
        // 69 characters fit version 5 at this level: 37 squares a side.
        assert_eq!(code.size(), 37);
        let svg = qr_svg(&address).unwrap();
        assert!(svg.contains(r#"viewBox="0 0 45 45""#));
        // The top-left finder's first row: seven dark squares, four in from the edge.
        assert!(svg.contains(r#"<path d="M4,4h7v1h-7z"#), "{}", &svg[..300]);
        // As many dark squares in the path as in the code.
        let dark = (0..37).flat_map(|y| (0..37).map(move |x| (x, y))).filter(|(x, y)| code.get_module(*x, *y)).count();
        let drawn: usize = svg
            .split('M')
            .skip(1)
            .map(|run| run.split('h').nth(1).unwrap().split('v').next().unwrap().parse::<usize>().unwrap())
            .sum();
        assert_eq!(drawn, dark);
        // Nothing reaches into the border.
        assert!(!svg.contains("M3,") && !svg.contains(",3h"));
    }

    #[test]
    fn the_receiver_is_asked_in_a_dialog_and_told_what_it_would_lose() {
        let mut shown = Dialog::default();
        shown.offered = Some(invite());
        let html = draw(&shown);
        assert!(html.contains(r#"role="dialog" aria-modal="true" aria-label="Take the game from another device""#));
        assert!(html.contains("This device has no game in progress."));
        assert!(html.contains(r#"data-engine-body="action=receive&amp;invite=ABCD-EFGHJKLMNPQRSTUVWXYZ""#));
        assert!(html.contains(r#"data-engine-body="action=decline&amp;invite=ABCD-EFGHJKLMNPQRSTUVWXYZ""#));

        let here = Here {
            game: Progress { damaged: 2, timers: 1, affinity: Some("Brutal".to_string()) },
            ..Here::default()
        };
        let html = render(&shown, &here);
        assert!(html.contains("This device's game will be removed: 2 cards with damage, 1 timer, the Brutal affinity."));
        let here = Here { game: Progress { damaged: 1, timers: 3, affinity: None }, ..Here::default() };
        assert!(render(&shown, &here).contains("removed: 1 card with damage, 3 timers."));

        // The room it is in is part of the game it would lose.
        let here = Here { room: Some("HERE".to_string()), ..here };
        assert!(render(&shown, &here).contains("removed: 1 card with damage, 3 timers. It will leave room HERE."));
        let here = Here { room: Some("HERE".to_string()), ..Here::default() };
        assert!(render(&shown, &here).contains("This device has no game in progress. It will leave room HERE."));
    }

    #[test]
    fn a_sender_in_a_room_is_told_its_place_goes_with_the_game() {
        let asked = at(Stage::Asked, None);
        assert!(!draw(&asked).contains("room"));
        let here = Here { room: Some("WXYZ".to_string()), seat: Some("WXYZ".to_string()), ..Here::default() };
        assert!(render(&asked, &here).contains(
            "Your place in room WXYZ goes with the game: the other device carries on there, and this device leaves the room."
        ));
        // Another tab holds the room: this one has no place to send.
        let here = Here { room: Some("WXYZ".to_string()), ..Here::default() };
        assert!(!render(&asked, &here).contains("room"));
    }

    #[test]
    fn both_are_told_where_the_place_in_the_room_went() {
        let mut shown = at(Stage::Done, Some(Side::Receiver));
        shown.taken = Some(Taken::default());
        assert!(!draw(&shown).contains("room"));
        shown.room = Some("WXYZ".to_string());
        assert!(draw(&shown).contains("It has taken your place in room WXYZ."));

        let mut shown = at(Stage::Done, Some(Side::Sender));
        assert!(!draw(&shown).contains("room"));
        shown.room = Some("WXYZ".to_string());
        let html = draw(&shown);
        assert!(html.contains("Your record is the same on both."));
        assert!(html.contains("It has your place in room WXYZ, and this device has left the room."));
        // The other device was seen to take the place, and not heard from.
        shown.unheard = true;
        let html = draw(&shown);
        assert!(html.contains("✓ The other device now has this game."));
        assert!(html.contains("It was not heard from after it took the game"));
        assert!(!html.contains("Your record is the same on both."));
    }

    #[test]
    fn a_move_on_its_way_says_so_and_can_be_called_off_only_before_the_game_moves() {
        let html = draw(&at(Stage::Joining, Some(Side::Receiver)));
        assert!(html.contains("Finding the other device…") && html.contains("action=close"));
        let html = draw(&at(Stage::Moving, Some(Side::Receiver)));
        assert!(html.contains("Moving the game…") && !html.contains("action=close"));
    }

    #[test]
    fn the_receiver_is_told_what_came_of_it() {
        let mut shown = at(Stage::Done, Some(Side::Receiver));
        shown.confirmed = true;
        for (here, there, said) in [
            (0, 0, "Your record was already the same on both."),
            (3, 0, "Your record gained 3 days of play from the other device."),
            (0, 1, "This device added 1 day of play to your record."),
            (1, 2, "Your record gained 1 day of play from the other device, and this device added 2 days of play."),
        ] {
            shown.taken = Some(Taken { days_new_here: here, days_new_there: there, ..Taken::default() });
            let html = draw(&shown);
            assert!(html.contains("✓ This device now has the game from your other device."));
            assert!(html.contains(said), "{}", html);
            assert!(!html.contains("went away"));
            assert!(html.contains(r#"data-engine-body="action=close""#));
        }
        // The other device was not heard from after this one sent its part.
        shown.confirmed = false;
        assert!(draw(&shown).contains("went away before it said it had this device's part of the record"));
        shown.taken = Some(Taken { days_new_here: 3, ..Taken::default() });
        assert!(!draw(&shown).contains("went away"), "it had nothing to send");
    }

    #[test]
    fn the_sender_is_told_the_other_device_has_the_game() {
        let mut shown = at(Stage::Done, Some(Side::Sender));
        let html = draw(&shown);
        assert!(html.contains("✓ The other device now has this game."));
        assert!(html.contains("Your record is the same on both."));
        shown.days_brought = 2;
        assert!(draw(&shown).contains("The other device added 2 days of play to your record."));
    }

    #[test]
    fn the_buttons_and_the_form_reach_the_move() {
        fresh();
        assert_eq!(handle_sync_post("action=ask"), "");
        assert!(handle_sync_get("").contains(r#"aria-label="Sync a new device""#));
        handle_sync_post("action=close");
        assert!(!handle_sync_get("").contains(r#"role="dialog""#));

        handle_sync_post("action=offer&invite=nonsense");
        assert!(handle_sync_get("").contains("That is not a sync code."));
        handle_sync_post("action=offer&invite=abcd-efghj-klmnp-qrstu-vwxyz");
        assert!(handle_sync_get("").contains(r#"aria-label="Take the game from another device""#));
        handle_sync_post("action=decline&invite=ABCD-EFGHJKLMNPQRSTUVWXYZ");
        assert!(!handle_sync_get("").contains(r#"role="dialog""#));
        fresh();
    }

    #[test]
    fn an_invitation_in_the_pages_address_is_asked_about_and_drawing_it_starts_nothing() {
        fresh();
        // As the page sends what follows its #, encoded.
        let html = handle_sync_get("?link=sync%3DABCD-EFGHJKLMNPQRSTUVWXYZ");
        assert!(html.contains(r#"aria-label="Take the game from another device""#));
        assert!(crate::wire::take().is_empty(), "nothing is connected by drawing");
        assert_eq!(device_sync::dialog(), Dialog::default());

        // Said no to, it is not asked about again.
        handle_sync_post("action=decline&invite=ABCD-EFGHJKLMNPQRSTUVWXYZ");
        assert!(!handle_sync_get("?link=sync%3DABCD-EFGHJKLMNPQRSTUVWXYZ").contains(r#"role="dialog""#));

        // Said yes to, the engine goes to the sender's room.
        fresh();
        handle_sync_post("action=receive&invite=ABCD-EFGHJKLMNPQRSTUVWXYZ");
        assert!(handle_sync_get("?link=sync%3DABCD-EFGHJKLMNPQRSTUVWXYZ").contains("Finding the other device…"));
        assert!(crate::wire::take().iter().any(|op| op["open"].is_string()));
        fresh();
    }
}
