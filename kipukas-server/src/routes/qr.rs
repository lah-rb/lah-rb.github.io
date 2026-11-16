/// Route handlers for /api/qr/* routes
///
/// Manages the QR scanner UI state machine via server-driven HTML fragments.
/// ZXing decode happens in the JS Web Worker (middleware); these Rust routes handle:
///   - Scanner UI state transitions (open/close, privacy modal)
///   - Decode result formatting and URL validation
///
/// Query params for /api/qr/status:
///   action  — "open", "close", "error"
///   privacy — "true" or "false" (whether user accepted camera privacy notice)
///   msg     — error message (when action=error)
///
/// Query params for /api/qr/found:
///   url — the decoded QR URL to validate and display

use crate::cards_generated::CARDS;

// ── SVG icon constants ─────────────────────────────────────────────

const SVG_EYE_OFF: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-slate-100 hover:stroke-kip-red active:stroke-kip-drk-sienna stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="M3.98 8.223A10.477 10.477 0 0 0 1.934 12C3.226 16.338 7.244 19.5 12 19.5c.993 0 1.953-.138 2.863-.395M6.228 6.228A10.451 10.451 0 0 1 12 4.5c4.756 0 8.773 3.162 10.065 7.498a10.522 10.522 0 0 1-4.293 5.774M6.228 6.228 3 3m3.228 3.228 3.65 3.65m7.894 7.894L21 21m-3.228-3.228-3.65-3.65m0 0a3 3 0 1 0-4.243-4.243m4.242 4.242L9.88 9.88" /></svg>"#;

const SVG_FLASH_OFF: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-slate-100 hover:stroke-kip-red active:stroke-kip-drk-sienna stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="M11.412 15.655 9.75 21.75l3.745-4.012M9.257 13.5H3.75l2.659-2.849m2.048-2.194L14.25 2.25 12 10.5h8.25l-4.707 5.043M8.457 8.457 3 3m5.457 5.457 7.086 7.086m0 0L21 21" /></svg>"#;

const SVG_FLASH_ON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-slate-100 hover:stroke-kip-red active:stroke-kip-drk-sienna stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="m3.75 13.5 10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75Z" /></svg>"#;

const SVG_CAMERA_SWITCH: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-slate-100 hover:stroke-kip-red active:stroke-kip-drk-sienna stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0 3.181 3.183a8.25 8.25 0 0 0 13.803-3.7M4.031 9.865a8.25 8.25 0 0 1 13.803-3.7l3.181 3.182m0-4.991v4.99" /></svg>"#;

// CV toggle: "cpu-chip" icon — represents computer vision / neural network
const SVG_CV_OFF: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-slate-400 stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="M8.25 3v1.5M4.5 8.25H3m18 0h-1.5M4.5 12H3m18 0h-1.5m-15 3.75H3m18 0h-1.5M8.25 19.5V21M12 3v1.5m0 15V21m3.75-18v1.5m0 15V21m-9-1.5h10.5a2.25 2.25 0 0 0 2.25-2.25V6.75a2.25 2.25 0 0 0-2.25-2.25H6.75A2.25 2.25 0 0 0 4.5 6.75v10.5a2.25 2.25 0 0 0 2.25 2.25Zm.75-12h9v9h-9v-9Z" /></svg>"#;

const SVG_CV_ON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-emerald-400 stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="M8.25 3v1.5M4.5 8.25H3m18 0h-1.5M4.5 12H3m18 0h-1.5m-15 3.75H3m18 0h-1.5M8.25 19.5V21M12 3v1.5m0 15V21m3.75-18v1.5m0 15V21m-9-1.5h10.5a2.25 2.25 0 0 0 2.25-2.25V6.75a2.25 2.25 0 0 0-2.25-2.25H6.75A2.25 2.25 0 0 0 4.5 6.75v10.5a2.25 2.25 0 0 0 2.25 2.25Zm.75-12h9v9h-9v-9Z" /></svg>"#;

// ── Query string helpers ───────────────────────────────────────────

/// Extract a single query param value.
fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{}=", key);
    query.split('&').find_map(|pair| pair.strip_prefix(&prefix))
}

