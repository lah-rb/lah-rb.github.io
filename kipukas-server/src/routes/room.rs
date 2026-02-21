//! `/api/room/*` routes — multiplayer room management and fists combat.
//!
//! Phase 4: Room state is global (shared between peers via WebRTC).
//! Local game state (damage, turns) remains per-user.

use crate::cards_generated::CARDS;
use crate::game::room::{self, CombatRole, FistsSubmission};
use crate::typing;

/// Parse URL-encoded form body into key-value pairs.
fn parse_form_body(body: &str) -> Vec<(String, String)> {
    if body.is_empty() {
        return Vec::new();
    }
    body.split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?;
            let val = parts.next().unwrap_or("");
            Some((percent_decode(key), percent_decode(val)))
        })
        .collect()
}

fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(b'0');
            let lo = chars.next().unwrap_or(b'0');
            let hex = [hi, lo];
            if let Ok(s) = core::str::from_utf8(&hex) {
                if let Ok(val) = u8::from_str_radix(s, 16) {
                    result.push(val as char);
                    continue;
                }
            }
            result.push('%');
            result.push(hi as char);
            result.push(lo as char);
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    let q = query.strip_prefix('?').unwrap_or(query);
    parse_form_body(q)
}

fn get_param<'a>(params: &'a [(String, String)], key: &str) -> Option<&'a str> {
    params.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

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

    room::with_room_mut(|r| {
        r.room_code = code.to_uppercase();
        r.room_name = name.to_string();
        r.connected = false;
        r.fists.reset();
    });

    r#"<span class="text-emerald-600 text-sm">Room created. Waiting for peer...</span>"#
        .to_string()
}

// ── POST /api/room/join ────────────────────────────────────────────

