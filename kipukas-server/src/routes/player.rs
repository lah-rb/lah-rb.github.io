//! `/api/player/*` routes — player-level features.
//!
//! ## Affinity tracking
//! Players declare affinity for one of the 15 archetypes once per day.
//! Each declaration increments the level for that archetype. The most
//! recently declared archetype is the "active" affinity, granting a +1
//! roll bonus on matching cards during fists combat.
//!
//! ## Signed export/import (Phase D)
//! HMAC-SHA256 signed exports for tamper-resistant player data backups.
//! The WASM layer owns the integrity check (HMAC); an optional JS-side
//! AES-GCM encryption layer provides confidentiality on top.

use crate::game::{crypto, player_doc};
use crate::routes::util::{get_param, parse_form_body, parse_query};

// ── GET /api/player/affinity ───────────────────────────────────────

/// Handle GET /api/player/affinity?today={YYYY-MM-DD}
/// Returns the full affinity panel HTML showing all 15 archetypes.
pub fn handle_affinity_get(query: &str) -> String {
    let params = parse_query(query);
    let today = get_param(&params, "today").unwrap_or("");
    render_affinity_panel(today)
}

// ── POST /api/player/affinity ──────────────────────────────────────

/// Handle POST /api/player/affinity
/// Body: archetype={name}&today={YYYY-MM-DD}
/// Declares affinity for the given archetype. Returns re-rendered panel.
pub fn handle_affinity_post(body: &str) -> String {
    let params = parse_form_body(body);
    let archetype = get_param(&params, "archetype").unwrap_or("");
    let today = get_param(&params, "today").unwrap_or("");

    if archetype.is_empty() {
        return r#"<span class="text-kip-red">Missing archetype parameter</span>"#.to_string();
    }

    match player_doc::declare_affinity(archetype, today) {
        Ok(_) => render_affinity_panel(today),
        Err(e) => {
            // Re-render panel with error toast at top
            let mut html = String::with_capacity(4096);
            html.push_str(&format!(
                r#"<div class="text-center text-xs text-kip-red mb-2">{}</div>"#,
                e
            ));
            html.push_str(&render_affinity_panel_inner(today));
            html
        }
    }
}

// ── Panel rendering ────────────────────────────────────────────────