/// Percent-decode a URL-encoded string.
fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut bytes = input.bytes();
    while let Some(b) = bytes.next() {
        match b {
            b'%' => {
                let hi = bytes.next().unwrap_or(0);
                let lo = bytes.next().unwrap_or(0);
                if let (Some(h), Some(l)) = (hex_val(hi), hex_val(lo)) {
                    result.push((h << 4 | l) as char);
                } else {
                    result.push('%');
                    result.push(hi as char);
                    result.push(lo as char);
                }
            }
            b'+' => result.push(' '),
            _ => result.push(b as char),
        }
    }
    result
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ── URL validation ─────────────────────────────────────────────────

/// The Kipukas addresses a QR code may carry.
const KIPUKAS_PREFIXES: [&str; 7] = [
    "kpks.us/",
    "https://kpks.us/",
    "https://www.kpks.us/",
    "http://kpks.us/",
    "http://www.kpks.us/",
    "https://www.kipukas.cards/",
    "https://kipukas.cards/",
];

/// The path part of a decoded QR address, without leading slashes.
/// Only Kipukas addresses are accepted (kpks.us, kipukas.cards).
fn kipukas_path(raw: &str) -> Result<&str, &'static str> {
    let url = raw.trim();
    KIPUKAS_PREFIXES
        .iter()
        .find_map(|prefix| url.strip_prefix(prefix))
        // "kpks.us//other.site/x" must not turn into the protocol-relative
        // address "//other.site/x".
        .map(|path| path.trim_start_matches('/'))
        .ok_or("Invalid QR code — not a Kipukas URL")
}

/// Where a scanned address takes the player, as a path on this site.
///
/// A card's short code (what the printed QR codes carry) or its slug resolves
/// straight to the card page. Going to the short-code address instead would
/// load a redirect stub first and the card second: two full page loads, each
/// of which restarts the game engine and the multiplayer connection.
///
/// Anything else on the site is passed through as a relative path, provided
/// it is made only of characters that are safe inside the navigation script.
/// The address comes from whatever QR code the camera saw, so it is not
/// trusted.
fn destination(path: &str) -> Result<String, &'static str> {
    let first_segment = path.split(['/', '?', '#']).next().unwrap_or("");
    if let Some(card) = CARDS
        .iter()
        .find(|c| (!c.code.is_empty() && c.code == first_segment) || c.slug == first_segment)
    {
        return Ok(card.url.to_string());
    }
    if path.bytes().all(is_safe_path_byte) {
        Ok(format!("/{}", path))
    } else {
        Err("Invalid QR code — unrecognised address")
    }
}

/// Characters allowed in a pass-through path. None of them can end the
/// quoted string or the script element the path is written into.
fn is_safe_path_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-_./?=&#%".contains(&b)
}

// ── Route handlers ─────────────────────────────────────────────────

/// Handle GET /api/qr/status — scanner UI state machine.
pub fn handle_status(query: &str) -> String {
    let q = query.strip_prefix('?').unwrap_or(query);
    let action = query_value(q, "action").unwrap_or("");
    let privacy = query_value(q, "privacy").unwrap_or("false") == "true";

    match action {
        "open" if !privacy => privacy_modal(),
        "open" => scanning_ui(),
        "close" => String::new(),
        "error" => {
            let msg = query_value(q, "msg")
                .map(|m| percent_decode(m))
                .unwrap_or_else(|| "An unknown error occurred".to_string());
            error_html(&msg)
        }
        _ => String::new(),
    }
}

/// Handle GET /api/qr/found — validate decoded URL, return result HTML.
pub fn handle_found(query: &str) -> String {
    let q = query.strip_prefix('?').unwrap_or(query);
    let raw_url = match query_value(q, "url") {
        Some(u) => percent_decode(u),
        None => return error_html("No URL provided in QR decode result"),
    };

    match kipukas_path(&raw_url).and_then(destination) {
        Ok(path) => found_html(&path),
        Err(msg) => error_html(msg),
    }
}

// ── HTML fragment builders ─────────────────────────────────────────

