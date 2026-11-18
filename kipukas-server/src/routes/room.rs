//! `/api/room/*` routes — multiplayer room management and fists combat.
//!
//! Shared state (timers, fists combat) lives in the room document, which the
//! two players sync through the WebSocket relay; see `game::crdt` and `game::fists`.
//! Local game state (damage, turns) remains per-user.
//!
//! A route here that changes what a view elsewhere on the page shows asks
//! for that view to be drawn again (`page::redraw`): whose timers the turn
//! tracker lists depends on being in a room, for one.

use crate::catalog::CARDS;
use crate::clock;
use crate::game::crdt;
use crate::game::damage;
use crate::game::fists;
use crate::game::player_doc;
use crate::game::room::{self, CombatRole, FistsChoice, FistsSubmission};
use crate::game::state::new_alarm_id;
use crate::page;
use crate::routes::util::{escape_html, get_param, parse_form_body, parse_query};
use crate::typing;

fn find_card(slug: &str) -> Option<&'static crate::catalog::Card> {
    CARDS.iter().find(|c| c.slug == slug)
}

// ── GET /api/room/status ───────────────────────────────────────────

pub fn handle_status_get(_query: &str) -> String {
    room::with_room(|r| {
        if r.connected {
            render_connected_status(r)
        } else if !r.room_code.is_empty() {
            render_waiting_status(r)
        } else {
            render_disconnected_status()
        }
    })
}