/// Render the full affinity panel (wrapper).
fn render_affinity_panel(today: &str) -> String {
    let mut html = String::with_capacity(4096);
    html.push_str(r#"<div class="p-4 text-kip-drk-sienna">"#);
    html.push_str(r#"<p class="text-lg font-bold text-center mb-1">Archetypal Affinity</p>"#);
    html.push_str(
        r#"<p class="text-xs text-slate-500 text-center mb-3">Declare once per day to grow your bond</p>"#,
    );
    html.push_str(&render_affinity_panel_inner(today));
    html.push_str(r#"</div>"#);
    html
}

/// Render the inner panel content (archetype list).
fn render_affinity_panel_inner(today: &str) -> String {
    let archetypes = player_doc::valid_archetypes();
    let active = player_doc::get_active_affinity();
    let has_affinity = active.is_some();

    let mut html = String::with_capacity(3072);

    // Active affinity highlight
    if let Some((ref active_name, _active_level)) = active {
        html.push_str(r#"<div class="bg-amber-100 border border-amber-300 rounded-lg p-2 mb-3 text-center">"#);
        html.push_str(&format!(
            r#"<p class="text-sm font-bold">♥ <span class="text-kip-red">{}</span></p>"#,
            active_name
        ));
        html.push_str(
            r#"<p class="text-xs text-slate-500">+1 roll bonus on matching cards</p>"#,
        );
        html.push_str(r#"<p class="text-xs text-slate-400 mt-1">Start a New Game to change affinity</p>"#);
        html.push_str(r#"</div>"#);
    }

    // Archetype grid
    html.push_str(r#"<div class="grid grid-cols-1 gap-1">"#);

    for &archetype in archetypes {
        let is_active = active
            .as_ref()
            .map(|(name, _)| name == archetype)
            .unwrap_or(false);

        // Row container
        let bg = if is_active {
            "bg-amber-50 border-amber-300"
        } else {
            "bg-white border-slate-200"
        };
        html.push_str(&format!(
            r#"<div class="flex items-center justify-between border rounded px-2 py-1 {}">"#,
            bg
        ));

        // Left: name
        html.push_str(r#"<div class="flex items-center gap-2">"#);
        if is_active {
            html.push_str(r#"<span class="text-amber-500 text-xs">♥</span>"#);
        }
        html.push_str(&format!(
            r#"<span class="text-sm font-medium">{}</span>"#,
            archetype
        ));
        html.push_str(r#"</div>"#);

        // Right: declare button or locked indicator
        html.push_str(r#"<div class="flex items-center gap-2">"#);
        if is_active {
            html.push_str(
                r#"<span class="text-xs text-amber-600 px-2 py-0.5 font-bold">♥ Active</span>"#,
            );
        } else if has_affinity {
            // Another archetype is active — this one is locked
            html.push_str(
                r#"<span class="text-xs text-slate-300 px-2 py-0.5">Locked</span>"#,
            );
        } else {
            // No affinity yet — show declare button
            html.push_str(&format!(
                "<button class=\"text-xs bg-kip-red text-amber-50 px-2 py-0.5 rounded hover:bg-red-700 active:bg-red-800\" hx-post=\"/api/player/affinity\" hx-vals='{{\"archetype\":\"{}\",\"today\":\"{}\"}}' hx-target=\"#affinity-container\" hx-swap=\"innerHTML\">Declare</button>",
                archetype, today
            ));
        }
        html.push_str(r#"</div>"#);

        html.push_str(r#"</div>"#); // close row
    }

    html.push_str(r#"</div>"#); // close grid
    html
}

// ── POST /api/player/export/signed ─────────────────────────────────

/// Handle POST /api/player/export/signed
/// Body: passphrase={...}
/// Signs the PLAYER_DOC base64 data with HMAC-SHA256 using the passphrase.
/// Returns a JSON inner payload: { player_doc, mac, exported_at }
/// The JS layer encrypts this with AES-GCM before writing to file.
pub fn handle_export_signed_post(body: &str) -> String {
    let params = parse_form_body(body);
    let passphrase = get_param(&params, "passphrase").unwrap_or("");

    if passphrase.is_empty() {
        return r#"{"error":"Missing passphrase"}"#.to_string();
    }

    let state_b64 = player_doc::encode_full_state();

    match crypto::sign_export(passphrase, &state_b64) {
        Ok(mac) => {
            // Build JSON manually to avoid pulling in extra serde features.
            // The player_doc base64 and mac hex are guaranteed safe for JSON
            // (no special chars). exported_at is passed from JS as a param,
            // or we use a placeholder that JS will fill.
            let exported_at = get_param(&params, "exported_at").unwrap_or("");
            format!(
                r#"{{"player_doc":"{}","mac":"{}","exported_at":"{}"}}"#,
                state_b64, mac, exported_at
            )
        }
        Err(e) => {
            format!(r#"{{"error":"{}"}}"#, e)
        }
    }
}

// ── POST /api/player/import/signed ─────────────────────────────────

/// Handle POST /api/player/import/signed
/// Body: payload={JSON}&passphrase={...}
/// Verifies the HMAC-SHA256 signature and restores PLAYER_DOC if valid.
/// Returns an HTML fragment indicating success or failure.
pub fn handle_import_signed_post(body: &str) -> String {
    let params = parse_form_body(body);
    let passphrase = get_param(&params, "passphrase").unwrap_or("");
    let payload_json = get_param(&params, "payload").unwrap_or("");

    if passphrase.is_empty() {
        return r#"<span class="text-kip-red">Missing passphrase</span>"#.to_string();
    }
    if payload_json.is_empty() {
        return r#"<span class="text-kip-red">Missing payload</span>"#.to_string();
    }

    // Parse the inner payload JSON to extract player_doc and mac.
    // Using serde_json since we already have it as a dependency.
    let parsed: serde_json::Value = match serde_json::from_str(payload_json) {
        Ok(v) => v,
        Err(e) => {
            return format!(
                r#"<span class="text-kip-red">Invalid payload format: {}</span>"#,
                e
            );
        }
    };

    let player_doc_b64 = match parsed.get("player_doc").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return r#"<span class="text-kip-red">Missing player_doc in payload</span>"#
                .to_string();
        }
    };

    let mac_hex = match parsed.get("mac").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return r#"<span class="text-kip-red">Missing mac in payload</span>"#.to_string();
        }
    };

    // Verify HMAC
    match crypto::verify_export(passphrase, player_doc_b64, mac_hex) {
        Ok(true) => {
            // Signature valid — restore PLAYER_DOC
            match player_doc::restore_from_state(player_doc_b64) {
                Ok(()) => {
                    r#"<span class="text-emerald-600">✓ Player data imported and verified successfully</span>"#.to_string()
                }
                Err(e) => {
                    format!(
                        r#"<span class="text-kip-red">Signature valid but restore failed: {}</span>"#,
                        e
                    )
                }
            }
        }
        Ok(false) => {
            r#"<span class="text-kip-red">Verification failed — wrong passphrase or data has been tampered with</span>"#.to_string()
        }
        Err(e) => {
            format!(
                r#"<span class="text-kip-red">Verification error: {}</span>"#,
                e
            )
        }
    }
}

// ── GET /api/player/sync/sv ────────────────────────────────────────

/// Handle GET /api/player/sync/sv
/// Returns the PLAYER_DOC state vector as URL-safe base64 string.
/// Used for sync handshake step 1.
pub fn handle_sync_sv_get(_query: &str) -> String {
    player_doc::encode_state_vector()
}

// ── POST /api/player/sync/diff ─────────────────────────────────────

/// Handle POST /api/player/sync/diff
/// Body: sv={base64 state vector}
/// Computes the PLAYER_DOC diff for a remote device given its state vector.
/// Returns the diff as URL-safe base64 update.
pub fn handle_sync_diff_post(body: &str) -> String {
    let params = parse_form_body(body);
    let sv = get_param(&params, "sv").unwrap_or("");

    if sv.is_empty() {
        return r#"{"error":"Missing sv parameter"}"#.to_string();
    }

    match player_doc::encode_diff(sv) {
        Ok(diff) => diff,
        Err(e) => format!(r#"{{"error":"{}"}}"#, e),
    }
}

// ── POST /api/player/sync/apply ────────────────────────────────────

/// Handle POST /api/player/sync/apply
/// Body: update={base64 yrs update}
/// Applies a remote yrs update to PLAYER_DOC.
/// Returns "ok" on success or an error HTML fragment.
pub fn handle_sync_apply_post(body: &str) -> String {
    let params = parse_form_body(body);
    let update = get_param(&params, "update").unwrap_or("");

    if update.is_empty() {
        return r#"<span class="text-kip-red">Missing update parameter</span>"#.to_string();
    }

    match player_doc::apply_update(update) {
        Ok(()) => "ok".to_string(),
        Err(e) => format!(
            r#"<span class="text-kip-red">Sync apply error: {}</span>"#,
            e
        ),
    }
}

// ── POST /api/player/sync/auth ─────────────────────────────────────

/// Handle POST /api/player/sync/auth
/// Body: passphrase={...}&room_code={...}
/// Computes HMAC-SHA256(passphrase, room_code) for mutual authentication.
/// Returns the MAC as a 64-char hex string.
pub fn handle_sync_auth_post(body: &str) -> String {
    let params = parse_form_body(body);
    let passphrase = get_param(&params, "passphrase").unwrap_or("");
    let room_code = get_param(&params, "room_code").unwrap_or("");

    if passphrase.is_empty() {
        return r#"{"error":"Missing passphrase"}"#.to_string();
    }
    if room_code.is_empty() {
        return r#"{"error":"Missing room_code"}"#.to_string();
    }

    match crypto::sign_export(passphrase, room_code) {
        Ok(mac) => mac,
        Err(e) => format!(r#"{{"error":"{}"}}"#, e),
    }
}

// ── POST /api/player/sync/verify ───────────────────────────────────

/// Handle POST /api/player/sync/verify
/// Body: passphrase={...}&room_code={...}&mac={hex}
/// Verifies the peer's HMAC matches our computation.
/// Returns "ok" or "fail".
pub fn handle_sync_verify_post(body: &str) -> String {
    let params = parse_form_body(body);
    let passphrase = get_param(&params, "passphrase").unwrap_or("");
    let room_code = get_param(&params, "room_code").unwrap_or("");
    let mac = get_param(&params, "mac").unwrap_or("");

    if passphrase.is_empty() || room_code.is_empty() || mac.is_empty() {
        return "fail".to_string();
    }

    match crypto::verify_export(passphrase, room_code, mac) {
        Ok(true) => "ok".to_string(),
        _ => "fail".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::player_doc;

    fn reset() {
        player_doc::init_player_doc();
    }

    #[test]
    fn affinity_get_renders_all_archetypes() {
        reset();
        let html = handle_affinity_get("?today=2026-02-26");
        assert!(html.contains("Archetypal Affinity"));
        assert!(html.contains("Brutal"));
        assert!(html.contains("Avian"));
        assert!(html.contains("Entropic"));
        assert!(html.contains("Declare"));
        // No active affinity yet
        assert!(!html.contains("Active:"));
        reset();
    }

    #[test]
    fn affinity_post_declares_and_rerenders() {
        reset();
        let html = handle_affinity_post("archetype=Brutal&today=2026-02-26");
        // Should show active indicator
        assert!(html.contains("Brutal"));
        assert!(html.contains("Active"));
        // All other archetypes should be locked
        assert!(html.contains("Locked"));
        // Should not show any Declare buttons
        assert!(!html.contains(">Declare</button>"));
        reset();
    }

    #[test]
    fn affinity_post_rejects_second_declaration() {
        reset();
        handle_affinity_post("archetype=Brutal&today=2026-02-26");
        // Any second declaration rejected — one per game
        let html = handle_affinity_post("archetype=Avian&today=2026-02-26");
        assert!(html.contains("already declared"));
        reset();
    }

    #[test]
    fn affinity_post_missing_archetype() {
        reset();
        let html = handle_affinity_post("today=2026-02-26");
        assert!(html.contains("Missing archetype"));
        reset();
    }

    #[test]
    fn affinity_get_shows_active_and_locked() {
        reset();
        player_doc::declare_affinity("Brutal", "2026-02-26").unwrap();
        let html = handle_affinity_get("?today=2026-02-26");
        // Brutal is active
        assert!(html.contains("♥ Active"));
        assert!(html.contains("♥"));
        // Other archetypes should be locked
        assert!(html.contains("Locked"));
        // No Declare buttons visible
        assert!(!html.contains(">Declare</button>"));
        reset();
    }

    #[test]
    fn affinity_get_no_affinity_shows_all_declare_buttons() {
        reset();
        let html = handle_affinity_get("?today=2026-02-26");
        // All 15 archetypes should have Declare buttons
        let count = html.matches(">Declare</button>").count();
        assert_eq!(count, 15);
        // No locked indicators
        assert!(!html.contains("Locked"));
        reset();
    }

    // ── Signed export/import tests ─────────────────────────────────

    #[test]
    fn signed_export_returns_valid_json() {
        reset();
        player_doc::add_alarm(5, "test", "red");
        let json = handle_export_signed_post("passphrase=secret&exported_at=2026-02-27T12:00:00Z");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("player_doc").is_some());
        assert!(parsed.get("mac").is_some());
        assert_eq!(parsed["exported_at"], "2026-02-27T12:00:00Z");
        // MAC should be 64 hex chars
        assert_eq!(parsed["mac"].as_str().unwrap().len(), 64);
        reset();
    }

    #[test]
    fn signed_export_rejects_empty_passphrase() {
        reset();
        let json = handle_export_signed_post("passphrase=");
        assert!(json.contains("error"));
        assert!(json.contains("Missing passphrase"));
        reset();
    }

    /// URL-encode a string for safe embedding in a form body value.
    fn url_encode(s: &str) -> String {
        let mut out = String::with_capacity(s.len() * 3);
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char);
                }
                _ => {
                    out.push_str(&format!("%{:02X}", b));
                }
            }
        }
        out
    }

    #[test]
    fn signed_import_roundtrip() {
        reset();
        // Set up some state
        player_doc::add_alarm(3, "roundtrip", "green");
        player_doc::declare_affinity("Brutal", "2026-02-25").unwrap();

        // Export signed
        let json = handle_export_signed_post("passphrase=test123&exported_at=2026-02-27");
        assert!(!json.contains("error"), "Export failed: {}", json);

        // Reset state
        player_doc::init_player_doc();
        assert!(player_doc::get_alarms().is_empty());

        // Import signed — URL-encode the full JSON payload
        let encoded_payload = url_encode(&json);
        let body = format!("passphrase=test123&payload={}", encoded_payload);
        let result = handle_import_signed_post(&body);
        assert!(result.contains("successfully"), "Import failed: {}", result);

        // Verify state restored
        let alarms = player_doc::get_alarms();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].name, "roundtrip");
        assert!(player_doc::get_active_affinity().is_some());
        reset();
    }

    #[test]
    fn signed_import_rejects_wrong_passphrase() {
        reset();
        player_doc::add_alarm(1, "test", "red");
        let json = handle_export_signed_post("passphrase=correct&exported_at=2026-02-27");

        player_doc::init_player_doc();

        let encoded_payload = url_encode(&json);
        let body = format!("passphrase=wrong&payload={}", encoded_payload);
        let result = handle_import_signed_post(&body);
        assert!(result.contains("Verification failed"));
        reset();
    }

    #[test]
    fn signed_import_rejects_tampered_data() {
        reset();
        player_doc::add_alarm(1, "test", "red");
        let json = handle_export_signed_post("passphrase=secret&exported_at=2026-02-27");

        // Tamper with the payload: modify the player_doc value
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let mac = parsed["mac"].as_str().unwrap();
        let tampered_json = format!(
            r#"{{"player_doc":"TAMPERED","mac":"{}","exported_at":"2026-02-27"}}"#,
            mac
        );
        let encoded_payload = url_encode(&tampered_json);
        let body = format!("passphrase=secret&payload={}", encoded_payload);
        let result = handle_import_signed_post(&body);
        assert!(
            result.contains("Verification failed") || result.contains("failed"),
            "Expected failure but got: {}",
            result
        );
        reset();
    }

    #[test]
    fn signed_import_rejects_missing_passphrase() {
        reset();
        let result = handle_import_signed_post("payload={\"player_doc\":\"x\",\"mac\":\"y\"}");
        assert!(result.contains("Missing passphrase"));
        reset();
    }

    #[test]
    fn signed_import_rejects_missing_payload() {
        reset();
        let result = handle_import_signed_post("passphrase=test");
        assert!(result.contains("Missing payload"));
        reset();
    }

    #[test]
    fn signed_import_rejects_invalid_json() {
        reset();
        let result = handle_import_signed_post("passphrase=test&payload=not-json");
        assert!(result.contains("Invalid payload format"));
        reset();
    }

    // ── Sync route tests (Phase E) ─────────────────────────────────

    #[test]
    fn sync_sv_get_returns_nonempty_base64() {
        reset();
        player_doc::add_alarm(3, "test", "red");
        let sv = handle_sync_sv_get("");
        assert!(!sv.is_empty());
        assert!(!sv.contains("error"));
        reset();
    }

    #[test]
    fn sync_diff_post_returns_update() {
        reset();
        player_doc::add_alarm(5, "local timer", "green");
        // Use an empty SV (fresh device) — encoded as URL-safe base64
        let empty_sv = {
            use base64::engine::general_purpose::URL_SAFE_NO_PAD;
            use base64::Engine;
            use yrs::updates::encoder::Encode;
            use yrs::{Doc, ReadTxn, Transact};
            let doc = Doc::new();
            let sv = doc.transact().state_vector().encode_v1();
            URL_SAFE_NO_PAD.encode(&sv)
        };
        let diff = handle_sync_diff_post(&format!("sv={}", empty_sv));
        assert!(!diff.is_empty());
        assert!(!diff.contains("error"));
        reset();
    }

    #[test]
    fn sync_diff_post_rejects_empty_sv() {
        reset();
        let result = handle_sync_diff_post("sv=");
        assert!(result.contains("error"));
        reset();
    }

    #[test]
    fn sync_apply_post_returns_ok() {
        reset();
        // Create a remote update to apply
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        use yrs::{Any, Doc, Map, ReadTxn, Transact, WriteTxn};
        let remote = Doc::new();
        {
            let mut txn = remote.transact_mut();
            txn.get_or_insert_map("cards");
            txn.get_or_insert_array("alarms");
            txn.get_or_insert_map("affinity");
            txn.get_or_insert_map("loyalty");
            let settings = txn.get_or_insert_map("settings");
            settings.insert(&mut txn, "show_alarms", Any::from(false));
        }
        let update = remote
            .transact()
            .encode_diff_v1(&yrs::StateVector::default());
        let update_b64 = URL_SAFE_NO_PAD.encode(&update);

        let result = handle_sync_apply_post(&format!("update={}", update_b64));
        assert_eq!(result, "ok");
        reset();
    }

    #[test]
    fn sync_apply_post_rejects_empty_update() {
        reset();
        let result = handle_sync_apply_post("update=");
        assert!(result.contains("Missing update"));
        reset();
    }

    #[test]
    fn sync_auth_roundtrip() {
        reset();
        let mac = handle_sync_auth_post("passphrase=secret&room_code=ABCD");
        assert_eq!(mac.len(), 64); // 32 bytes = 64 hex chars
        assert!(!mac.contains("error"));

        let verify = handle_sync_verify_post(&format!(
            "passphrase=secret&room_code=ABCD&mac={}",
            mac
        ));
        assert_eq!(verify, "ok");
        reset();
    }

    #[test]
    fn sync_auth_rejects_wrong_passphrase() {
        reset();
        let mac = handle_sync_auth_post("passphrase=correct&room_code=ABCD");
        let verify = handle_sync_verify_post(&format!(
            "passphrase=wrong&room_code=ABCD&mac={}",
            mac
        ));
        assert_eq!(verify, "fail");
        reset();
    }

    #[test]
    fn sync_auth_rejects_wrong_room_code() {
        reset();
        let mac = handle_sync_auth_post("passphrase=secret&room_code=ABCD");
        let verify = handle_sync_verify_post(&format!(
            "passphrase=secret&room_code=WXYZ&mac={}",
            mac
        ));
        assert_eq!(verify, "fail");
        reset();
    }

    #[test]
    fn sync_auth_rejects_empty_params() {
        reset();
        let result = handle_sync_auth_post("passphrase=&room_code=ABCD");
        assert!(result.contains("error"));

        let result2 = handle_sync_auth_post("passphrase=secret&room_code=");
        assert!(result2.contains("error"));

        let verify = handle_sync_verify_post("passphrase=&room_code=&mac=");
        assert_eq!(verify, "fail");
        reset();
    }
}
