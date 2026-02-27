//! `/api/room/*` routes — multiplayer room management and fists combat.
//!
//! Phase 4: Room state is global (shared between peers via WebRTC).
//! Local game state (damage, turns) remains per-user.

use crate::cards_generated::CARDS;
use crate::game::crdt;
use crate::game::damage;
use crate::game::player_doc;
use crate::game::room::{self, CombatRole, FistsSubmission};
use crate::game::turns;
use crate::routes::util::{get_param, parse_form_body, parse_query};
use crate::typing;

fn find_card(slug: &str) -> Option<&'static crate::cards_generated::Card> {
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

    room::with_room_mut(|r| {
        r.room_code = code.to_uppercase();
        r.room_name = name.to_string();
        r.connected = false;
        r.fists.reset();
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

    r#"<span class="text-emerald-600 text-sm">Room created. Waiting for peer...</span>"#
        .to_string()
}

// ── POST /api/room/join ────────────────────────────────────────────

pub fn handle_join_post(body: &str) -> String {
    let params = parse_form_body(body);
    let code = get_param(&params, "code").unwrap_or("");
    let name = get_param(&params, "name").unwrap_or("");
    let skip_seed = get_param(&params, "skip_seed").unwrap_or("false") == "true";

    room::with_room_mut(|r| {
        r.room_code = code.to_uppercase();
        if !name.is_empty() {
            r.room_name = name.to_string();
        }
        r.connected = false;
        r.fists.reset();
    });

    // Initialize yrs CRDT Doc for this multiplayer session.
    // See handle_create_post for skip_seed rationale.
    crdt::init_doc();
    if !skip_seed {
        crdt::seed_from_local();
    }

    room::with_room(|r| render_waiting_status(r))
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

// ── GET /api/room/fists ────────────────────────────────────────────

pub fn handle_fists_get(query: &str) -> String {
    let params = parse_query(query);
    let slug = get_param(&params, "card").unwrap_or("");

    let (connected, in_room) = room::with_room(|r| (r.connected, !r.room_code.is_empty()));
    if !connected {
        if in_room {
            return r#"<div class="p-4 text-kip-drk-sienna"><p class="text-sm text-amber-600">Waiting for peer to connect…</p></div>"#.to_string();
        }
        return r#"<div class="p-4 text-kip-drk-sienna"><p class="text-sm text-kip-red">Not connected to a room. Use the fields above to create or join one.</p></div>"#.to_string();
    }

    // Check if we already have a result
    let is_complete = room::with_room(|r| r.fists.is_complete());
    if is_complete {
        return render_fists_result();
    }

    // Check if local already submitted (waiting for remote)
    let local_submitted = room::with_room(|r| r.fists.local.is_some());
    if local_submitted {
        return render_fists_waiting();
    }

    // Check if local submitted Final Blows (waiting for remote)
    let local_final_submitted = room::with_room(|r| r.fists.local_final_blows.is_some());
    if local_final_submitted {
        return render_final_blows_waiting();
    }

    render_fists_form(slug)
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
fn render_final_blows(card: &crate::cards_generated::Card) -> String {
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
        r#"<button onclick="kipukasMultiplayer.submitFinalBlows('{}')" class="w-full bg-kip-red hover:bg-kip-drk-sienna text-amber-50 font-bold py-2 px-4 rounded text-sm">State Final Blows to Opponent</button>"#,
        card.slug
    ));

    // Dedicated message area at the bottom
    h.push_str(r#"<div id="fists-message" class="mt-2 text-center"></div>"#);

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
    // Poll for result every 2s via HTMX
    h.push_str(r##"<div hx-get="/api/room/fists/poll" hx-trigger="every 2s" hx-target="#fists-container" hx-swap="innerHTML"></div>"##);
    h.push_str(r#"</div>"#);
    h
}

fn render_fists_form(slug: &str) -> String {
    let card = match find_card(slug) {
        Some(c) => c,
        None if slug.is_empty() => return render_fists_no_card(),
        None => {
            return format!(
                r#"<div class="p-4"><span class="text-kip-red">Card not found: {}</span></div>"#,
                slug
            );
        }
    };

    if card.keal_means.is_empty() {
        return r#"<div class="p-4 text-kip-drk-sienna"><p class="text-sm">This card has no keal means for combat.</p></div>"#.to_string();
    }

    // If all keal means are exhausted, show Final Blows instead of the normal form
    if all_keal_means_exhausted(slug) {
        return render_final_blows(card);
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
    h.push_str(r#"<label class="flex-1 text-center"><input type="radio" name="fists-role" value="attacking" class="mr-1 text-kip-red focus:ring-kip-red">Attacking</label>"#);
    h.push_str(r#"<label class="flex-1 text-center"><input type="radio" name="fists-role" value="defending" class="mr-1 text-kip-red focus:ring-kip-red">Defending</label>"#);
    h.push_str(r#"</div>"#);

    // Keal means selector — disable groups where all damage slots are checked
    h.push_str(r#"<p class="text-sm mb-2 font-bold">Select Keal Means:</p>"#);

    let mut slot_start: u8 = 1;
    let mut idx: u8 = 1;
    for km in card.keal_means {
        // Check if all slots in this keal means group are checked (exhausted)
        let all_checked = if km.count > 0 {
            (slot_start..slot_start + km.count)
                .all(|s| player_doc::get_slot(slug, s))
        } else {
            false
        };
        slot_start += km.count;

        let genetics_str = km.genetics.join("-");
        if all_checked {
            // Disabled: all checkboxes used up
            h.push_str(&format!(
                r#"<label class="block mb-1 text-sm opacity-40 line-through"><input type="radio" name="fists-keal" value="{}" class="mr-1" disabled><span class="font-bold">{}</span> <span class="text-xs">({})</span></label>"#,
                idx, km.name, genetics_str
            ));
        } else {
            h.push_str(&format!(
                r#"<label class="block mb-1 text-sm"><input type="radio" name="fists-keal" value="{}" class="mr-1 text-kip-red focus:ring-kip-red"><span class="text-kip-red font-bold">{}</span> <span class="text-xs">({})</span></label>"#,
                idx, km.name, genetics_str
            ));
        }
        idx += 1;
    }

    // Submit button
    h.push_str(&format!(
        r#"<button onclick="kipukasMultiplayer.submitFists('{}')" class="w-full bg-kip-red hover:bg-kip-drk-sienna text-amber-50 font-bold py-2 px-4 rounded mt-3 text-sm">Lock In Choice</button>"#,
        slug
    ));

    // Dedicated message area at the bottom (used by JS for status messages)
    h.push_str(r#"<div id="fists-message" class="mt-2 text-center"></div>"#);

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
    // Poll for result every 2s via HTMX
    h.push_str(r##"<div hx-get="/api/room/fists/poll" hx-trigger="every 2s" hx-target="#fists-container" hx-swap="innerHTML"></div>"##);
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
    h.push_str(r#"<button onclick="kipukasMultiplayer.resetFists()" class="w-full bg-kip-red hover:bg-kip-drk-sienna text-amber-50 font-bold py-2 px-4 rounded text-sm">Try Again</button>"#);
    h.push_str(r#"</div>"#);
    h
}

// ── POST /api/room/fists ───────────────────────────────────────────

pub fn handle_fists_post(body: &str) -> String {
    let params = parse_form_body(body);
    let role_str = get_param(&params, "role").unwrap_or("");
    let card = get_param(&params, "card").unwrap_or("");
    let keal_str = get_param(&params, "keal").unwrap_or("1");

    let role = match role_str {
        "attacking" => CombatRole::Attacking,
        "defending" => CombatRole::Defending,
        _ => {
            return r#"<span class="text-kip-red text-sm">Please select Attacking or Defending</span>"#.to_string();
        }
    };

    let keal_idx: u8 = keal_str.parse().unwrap_or(1);

    if card.is_empty() {
        return r#"<span class="text-kip-red text-sm">Missing card</span>"#.to_string();
    }

    room::with_room_mut(|r| {
        r.fists.local = Some(FistsSubmission {
            role,
            card: card.to_string(),
            keal_idx,
        });
    });

    // Always send local submission to peer via data channel
    let fists_json = room::export_fists_json();

    // Check if both submitted
    let is_complete = room::with_room(|r| r.fists.is_complete());
    if is_complete {
        let mut h = render_fists_result();
        h.push_str(&format!(
            r#"<script>if(window.kipukasMultiplayer)kipukasMultiplayer.sendFists({});</script>"#,
            fists_json
        ));
        h
    } else {
        // Return the local submission as JSON for WebRTC send, plus waiting UI
        let mut h = render_fists_waiting();
        h.push_str(&format!(
            r#"<script>if(window.kipukasMultiplayer)kipukasMultiplayer.sendFists({});</script>"#,
            fists_json
        ));
        h
    }
}

// ── POST /api/room/fists/sync ──────────────────────────────────────

pub fn handle_fists_sync_post(body: &str) -> String {
    // Body is JSON: a FistsSubmission from the remote peer
    match serde_json::from_str::<FistsSubmission>(body) {
        Ok(submission) => {
            room::with_room_mut(|r| {
                r.fists.remote = Some(submission);
            });

            let is_complete = room::with_room(|r| r.fists.is_complete());
            if is_complete {
                render_fists_result()
            } else {
                r#"<span class="text-emerald-600 text-sm">Opponent's choice received. Waiting for your submission.</span>"#.to_string()
            }
        }
        Err(e) => {
            format!(
                r#"<span class="text-kip-red text-sm">Sync error: {}</span>"#,
                e
            )
        }
    }
}

// ── GET /api/room/fists/poll ───────────────────────────────────────

pub fn handle_fists_poll_get(_query: &str) -> String {
    let is_complete = room::with_room(|r| r.fists.is_complete());
    if is_complete {
        render_fists_result()
    } else {
        let is_final = room::with_room(|r| r.fists.local_final_blows.is_some());
        if is_final {
            render_final_blows_waiting()
        } else {
            render_fists_waiting()
        }
    }
}

// ── POST /api/room/fists/final ─────────────────────────────────────

/// Handle Final Blows submission from a player with exhausted keal means.
pub fn handle_final_blows_post(body: &str) -> String {
    let params = parse_form_body(body);
    let card = get_param(&params, "card").unwrap_or("");

    if card.is_empty() {
        return r#"<span class="text-kip-red text-sm">Missing card</span>"#.to_string();
    }

    room::with_room_mut(|r| {
        r.fists.local_final_blows = Some(room::FinalBlowsSubmission {
            card: card.to_string(),
        });
    });

    // Send to peer via data channel
    let json = serde_json::json!({ "card": card }).to_string();

    // Check if both sides have submitted (remote might have already submitted via normal fists)
    let is_complete = room::with_room(|r| r.fists.is_complete());
    if is_complete {
        let mut h = render_fists_result();
        h.push_str(&format!(
            r#"<script>if(window.kipukasMultiplayer)kipukasMultiplayer.sendFinalBlows({});</script>"#,
            json
        ));
        h
    } else {
        let mut h = render_final_blows_waiting();
        h.push_str(&format!(
            r#"<script>if(window.kipukasMultiplayer)kipukasMultiplayer.sendFinalBlows({});</script>"#,
            json
        ));
        h
    }
}

// ── POST /api/room/fists/final/sync ────────────────────────────────

/// Handle Final Blows sync from remote peer.
pub fn handle_final_blows_sync_post(body: &str) -> String {
    // Body is JSON: { "card": "slug" }
    match serde_json::from_str::<room::FinalBlowsSubmission>(body) {
        Ok(submission) => {
            room::with_room_mut(|r| {
                r.fists.remote_final_blows = Some(submission);
            });

            let is_complete = room::with_room(|r| r.fists.is_complete());
            if is_complete {
                render_fists_result()
            } else {
                r#"<span class="text-emerald-600 text-sm">Opponent has submitted. Waiting for your submission.</span>"#.to_string()
            }
        }
        Err(e) => {
            format!(
                r#"<span class="text-kip-red text-sm">Final Blows sync error: {}</span>"#,
                e
            )
        }
    }
}

// ── POST /api/room/fists/reset ─────────────────────────────────────

pub fn handle_fists_reset_post(_body: &str) -> String {
    room::with_room_mut(|r| r.fists.reset());
    r#"<div class="p-4 text-center text-emerald-600 text-sm">Combat reset. Ready for next round.</div>"#.to_string()
}

// ── POST /api/room/fists/outcome ───────────────────────────────────

/// Handle the "Did you win?" answer.
///
/// Accepts either:
///   - `won=yes|no` — from the local player clicking a button
///   - `attacker_won=true|false` — from a peer sync via data channel
///
/// Logic:
///   1. Derive `attacker_won` from the local role and the `won` param (or use directly).
///   2. If attacker won AND the local player is the defender → auto-mark damage on the
///      next unchecked slot of the local card's keal means group that was used in combat.
///   3. Return role-aware HTML message with appropriate buttons.
pub fn handle_fists_outcome_post(body: &str) -> String {
    let params = parse_form_body(body);

    // Determine the effective local role. For Final Blows (no regular submission),
    // the Final Blows card is always the defender.
    let (local_role, is_final_blows) = room::with_room(|r| {
        let role = r.fists.local.as_ref().map(|s| s.role)
            .or_else(|| {
                // If local submitted Final Blows, treat as Defending
                r.fists.local_final_blows.as_ref().map(|_| CombatRole::Defending)
            });
        (role, r.fists.is_final_blows())
    });

    // Determine if attacker won
    let attacker_won = if let Some(aw) = get_param(&params, "attacker_won") {
        // Peer sync path — attacker_won is already resolved
        aw == "true"
    } else if let Some(won) = get_param(&params, "won") {
        // Local click path — derive from role
        match (local_role, won) {
            (Some(CombatRole::Attacking), "yes") => true,
            (Some(CombatRole::Attacking), "no") => false,
            (Some(CombatRole::Defending), "yes") => false,
            (Some(CombatRole::Defending), "no") => true,
            _ => {
                return r#"<div class="p-4 text-kip-red text-sm">Error: No combat data.</div>"#
                    .to_string();
            }
        }
    } else {
        return r#"<div class="p-4 text-kip-red text-sm">Error: Missing outcome param.</div>"#
            .to_string();
    };

    let local_role = local_role.unwrap_or(CombatRole::Attacking);

    // Get local card and keal info from either regular or Final Blows submission
    let (local_card, local_keal_idx) = room::with_room(|r| {
        if let Some(ref s) = r.fists.local {
            (s.card.clone(), s.keal_idx)
        } else if let Some(ref f) = r.fists.local_final_blows {
            (f.card.clone(), 0)
        } else {
            (String::new(), 0)
        }
    });

    let mut h = String::with_capacity(1024);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna text-center">"#);

    if attacker_won {
        // Attacker won — defender lost
        if local_role == CombatRole::Defending {
            if is_final_blows && !local_card.is_empty() {
                // Final Blows: defender lost → card is wasted
                h.push_str(r#"<p class="text-2xl mb-2">&#x1F480;</p>"#);
                h.push_str(r#"<p class="text-lg font-bold mb-2 text-kip-red">Yikes! Your card was wasted, remove it from the field.</p>"#);

                // Auto-mark the wasted checkbox on the local card
                damage::toggle_wasted(&local_card);
                h.push_str(r#"<p class="text-sm mb-3">The final blows indicator has been marked.</p>"#);

            } else {
                // Regular combat: defender lost — auto-mark damage
                h.push_str(r#"<p class="text-2xl mb-2">&#x1F4A5;</p>"#);
                h.push_str(r#"<p class="text-lg font-bold mb-2 text-kip-red">Ouch, let me mark that for you.</p>"#);

                let marked = auto_mark_damage(&local_card, local_keal_idx);
                if marked {
                    h.push_str(r#"<p class="text-sm mb-3">Damage has been recorded on your card.</p>"#);
                } else {
                    h.push_str(r#"<p class="text-sm mb-3">All slots in that keal means are already marked.</p>"#);
                }

                h.push_str(r#"<button onclick="kipukasMultiplayer.resetFists()" class="w-full bg-emerald-600 hover:bg-emerald-700 text-amber-50 font-bold py-2 px-4 rounded text-sm">New Round</button>"#);
            }
        } else {
            // I'm the attacker and I won
            if is_final_blows {
                // Final Blows: attacker won → opponent's card is wasted
                h.push_str(r#"<p class="text-2xl mb-2">&#x1F3C6;</p>"#);
                h.push_str(r#"<p class="text-lg font-bold mb-2 text-emerald-600">You clinched victory! Keep pushing.</p>"#);
            } else {
                // Regular combat: attacker won
                h.push_str(r#"<p class="text-2xl mb-2">&#x2694;</p>"#);
                h.push_str(r#"<p class="text-lg font-bold mb-2 text-emerald-600">Nice Play! Damage is now reflected on the opponent.</p>"#);

                h.push_str(r#"<button onclick="kipukasMultiplayer.resetFists()" class="w-full bg-emerald-600 hover:bg-emerald-700 text-amber-50 font-bold py-2 px-4 rounded text-sm">New Round</button>"#);
            }
        }
    } else {
        // Defender won — same messaging for both regular and Final Blows.
        if local_role == CombatRole::Defending {
            h.push_str(r#"<p class="text-2xl mb-2">&#x1F6E1;</p>"#);
            h.push_str(r#"<p class="text-lg font-bold mb-2 text-emerald-600">Great defense, keep it up!</p>"#);
        } else {
            h.push_str(r#"<p class="text-2xl mb-2">&#x1F614;</p>"#);
            h.push_str(r#"<p class="text-lg font-bold mb-2 text-amber-600">Too bad, try next turn!</p>"#);
        }
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
/// Returns fresh alarm list HTML for the multiplayer turn tracker.
pub fn handle_yrs_apply_post(body: &str) -> String {
    let params = parse_form_body(body);
    let update = get_param(&params, "update").unwrap_or(body.trim());
    match crdt::apply_update(update) {
        Ok(()) => turns::render_alarm_list(),
        Err(e) => format!(
            r#"<span class="text-kip-red text-sm">CRDT apply error: {}</span>"#,
            e
        ),
    }
}

/// POST /api/room/yrs/alarm/add — add an alarm via yrs CRDT.
/// Returns JSON: { "update": "<base64>", "html": "<alarm list>" }
pub fn handle_yrs_alarm_add_post(body: &str) -> String {
    let params = parse_form_body(body);
    let turns_str = get_param(&params, "turns").unwrap_or("1");
    let name = get_param(&params, "name").unwrap_or("");
    let color_set = get_param(&params, "color_set").unwrap_or("red");
    let turns_val: i32 = turns_str.parse().unwrap_or(1).max(1).min(99);

    let update = crdt::add_alarm(turns_val, name, color_set);
    // Keep PLAYER_DOC in sync so page navigation shows correct alarms
    // (the #turn-alarms hx-trigger="load" reads from PLAYER_DOC).
    crdt::export_to_local();
    let html = turns::render_alarm_list();

    format!(
        r#"{{"update":"{}","html":{}}}"#,
        update,
        serde_json::to_string(&html).unwrap_or_else(|_| "\"\"".to_string())
    )
}

/// POST /api/room/yrs/alarm/tick — tick all alarms via yrs CRDT.
/// Returns JSON: { "update": "<base64>", "html": "<alarm list>" }
pub fn handle_yrs_alarm_tick_post(_body: &str) -> String {
    let update = crdt::tick_alarms();
    // Keep PLAYER_DOC in sync so page navigation shows correct alarms.
    crdt::export_to_local();
    let html = turns::render_alarm_list();

    format!(
        r#"{{"update":"{}","html":{}}}"#,
        update,
        serde_json::to_string(&html).unwrap_or_else(|_| "\"\"".to_string())
    )
}

/// POST /api/room/yrs/alarm/toggle — toggle alarm visibility then re-render.
/// Returns the multiplayer alarm list HTML (reads from CRDT Doc).
pub fn handle_yrs_alarm_toggle_post(_body: &str) -> String {
    turns::toggle_alarms_visibility();
    turns::render_alarm_list()
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

/// POST /api/room/yrs/alarm/remove — remove an alarm by index via yrs CRDT.
/// Returns JSON: { "update": "<base64>", "html": "<alarm list>" }
pub fn handle_yrs_alarm_remove_post(body: &str) -> String {
    let params = parse_form_body(body);
    let idx_str = get_param(&params, "index").unwrap_or("0");
    let idx: u32 = idx_str.parse().unwrap_or(0);

    let update = crdt::remove_alarm(idx);
    // Keep PLAYER_DOC in sync so page navigation shows correct alarms.
    crdt::export_to_local();
    let html = turns::render_alarm_list();

    format!(
        r#"{{"update":"{}","html":{}}}"#,
        update,
        serde_json::to_string(&html).unwrap_or_else(|_| "\"\"".to_string())
    )
}

// ── Result rendering ───────────────────────────────────────────────

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
    h.push_str(r#"<div class="mt-3 border-t border-slate-300 pt-3">"#);
    h.push_str(r#"<p class="text-sm font-bold text-center mb-2">Did you win?</p>"#);
    h.push_str(r#"<div class="flex gap-2">"#);
    h.push_str(r#"<button onclick="kipukasMultiplayer.reportOutcome('yes')" class="flex-1 bg-emerald-600 hover:bg-emerald-700 text-amber-50 font-bold py-2 px-4 rounded text-sm">Yes!</button>"#);
    h.push_str(r#"<button onclick="kipukasMultiplayer.reportOutcome('no')" class="flex-1 bg-kip-red hover:bg-kip-drk-sienna text-amber-50 font-bold py-2 px-4 rounded text-sm">No</button>"#);
    h.push_str(r#"</div>"#);
    h.push_str(r#"</div>"#);

    h.push_str(r#"</div>"#);
    h
}

fn build_result_html(
    atk_card: &crate::cards_generated::Card,
    atk_slug: &str,
    atk_km: &crate::cards_generated::KealMeans,
    def_card: &crate::cards_generated::Card,
    def_slug: &str,
    def_km: &crate::cards_generated::KealMeans,
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
    h.push_str(r#"<div class="mt-3 border-t border-slate-300 pt-3">"#);
    h.push_str(r#"<p class="text-sm font-bold text-center mb-2">Did you win?</p>"#);
    h.push_str(r#"<div class="flex gap-2">"#);
    h.push_str(r#"<button onclick="kipukasMultiplayer.reportOutcome('yes')" class="flex-1 bg-emerald-600 hover:bg-emerald-700 text-amber-50 font-bold py-2 px-4 rounded text-sm">Yes!</button>"#);
    h.push_str(r#"<button onclick="kipukasMultiplayer.reportOutcome('no')" class="flex-1 bg-kip-red hover:bg-kip-drk-sienna text-amber-50 font-bold py-2 px-4 rounded text-sm">No</button>"#);
    h.push_str(r#"</div>"#);
    h.push_str(r#"</div>"#);

    h.push_str(r#"</div>"#);
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        room::reset_room();
        crate::game::player_doc::init_player_doc();
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
        // Join no longer sets connected=true; that happens after WebRTC connects
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
        assert!(html.contains("resetFists"));
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
        let html = handle_fists_get("?card=brox_the_defiant");
        assert!(html.contains("Not connected"));
        reset();
    }

    #[test]
    fn fists_get_shows_form_when_connected() {
        reset();
        room::with_room_mut(|r| r.connected = true);
        let html = handle_fists_get("?card=brox_the_defiant");
        assert!(html.contains("Brox The Defiant"));
        assert!(html.contains("Crushing Hope"));
        assert!(html.contains("Chain Raid"));
        assert!(html.contains("Attacking"));
        assert!(html.contains("Defending"));
        reset();
    }

    #[test]
    fn fists_post_stores_local() {
        reset();
        room::with_room_mut(|r| r.connected = true);
        let html = handle_fists_post("role=attacking&card=brox_the_defiant&keal=1");
        assert!(html.contains("Waiting for opponent"));
        room::with_room(|r| {
            assert!(r.fists.local.is_some());
            let local = r.fists.local.as_ref().unwrap();
            assert_eq!(local.role, CombatRole::Attacking);
            assert_eq!(local.card, "brox_the_defiant");
        });
        reset();
    }

    #[test]
    fn fists_sync_stores_remote() {
        reset();
        room::with_room_mut(|r| r.connected = true);
        let json = r#"{"role":"Defending","card":"liliel_healing_fairy","keal_idx":1}"#;
        let html = handle_fists_sync_post(json);
        assert!(html.contains("Waiting for your submission") || html.contains("Combat Result"));
        room::with_room(|r| assert!(r.fists.remote.is_some()));
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
        assert!(html.contains("reportOutcome"));
        reset();
    }

    #[test]
    fn fists_outcome_attacker_wins_marks_defender_damage() {
        reset();
        crate::game::player_doc::init_player_doc();
        room::with_room_mut(|r| {
            r.connected = true;
            r.fists.local = Some(FistsSubmission {
                role: CombatRole::Defending,
                card: "brox_the_defiant".to_string(),
                keal_idx: 1,
            });
            r.fists.remote = Some(FistsSubmission {
                role: CombatRole::Attacking,
                card: "liliel_healing_fairy".to_string(),
                keal_idx: 1,
            });
        });
        let html = handle_fists_outcome_post("won=no"); // defender says no → attacker won
        assert!(html.contains("Ouch"));
        assert!(html.contains("Damage has been recorded"));
        assert!(html.contains("New Round"));
        // Verify slot 1 of brox was toggled in PLAYER_DOC
        assert!(player_doc::get_slot("brox_the_defiant", 1));
        reset();
    }

    #[test]
    fn fists_outcome_defender_wins_shows_message_no_buttons() {
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
        let html = handle_fists_outcome_post("won=no"); // attacker says no → defender won
        assert!(html.contains("Too bad"));
        // Defender-won outcomes have no action buttons — modal Close resets both clients
        assert!(!html.contains("New Round"));
        assert!(!html.contains("resetFists"));
        reset();
    }

    #[test]
    fn fists_outcome_peer_sync_attacker_won() {
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
        let html = handle_fists_outcome_post("attacker_won=true");
        assert!(html.contains("Nice Play"));
        assert!(html.contains("New Round"));
        reset();
    }

    #[test]
    fn fists_reset_clears() {
        reset();
        room::with_room_mut(|r| {
            r.connected = true;
            r.fists.local = Some(FistsSubmission {
                role: CombatRole::Attacking,
                card: "test".to_string(),
                keal_idx: 1,
            });
        });
        handle_fists_reset_post("");
        room::with_room(|r| {
            assert!(r.fists.local.is_none());
            assert!(r.fists.remote.is_none());
        });
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
        room::with_room_mut(|r| r.connected = true);
        // Brox has keal means — mark ALL slots as checked
        let card = find_card("brox_the_defiant").unwrap();
        let total: u8 = card.keal_means.iter().map(|k| k.count).sum();
        damage::ensure_card_state("brox_the_defiant", total);
        for slot in 1..=total {
            damage::toggle_slot("brox_the_defiant", slot);
        }
        let html = handle_fists_get("?card=brox_the_defiant");
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
    fn fists_form_normal_when_keal_not_exhausted() {
        reset();
        room::with_room_mut(|r| r.connected = true);
        let html = handle_fists_get("?card=brox_the_defiant");
        assert!(!html.contains("Final Blows"));
        assert!(html.contains("Your Role"));
        assert!(html.contains("Lock In Choice"));
        reset();
    }

    #[test]
    fn fists_no_card_shows_guidance() {
        reset();
        room::with_room_mut(|r| r.connected = true);
        let html = handle_fists_get("");
        assert!(html.contains("Navigate to a Character"));
        reset();
    }
}