fn render_disconnected_status() -> String {
    let mut h = String::with_capacity(1024);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna">"#);
    h.push_str(r#"<p class="text-lg font-bold mb-3">Multiplayer</p>"#);
    // ── Create section ──
    h.push_str(r#"<div class="mb-3">"#);
    h.push_str(r#"<label class="block text-sm mb-1">Room Name</label>"#);
    h.push_str(r#"<input type="text" id="room-name-input" placeholder="My Room" class="w-full border rounded px-2 py-1 text-sm text-kip-drk-sienna border-kip-drk-sienna focus:border-kip-red focus:ring-kip-red">"#);
    h.push_str(r#"</div>"#);
    h.push_str(r#"<button onclick="kipukasMultiplayer.createRoom()" class="w-full bg-kip-red hover:bg-kip-drk-sienna text-amber-50 font-bold py-2 px-4 rounded mb-2 text-sm">Create Room</button>"#);
    // ── Join section ──
    h.push_str(r#"<div class="border-t border-slate-300 my-3"></div>"#);
    h.push_str(r#"<label class="block text-sm mb-1">Room Code</label>"#);
    h.push_str(r#"<input type="text" id="room-code-input" placeholder="ABCD" maxlength="4" class="w-full border rounded px-2 py-1 text-sm uppercase text-kip-drk-sienna border-kip-drk-sienna focus:border-kip-red focus:ring-kip-red mb-2">"#);
    h.push_str(r#"<label class="block text-sm mb-1">Room Name</label>"#);
    h.push_str(r#"<input type="text" id="room-name-join-input" placeholder="My Room" class="w-full border rounded px-2 py-1 text-sm text-kip-drk-sienna border-kip-drk-sienna focus:border-kip-red focus:ring-kip-red">"#);
    h.push_str(r#"<button onclick="kipukasMultiplayer.joinRoom()" class="w-full bg-emerald-600 hover:bg-emerald-700 text-amber-50 font-bold py-2 px-4 rounded mt-2 text-sm">Join Room</button>"#);
    h.push_str(r#"</div>"#);
    h
}

/// Render the "waiting for peer" status — room created, code visible, not yet connected.
fn render_waiting_status(r: &room::RoomState) -> String {
    let mut h = String::with_capacity(512);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna">"#);
    h.push_str(r#"<p class="text-lg font-bold mb-1">Multiplayer</p>"#);
    h.push_str(r#"<p class="text-sm text-amber-600 mb-1">&#x23F3; Waiting for peer…</p>"#);
    if !r.room_name.is_empty() {
        h.push_str(&format!(
            r#"<p class="text-sm mb-1">Room: <strong>{}</strong></p>"#,
            r.room_name
        ));
    }
    h.push_str(&format!(
        r#"<p class="text-sm mb-3 font-mono tracking-wider">Code: <strong>{}</strong></p>"#,
        r.room_code
    ));
    h.push_str(r#"<button onclick="kipukasMultiplayer.disconnect()" class="w-full bg-slate-400 hover:bg-slate-500 text-amber-50 font-bold py-2 px-4 rounded text-sm">Cancel</button>"#);
    h.push_str(r#"</div>"#);
    h
}

fn render_connected_status(r: &room::RoomState) -> String {
    let mut h = String::with_capacity(512);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna">"#);
    h.push_str(r#"<p class="text-lg font-bold mb-1">Multiplayer</p>"#);
    h.push_str(&format!(
        r#"<p class="text-sm text-emerald-600 mb-1">&#x2713; Connected</p>"#
    ));
    if !r.room_name.is_empty() {
        h.push_str(&format!(
            r#"<p class="text-sm mb-1">Room: <strong>{}</strong></p>"#,
            r.room_name
        ));
    }
    h.push_str(&format!(
        r#"<p class="text-sm mb-3 font-mono tracking-wider">Code: <strong>{}</strong></p>"#,
        r.room_code
    ));
    h.push_str(r#"<button onclick="kipukasMultiplayer.disconnect()" class="w-full bg-slate-400 hover:bg-slate-500 text-amber-50 font-bold py-2 px-4 rounded text-sm">Disconnect</button>"#);
    h.push_str(r#"</div>"#);
    h
}

// ── POST /api/room/create ──────────────────────────────────────────

pub fn handle_create_post(body: &str) -> String {
    let params = parse_form_body(body);
    let code = get_param(&params, "code").unwrap_or("");
    let name = get_param(&params, "name").unwrap_or("");
    let skip_seed = get_param(&params, "skip_seed").unwrap_or("false") == "true";
    let player = get_param(&params, "player").unwrap_or("");

    room::with_room_mut(|r| {
        r.room_code = code.to_uppercase();
        r.room_name = name.to_string();
        r.connected = false;
        r.player_id = player_id_or_new(player);
        r.peer_id.clear();
        r.fists.reset();
        r.choice = FistsChoice::default();
    });

    // Initialize yrs CRDT Doc for this multiplayer session.
    // On fresh room creation, seed with pre-existing local alarms so
    // they become shared. On reconnect (skip_seed=true), the CRDT Doc
    // will be restored from sessionStorage instead — seeding here would
    // introduce stale alarms with new yrs client IDs that survive the
    // restore merge, causing timers to reappear or duplicate.
    crdt::init_doc();
    if !skip_seed {
        crdt::seed_from_local();
    }
    // The turn tracker now lists the room's timers.
    page::redraw("/api/game/turns");

    r#"<span class="text-emerald-600 text-sm">Room created. Waiting for peer...</span>"#
        .to_string()
}

// ── POST /api/room/join ────────────────────────────────────────────

pub fn handle_join_post(body: &str) -> String {
    let params = parse_form_body(body);
    let code = get_param(&params, "code").unwrap_or("");
    let name = get_param(&params, "name").unwrap_or("");
    let skip_seed = get_param(&params, "skip_seed").unwrap_or("false") == "true";
    let player = get_param(&params, "player").unwrap_or("");

    room::with_room_mut(|r| {
        r.room_code = code.to_uppercase();
        if !name.is_empty() {
            r.room_name = name.to_string();
        }
        r.connected = false;
        r.player_id = player_id_or_new(player);
        r.peer_id.clear();
        r.fists.reset();
        r.choice = FistsChoice::default();
    });

    // Initialize yrs CRDT Doc for this multiplayer session.
    // See handle_create_post for skip_seed rationale.
    crdt::init_doc();
    if !skip_seed {
        crdt::seed_from_local();
    }
    // The turn tracker now lists the room's timers.
    page::redraw("/api/game/turns");

    room::with_room(|r| render_waiting_status(r))
}

// ── POST /api/room/resume ─────────────────────────────────────────

/// Put this player back in a room after a page load.
///
/// Body: `code=<code>&name=<name>&player=<this player's id>&peer=<opponent's
/// id, if known>&state=<url-safe base64 room document>`
///
/// Unlike create/join this is not the start of a session: it never seeds the
/// room document from local timers and never resets combat. It restores the
/// room document exactly as it was persisted, so the first render on the new
/// page already shows shared state — timers and the round in progress. The
/// peer is marked absent until the relay reports `peer_joined`.
pub fn handle_resume_post(body: &str) -> String {
    let params = parse_form_body(body);
    let code = get_param(&params, "code").unwrap_or("");
    let name = get_param(&params, "name").unwrap_or("");
    let player = get_param(&params, "player").unwrap_or("");
    let peer = get_param(&params, "peer").unwrap_or("");
    let state = get_param(&params, "state").unwrap_or("");

    if code.is_empty() {
        return r#"{"error":"missing room code"}"#.to_string();
    }

    room::with_room_mut(|r| {
        r.room_code = code.to_uppercase();
        r.room_name = name.to_string();
        r.connected = false;
        r.player_id = player_id_or_new(player);
        r.peer_id = if fists::is_valid_player_id(peer) {
            peer.to_string()
        } else {
            String::new()
        };
    });

    crdt::init_doc();
    let restored = crdt::restore_from_state(state);

    // A finished round may be waiting to be applied (the page was left before
    // it could be). The opponent learns of it on the next sync handshake.
    settle_fists();
    sync_fists_view();

    match restored {
        Ok(()) => "ok".to_string(),
        Err(e) => format!(r#"{{"error":"{}"}}"#, e),
    }
}

/// This player's id for the room. The page always supplies one; a random one
/// is used if it is missing or malformed, so two players can never share an id.
fn player_id_or_new(player: &str) -> String {
    if fists::is_valid_player_id(player) {
        player.to_string()
    } else {
        new_alarm_id()
    }
}

// ── POST /api/room/peer ───────────────────────────────────────────

/// Record the opponent's id (from their hello). Only that player's entries
/// then count as the opponent's submissions.
/// Body: `id=<opponent's id>`.
pub fn handle_peer_post(body: &str) -> String {
    let params = parse_form_body(body);
    let id = get_param(&params, "id").unwrap_or("");
    if fists::is_valid_player_id(id) {
        room::with_room_mut(|r| r.peer_id = id.to_string());
    }
    sync_fists_view();
    // Whose entries count as the opponent's has changed, and with it what
    // the fists dialog shows.
    page::redraw("/api/room/fists");
    "ok".to_string()
}

// ── POST /api/room/connected ──────────────────────────────────────

pub fn handle_connected_post(body: &str) -> String {
    let params = parse_form_body(body);
    let code = get_param(&params, "code").unwrap_or("");
    let name = get_param(&params, "name").unwrap_or("");

    room::with_room_mut(|r| {
        r.connected = true;
        if !code.is_empty() {
            r.room_code = code.to_uppercase();
        }
        if !name.is_empty() {
            r.room_name = name.to_string();
        }
    });
    page::redraw("/api/game/turns");

    room::with_room(|r| render_connected_status(r))
}

// ── POST /api/room/disconnect ──────────────────────────────────────

pub fn handle_disconnect_post(_body: &str) -> String {
    // Export shared CRDT alarms back to local PLAYER_DOC before reset.
    // The PERSIST_STATE message (triggered by the worker for /api/room/*
    // POSTs) ensures this change is saved to localStorage, preventing
    // stale timer data from ghosting back on the next page load.
    crdt::export_to_local();
    room::reset_room();
    crdt::reset_doc();
    // The turn tracker goes back to listing the player's own timers.
    page::redraw("/api/game/turns");
    render_disconnected_status()
}

// ── POST /api/room/peer_left ───────────────────────────────────────

/// Peer disconnected (e.g. navigated away). Keep room data so we can reconnect.
pub fn handle_peer_left_post(_body: &str) -> String {
    room::with_room_mut(|r| {
        r.connected = false;
        // Keep room_code and room_name so auto-reconnect works
    });

    room::with_room(|r| render_waiting_status(r))
}

// ── Fists combat ───────────────────────────────────────────────────
//
// Combat state lives in the room document (see `game::fists`). The fists
// dialog is an engine-owned view of it (Pattern 13 in CONTRIBUTING.md): the
// GET route draws this player's side of the round, and every control in the
// dialog posts to the same route.
//
// A post that changes the room document queues the change for the opponent
// (`crdt::queue_for_peer`). When the opponent's change arrives, the engine
// asks for the dialog to be drawn again (`handle_yrs_apply_post`). The page
// script takes no part in either.

fn player_ids() -> (String, String) {
    room::with_room(|r| (r.player_id.clone(), r.peer_id.clone()))
}

/// Recompute this player's view of combat from the room document, and cache
/// the round's submissions in RoomState for the result renderers.
fn sync_fists_view() -> fists::View {
    let (me, peer) = player_ids();
    let view = crdt::with_doc(|doc| fists::view(doc, &me, &peer));
    room::with_room_mut(|r| r.fists = view.combat.clone());
    view
}

/// Apply every finished round this player has not applied yet: if this
/// player defended and lost, mark the damage on their card.
///
/// Each round is applied once. The `settled` flag is written to the room
/// document, so a reload, a reconnect, or the opponent reporting the same
/// result again cannot apply it a second time.
///
/// Returns whether the room document changed (so the caller knows there is
/// an update for the opponent).
fn settle_fists() -> bool {
    let (me, peer) = player_ids();
    let pending = crdt::with_doc(|doc| fists::unsettled(doc, &me, &peer));
    let changed = !pending.is_empty();
    for p in pending {
        let marked = apply_outcome(&p);
        if marked {
            // The card's panel, which shows its damage, is a view of its
            // own elsewhere on the page.
            page::redraw("/api/card/panel");
        }
        crdt::with_doc(|doc| fists::mark_settled(doc, p.round, &me, marked));
    }
    changed
}

/// Apply one finished round to this player's own card. Returns whether
/// anything was marked.
fn apply_outcome(p: &fists::Pending) -> bool {
    // Only the defender's card takes damage, and only if the attacker won.
    // A disputed result applies nothing.
    if p.outcome != fists::Outcome::AttackerWon || p.mine.role() != Some(CombatRole::Defending) {
        return false;
    }
    let card = p.mine.card();
    if card.is_empty() {
        return false;
    }
    if p.final_blows {
        // Final Blows: the defender lost, so the card is wasted.
        damage::toggle_wasted(card);
        true
    } else {
        auto_mark_damage(card, p.mine.keal_idx())
    }
}

// ── GET /api/room/fists ────────────────────────────────────────────

/// The fists dialog's contents for the page's card: `?card=<slug>`.
pub fn handle_fists_get(query: &str) -> String {
    let params = parse_query(query);
    let slug = get_param(&params, "card").unwrap_or("");

    // Being in a room is enough. The opponent may be away for a moment (a
    // page change, a locked phone): a choice locked in meanwhile is in the
    // room document and reaches them when they are back.
    if !room::in_room() {
        return r#"<div class="p-4 text-kip-drk-sienna"><p class="text-sm text-kip-red">Not connected to a room. Use the fields above to create or join one.</p></div>"#.to_string();
    }

    render_fists_view(&sync_fists_view(), slug)
}

/// The dialog contents for the phase this player is in.
fn render_fists_view(view: &fists::View, slug: &str) -> String {
    match view.phase() {
        fists::Phase::Outcome => render_fists_outcome(view),
        fists::Phase::Result => render_fists_result(),
        fists::Phase::Waiting => {
            if view.combat.local_final_blows.is_some() {
                render_final_blows_waiting()
            } else {
                render_fists_waiting()
            }
        }
        fists::Phase::Form => render_fists_form(slug, view),
    }
}

/// This player's picks for a card: those noted for it, or none yet.
fn choice_for(slug: &str) -> FistsChoice {
    room::with_room(|r| {
        if r.choice.card == slug {
            r.choice.clone()
        } else {
            FistsChoice {
                card: slug.to_string(),
                ..FistsChoice::default()
            }
        }
    })
}

/// Forget this player's picks: the next form starts with nothing chosen.
fn clear_choice() {
    room::with_room_mut(|r| r.choice = FistsChoice::default());
}

/// The message area the forms end with: why locking in was refused, and
/// whether the opponent has locked in and is waiting.
fn fists_message(view: &fists::View, refusal: &str) -> String {
    let mut h = String::from(r#"<div class="mt-2 text-center">"#);
    if !refusal.is_empty() {
        h.push_str(&format!(r#"<p class="text-sm text-kip-red">{}</p>"#, refusal));
    }
    if view.remote_ready() {
        h.push_str(r#"<p class="text-sm text-emerald-600">&#x2713; Opponent has locked in their choice!</p>"#);
    }
    h.push_str("</div>");
    h
}

/// For each of a card's keal means, in order, whether every damage slot of
/// it is marked: such a keal means can no longer be fought with.
fn exhausted_keal_means(card: &crate::catalog::Card) -> Vec<bool> {
    let mut slot_start: u8 = 1;
    card.keal_means
        .iter()
        .map(|km| {
            let slots = slot_start..slot_start + km.count;
            slot_start += km.count;
            km.count > 0 && slots.into_iter().all(|s| player_doc::get_slot(card.slug, s))
        })
        .collect()
}

/// Whether a card can still fight with its keal means number `idx` (1-based).
fn keal_means_is_open(card: &crate::catalog::Card, idx: u8) -> bool {
    idx >= 1 && exhausted_keal_means(card).get(idx as usize - 1) == Some(&false)
}

/// Check if ALL keal means groups on a card are fully exhausted (every damage slot checked).
fn all_keal_means_exhausted(slug: &str) -> bool {
    let card = match find_card(slug) {
        Some(c) => c,
        None => return false,
    };
    if card.keal_means.is_empty() {
        return false;
    }
    let total: u8 = card.keal_means.iter().map(|km| km.count).sum();
    if total == 0 {
        return false;
    }
    for slot in 1..=total {
        if !player_doc::get_slot(slug, slot) {
            return false;
        }
    }
    true
}

/// Render Final Blows section for a card whose keal means are all exhausted.
/// Shows local card motivation + D20 roll instruction + button to send to opponent.
fn render_final_blows(card: &crate::catalog::Card, message: &str) -> String {
    let mut h = String::with_capacity(1024);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna">"#);
    h.push_str(&format!(
        r#"<p class="text-lg font-bold mb-2">Fists: {}</p>"#,
        card.title
    ));

    h.push_str(r#"<div class="bg-slate-50 border border-slate-300 rounded p-3 mb-3">"#);
    h.push_str(r#"<p class="text-sm font-bold text-center mb-2">&#x1F525; Final Blows</p>"#);
    h.push_str(r#"<p class="text-xs text-center mb-2 text-amber-600">All keal means are exhausted. This is the final combat.</p>"#);

    // Local card motivation
    h.push_str(r#"<div class="mb-2 text-xs">"#);
    h.push_str(r#"<p class="font-bold">Your Motivation</p>"#);
    if let Some(mot) = card.motivation {
        h.push_str(&format!(r#"<p>{}</p>"#, mot));
    } else {
        h.push_str(r#"<p class="text-slate-400">None</p>"#);
    }
    h.push_str(r#"</div>"#);

    // D20 roll instruction
    h.push_str(r#"<div class="bg-amber-100 rounded p-2 text-center">"#);
    h.push_str(r#"<p class="text-sm font-bold mb-1">Both Players Roll</p>"#);
    h.push_str(r#"<p class="text-2xl font-bold text-kip-drk-sienna">D20</p>"#);
    h.push_str(r#"<p class="text-xs text-slate-500 mt-1">Compare motivation modifiers to determine the final winner</p>"#);
    h.push_str(r#"</div>"#);

    h.push_str(r#"</div>"#); // close final-blows box

    // State Final Blows button
    h.push_str(&format!(
        r#"<button type="button" data-engine-post="/api/room/fists" data-engine-body="action=final&card={}" class="w-full bg-kip-red hover:bg-kip-drk-sienna text-amber-50 font-bold py-2 px-4 rounded text-sm">State Final Blows to Opponent</button>"#,
        card.slug
    ));

    h.push_str(message);

    h.push_str(r#"</div>"#);
    h
}

/// Render the "waiting for opponent" state when Final Blows has been stated.
fn render_final_blows_waiting() -> String {
    let mut h = String::with_capacity(512);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna text-center">"#);
    h.push_str(r#"<p class="text-lg font-bold mb-2">Final Blows Stated!</p>"#);
    h.push_str(r#"<div class="animate-pulse text-kip-red text-2xl mb-2">&#9876;</div>"#);
    h.push_str(r#"<p class="text-sm">Waiting for opponent to submit...</p>"#);
    h.push_str(CANCEL_ROUND);
    h.push_str(r#"</div>"#);
    h
}

/// What both waiting views end with: a way to call the round off. (Closing
/// the dialog does not: the round carries on.) Nothing here asks whether the
/// opponent has submitted yet: when they do, their change arrives and the
/// dialog is drawn again.
const CANCEL_ROUND: &str = r#"<button type="button" data-engine-post="/api/room/fists" data-engine-body="action=reset" class="mt-4 text-xs underline text-slate-500 hover:text-kip-red cursor-pointer">Cancel round</button>"#;

/// The form a round starts with: a role and a keal means to pick, and a
/// button to lock them in.
///
/// A pick is posted to the engine like any other tap. The engine notes it
/// (`choose`) and the form is drawn again with it checked, so the form can
/// be drawn again whenever something changes, such as the opponent locking
/// in, and the picks are still there.
fn render_fists_form(slug: &str, view: &fists::View) -> String {
    let card = match find_card(slug) {
        Some(c) => c,
        None if slug.is_empty() => return render_fists_no_card(),
        None => {
            return format!(
                r#"<div class="p-4"><span class="text-kip-red">Card not found: {}</span></div>"#,
                escape_html(slug)
            );
        }
    };

    if card.keal_means.is_empty() {
        return r#"<div class="p-4 text-kip-drk-sienna"><p class="text-sm">This card has no keal means for combat.</p></div>"#.to_string();
    }

    let choice = choice_for(card.slug);
    let message = fists_message(view, &choice.refusal);

    // If all keal means are exhausted, show Final Blows instead of the normal form
    if all_keal_means_exhausted(slug) {
        return render_final_blows(card, &message);
    }

    let mut h = String::with_capacity(2048);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna">"#);
    h.push_str(&format!(
        r#"<p class="text-lg font-bold mb-2">Fists: {}</p>"#,
        card.title
    ));

    // Role selector
    h.push_str(r#"<p class="text-sm mb-2 font-bold">Your Role:</p>"#);
    h.push_str(r#"<div class="flex gap-2 mb-3">"#);
    for (value, label, role) in [
        ("attacking", "Attacking", CombatRole::Attacking),
        ("defending", "Defending", CombatRole::Defending),
    ] {
        h.push_str(&format!(
            r#"<label class="flex-1 text-center"><input type="radio" name="fists-role" value="{value}" class="mr-1 text-kip-red focus:ring-kip-red" data-engine-post="/api/room/fists" data-engine-body="action=choose&card={slug}&role={value}"{checked}>{label}</label>"#,
            slug = card.slug,
            checked = if choice.role == Some(role) { " checked" } else { "" },
        ));
    }
    h.push_str(r#"</div>"#);

    // Keal means selector — disable groups where all damage slots are checked
    h.push_str(r#"<p class="text-sm mb-2 font-bold">Select Keal Means:</p>"#);

    let exhausted = exhausted_keal_means(card);
    for (place, km) in card.keal_means.iter().enumerate() {
        let idx = place as u8 + 1;
        let genetics_str = km.genetics.join("-");
        if exhausted[place] {
            // Disabled: all checkboxes used up
            h.push_str(&format!(
                r#"<label class="block mb-1 text-sm opacity-40 line-through"><input type="radio" name="fists-keal" value="{}" class="mr-1" disabled><span class="font-bold">{}</span> <span class="text-xs">({})</span></label>"#,
                idx, km.name, genetics_str
            ));
        } else {
            h.push_str(&format!(
                r#"<label class="block mb-1 text-sm"><input type="radio" name="fists-keal" value="{idx}" class="mr-1 text-kip-red focus:ring-kip-red" data-engine-post="/api/room/fists" data-engine-body="action=choose&card={slug}&keal={idx}"{checked}><span class="text-kip-red font-bold">{name}</span> <span class="text-xs">({genetics_str})</span></label>"#,
                slug = card.slug,
                checked = if choice.keal_idx == Some(idx) { " checked" } else { "" },
                name = km.name,
            ));
        }
    }

    // Lock In button
    h.push_str(&format!(
        r#"<button type="button" data-engine-post="/api/room/fists" data-engine-body="action=lock&card={}" class="w-full bg-kip-red hover:bg-kip-drk-sienna text-amber-50 font-bold py-2 px-4 rounded mt-3 text-sm">Lock In Choice</button>"#,
        card.slug
    ));

    h.push_str(&message);

    h.push_str(r#"</div>"#);
    h
}

fn render_fists_no_card() -> String {
    r#"<div class="p-4 text-kip-drk-sienna"><p class="text-sm">Navigate to a Character or Species card page, then open the Fists tool to select your keal means for combat.</p></div>"#.to_string()
}

fn render_fists_waiting() -> String {
    let mut h = String::with_capacity(512);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna text-center">"#);
    h.push_str(r#"<p class="text-lg font-bold mb-2">Choice Locked In!</p>"#);
    h.push_str(r#"<div class="animate-pulse text-kip-red text-2xl mb-2">&#9876;</div>"#);
    h.push_str(r#"<p class="text-sm">Waiting for opponent...</p>"#);
    h.push_str(CANCEL_ROUND);
    h.push_str(r#"</div>"#);
    h
}

/// Render a friendly error when both players chose the same combat role.
fn render_role_conflict(role_name: &str) -> String {
    let mut h = String::with_capacity(512);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna text-center">"#);
    h.push_str(r#"<p class="text-2xl mb-2">&#x26A0;</p>"#);
    h.push_str(&format!(
        r#"<p class="text-lg font-bold mb-2 text-kip-red">Both players chose {}!</p>"#,
        role_name
    ));
    h.push_str(r#"<p class="text-sm mb-4">One player must be <strong>Attacking</strong> and the other <strong>Defending</strong>. Please try again with different roles.</p>"#);
    h.push_str(r#"<button type="button" data-engine-post="/api/room/fists" data-engine-body="action=reset" class="w-full bg-kip-red hover:bg-kip-drk-sienna text-amber-50 font-bold py-2 px-4 rounded text-sm">Try Again</button>"#);
    h.push_str(r#"</div>"#);
    h
}

// ── POST /api/room/fists ───────────────────────────────────────────

/// A tap in the fists dialog. Body:
///
///   - action=choose&card=<slug>&role=attacking|defending   pick a role
///   - action=choose&card=<slug>&keal=<n>                   pick a keal means
///   - action=lock&card=<slug>                              lock the picks in
///   - action=final&card=<slug>                             state Final Blows
///   - action=outcome&won=yes|no                            answer "Did you win?"
///   - action=reset          New Round, Try Again, Cancel round
///   - action=close          the dialog was closed
///
/// Returns nothing: the dialog is drawn again from the GET route.
pub fn handle_fists_post(body: &str) -> String {
    let params = parse_form_body(body);
    if room::in_room() {
        let card = get_param(&params, "card").unwrap_or("");
        match get_param(&params, "action").unwrap_or("") {
            "choose" => choose(card, get_param(&params, "role"), get_param(&params, "keal")),
            "lock" => lock_in(card),
            "final" => state_final_blows(card),
            "outcome" => answer_did_you_win(get_param(&params, "won").unwrap_or("")),
            "reset" => start_new_round(),
            "close" => close_dialog(),
            _ => {}
        }
    }
    String::new()
}

/// Note a pick made in the form. Nothing is shared: the opponent learns of a
/// choice when it is locked in.
fn choose(slug: &str, role: Option<&str>, keal: Option<&str>) {
    let Some(card) = find_card(slug) else {
        return;
    };
    let mut choice = choice_for(card.slug);
    choice.refusal.clear();
    match role {
        Some("attacking") => choice.role = Some(CombatRole::Attacking),
        Some("defending") => choice.role = Some(CombatRole::Defending),
        _ => {}
    }
    if let Some(idx) = keal.and_then(|k| k.parse::<u8>().ok()) {
        if keal_means_is_open(card, idx) {
            choice.keal_idx = Some(idx);
        }
    }
    room::with_room_mut(|r| r.choice = choice);
}

/// A Character or Species card used in combat grows more loyal, once a day.
fn count_loyalty(card: &crate::catalog::Card) {
    if card.layout == "Character" || card.layout == "Species" {
        let plays = || player_doc::get_loyalty(card.slug).map(|(plays, _)| plays);
        let before = plays();
        let _ = player_doc::increment_loyalty(card.slug, &clock::today());
        if plays() != before {
            // The card's panel shows its loyalty.
            page::redraw("/api/card/panel");
        }
    }
}

/// Lock in this player's picks for the round, or say what is still to pick.
fn lock_in(slug: &str) {
    let Some(card) = find_card(slug) else {
        return;
    };
    // Only the form locks in: a second tap that was on its way when the
    // first changed the screen asks for nothing.
    if sync_fists_view().phase() != fists::Phase::Form {
        return;
    }

    let mut choice = choice_for(card.slug);
    let keal_idx = choice.keal_idx.filter(|idx| keal_means_is_open(card, *idx));
    let (Some(role), Some(keal_idx)) = (choice.role, keal_idx) else {
        choice.refusal = if choice.role.is_none() {
            "Please select Attacking or Defending."
        } else {
            "Please select a Keal Means."
        }
        .to_string();
        room::with_room_mut(|r| r.choice = choice);
        return;
    };

    count_loyalty(card);

    let (me, peer) = player_ids();
    let submission = FistsSubmission {
        role,
        card: card.slug.to_string(),
        keal_idx,
    };
    crdt::queue_for_peer(crdt::capture_update(|| {
        crdt::with_doc(|doc| fists::submit(doc, &me, &peer, submission));
        true
    }));
    clear_choice();
    sync_fists_view();
}

/// State Final Blows for a card whose keal means are all exhausted.
fn state_final_blows(slug: &str) {
    let Some(card) = find_card(slug) else {
        return;
    };
    if sync_fists_view().phase() != fists::Phase::Form || !all_keal_means_exhausted(slug) {
        return;
    }

    count_loyalty(card);

    let (me, peer) = player_ids();
    let submission = room::FinalBlowsSubmission {
        card: card.slug.to_string(),
    };
    crdt::queue_for_peer(crdt::capture_update(|| {
        crdt::with_doc(|doc| fists::final_blows(doc, &me, &peer, submission));
        true
    }));
    clear_choice();
    sync_fists_view();
}

/// Start a new round for both players: "New Round", "Try Again" and
/// "Cancel round".
fn start_new_round() {
    let (me, _) = player_ids();
    crdt::queue_for_peer(crdt::capture_update(|| {
        crdt::with_doc(|doc| fists::new_round(doc, &me))
    }));
    clear_choice();
    sync_fists_view();
}

/// The fists dialog was closed. A round in progress carries on untouched; a
/// finished round gives way to a new one so reopening shows a fresh form.
/// Picks that were not locked in are dropped.
fn close_dialog() {
    let (me, peer) = player_ids();
    crdt::queue_for_peer(crdt::capture_update(|| {
        crdt::with_doc(|doc| fists::close(doc, &me, &peer))
    }));
    clear_choice();
    sync_fists_view();
}

/// This player's answer to "Did you win?": `yes` or `no`.
///
/// The answer is recorded in the room document as `attacker_won`, which is
/// how the opponent learns of it. This player's side of the round is then
/// applied: if the attacker won and this player defended, the next unchecked
/// slot of the keal means used is marked on their card (see `settle_fists`).
fn answer_did_you_win(won: &str) {
    let view = sync_fists_view();
    // For Final Blows (no regular submission) the Final Blows card defends.
    let Some(local_role) = view.mine.as_ref().and_then(|e| e.role()) else {
        return;
    };
    let attacker_won = match (local_role, won) {
        (CombatRole::Attacking, "yes") => true,
        (CombatRole::Attacking, "no") => false,
        (CombatRole::Defending, "yes") => false,
        (CombatRole::Defending, "no") => true,
        _ => return,
    };

    let (me, _) = player_ids();
    crdt::queue_for_peer(crdt::capture_update(|| {
        let reported = crdt::with_doc(|doc| fists::report_outcome(doc, &me, attacker_won));
        let settled = settle_fists();
        reported || settled
    }));
    sync_fists_view();
}

/// The outcome screen for this player. Pure: applying the outcome (marking
/// damage) is done once by `settle_fists`, and this only reports what it did.
fn render_fists_outcome(view: &fists::View) -> String {
    let mine = view.mine.clone().unwrap_or_default();
    let local_role = mine.role().unwrap_or(CombatRole::Attacking);
    let is_final_blows = view.combat.is_final_blows();
    let local_card = mine.card();

    const NEW_ROUND: &str = r#"<button type="button" data-engine-post="/api/room/fists" data-engine-body="action=reset" class="w-full bg-emerald-600 hover:bg-emerald-700 text-amber-50 font-bold py-2 px-4 rounded text-sm">New Round</button>"#;

    let mut h = String::with_capacity(1024);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna text-center">"#);

    match view.outcome {
        Some(fists::Outcome::Disputed) => {
            h.push_str(r#"<p class="text-2xl mb-2">&#x1F914;</p>"#);
            h.push_str(r#"<p class="text-lg font-bold mb-2 text-amber-600">You and your opponent reported different results.</p>"#);
            h.push_str(r#"<p class="text-sm mb-3">Nothing was marked. Settle it at the table, mark any damage by hand, then start a new round.</p>"#);
            h.push_str(NEW_ROUND);
        }
        Some(fists::Outcome::AttackerWon) => {
            // Attacker won — defender lost
            if local_role == CombatRole::Defending {
                if is_final_blows && !local_card.is_empty() {
                    // Final Blows: defender lost → card is wasted
                    h.push_str(r#"<p class="text-2xl mb-2">&#x1F480;</p>"#);
                    h.push_str(r#"<p class="text-lg font-bold mb-2 text-kip-red">Yikes! Your card was wasted, remove it from the field.</p>"#);
                    h.push_str(r#"<p class="text-sm mb-3">The final blows indicator has been marked.</p>"#);
                } else {
                    // Regular combat: defender lost — damage was auto-marked
                    h.push_str(r#"<p class="text-2xl mb-2">&#x1F4A5;</p>"#);
                    h.push_str(r#"<p class="text-lg font-bold mb-2 text-kip-red">Ouch, let me mark that for you.</p>"#);
                    if mine.marked {
                        h.push_str(r#"<p class="text-sm mb-3">Damage has been recorded on your card.</p>"#);
                    } else {
                        h.push_str(r#"<p class="text-sm mb-3">All slots in that keal means are already marked.</p>"#);
                    }
                    h.push_str(NEW_ROUND);
                }
            } else if is_final_blows {
                // Final Blows: attacker won → opponent's card is wasted
                h.push_str(r#"<p class="text-2xl mb-2">&#x1F3C6;</p>"#);
                h.push_str(r#"<p class="text-lg font-bold mb-2 text-emerald-600">You clinched victory! Keep pushing.</p>"#);
            } else {
                // Regular combat: attacker won
                h.push_str(r#"<p class="text-2xl mb-2">&#x2694;</p>"#);
                h.push_str(r#"<p class="text-lg font-bold mb-2 text-emerald-600">Nice Play! Damage is now reflected on the opponent.</p>"#);
                h.push_str(NEW_ROUND);
            }
        }
        Some(fists::Outcome::DefenderWon) => {
            // Defender won — same messaging for both regular and Final Blows.
            if local_role == CombatRole::Defending {
                h.push_str(r#"<p class="text-2xl mb-2">&#x1F6E1;</p>"#);
                h.push_str(r#"<p class="text-lg font-bold mb-2 text-emerald-600">Great defense, keep it up!</p>"#);
            } else {
                h.push_str(r#"<p class="text-2xl mb-2">&#x1F614;</p>"#);
                h.push_str(r#"<p class="text-lg font-bold mb-2 text-amber-600">Too bad, try next turn!</p>"#);
            }
        }
        None => {}
    }

    h.push_str(r#"</div>"#);
    h
}

/// Find the next unchecked damage slot in a specific keal means group and toggle it.
/// Returns true if a slot was marked, false if all slots are already checked.
fn auto_mark_damage(card_slug: &str, keal_idx: u8) -> bool {
    let card = match find_card(card_slug) {
        Some(c) => c,
        None => return false,
    };

    if card.keal_means.is_empty() || keal_idx == 0 {
        return false;
    }

    // Compute the slot range for this keal means group
    let km_index = (keal_idx as usize).saturating_sub(1);
    let mut slot_start: u8 = 1;
    for (i, km) in card.keal_means.iter().enumerate() {
        if i == km_index {
            // Found the target group — find the first unchecked slot in range
            let slot_end = slot_start + km.count;
            let total: u8 = card.keal_means.iter().map(|k| k.count).sum();
            damage::ensure_card_state(card_slug, total);

            for slot in slot_start..slot_end {
                if !player_doc::get_slot(card_slug, slot) {
                    damage::toggle_slot(card_slug, slot);
                    return true;
                }
            }
            return false; // All slots in this group are already checked
        }
        slot_start += km.count;
    }

    false
}

// ── GET /api/room/state ────────────────────────────────────────────

pub fn handle_room_state_get(_query: &str) -> String {
    room::export_room_json()
}

// ── Yrs CRDT sync routes ──────────────────────────────────────────

/// GET /api/room/yrs/sv — return this peer's state vector (base64).
pub fn handle_yrs_sv_get(_query: &str) -> String {
    crdt::encode_state_vector()
}

/// POST /api/room/yrs/diff — receive a remote state vector (base64 in body),
/// return the diff update (base64) that the remote peer needs.
pub fn handle_yrs_diff_post(body: &str) -> String {
    let params = parse_form_body(body);
    let sv = get_param(&params, "sv").unwrap_or(body.trim());
    match crdt::encode_diff(sv) {
        Ok(diff) => diff,
        Err(e) => format!(r#"{{"error":"{}"}}"#, e),
    }
}

/// POST /api/room/yrs/apply — apply a base64-encoded update from peer.
///
/// The update may carry timers, combat, or both. Returns `ok`, or
/// `{ "error": "..." }`.
///
/// What follows from it is queued for the page (see `crate::page`): the
/// views that show the room's state are drawn again, and so are the card
/// panels if the update delivered a lost round, to show the damage.
pub fn handle_yrs_apply_post(body: &str) -> String {
    let params = parse_form_body(body);
    let update = get_param(&params, "update").unwrap_or(body.trim());
    match crdt::apply_update(update) {
        Ok(()) => {
            // Keep PLAYER_DOC in sync with what the peer changed, the same as
            // this player's own changes do. If the session ends without a
            // clean disconnect, this player's own timers are still current.
            crdt::export_to_local();

            // The update may have delivered the outcome of a round. Applying
            // it is recorded in the room document, and that is sent back.
            crdt::queue_for_peer(crdt::capture_update(settle_fists));
            sync_fists_view();

            page::redraw("/api/game/turns");
            page::redraw("/api/room/fists");
            "ok".to_string()
        }
        Err(e) => serde_json::json!({ "error": e }).to_string(),
    }
}

/// GET /api/room/yrs/state — return the full CRDT Doc state as URL-safe base64.
/// Used by JS to persist Doc to sessionStorage across page navigation.
pub fn handle_yrs_state_get(_query: &str) -> String {
    crdt::encode_full_state()
}

/// POST /api/room/yrs/restore — restore CRDT Doc from persisted state.
/// Body: state=<url-safe-base64>
/// Called on page load to recover Doc before sync handshake.
pub fn handle_yrs_restore_post(body: &str) -> String {
    let params = parse_form_body(body);
    let state = get_param(&params, "state").unwrap_or(body.trim());
    match crdt::restore_from_state(state) {
        Ok(()) => "ok".to_string(),
        Err(e) => format!(r#"{{"error":"{}"}}"#, e),
    }
}

// ── Result rendering ───────────────────────────────────────────────

/// What both result screens end with. Each player answers for themselves;
/// see `answer_did_you_win`.
const DID_YOU_WIN: &str = concat!(
    r#"<div class="mt-3 border-t border-slate-300 pt-3">"#,
    r#"<p class="text-sm font-bold text-center mb-2">Did you win?</p>"#,
    r#"<div class="flex gap-2">"#,
    r#"<button type="button" data-engine-post="/api/room/fists" data-engine-body="action=outcome&won=yes" class="flex-1 bg-emerald-600 hover:bg-emerald-700 text-amber-50 font-bold py-2 px-4 rounded text-sm">Yes!</button>"#,
    r#"<button type="button" data-engine-post="/api/room/fists" data-engine-body="action=outcome&won=no" class="flex-1 bg-kip-red hover:bg-kip-drk-sienna text-amber-50 font-bold py-2 px-4 rounded text-sm">No</button>"#,
    r#"</div>"#,
    r#"</div>"#,
);

fn render_fists_result() -> String {
    room::with_room(|r| {
        // Check if this is a Final Blows combat (one player has exhausted keal means)
        if r.fists.is_final_blows() {
            return render_final_blows_result();
        }

        // Check for same-role conflict first
        if let Some(conflict_role) = r.fists.has_role_conflict() {
            let role_name = match conflict_role {
                CombatRole::Attacking => "Attacking",
                CombatRole::Defending => "Defending",
            };
            return render_role_conflict(role_name);
        }

        let atk = match r.fists.attacker() {
            Some(a) => a,
            None => {
                return render_role_conflict("the same role");
            }
        };
        let def = match r.fists.defender() {
            Some(d) => d,
            None => {
                return render_role_conflict("the same role");
            }
        };

        let atk_card = find_card(&atk.card);
        let def_card = find_card(&def.card);

        if atk_card.is_none() || def_card.is_none() {
            return r#"<div class="p-4 text-kip-red">Error: Card not found in catalog.</div>"#
                .to_string();
        }

        let atk_card = atk_card.unwrap();
        let def_card = def_card.unwrap();

        // Get keal means genetics
        let atk_km_idx = (atk.keal_idx as usize).saturating_sub(1);
        let def_km_idx = (def.keal_idx as usize).saturating_sub(1);

        let atk_km = atk_card.keal_means.get(atk_km_idx);
        let def_km = def_card.keal_means.get(def_km_idx);

        if atk_km.is_none() || def_km.is_none() {
            return r#"<div class="p-4 text-kip-red">Error: Invalid keal means index.</div>"#
                .to_string();
        }

        let atk_km = atk_km.unwrap();
        let def_km = def_km.unwrap();

        // Parse genetics into Archetypes
        let atk_types: Vec<typing::Archetype> = atk_km
            .genetics
            .iter()
            .filter_map(|g| typing::parse_archetype(g))
            .collect();
        let def_types: Vec<typing::Archetype> = def_km
            .genetics
            .iter()
            .filter_map(|g| typing::parse_archetype(g))
            .collect();

        // Parse motivations
        let atk_motive = atk_card
            .motivation
            .and_then(|m| typing::parse_motive(m));
        let def_motive = def_card
            .motivation
            .and_then(|m| typing::parse_motive(m));

        // Compute matchup
        let result = typing::type_matchup(&atk_types, &def_types, atk_motive, def_motive);

        // Build result HTML
        build_result_html(atk_card, &atk.card, atk_km, def_card, &def.card, def_km, &result)
    })
}

/// Render Final Blows result showing both motivations and modifiers (no archetypes).
/// Uses local_card()/remote_card() helpers which correctly resolve the card slug
/// from either regular fists or final blows submissions on each side.
///
/// Determines attacker/defender based on submissions so that motivation modifiers
/// are computed identically on both clients (local/remote are perspective-relative).
fn render_final_blows_result() -> String {
    // Determine attacker and defender cards based on submission types.
    // The Final Blows player is always the defender.
    let (atk_slug, def_slug) = room::with_room(|r| {
        if r.fists.local_final_blows.is_some() {
            // Local submitted Final Blows → local is defender, remote is attacker
            let def = r.fists.local_card().unwrap_or_default().to_string();
            let atk = r.fists.remote_card().unwrap_or_default().to_string();
            (atk, def)
        } else if r.fists.remote_final_blows.is_some() {
            // Remote submitted Final Blows → remote is defender, local is attacker
            let atk = r.fists.local_card().unwrap_or_default().to_string();
            let def = r.fists.remote_card().unwrap_or_default().to_string();
            (atk, def)
        } else {
            // Both have Final Blows (rare) — pick a convention
            let atk = r.fists.local_card().unwrap_or_default().to_string();
            let def = r.fists.remote_card().unwrap_or_default().to_string();
            (atk, def)
        }
    });

    let atk_card = find_card(&atk_slug);
    let def_card = find_card(&def_slug);

    if atk_card.is_none() || def_card.is_none() {
        return r#"<div class="p-4 text-kip-red">Error: Card not found in catalog.</div>"#.to_string();
    }

    let atk_card = atk_card.unwrap();
    let def_card = def_card.unwrap();

    // Parse motivations in attacker, defender order for correct modifier computation
    let atk_motive = atk_card.motivation.and_then(|m| typing::parse_motive(m));
    let def_motive = def_card.motivation.and_then(|m| typing::parse_motive(m));

    // Compute matchup using empty archetype lists (motivation-only)
    let result = typing::type_matchup(&[], &[], atk_motive, def_motive);

    // Compute damage bonus from genetic_disposition
    let atk_genetic = atk_card.genetic_disposition.and_then(|g| typing::parse_archetype(g));
    let def_genetic = def_card.genetic_disposition.and_then(|g| typing::parse_archetype(g));
    let damage_bonus = typing::compute_damage_bonus(atk_genetic, def_genetic, atk_motive, def_motive);

    let mut h = String::with_capacity(2048);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna">"#);
    h.push_str(r#"<p class="text-xl font-bold text-center mb-4">&#x1F525; Final Blows &#x1F525;</p>"#);

    // Attacker card info
    h.push_str(r#"<div class="bg-red-50 rounded p-3 mb-2">"#);
    h.push_str(r#"<p class="font-bold text-kip-red text-sm">&#x2694; ATTACKER</p>"#);
    h.push_str(&format!(r#"<p class="font-bold">{}</p>"#, atk_card.title));
    if let Some(mot) = atk_card.motivation {
        h.push_str(&format!(r#"<p class="text-sm">Motivation: <strong>{}</strong></p>"#, mot));
    } else {
        h.push_str(r#"<p class="text-sm text-slate-400">No motivation</p>"#);
    }
    if let Some(gd) = atk_card.genetic_disposition {
        h.push_str(&format!(r#"<p class="text-xs">Disposition: {}</p>"#, gd));
    }
    h.push_str(r#"</div>"#);

    // Defender card info
    h.push_str(r#"<div class="bg-blue-50 rounded p-3 mb-3">"#);
    h.push_str(r#"<p class="font-bold text-blue-600 text-sm">&#x1F6E1; DEFENDER</p>"#);
    h.push_str(&format!(r#"<p class="font-bold">{}</p>"#, def_card.title));
    if let Some(mot) = def_card.motivation {
        h.push_str(&format!(r#"<p class="text-sm">Motivation: <strong>{}</strong></p>"#, mot));
    } else {
        h.push_str(r#"<p class="text-sm text-slate-400">No motivation</p>"#);
    }
    if let Some(gd) = def_card.genetic_disposition {
        h.push_str(&format!(r#"<p class="text-xs">Disposition: {}</p>"#, gd));
    }
    h.push_str(r#"</div>"#);

    // Damage Bonus from genetic disposition
    let bonus_color = if damage_bonus > 0 {
        "text-emerald-600"
    } else if damage_bonus < 0 {
        "text-kip-red"
    } else {
        "text-slate-600"
    };
    let bonus_sign = if damage_bonus > 0 { "+" } else { "" };

    // Single unified Final Blows info box: Damage Bonus + Motivation Modifiers + D20
    h.push_str(r#"<div class="bg-amber-50 border-2 border-kip-drk-sienna rounded p-3 mb-3">"#);

    // Damage Bonus
    h.push_str(&format!(
        r#"<p class="text-sm font-bold text-center">Damage Bonus</p><p class="text-3xl font-bold text-center {}">{}{}</p>"#,
        bonus_color, bonus_sign, damage_bonus
    ));
    // Breakdown
    let mut breakdown_parts: Vec<String> = Vec::new();
    if let (Some(_ag), Some(_dg)) = (atk_genetic, def_genetic) {
        let raw = typing::compute_damage_bonus(atk_genetic, def_genetic, None, None);
        let atk_name = atk_card.genetic_disposition.unwrap_or("?");
        let def_name = def_card.genetic_disposition.unwrap_or("?");
        breakdown_parts.push(format!("{} vs {} ×2 = {}", atk_name, def_name, raw));
    }
    if typing::motives_interact(atk_motive, def_motive) {
        breakdown_parts.push("+10 motives interact".to_string());
    }
    if !breakdown_parts.is_empty() {
        h.push_str(&format!(
            r#"<p class="text-xs text-center text-slate-500">({})</p>"#,
            breakdown_parts.join(", ")
        ));
    }

    // Motivation modifiers section
    let has_mods = result.societal_mod.is_some() || result.self_mod.is_some() || result.support_mod.is_some();
    if has_mods {
        h.push_str(r#"<div class="border-t border-slate-200 mt-2 pt-2">"#);
        h.push_str(r#"<p class="text-sm font-bold text-center mb-1">Motivation Modifiers</p>"#);
        if let Some(ref s) = result.societal_mod {
            let text = s.trim_start_matches('\n');
            h.push_str(&format!(r#"<p class="text-xs text-amber-700 font-bold mb-1">&#x2696; {}</p>"#, text));
        }
        if let Some(ref s) = result.self_mod {
            let text = s.trim_start_matches('\n');
            h.push_str(&format!(r#"<p class="text-xs text-amber-700 font-bold mb-1">&#x1F3C3; {}</p>"#, text));
        }
        if let Some(ref s) = result.support_mod {
            let text = s.trim_start_matches('\n');
            h.push_str(&format!(r#"<p class="text-xs text-amber-700 font-bold mb-1">&#x1F91D; {}</p>"#, text));
        }
        h.push_str(r#"</div>"#);
    }

    // D20 roll instruction
    h.push_str(r#"<div class="border-t border-slate-200 mt-2 pt-2 text-center">"#);
    h.push_str(r#"<p class="text-sm font-bold mb-1">Both Players Roll</p>"#);
    h.push_str(r#"<p class="text-2xl font-bold text-kip-drk-sienna">D20</p>"#);
    if result.modifier != 0 {
        let mod_sign = if result.modifier > 0 { "+" } else { "" };
        h.push_str(&format!(r#"<p class="text-xs text-slate-500 mt-1">Attacker gets {}{} from motivation</p>"#, mod_sign, result.modifier));
    }
    h.push_str(r#"</div>"#);

    h.push_str(r#"</div>"#); // close unified Final Blows info box

    // "Did you win?" outcome buttons
    h.push_str(DID_YOU_WIN);

    h.push_str(r#"</div>"#);
    h
}

fn build_result_html(
    atk_card: &crate::catalog::Card,
    atk_slug: &str,
    atk_km: &crate::catalog::KealMeans,
    def_card: &crate::catalog::Card,
    def_slug: &str,
    def_km: &crate::catalog::KealMeans,
    result: &typing::MatchupResult,
) -> String {
    let mut h = String::with_capacity(2048);

    h.push_str(r#"<div class="p-4 text-kip-drk-sienna">"#);
    h.push_str(r#"<p class="text-xl font-bold text-center mb-4">&#9876; Combat Result &#9876;</p>"#);

    // Attacker info
    h.push_str(r#"<div class="bg-red-50 rounded p-3 mb-2">"#);
    h.push_str(r#"<p class="font-bold text-kip-red text-sm">&#x2694; ATTACKER</p>"#);
    h.push_str(&format!(
        r#"<p class="font-bold">{}</p>"#,
        atk_card.title
    ));
    h.push_str(&format!(
        r#"<p class="text-sm">Keal: <strong class="text-kip-red">{}</strong></p>"#,
        atk_km.name
    ));
    h.push_str(&format!(
        r#"<p class="text-xs">Archetypes: {}</p>"#,
        atk_km.genetics.join(", ")
    ));
    if !atk_card.die.is_empty() {
        h.push_str(&format!(
            r#"<p class="text-sm mt-1">Rolls: <strong>{}</strong></p>"#,
            atk_card.die
        ));
    }
    h.push_str(r#"</div>"#);

    // Defender info
    h.push_str(r#"<div class="bg-blue-50 rounded p-3 mb-3">"#);
    h.push_str(r#"<p class="font-bold text-blue-600 text-sm">&#x1F6E1; DEFENDER</p>"#);
    h.push_str(&format!(
        r#"<p class="font-bold">{}</p>"#,
        def_card.title
    ));
    h.push_str(&format!(
        r#"<p class="text-sm">Keal: <strong class="text-blue-600">{}</strong></p>"#,
        def_km.name
    ));
    h.push_str(&format!(
        r#"<p class="text-xs">Archetypes: {}</p>"#,
        def_km.genetics.join(", ")
    ));
    if !def_card.die.is_empty() {
        h.push_str(&format!(
            r#"<p class="text-sm mt-1">Rolls: <strong>{}</strong></p>"#,
            def_card.die
        ));
    }
    h.push_str(r#"</div>"#);

    // Compute affinity bonus: +1 if attacker's genetic_disposition matches active affinity
    let affinity_bonus: i32 = match player_doc::get_active_affinity() {
        Some((ref active_name, _)) => {
            match atk_card.genetic_disposition {
                Some(gd) if gd == active_name => 1,
                _ => 0,
            }
        }
        None => 0,
    };
    let total_modifier = result.modifier + affinity_bonus;
    let total_color = if total_modifier > 0 {
        "text-emerald-600"
    } else if total_modifier < 0 {
        "text-kip-red"
    } else {
        "text-slate-600"
    };
    let total_sign = if total_modifier > 0 { "+" } else { "" };

    h.push_str(r#"<div class="bg-amber-50 border-2 border-kip-drk-sienna rounded p-3 text-center mb-3">"#);
    h.push_str(&format!(
        r#"<p class="text-sm font-bold">Attack Die Modifier</p><p class="text-3xl font-bold {}">{}{}</p>"#,
        total_color, total_sign, total_modifier
    ));
    if affinity_bonus > 0 {
        h.push_str(&format!(
            r#"<p class="text-xs text-amber-600 mt-1">+1 from Affinity ({})</p>"#,
            player_doc::get_active_affinity().map(|(n, _)| n).unwrap_or_default()
        ));
    }

    h.push_str(r#"</div>"#);

    // Final Blows box — only shows when at least one player's keal means are exhausted
    let atk_exhausted = all_keal_means_exhausted(atk_slug);
    let def_exhausted = all_keal_means_exhausted(def_slug);
    if atk_exhausted || def_exhausted {
        let atk_genetic = atk_card.genetic_disposition.and_then(|g| typing::parse_archetype(g));
        let def_genetic = def_card.genetic_disposition.and_then(|g| typing::parse_archetype(g));
        let atk_motive = atk_card.motivation.and_then(|m| typing::parse_motive(m));
        let def_motive = def_card.motivation.and_then(|m| typing::parse_motive(m));
        let damage_bonus = typing::compute_damage_bonus(atk_genetic, def_genetic, atk_motive, def_motive);

        h.push_str(r#"<div class="bg-slate-50 border border-slate-300 rounded p-3 mb-3">"#);
        h.push_str(r#"<p class="text-sm font-bold text-center mb-2">&#x1F525; Final Blows</p>"#);
        if atk_exhausted && def_exhausted {
            h.push_str(r#"<p class="text-xs text-center mb-2 text-amber-600">Both players' keal means are exhausted. Next round is the final combat.</p>"#);
        } else if def_exhausted {
            h.push_str(r#"<p class="text-xs text-center mb-2 text-amber-600">Defender's keal means are exhausted. Next round is the final combat.</p>"#);
        } else {
            h.push_str(r#"<p class="text-xs text-center mb-2 text-amber-600">Attacker's keal means are exhausted. Next round is the final combat.</p>"#);
        }

        // Damage bonus display
        let bonus_color = if damage_bonus > 0 {
            "text-emerald-600"
        } else if damage_bonus < 0 {
            "text-kip-red"
        } else {
            "text-slate-600"
        };
        let bonus_sign = if damage_bonus > 0 { "+" } else { "" };
        h.push_str(&format!(
            r#"<p class="text-sm font-bold text-center">Damage Bonus: <span class="{}">{}{}</span></p>"#,
            bonus_color, bonus_sign, damage_bonus
        ));

        // Breakdown
        let mut breakdown_parts: Vec<String> = Vec::new();
        if let (Some(ag), Some(dg)) = (atk_genetic, def_genetic) {
            let raw = crate::typing::compute_damage_bonus(Some(ag), Some(dg), None, None);
            let atk_name = atk_card.genetic_disposition.unwrap_or("?");
            let def_name = def_card.genetic_disposition.unwrap_or("?");
            breakdown_parts.push(format!("{} vs {} ×2 = {}", atk_name, def_name, raw));
        }
        if typing::motives_interact(atk_motive, def_motive) {
            breakdown_parts.push("+10 motives interact".to_string());
        }
        if !breakdown_parts.is_empty() {
            h.push_str(&format!(
                r#"<p class="text-xs text-center text-slate-500">({})</p>"#,
                breakdown_parts.join(", ")
            ));
        }

        // Motivation modifiers (only shown in Final Blows context)
        let fb_result = typing::type_matchup(&[], &[], atk_motive, def_motive);
        let has_mods = fb_result.societal_mod.is_some() || fb_result.self_mod.is_some() || fb_result.support_mod.is_some();
        if has_mods {
            h.push_str(r#"<div class="border-t border-slate-200 mt-2 pt-2">"#);
            h.push_str(r#"<p class="text-sm font-bold text-center mb-1">Motivation Modifiers</p>"#);
            if let Some(ref s) = fb_result.societal_mod {
                let text = s.trim_start_matches('\n');
                h.push_str(&format!(r#"<p class="text-xs text-amber-700 font-bold mb-1">&#x2696; {}</p>"#, text));
            }
            if let Some(ref s) = fb_result.self_mod {
                let text = s.trim_start_matches('\n');
                h.push_str(&format!(r#"<p class="text-xs text-amber-700 font-bold mb-1">&#x1F3C3; {}</p>"#, text));
            }
            if let Some(ref s) = fb_result.support_mod {
                let text = s.trim_start_matches('\n');
                h.push_str(&format!(r#"<p class="text-xs text-amber-700 font-bold mb-1">&#x1F91D; {}</p>"#, text));
            }
            h.push_str(r#"</div>"#);
        }

        // D20 roll instruction for the final combat
        h.push_str(r#"<div class="border-t border-slate-200 mt-2 pt-2 text-center">"#);
        h.push_str(r#"<p class="text-sm font-bold mb-1">Both Players Roll</p>"#);
        h.push_str(r#"<p class="text-2xl font-bold text-kip-drk-sienna">D20</p>"#);
        h.push_str(r#"</div>"#);

        h.push_str(r#"</div>"#);
    }

    // "Did you win?" outcome buttons
    h.push_str(DID_YOU_WIN);

    h.push_str(r#"</div>"#);
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use yrs::updates::decoder::Decode;
    use yrs::{Doc, ReadTxn, Transact, Update};

    const ME: &str = "me";
    const THEM: &str = "them";
    const BROX: &str = "?card=brox_the_defiant";

    fn reset() {
        room::reset_room();
        crdt::reset_doc();
        crdt::take_outbox();
        page::take_redraws();
        crate::game::player_doc::init_player_doc();
    }

    /// Put this player in a room as ME, with the opponent known as THEM.
    /// The opponent is not marked connected: none of the fists routes need
    /// them to be. Nothing is left queued for the page.
    fn enter_room() {
        handle_create_post(&format!("code=ABCD&name=Test&player={}", ME));
        handle_peer_post(&format!("id={}", THEM));
        handed_over();
    }

    /// What the engine has queued for the page since this was last asked:
    /// how many updates for the opponent, and the routes to draw again.
    fn handed_over() -> (usize, Vec<&'static str>) {
        (crdt::take_outbox().len(), page::take_redraws())
    }

    /// Act as the opponent's device: start from the room document as we have
    /// it, change it with `f`, and deliver the change the way the relay
    /// would, as a peer update.
    fn opponent(f: impl FnOnce(&Doc)) {
        let theirs = Doc::new();
        let ours = URL_SAFE_NO_PAD.decode(crdt::encode_full_state()).unwrap();
        theirs
            .transact_mut()
            .apply_update(Update::decode_v1(&ours).unwrap())
            .unwrap();
        let before = theirs.transact().state_vector();
        f(&theirs);
        let diff = theirs.transact().encode_diff_v1(&before);
        let reply = handle_yrs_apply_post(&format!("update={}", URL_SAFE_NO_PAD.encode(diff)));
        assert_eq!(reply, "ok");
    }

    /// A tap in the fists dialog on Brox's page, and what the dialog then
    /// shows: the page posts, then draws the view again.
    fn tap(body: &str) -> String {
        assert_eq!(handle_fists_post(body), "");
        handle_fists_get(BROX)
    }

    /// Pick a role and Brox's first keal means, and lock them in.
    fn lock_in_brox(role: &str) -> String {
        tap(&format!("action=choose&card=brox_the_defiant&role={}", role));
        tap("action=choose&card=brox_the_defiant&keal=1");
        tap("action=lock&card=brox_the_defiant")
    }

    /// The values of the radio buttons a drawing of the form shows checked.
    fn checked(html: &str) -> Vec<String> {
        html.split("<input")
            .skip(1)
            .filter_map(|rest| {
                let tag = rest.split('>').next()?;
                if !tag.ends_with(" checked") {
                    return None;
                }
                let value = tag.split("value=\"").nth(1)?.split('"').next()?;
                Some(value.to_string())
            })
            .collect()
    }

    /// Mark every damage slot of one of Brox's keal means (1-based).
    fn exhaust_brox_keal_means(idx: usize) {
        let card = find_card("brox_the_defiant").unwrap();
        let total: u8 = card.keal_means.iter().map(|k| k.count).sum();
        damage::ensure_card_state("brox_the_defiant", total);
        let before: u8 = card.keal_means[..idx - 1].iter().map(|k| k.count).sum();
        for slot in before + 1..=before + card.keal_means[idx - 1].count {
            damage::toggle_slot("brox_the_defiant", slot);
        }
    }

    fn their_attack(card: &str) -> FistsSubmission {
        FistsSubmission {
            role: CombatRole::Attacking,
            card: card.to_string(),
            keal_idx: 1,
        }
    }

    fn their_defence(card: &str) -> FistsSubmission {
        FistsSubmission {
            role: CombatRole::Defending,
            card: card.to_string(),
            keal_idx: 1,
        }
    }

    #[test]
    fn resume_restores_the_room_document_without_seeding() {
        reset();
        // A solo timer that must NOT be copied into the room on resume.
        crate::game::player_doc::add_alarm(9, "solo", "red");

        // The room document as it was persisted on the previous page.
        crdt::init_doc();
        crdt::add_alarm(4, "shared", "blue");
        let state = crdt::encode_full_state();
        crdt::reset_doc();

        let out = handle_resume_post(&format!("code=abcd&name=My%20Room&state={}", state));
        assert_eq!(out, "ok");

        assert!(room::in_room());
        assert!(!room::is_peer_connected());
        room::with_room(|r| {
            assert_eq!(r.room_code, "ABCD");
            assert_eq!(r.room_name, "My Room");
        });

        let shared = crdt::get_alarms();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].name, "shared");
        assert_eq!(shared[0].remaining, 4);

        // The solo timer was neither seeded into the room nor consumed.
        let local = crate::game::player_doc::get_alarms();
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].name, "solo");

        crdt::reset_doc();
        reset();
    }

    #[test]
    fn resume_without_a_code_is_refused() {
        reset();
        let out = handle_resume_post("name=table");
        assert!(out.contains("error"));
        assert!(!room::in_room());
        reset();
    }

    #[test]
    fn applying_a_peer_update_mirrors_timers_to_the_player_doc() {
        reset();
        // Build an update as the peer would produce it.
        crdt::init_doc();
        let update = crdt::add_alarm(6, "from peer", "green");
        crdt::reset_doc(); // our own doc starts empty

        assert!(crate::game::player_doc::get_alarms().is_empty());
        assert_eq!(handle_yrs_apply_post(&format!("update={}", update)), "ok");

        let local = crate::game::player_doc::get_alarms();
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].name, "from peer");
        assert_eq!(local[0].remaining, 6);
        crdt::reset_doc();
        reset();
    }

    #[test]
    fn applying_a_peer_update_asks_for_the_rooms_views_to_be_drawn_again() {
        reset();
        enter_room();
        opponent(|doc| {
            fists::submit(doc, THEM, ME, their_defence("liliel_healing_fairy"));
        });
        let (updates, redraw) = handed_over();
        assert_eq!(updates, 0, "nothing to send back");
        assert_eq!(redraw, ["/api/game/turns", "/api/room/fists"]);
        reset();
    }

    #[test]
    fn a_peer_update_that_cannot_be_read_is_an_error_and_asks_for_nothing() {
        reset();
        enter_room();
        let reply = handle_yrs_apply_post("update=not-an-update");
        assert!(reply.contains("error"));
        assert_eq!(handed_over(), (0, vec![]));
        reset();
    }

    #[test]
    fn status_disconnected() {
        reset();
        let html = handle_status_get("");
        assert!(html.contains("Create Room"));
        assert!(html.contains("Join Room"));
        reset();
    }

    #[test]
    fn create_and_status() {
        reset();
        handle_create_post("code=ABCD&name=Test+Room");
        let state = room::with_room(|r| (r.room_code.clone(), r.room_name.clone()));
        assert_eq!(state.0, "ABCD");
        assert_eq!(state.1, "Test Room");
        reset();
    }

    #[test]
    fn join_stores_room_but_not_connected() {
        reset();
        let html = handle_join_post("code=WXYZ&name=Fun+Room");
        // Join does not set connected=true; that happens when the relay reports the peer
        assert!(html.contains("Waiting for peer"));
        assert!(html.contains("WXYZ"));
        room::with_room(|r| {
            assert!(!r.connected);
            assert_eq!(r.room_code, "WXYZ");
        });
        reset();
    }

    #[test]
    fn connected_post_sets_connected() {
        reset();
        handle_join_post("code=WXYZ&name=Fun+Room");
        let html = handle_connected_post("code=WXYZ&name=Fun+Room");
        assert!(html.contains("Connected"));
        room::with_room(|r| assert!(r.connected));
        reset();
    }

    #[test]
    fn entering_and_leaving_a_room_draws_the_turn_tracker_again() {
        reset();
        handle_create_post("code=ABCD&name=Test");
        assert_eq!(page::take_redraws(), ["/api/game/turns"]);
        handle_connected_post("code=ABCD&name=Test");
        assert_eq!(page::take_redraws(), ["/api/game/turns"]);
        handle_disconnect_post("");
        assert_eq!(page::take_redraws(), ["/api/game/turns"]);
        handle_join_post("code=WXYZ&name=Other");
        assert_eq!(page::take_redraws(), ["/api/game/turns"]);
        reset();
    }

    #[test]
    fn learning_who_the_opponent_is_draws_the_fists_dialog_again() {
        reset();
        handle_create_post(&format!("code=ABCD&name=Test&player={}", ME));
        handed_over();
        assert_eq!(handle_peer_post(&format!("id={}", THEM)), "ok");
        assert_eq!(page::take_redraws(), ["/api/room/fists"]);
        room::with_room(|r| assert_eq!(r.peer_id, THEM));
        reset();
    }

    #[test]
    fn fists_role_conflict_shows_try_again() {
        reset();
        room::with_room_mut(|r| {
            r.connected = true;
            r.fists.local = Some(FistsSubmission {
                role: CombatRole::Attacking,
                card: "brox_the_defiant".to_string(),
                keal_idx: 1,
            });
            r.fists.remote = Some(FistsSubmission {
                role: CombatRole::Attacking,
                card: "liliel_healing_fairy".to_string(),
                keal_idx: 1,
            });
        });
        let html = render_fists_result();
        assert!(html.contains("Both players chose Attacking"));
        assert!(html.contains("Try Again"));
        assert!(html.contains(r#"data-engine-body="action=reset""#));
        reset();
    }

    #[test]
    fn disconnect_resets() {
        reset();
        handle_join_post("code=ABCD&name=Test");
        handle_disconnect_post("");
        room::with_room(|r| {
            assert!(!r.connected);
            assert!(r.room_code.is_empty());
        });
        reset();
    }

    #[test]
    fn fists_get_not_connected() {
        reset();
        let html = handle_fists_get(BROX);
        assert!(html.contains("Not connected"));
        reset();
    }

    #[test]
    fn a_tap_outside_a_room_does_nothing() {
        reset();
        handle_fists_post("action=choose&card=brox_the_defiant&role=attacking");
        handle_fists_post("action=lock&card=brox_the_defiant");
        handle_fists_post("action=close");
        room::with_room(|r| {
            assert!(r.choice.role.is_none());
            assert!(r.choice.refusal.is_empty());
        });
        assert_eq!(handed_over(), (0, vec![]));
        reset();
    }

    #[test]
    fn fists_get_shows_form_in_a_room_even_while_the_opponent_is_away() {
        reset();
        enter_room();
        assert!(!room::is_peer_connected());
        let html = handle_fists_get(BROX);
        assert!(html.contains("Brox The Defiant"));
        assert!(html.contains("Crushing Hope"));
        assert!(html.contains("Chain Raid"));
        assert!(html.contains("Attacking"));
        assert!(html.contains("Defending"));
        assert!(checked(&html).is_empty(), "nothing is picked to begin with");
        reset();
    }

    #[test]
    fn the_dialog_asks_nothing_of_the_page_script_and_does_not_poll() {
        reset();
        enter_room();
        let form = handle_fists_get(BROX);
        let waiting = lock_in_brox("attacking");
        opponent(|doc| fists::submit(doc, THEM, ME, their_defence("liliel_healing_fairy")));
        let result = handle_fists_get(BROX);
        let outcome = tap("action=outcome&won=yes");
        for html in [form, waiting, result, outcome] {
            assert!(!html.contains("onclick"));
            assert!(!html.contains("kipukasMultiplayer"));
            assert!(!html.contains("hx-"));
        }
        reset();
    }

    #[test]
    fn a_pick_is_noted_and_drawn_checked_and_is_not_shared() {
        reset();
        enter_room();
        let html = tap("action=choose&card=brox_the_defiant&role=defending");
        assert_eq!(checked(&html), ["defending"]);
        let html = tap("action=choose&card=brox_the_defiant&keal=2");
        assert_eq!(checked(&html), ["defending", "2"]);

        // Changing one's mind replaces the pick.
        let html = tap("action=choose&card=brox_the_defiant&role=attacking");
        assert_eq!(checked(&html), ["attacking", "2"]);

        assert_eq!(handed_over(), (0, vec![]), "the opponent is told nothing yet");
        reset();
    }

    #[test]
    fn picks_made_for_another_card_do_not_count() {
        reset();
        enter_room();
        handle_fists_post("action=choose&card=liliel_healing_fairy&role=attacking");
        let html = handle_fists_get(BROX);
        assert!(checked(&html).is_empty());

        // Nor does a pick that names no card of the catalog, or no such
        // role or keal means.
        tap("action=choose&card=no_such_card&role=attacking");
        tap("action=choose&card=brox_the_defiant&role=sideways");
        let html = tap("action=choose&card=brox_the_defiant&keal=9");
        assert!(checked(&html).is_empty());
        reset();
    }

    #[test]
    fn the_picks_survive_the_form_being_drawn_again() {
        reset();
        enter_room();
        tap("action=choose&card=brox_the_defiant&role=attacking");
        // The opponent locks in while this player is still choosing: the
        // dialog is drawn again, and must not lose what was picked.
        opponent(|doc| {
            fists::submit(doc, THEM, ME, their_defence("liliel_healing_fairy"));
        });
        let html = handle_fists_get(BROX);
        assert!(html.contains("Lock In Choice"));
        assert!(html.contains("Opponent has locked in their choice"));
        assert_eq!(checked(&html), ["attacking"]);
        reset();
    }

    #[test]
    fn locking_in_queues_the_choice_for_the_opponent() {
        reset();
        enter_room();
        let html = lock_in_brox("attacking");
        assert!(html.contains("Waiting for opponent"));
        assert!(html.contains("Cancel round"));
        assert_eq!(handed_over().0, 1);
        room::with_room(|r| {
            let local = r.fists.local.as_ref().unwrap();
            assert_eq!(local.role, CombatRole::Attacking);
            assert_eq!(local.card, "brox_the_defiant");
            assert_eq!(local.keal_idx, 1);
            assert!(r.choice.role.is_none(), "the picks are spent");
        });
        reset();
    }

    #[test]
    fn a_cards_first_fight_of_the_day_counts_towards_its_loyalty_and_shows_on_its_panel() {
        reset();
        enter_room();
        assert!(player_doc::get_loyalty("brox_the_defiant").is_none());
        lock_in_brox("attacking");
        assert_eq!(player_doc::get_loyalty("brox_the_defiant").unwrap().0, 1);
        assert_eq!(handed_over(), (1, vec!["/api/card/panel"]));

        // A second fight the same day counts nothing, and draws nothing again.
        tap("action=reset");
        handed_over();
        lock_in_brox("defending");
        assert_eq!(player_doc::get_loyalty("brox_the_defiant").unwrap().0, 1);
        assert_eq!(handed_over(), (1, vec![]));
        reset();
    }

    #[test]
    fn locking_in_says_what_is_still_to_pick_and_changes_nothing() {
        reset();
        enter_room();
        let html = tap("action=lock&card=brox_the_defiant");
        assert!(html.contains("Please select Attacking or Defending."));
        assert!(html.contains("Lock In Choice"));

        // A pick clears the message.
        let html = tap("action=choose&card=brox_the_defiant&role=attacking");
        assert!(!html.contains("Please select"));

        let html = tap("action=lock&card=brox_the_defiant");
        assert!(html.contains("Please select a Keal Means."));
        assert_eq!(checked(&html), ["attacking"], "the role picked is still picked");

        assert_eq!(handed_over(), (0, vec![]));
        room::with_room(|r| assert!(r.fists.local.is_none()));
        reset();
    }

    #[test]
    fn an_exhausted_keal_means_cannot_be_picked_or_locked_in() {
        reset();
        enter_room();
        exhaust_brox_keal_means(1);
        let html = tap("action=choose&card=brox_the_defiant&keal=1");
        assert!(checked(&html).is_empty());

        // One that was picked and has run out since no longer counts.
        tap("action=choose&card=brox_the_defiant&role=attacking");
        let html = tap("action=choose&card=brox_the_defiant&keal=2");
        assert_eq!(checked(&html), ["attacking", "2"]);
        exhaust_brox_keal_means(2);
        // (Every keal means is now exhausted, so the form is Final Blows.)
        let html = tap("action=lock&card=brox_the_defiant");
        assert!(html.contains("Please select a Keal Means."));
        assert_eq!(handed_over(), (0, vec![]));
        reset();
    }

    #[test]
    fn a_second_tap_on_lock_in_asks_for_nothing() {
        reset();
        enter_room();
        lock_in_brox("attacking");
        handed_over();

        let html = tap("action=lock&card=brox_the_defiant");
        assert!(html.contains("Waiting for opponent"));
        assert_eq!(handed_over(), (0, vec![]));

        // And leaves no complaint behind for the next form.
        let html = tap("action=reset");
        assert!(html.contains("Lock In Choice"));
        assert!(!html.contains("Please select"));
        reset();
    }

    #[test]
    fn the_opponents_submission_completes_the_round() {
        reset();
        enter_room();
        lock_in_brox("attacking");
        handed_over();
        opponent(|doc| {
            fists::submit(doc, THEM, ME, their_defence("liliel_healing_fairy"));
        });
        let (updates, redraw) = handed_over();
        assert_eq!(updates, 0, "nothing to send back");
        assert!(redraw.contains(&"/api/room/fists"));
        let html = handle_fists_get(BROX);
        assert!(html.contains("Combat Result"));
        assert!(html.contains("Did you win"));
        reset();
    }

    #[test]
    fn the_opponent_locking_in_first_is_noted_on_the_form() {
        reset();
        enter_room();
        opponent(|doc| {
            fists::submit(doc, THEM, ME, their_defence("liliel_healing_fairy"));
        });
        let html = handle_fists_get(BROX);
        assert!(html.contains("Lock In Choice"));
        assert!(html.contains("Opponent has locked in their choice"));
        reset();
    }

    #[test]
    fn the_form_comes_back_for_this_card_when_the_opponent_cancels() {
        reset();
        enter_room();
        lock_in_brox("attacking");
        opponent(|doc| {
            assert!(fists::new_round(doc, THEM));
        });
        let html = handle_fists_get(BROX);
        assert!(html.contains("Brox The Defiant"));
        assert!(html.contains("Lock In Choice"));
        assert!(checked(&html).is_empty());
        reset();
    }

    #[test]
    fn resume_restores_a_round_in_progress() {
        reset();
        enter_room();
        lock_in_brox("attacking");
        let state = crdt::encode_full_state();

        // A page change: the engine starts from nothing.
        reset();
        assert!(!room::in_room());

        handle_resume_post(&format!(
            "code=ABCD&name=Test&player={}&peer={}&state={}",
            ME, THEM, state
        ));
        let html = handle_fists_get("?card=liliel_healing_fairy");
        assert!(html.contains("Choice Locked In"));
        room::with_room(|r| {
            assert_eq!(r.fists.local.as_ref().unwrap().card, "brox_the_defiant");
        });

        // The opponent, who stayed, locks in: the round completes here.
        opponent(|doc| {
            fists::submit(doc, THEM, ME, their_defence("liliel_healing_fairy"));
        });
        assert!(handle_fists_get("?card=liliel_healing_fairy").contains("Combat Result"));
        reset();
    }

    #[test]
    fn fists_complete_shows_result() {
        reset();
        room::with_room_mut(|r| {
            r.connected = true;
            r.fists.local = Some(FistsSubmission {
                role: CombatRole::Attacking,
                card: "brox_the_defiant".to_string(),
                keal_idx: 1,
            });
            r.fists.remote = Some(FistsSubmission {
                role: CombatRole::Defending,
                card: "liliel_healing_fairy".to_string(),
                keal_idx: 1,
            });
        });
        let html = render_fists_result();
        assert!(html.contains("Combat Result"));
        assert!(html.contains("ATTACKER"));
        assert!(html.contains("DEFENDER"));
        assert!(html.contains("Brox The Defiant"));
        assert!(html.contains("Liliel: Healing Fairy"));
        assert!(html.contains("Attack Die Modifier"));
        assert!(html.contains("Did you win"));
        assert!(html.contains(r#"data-engine-body="action=outcome&won=yes""#));
        assert!(html.contains(r#"data-engine-body="action=outcome&won=no""#));
        reset();
    }

    #[test]
    fn fists_outcome_attacker_wins_marks_defender_damage() {
        reset();
        enter_room();
        lock_in_brox("defending");
        opponent(|doc| fists::submit(doc, THEM, ME, their_attack("liliel_healing_fairy")));
        handed_over();

        let html = tap("action=outcome&won=no"); // defender says no → attacker won
        assert!(html.contains("Ouch"));
        assert!(html.contains("Damage has been recorded"));
        assert!(html.contains("New Round"));
        // The answer goes to the opponent, and the card's panel, a view of
        // its own that shows the damage, is drawn again.
        assert_eq!(handed_over(), (1, vec!["/api/card/panel"]));
        // Verify slot 1 of brox was toggled in PLAYER_DOC
        assert!(player_doc::get_slot("brox_the_defiant", 1));
        assert!(!player_doc::get_slot("brox_the_defiant", 2));
        reset();
    }

    #[test]
    fn damage_is_marked_once_when_both_players_answer() {
        reset();
        enter_room();
        lock_in_brox("defending");
        opponent(|doc| fists::submit(doc, THEM, ME, their_attack("liliel_healing_fairy")));

        // This player answers...
        tap("action=outcome&won=no");
        assert!(player_doc::get_slot("brox_the_defiant", 1));
        handed_over();

        // ...and so does the opponent, with the same result.
        opponent(|doc| {
            assert!(fists::report_outcome(doc, THEM, true));
        });
        let (updates, redraw) = handed_over();
        assert_eq!(updates, 0);
        assert!(!redraw.contains(&"/api/card/panel"));

        // Reopening the dialog, or the answer arriving again, marks nothing more.
        let html = handle_fists_get(BROX);
        assert!(html.contains("Damage has been recorded"));
        opponent(|doc| {
            fists::report_outcome(doc, THEM, true);
        });
        assert!(!player_doc::get_slot("brox_the_defiant", 2));
        reset();
    }

    #[test]
    fn an_outcome_reported_by_the_opponent_marks_damage_here() {
        reset();
        enter_room();
        lock_in_brox("defending");
        opponent(|doc| fists::submit(doc, THEM, ME, their_attack("liliel_healing_fairy")));
        handed_over();

        opponent(|doc| {
            assert!(fists::report_outcome(doc, THEM, true));
        });
        let (updates, redraw) = handed_over();
        // Applying it is recorded in the room document and sent back.
        assert_eq!(updates, 1);
        assert!(redraw.contains(&"/api/card/panel"));
        assert!(redraw.contains(&"/api/room/fists"));
        assert!(player_doc::get_slot("brox_the_defiant", 1));

        let html = handle_fists_get(BROX);
        assert!(html.contains("Ouch"));
        reset();
    }

    #[test]
    fn a_finished_round_is_applied_even_if_the_opponent_already_moved_on() {
        reset();
        enter_room();
        lock_in_brox("defending");
        handed_over();

        // While this player was away the opponent attacked, reported a win,
        // closed the dialog and locked in for the next round. It all arrives
        // at once.
        opponent(|doc| {
            fists::submit(doc, THEM, ME, their_attack("liliel_healing_fairy"));
            assert!(fists::report_outcome(doc, THEM, true));
            assert!(fists::close(doc, THEM, ME));
            fists::submit(doc, THEM, ME, their_attack("liliel_healing_fairy"));
        });
        let (_, redraw) = handed_over();
        assert!(redraw.contains(&"/api/card/panel"));
        assert!(player_doc::get_slot("brox_the_defiant", 1));

        let html = handle_fists_get(BROX);
        assert!(html.contains("Lock In Choice"));
        assert!(html.contains("Opponent has locked in their choice"));
        reset();
    }

    #[test]
    fn a_disputed_outcome_is_reported_and_marks_nothing_more() {
        reset();
        enter_room();
        lock_in_brox("defending");
        opponent(|doc| fists::submit(doc, THEM, ME, their_attack("liliel_healing_fairy")));

        // This player says the defence held; the opponent says the attack won.
        tap("action=outcome&won=yes");
        handed_over();
        opponent(|doc| {
            assert!(fists::report_outcome(doc, THEM, true));
        });
        let (_, redraw) = handed_over();
        assert!(!redraw.contains(&"/api/card/panel"));
        assert!(!player_doc::get_slot("brox_the_defiant", 1));

        let html = handle_fists_get(BROX);
        assert!(html.contains("reported different results"));
        assert!(html.contains("New Round"));
        reset();
    }

    #[test]
    fn fists_outcome_defender_wins_shows_message_no_buttons() {
        reset();
        enter_room();
        lock_in_brox("attacking");
        opponent(|doc| fists::submit(doc, THEM, ME, their_defence("liliel_healing_fairy")));
        handed_over();

        let html = tap("action=outcome&won=no"); // attacker says no → defender won
        assert!(html.contains("Too bad"));
        // Defender-won outcomes have no action buttons — closing the dialog
        // after an outcome starts the next round.
        assert!(!html.contains("New Round"));
        assert!(!html.contains("<button"));
        assert_eq!(handed_over(), (1, vec![]));
        reset();
    }

    #[test]
    fn fists_outcome_attacker_sees_the_win() {
        reset();
        enter_room();
        lock_in_brox("attacking");
        opponent(|doc| fists::submit(doc, THEM, ME, their_defence("liliel_healing_fairy")));
        handed_over();

        let html = tap("action=outcome&won=yes");
        assert!(html.contains("Nice Play"));
        assert!(html.contains("New Round"));
        // The attacker's own card is untouched.
        assert_eq!(handed_over(), (1, vec![]));
        assert!(!player_doc::get_slot("brox_the_defiant", 1));
        reset();
    }

    #[test]
    fn an_answer_without_a_round_or_without_sense_changes_nothing() {
        reset();
        enter_room();
        let html = tap("action=outcome&won=yes");
        assert!(html.contains("Lock In Choice"));

        lock_in_brox("attacking");
        opponent(|doc| fists::submit(doc, THEM, ME, their_defence("liliel_healing_fairy")));
        handed_over();
        let html = tap("action=outcome&won=maybe");
        assert!(html.contains("Did you win"));
        assert_eq!(handed_over(), (0, vec![]));
        reset();
    }

    #[test]
    fn fists_reset_starts_a_new_round_for_both() {
        reset();
        enter_room();
        lock_in_brox("attacking");
        opponent(|doc| fists::submit(doc, THEM, ME, their_defence("liliel_healing_fairy")));
        handed_over();

        let html = tap("action=reset");
        assert!(html.contains("Lock In Choice"));
        assert!(!html.contains("Opponent has locked in"));
        assert_eq!(handed_over(), (1, vec![]));
        room::with_room(|r| {
            assert!(r.fists.local.is_none());
            assert!(r.fists.remote.is_none());
        });

        // Nothing to reset: nothing to send.
        tap("action=reset");
        assert_eq!(handed_over(), (0, vec![]));
        reset();
    }

    #[test]
    fn closing_the_dialog_keeps_a_round_in_progress() {
        reset();
        enter_room();
        lock_in_brox("attacking");
        handed_over();

        let html = tap("action=close");
        assert!(html.contains("Waiting for opponent"));
        assert_eq!(handed_over().0, 0, "closing mid-round changes nothing");
        room::with_room(|r| assert!(r.fists.local.is_some()));

        opponent(|doc| fists::submit(doc, THEM, ME, their_defence("liliel_healing_fairy")));
        handed_over();
        let html = tap("action=close");
        assert!(html.contains("Combat Result"));
        assert_eq!(handed_over().0, 0, "closing on the result changes nothing");
        reset();
    }

    #[test]
    fn closing_the_dialog_after_an_outcome_starts_the_next_round() {
        reset();
        enter_room();
        lock_in_brox("attacking");
        opponent(|doc| fists::submit(doc, THEM, ME, their_defence("liliel_healing_fairy")));
        tap("action=outcome&won=no");
        handed_over();

        let html = tap("action=close");
        assert!(html.contains("Lock In Choice"));
        assert_eq!(handed_over().0, 1);
        reset();
    }

    #[test]
    fn closing_the_dialog_drops_picks_that_were_not_locked_in() {
        reset();
        enter_room();
        tap("action=choose&card=brox_the_defiant&role=attacking");
        tap("action=lock&card=brox_the_defiant"); // refused: no keal means
        let html = tap("action=close");
        assert!(checked(&html).is_empty());
        assert!(!html.contains("Please select"));
        reset();
    }

    #[test]
    fn create_and_join_start_with_no_combat_and_no_known_opponent() {
        reset();
        enter_room();
        lock_in_brox("attacking");
        tap("action=reset");
        tap("action=choose&card=brox_the_defiant&role=attacking");
        handle_disconnect_post("");

        handle_join_post("code=WXYZ&name=Other&player=me2");
        room::with_room(|r| {
            assert_eq!(r.player_id, "me2");
            assert!(r.peer_id.is_empty());
            assert!(r.fists.local.is_none());
        });
        let html = handle_fists_get(BROX);
        assert!(html.contains("Lock In Choice"));
        assert!(checked(&html).is_empty());
        reset();
    }

    #[test]
    fn a_missing_or_malformed_player_id_gets_a_random_one() {
        reset();
        handle_create_post("code=ABCD&name=Test");
        let first = room::with_room(|r| r.player_id.clone());
        assert!(fists::is_valid_player_id(&first));
        handle_disconnect_post("");
        handle_create_post("code=ABCD&name=Test&player=bad%20id");
        let second = room::with_room(|r| r.player_id.clone());
        assert!(fists::is_valid_player_id(&second));
        assert_ne!(first, second);
        reset();
    }

    #[test]
    fn fists_result_no_final_blows_or_motivation_mods_in_regular_result() {
        reset();
        room::with_room_mut(|r| {
            r.connected = true;
            r.fists.local = Some(FistsSubmission {
                role: CombatRole::Attacking,
                card: "brox_the_defiant".to_string(),
                keal_idx: 1,
            });
            r.fists.remote = Some(FistsSubmission {
                role: CombatRole::Defending,
                card: "liliel_healing_fairy".to_string(),
                keal_idx: 1,
            });
        });
        let html = render_fists_result();
        // Regular combat should NOT show Final Blows or Motivation Modifiers
        assert!(!html.contains("Final Blows"));
        assert!(!html.contains("Motivation Modifiers"));
        assert!(html.contains("Combat Result"));
        assert!(html.contains("Did you win"));
        reset();
    }

    #[test]
    fn fists_form_shows_final_blows_when_all_keal_exhausted() {
        reset();
        enter_room();
        exhaust_brox_keal_means(1);
        exhaust_brox_keal_means(2);
        let html = handle_fists_get(BROX);
        assert!(html.contains("Final Blows"));
        assert!(html.contains("All keal means are exhausted"));
        assert!(html.contains("D20"));
        assert!(html.contains("Your Motivation"));
        // Should NOT contain the normal role selector
        assert!(!html.contains("Your Role"));
        assert!(!html.contains("Lock In Choice"));
        reset();
    }

    #[test]
    fn stating_final_blows_waits_for_the_opponent() {
        reset();
        enter_room();
        exhaust_brox_keal_means(1);
        exhaust_brox_keal_means(2);

        // The opponent has attacked already: the Final Blows form says so.
        opponent(|doc| fists::submit(doc, THEM, ME, their_attack("liliel_healing_fairy")));
        let html = handle_fists_get(BROX);
        assert!(html.contains("State Final Blows to Opponent"));
        assert!(html.contains("Opponent has locked in their choice"));
        handed_over();

        let html = tap("action=final&card=brox_the_defiant");
        assert!(html.contains("Final Blows"));
        assert!(html.contains("Did you win"));
        // Stated to the opponent; and the card's first fight today shows on
        // its panel.
        assert_eq!(handed_over(), (1, vec!["/api/card/panel"]));
        reset();
    }

    #[test]
    fn final_blows_stated_first_waits_and_can_be_called_off() {
        reset();
        enter_room();
        exhaust_brox_keal_means(1);
        exhaust_brox_keal_means(2);
        let html = tap("action=final&card=brox_the_defiant");
        assert!(html.contains("Final Blows Stated"));
        assert!(html.contains("Cancel round"));
        assert_eq!(handed_over().0, 1);

        let html = tap("action=reset");
        assert!(html.contains("State Final Blows to Opponent"));
        reset();
    }

    #[test]
    fn final_blows_cannot_be_stated_while_a_keal_means_is_left() {
        reset();
        enter_room();
        exhaust_brox_keal_means(1);
        let html = tap("action=final&card=brox_the_defiant");
        assert!(html.contains("Lock In Choice"));
        assert_eq!(handed_over(), (0, vec![]));
        reset();
    }

    #[test]
    fn fists_form_normal_when_keal_not_exhausted() {
        reset();
        enter_room();
        let html = handle_fists_get(BROX);
        assert!(!html.contains("Final Blows"));
        assert!(html.contains("Your Role"));
        assert!(html.contains("Lock In Choice"));
        reset();
    }

    #[test]
    fn fists_no_card_shows_guidance() {
        reset();
        enter_room();
        let html = handle_fists_get("");
        assert!(html.contains("Navigate to a Character"));
        reset();
    }

    #[test]
    fn a_card_that_is_not_in_the_catalog_is_named_safely() {
        reset();
        enter_room();
        let html = handle_fists_get("?card=%3Cb%3Ex");
        assert!(html.contains("Card not found"));
        assert!(!html.contains("<b>"));
        reset();
    }
}