fn privacy_modal() -> String {
    format!(
        r##"<div class="fixed inset-0 flex items-center justify-center z-50">
  <div class="z-50 absolute inset-0 bg-slate-300 opacity-75"></div>
  <div class="bg-amber-50 z-50 p-6 rounded-lg shadow-xl w-full max-w-lg">
    <h2 class="text-xl font-semibold mb-4 text-kip-drk-sienna">Privacy Notice</h2>
    <p class="mb-4 text-kip-drk-sienna">
      By allowing camera access to our QR code scanner, you agree to our
      <a href="/privacy_policy" class="text-kip-red hover:text-emerald-600 underline">privacy policy</a>.
      Breath easy, we don&rsquo;t collect any data.
    </p>
    <button onclick="localStorage.setItem('qr-privacy-accepted','true'); htmx.ajax('GET', '/api/qr/status?action=open&amp;privacy=true', {{target:'#qr-container', swap:'innerHTML'}})"
            class="bg-kip-red hover:bg-emerald-600 text-amber-50 font-bold py-2 my-2 px-4 rounded">
      Accept &amp; Continue
    </button>
    <button onclick="htmx.ajax('GET', '/api/qr/status?action=close', {{target:'#qr-container', swap:'innerHTML'}})"
            class="bg-kip-red hover:bg-emerald-600 text-amber-50 font-bold py-2 px-4 rounded">
      Don&rsquo;t Need It
    </button>
  </div>
</div>"##
    )
}

fn scanning_ui() -> String {
    format!(
        r##"<div x-data="{{ showFlash: false, cvOn: localStorage.getItem('kipukas-cv-enabled') === 'true' }}">
  <!-- Flash overlay - outside transformed container to cover full screen -->
  <div x-show="showFlash"
       class="fixed inset-0 z-40 bg-white w-screen h-screen"
       @click="showFlash = false"></div>
  
  <!-- Video container - higher z-index to stay above flash -->
  <div class="z-50 aspect-square fixed w-80 md:w-1/2 lg:w-1/3 -translate-x-1/2 lg:-translate-y-1/2 bottom-8 left-1/2 rounded-lg transition delay-150">
    <canvas id="canvas" class="-z-10 object-cover size-full scale-x-[-1] hidden"
            width="640" height="640"></canvas>
    
    <!-- Close button (left) -->
    <button class="z-50 absolute top-3 left-3 size-fit transition delay-150"
            onclick="kipukasQR.stop(); htmx.ajax('GET', '/api/qr/status?action=close', {{target:'#qr-container', swap:'innerHTML'}})">
      {eye_off}
    </button>
    
    <!-- Switch camera button (center) -->
    <button class="z-50 absolute top-3 left-1/2 -translate-x-1/2 size-fit transition delay-150"
            onclick="kipukasQR.switchCamera()">
      {camera_switch}
    </button>
    
    <!-- Flash button (right) -->
    <button class="z-50 absolute top-3 right-3 size-fit transition delay-150"
            @click="showFlash = !showFlash"
            x-show="showFlash">
      {flash_off}
    </button>
    <button class="z-50 absolute top-3 right-3 size-fit transition delay-150"
            @click="showFlash = !showFlash"
            x-show="!showFlash">
      {flash_on}
    </button>
    
    <video id="video"
           class="z-50 object-cover size-full scale-x-[-1] rounded-lg transition delay-150"
           autoplay playsinline></video>
    <!-- CV toggle button (bottom-left) — opt-in to YOLO neural network detection -->
    <button class="z-50 absolute bottom-3 left-3 size-fit transition delay-150"
            @click="cvOn = !cvOn; localStorage.setItem('kipukas-cv-enabled', cvOn); if (globalThis.kipukasWorker) kipukasWorker.postMessage({{ type: 'SET_CV_MODE', enabled: cvOn }})"
            x-show="!cvOn"
            title="Enable CV detection (YOLO)">
      {cv_off}
    </button>
    <button class="z-50 absolute bottom-3 left-3 size-fit transition delay-150"
            @click="cvOn = !cvOn; localStorage.setItem('kipukas-cv-enabled', cvOn); if (globalThis.kipukasWorker) kipukasWorker.postMessage({{ type: 'SET_CV_MODE', enabled: cvOn }})"
            x-show="cvOn"
            title="Disable CV detection (YOLO)">
      {cv_on}
    </button>

    <canvas id="bbox-overlay"
            class="z-[60] absolute inset-0 size-full scale-x-[-1] rounded-lg pointer-events-none"
            width="640" height="640"></canvas>
    <div id="qr-result"></div>
  </div>
</div>
<script>kipukasQR.start();</script>"##,
        eye_off = SVG_EYE_OFF,
        flash_off = SVG_FLASH_OFF,
        flash_on = SVG_FLASH_ON,
        camera_switch = SVG_CAMERA_SWITCH,
        cv_off = SVG_CV_OFF,
        cv_on = SVG_CV_ON,
    )
}