pub fn handle_join_post(body: &str) -> String {
    let params = parse_form_body(body);
    let code = get_param(&params, "code").unwrap_or("");
    let name = get_param(&params, "name").unwrap_or("");

    room::with_room_mut(|r| {
        r.room_code = code.to_uppercase();
        if !name.is_empty() {
            r.room_name = name.to_string();
        }
        r.connected = true;
        r.fists.reset();
    });

    room::with_room(|r| render_connected_status(r))
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
    room::reset_room();
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

    render_fists_form(slug)
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

    // Keal means selector
    h.push_str(r#"<p class="text-sm mb-2 font-bold">Select Keal Means:</p>"#);

    let mut idx: u8 = 1;
    for km in card.keal_means {
        let genetics_str = km.genetics.join("-");
        h.push_str(&format!(
            r#"<label class="block mb-1 text-sm"><input type="radio" name="fists-keal" value="{}" class="mr-1 text-kip-red focus:ring-kip-red"><span class="text-kip-red font-bold">{}</span> <span class="text-xs">({})</span></label>"#,
            idx, km.name, genetics_str
        ));
        idx += 1;
    }

    // Submit button
    h.push_str(&format!(
        r#"<button onclick="kipukasMultiplayer.submitFists('{}')" class="w-full bg-kip-red hover:bg-kip-drk-sienna text-amber-50 font-bold py-2 px-4 rounded mt-3 text-sm">Lock In Choice</button>"#,
        slug
    ));

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

    // Check if both submitted
    let is_complete = room::with_room(|r| r.fists.is_complete());
    if is_complete {
        render_fists_result()
    } else {
        // Return the local submission as JSON for WebRTC send, plus waiting UI
        let fists_json = room::export_fists_json();
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
        render_fists_waiting()
    }
}

// ── POST /api/room/fists/reset ─────────────────────────────────────

pub fn handle_fists_reset_post(_body: &str) -> String {
    room::with_room_mut(|r| r.fists.reset());
    r#"<div class="p-4 text-center text-emerald-600 text-sm">Combat reset. Ready for next round.</div>"#.to_string()
}

// ── GET /api/room/state ────────────────────────────────────────────

pub fn handle_room_state_get(_query: &str) -> String {
    room::export_room_json()
}

// ── Result rendering ───────────────────────────────────────────────

fn render_fists_result() -> String {
    room::with_room(|r| {
        // Check for same-role conflict first
        if r.fists.same_role() {
            return render_same_role_error(r);
        }

        let atk = match r.fists.attacker() {
            Some(a) => a,
            None => {
                return render_same_role_error(r);
            }
        };
        let def = match r.fists.defender() {
            Some(d) => d,
            None => {
                return render_same_role_error(r);
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
        build_result_html(atk_card, atk_km, def_card, def_km, &result)
    })
}

/// Render a friendly error when both players chose the same role.
fn render_same_role_error(r: &room::RoomState) -> String {
    let role_name = match (&r.fists.local, &r.fists.remote) {
        (Some(l), Some(_)) => match l.role {
            CombatRole::Attacking => "Attacking",
            CombatRole::Defending => "Defending",
        },
        _ => "the same role",
    };

    let mut h = String::with_capacity(512);
    h.push_str(r#"<div class="p-4 text-kip-drk-sienna text-center">"#);
    h.push_str(r#"<p class="text-2xl mb-2">&#x26A0;</p>"#);
    h.push_str(&format!(
        r#"<p class="text-lg font-bold mb-2">Both players chose {}!</p>"#,
        role_name
    ));
    h.push_str(r#"<p class="text-sm mb-4">One player must be the Attacker and the other the Defender. Coordinate and try again.</p>"#);
    h.push_str(r#"<button onclick="htmx.ajax('POST','/api/room/fists/reset',{target:'#fists-container',swap:'innerHTML'})" class="w-full bg-kip-red hover:bg-kip-drk-sienna text-amber-50 font-bold py-2 px-4 rounded text-sm">Try Again</button>"#);
    h.push_str(r#"</div>"#);
    h
}

fn build_result_html(
    atk_card: &crate::cards_generated::Card,
    atk_km: &crate::cards_generated::KealMeans,
    def_card: &crate::cards_generated::Card,
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

    // Modifier result
    let mod_color = if result.modifier > 0 {
        "text-emerald-600"
    } else if result.modifier < 0 {
        "text-kip-red"
    } else {
        "text-slate-600"
    };
    let mod_sign = if result.modifier > 0 { "+" } else { "" };

    h.push_str(r#"<div class="bg-amber-50 border-2 border-kip-drk-sienna rounded p-3 text-center mb-3">"#);
    h.push_str(&format!(
        r#"<p class="text-sm font-bold">Attack Die Modifier</p><p class="text-3xl font-bold {}">{}{}""#,
        mod_color, mod_sign, result.modifier
    ));
    h.push_str(r#"</p>"#);

    // Motivation notes
    if let Some(note) = result.societal_mod {
        let clean = note.trim_start_matches('\n');
        h.push_str(&format!(
            r#"<p class="text-xs mt-1 text-kip-drk-sienna">{}</p>"#,
            clean
        ));
    }
    if let Some(note) = result.self_mod {
        let clean = note.trim_start_matches('\n');
        h.push_str(&format!(
            r#"<p class="text-xs mt-1 text-kip-drk-sienna">{}</p>"#,
            clean
        ));
    }
    if let Some(note) = result.support_mod {
        let clean = note.trim_start_matches('\n');
        h.push_str(&format!(
            r#"<p class="text-xs mt-1 text-kip-drk-sienna">{}</p>"#,
            clean
        ));
    }
    h.push_str(r#"</div>"#);

    // New round button
    h.push_str(r#"<button onclick="htmx.ajax('POST','/api/room/fists/reset',{target:'#fists-container',swap:'innerHTML'})" class="w-full bg-emerald-600 hover:bg-emerald-700 text-amber-50 font-bold py-2 px-4 rounded text-sm">New Round</button>"#);

    h.push_str(r#"</div>"#);
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        room::reset_room();
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
    fn join_sets_connected() {
        reset();
        let html = handle_join_post("code=WXYZ&name=Fun+Room");
        assert!(html.contains("Connected"));
        assert!(html.contains("WXYZ"));
        room::with_room(|r| assert!(r.connected));
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
    fn fists_same_role_shows_error() {
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
