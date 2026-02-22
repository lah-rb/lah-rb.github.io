//! `/api/fists/tool` route — context-aware fists tool.
//!
//! Returns local type matchup UI when not connected to a room,
//! returns multiplayer fists combat UI when connected.

use crate::cards_generated::CARDS;
use crate::game::damage;
use crate::game::room;
use crate::game::state::with_state;
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

/// Simple query string parser: extracts all values for a given key.
fn query_values<'a>(query: &'a str, key: &str) -> Vec<&'a str> {
    let bracket_key = format!("{}[]=", key);
    let plain_key = format!("{}=", key);

    query
        .split('&')
        .filter_map(|pair| {
            if let Some(val) = pair.strip_prefix(&bracket_key) {
                Some(val)
            } else if let Some(val) = pair.strip_prefix(&plain_key) {
                Some(val)
            } else {
                None
            }
        })
        .collect()
}

/// Extract a single query param value.
fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{}=", key);
    query.split('&').find_map(|pair| pair.strip_prefix(&prefix))
}

// ── GET /api/fists/tool ────────────────────────────────────────────

pub fn handle_get(query: &str) -> String {
    let params = parse_query(query);
    let slug = get_param(&params, "card").unwrap_or("");

    // Check if connected to a room
    let connected = room::with_room(|r| r.connected);

    if connected {
        // Return multiplayer fists combat UI
        render_multiplayer_fists(slug)
    } else {
        // Return local type matchup UI
        render_local_type_matchup(query)
    }
}

/// Render the local type matchup calculator (original type_matchup.html behavior)
fn render_local_type_matchup(query: &str) -> String {
    // Strip leading '?' if present
    let q = query.strip_prefix('?').unwrap_or(query);

    // Parse attackers
    let atk_strs = query_values(q, "atk");
    let attackers: Vec<_> = atk_strs.iter().filter_map(|s| typing::parse_archetype(s)).collect();

    // Parse defenders
    let def_strs = query_values(q, "def");
    let defenders: Vec<_> = def_strs.iter().filter_map(|s| typing::parse_archetype(s)).collect();

    // Parse motives
    let atk_motive = query_value(q, "motAtk").and_then(typing::parse_motive);
    let def_motive = query_value(q, "motDef").and_then(typing::parse_motive);

    // Build the type matchup UI
    let mut h = String::with_capacity(2048);
    h.push_str(r#"<div class="text-kip-drk-sienna">"#);
    
    // Archetypes section
    h.push_str(r#"<div class="flex place-content-center">"#);
    h.push_str(r#"<svg xmlns="http://www.w3.org/2000/svg" alt="show keal" 
              @click="showKeal = !showKeal" :class="{ 'rotate-180': !showKeal }" x-transition
              class="fill-none stroke-2 stroke-kip-drk-goldenrod w-6 h-6 place-self-center col-span-2">
              <path stroke-linecap="round" stroke-linejoin="round" d="m4.5 15.75 7.5-7.5 7.5 7.5" />
            </svg>"#);
    h.push_str(r#"<b>Archetypes</b></div>"#);
    
    h.push_str(r#"<div class="grid grid-cols-2">"#);
    h.push_str(r#"<div x-show="showKeal">"#);
    h.push_str(r#"<span><b>Attacker</b></span>"#);
    // Note: Archetypes would need to be passed from Jekyll data
    // For now, render a simplified version that works with HTMX form
    h.push_str(r#"</div><div x-show="showKeal">"#);
    h.push_str(r#"<span><b>Defender</b></span>"#);
    h.push_str(r#"</div></div>"#);

    // Motivations section
    h.push_str(r#"<div class="flex place-content-center">"#);
    h.push_str(r#"<svg xmlns="http://www.w3.org/2000/svg" alt="show Motivation" 
              @click="showMot = !showMot" :class="{ 'rotate-180': !showMot }" x-transition
              class="fill-none stroke-2 stroke-kip-drk-goldenrod w-6 h-6 place-self-center col-span-2">
              <path stroke-linecap="round" stroke-linejoin="round" d="m4.5 15.75 7.5-7.5 7.5 7.5" />
            </svg>"#);
    h.push_str(r#"<b>Motivations</b></div>"#);

    // Result display
    if attackers.is_empty() && defenders.is_empty() {
        h.push_str(r#"<div id="type-result" class="col-span-2"><span><strong>Attack Die Modifier:</strong></span></div>"#);
    } else {
        let result = typing::type_matchup(&attackers, &defenders, atk_motive, def_motive);
        let display = result.to_display_string();
        let html_display = display.replace('\n', "<br>");
        h.push_str(&format!(
            r#"<div id="type-result" class="col-span-2"><span><strong>Attack Die Modifier:</strong> {}</span></div>"#,
            html_display
        ));
    }

    h.push_str(r#"</div>"#);
    h
}

/// Render the multiplayer fists combat UI
fn render_multiplayer_fists(slug: &str) -> String {
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
    if slug.is_empty() {
        return render_fists_no_card();
    }

    let card = match find_card(slug) {
        Some(c) => c,
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

    let mut slot_start: u8 = 1;
    let mut idx: u8 = 1;
    for km in card.keal_means {
        // Check if all slots in this keal means group are checked (exhausted)
        let all_checked = if km.count > 0 {
            with_state(|state| {
                if let Some(card_state) = state.cards.get(slug) {
                    (slot_start..slot_start + km.count)
                        .all(|s| card_state.slots.get(&s).copied().unwrap_or(false))
                } else {
                    false
                }
            })
        } else {
            false
        };
        slot_start += km.count;

        let genetics_str = km.genetics.join("-");
        if all_checked {
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

    // Dedicated message area at the bottom
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
    h.push_str(r##"<div hx-get="/api/room/fists/poll" hx-trigger="every 2s" hx-target="#fists-tool-content" hx-swap="innerHTML"></div>"##);
    h.push_str(r#"</div>"#);
    h
}

fn render_fists_result() -> String {
    room::with_room(|r| {
        use crate::game::room::CombatRole;
        
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
        build_result_html(atk_card, atk_km, def_card, def_km, &result)
    })
}

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
        r#"<p class="text-sm font-bold">Attack Die Modifier</p><p class="text-3xl font-bold {}">{}{}</p>"#,
        mod_color, mod_sign, result.modifier
    ));

    h.push_str(r#"</div>"#);

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