/// Stop scanning and go to `path`, a path on this site (see `destination`:
/// a card's own url, or one that passed the safe-character check).
///
/// The path is relative, so a scan stays on whatever origin the site is
/// being served from, including a local build.
fn found_html(path: &str) -> String {
    format!(
        r##"<div class="p-2 text-center">
  <span class="text-kip-drk-sienna font-semibold">QR Code Found!</span>
</div>
<script>
  kipukasQR.stop();
  window.location.assign('{}');
</script>"##,
        path
    )
}

fn error_html(msg: &str) -> String {
    format!(
        r#"<div class="p-2 text-center">
  <span class="text-kip-red">{}</span>
</div>"#,
        msg
    )
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // -- percent_decode --

    #[test]
    fn decode_simple() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
    }

    #[test]
    fn decode_url() {
        assert_eq!(
            percent_decode("https%3A%2F%2Fkpks.us%2Fsome-card"),
            "https://kpks.us/some-card"
        );
    }

    #[test]
    fn decode_plus_sign() {
        assert_eq!(percent_decode("hello+world"), "hello world");
    }

    #[test]
    fn decode_passthrough() {
        assert_eq!(percent_decode("no-encoding"), "no-encoding");
    }

    // -- kipukas_path --

    #[test]
    fn every_kipukas_address_form_yields_its_path() {
        for address in [
            "kpks.us/some-card",
            "https://kpks.us/some-card",
            "https://www.kpks.us/some-card",
            "http://kpks.us/some-card",
            "http://www.kpks.us/some-card",
            "https://www.kipukas.cards/some-card",
            "https://kipukas.cards/some-card",
            "  kpks.us/some-card  ",
        ] {
            assert_eq!(kipukas_path(address), Ok("some-card"), "{}", address);
        }
    }

    #[test]
    fn leading_slashes_are_dropped_so_a_scan_cannot_leave_the_site() {
        assert_eq!(kipukas_path("kpks.us//other.site/x"), Ok("other.site/x"));
        assert_eq!(
            destination(kipukas_path("kpks.us//other.site/x").unwrap()),
            Ok("/other.site/x".to_string())
        );
    }

    #[test]
    fn invalid_url_rejected() {
        assert!(kipukas_path("https://evil.com/phish").is_err());
    }

    #[test]
    fn empty_url_rejected() {
        assert!(kipukas_path("").is_err());
    }

    // -- handle_status --

    #[test]
    fn status_open_no_privacy_shows_modal() {
        let html = handle_status("?action=open&privacy=false");
        assert!(html.contains("Privacy Notice"));
        assert!(html.contains("htmx.ajax"));
    }

    #[test]
    fn status_open_with_privacy_shows_scanner() {
        let html = handle_status("?action=open&privacy=true");
        assert!(html.contains("<video"));
        assert!(html.contains("kipukasQR.start()"));
    }

    #[test]
    fn status_close_returns_empty() {
        let html = handle_status("?action=close");
        assert!(html.is_empty());
    }

    #[test]
    fn status_error_shows_message() {
        let html = handle_status("?action=error&msg=Camera%20denied");
        assert!(html.contains("Camera denied"));
        assert!(html.contains("text-kip-red"));
    }

    // -- handle_found --

    /// A real card and its short code, taken from the compiled catalog.
    fn a_card() -> &'static crate::cards_generated::Card {
        CARDS
            .iter()
            .find(|c| c.slug == "brox_the_defiant")
            .expect("brox_the_defiant is in the catalog")
    }

    #[test]
    fn every_card_has_a_unique_four_character_code() {
        let mut codes: Vec<&str> = CARDS.iter().map(|c| c.code).collect();
        assert!(codes.iter().all(|c| c.len() == 4), "{:?}", codes);
        codes.sort();
        let total = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), total, "short codes must be unique");
    }

    #[test]
    fn a_scanned_short_code_goes_straight_to_the_card_page() {
        let card = a_card();
        let html = handle_found(&format!("?url=kpks.us%2F{}", card.code));
        assert!(html.contains("QR Code Found"));
        assert!(html.contains("kipukasQR.stop()"));
        assert!(html.contains("window.location.assign('/brox_the_defiant/')"));
        // Not the redirect stub, and not a fixed domain.
        assert!(!html.contains(&format!("/{}'", card.code)));
        assert!(!html.contains("kipukas.cards"));
    }

    #[test]
    fn every_address_form_of_a_short_code_resolves_the_same() {
        let card = a_card();
        for prefix in KIPUKAS_PREFIXES {
            let address = format!("{}{}", prefix, card.code);
            let html = handle_found(&format!("?url={}", address.replace('/', "%2F").replace(':', "%3A")));
            assert!(html.contains("assign('/brox_the_defiant/')"), "{}", address);
        }
    }

    #[test]
    fn a_scanned_slug_goes_to_the_card_page() {
        let html = handle_found("?url=https%3A%2F%2Fwww.kipukas.cards%2Fbrox_the_defiant%2F");
        assert!(html.contains("window.location.assign('/brox_the_defiant/')"));
    }

    #[test]
    fn short_codes_are_case_sensitive() {
        let card = a_card();
        let wrong_case = if card.code == card.code.to_lowercase() {
            card.code.to_uppercase()
        } else {
            card.code.to_lowercase()
        };
        let html = handle_found(&format!("?url=kpks.us%2F{}", wrong_case));
        assert!(!html.contains("assign('/brox_the_defiant/')"));
    }

    #[test]
    fn another_page_on_the_site_is_reached_by_its_relative_path() {
        let html = handle_found("?url=https%3A%2F%2Fkpks.us%2Fgame_rules%2Findex.html");
        assert!(html.contains("window.location.assign('/game_rules/index.html')"));
        let html = handle_found("?url=kpks.us%2Fa-card");
        assert!(html.contains("window.location.assign('/a-card')"));
        assert!(!html.contains("kipukas.cards"));
    }

    #[test]
    fn an_address_that_could_break_out_of_the_script_is_refused() {
        for path in [
            "');alert(1);//",
            "x%27%29%3Balert%281%29%3B",
            "</script><script>alert(1)</script>",
            "a b",
            "a\\b",
            "a\"b",
        ] {
            let url = format!("kpks.us/{}", path);
            let encoded: String = url
                .bytes()
                .map(|b| format!("%{:02X}", b))
                .collect();
            let html = handle_found(&format!("?url={}", encoded));
            if path.starts_with("x%27") {
                // Still percent-encoded after one decode: harmless characters only.
                assert!(html.contains("assign('/x%27%29%3Balert%281%29%3B')"), "{}", html);
            } else {
                assert!(html.contains("text-kip-red"), "should refuse {:?}: {}", path, html);
                assert!(!html.contains("<script>"), "{:?}", path);
            }
        }
    }

    #[test]
    fn found_invalid_url_shows_error() {
        let html = handle_found("?url=https%3A%2F%2Fevil.com%2Fphish");
        assert!(html.contains("text-kip-red"));
        assert!(html.contains("not a Kipukas URL"));
    }

    #[test]
    fn found_missing_url_shows_error() {
        let html = handle_found("");
        assert!(html.contains("No URL provided"));
    }

}
