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

// ── SVG icon constants ─────────────────────────────────────────────

const SVG_EYE_OFF: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-slate-100 hover:stroke-kip-red active:stroke-kip-drk-sienna stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="M3.98 8.223A10.477 10.477 0 0 0 1.934 12C3.226 16.338 7.244 19.5 12 19.5c.993 0 1.953-.138 2.863-.395M6.228 6.228A10.451 10.451 0 0 1 12 4.5c4.756 0 8.773 3.162 10.065 7.498a10.522 10.522 0 0 1-4.293 5.774M6.228 6.228 3 3m3.228 3.228 3.65 3.65m7.894 7.894L21 21m-3.228-3.228-3.65-3.65m0 0a3 3 0 1 0-4.243-4.243m4.242 4.242L9.88 9.88" /></svg>"#;

const SVG_FLASH_OFF: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-slate-100 hover:stroke-kip-red active:stroke-kip-drk-sienna stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="M11.412 15.655 9.75 21.75l3.745-4.012M9.257 13.5H3.75l2.659-2.849m2.048-2.194L14.25 2.25 12 10.5h8.25l-4.707 5.043M8.457 8.457 3 3m5.457 5.457 7.086 7.086m0 0L21 21" /></svg>"#;

const SVG_FLASH_ON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-slate-100 hover:stroke-kip-red active:stroke-kip-drk-sienna stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="m3.75 13.5 10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75Z" /></svg>"#;

const SVG_CAMERA_SWITCH: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-slate-100 hover:stroke-kip-red active:stroke-kip-drk-sienna stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0 3.181 3.183a8.25 8.25 0 0 0 13.803-3.7M4.031 9.865a8.25 8.25 0 0 1 13.803-3.7l3.181 3.182m0-4.991v4.99" /></svg>"#;

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

/// Validate and normalize a decoded QR URL.
/// Only accepts kipukas domains (kpks.us, kipukas.cards).
/// Returns the normalized https://www.kipukas.cards/ URL on success.
fn validate_kipukas_url(raw: &str) -> Result<String, &'static str> {
    let url = raw.trim();
    // Match patterns: kpks.us/..., https://kpks.us/...,
    // https://www.kpks.us/..., https://www.kipukas.cards/...
    if let Some(path) = url.strip_prefix("kpks.us/") {
        Ok(format!("https://www.kipukas.cards/{}", path))
    } else if let Some(path) = url.strip_prefix("https://kpks.us/") {
        Ok(format!("https://www.kipukas.cards/{}", path))
    } else if let Some(path) = url.strip_prefix("https://www.kpks.us/") {
        Ok(format!("https://www.kipukas.cards/{}", path))
    } else if let Some(path) = url.strip_prefix("http://kpks.us/") {
        Ok(format!("https://www.kipukas.cards/{}", path))
    } else if let Some(path) = url.strip_prefix("http://www.kpks.us/") {
        Ok(format!("https://www.kipukas.cards/{}", path))
    } else if url.starts_with("https://www.kipukas.cards/") {
        Ok(url.to_string())
    } else if let Some(path) = url.strip_prefix("https://kipukas.cards/") {
        Ok(format!("https://www.kipukas.cards/{}", path))
    } else {
        Err("Invalid QR code — not a Kipukas URL")
    }
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

    match validate_kipukas_url(&raw_url) {
        Ok(normalized) => found_html(&normalized),
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
        r##"<div x-data="{{ showFlash: false }}">
  <!-- Flash overlay - outside transformed container to cover full screen -->
  <div x-show="showFlash"
       class="fixed inset-0 z-40 bg-white"
       @click="showFlash = false"></div>
  
  <!-- Video container - higher z-index to stay above flash -->
  <div class="z-50 aspect-square fixed w-80 md:w-1/2 lg:w-1/3 -translate-x-1/2 lg:-translate-y-1/2 bottom-4 lg:bottom-auto lg:top-1/2 left-1/2 rounded-lg transition delay-150">
    <canvas id="canvas" class="-z-10 object-cover size-full scale-x-[-1] hidden"
            width="640" height="480"></canvas>
    
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
    <div id="qr-result"></div>
  </div>
</div>
<script>kipukasQR.start();</script>"##,
        eye_off = SVG_EYE_OFF,
        flash_off = SVG_FLASH_OFF,
        flash_on = SVG_FLASH_ON,
        camera_switch = SVG_CAMERA_SWITCH,
    )
}

fn found_html(url: &str) -> String {
    // Return HTML that stops scanning and redirects to the card page.
    // The script tag handles immediate navigation.
    format!(
        r##"<div class="p-2 text-center">
  <span class="text-kip-drk-sienna font-semibold">QR Code Found!</span>
</div>
<script>
  kipukasQR.stop();
  window.location.href = '{}';
</script>"##,
        url
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

    // -- validate_kipukas_url --

    #[test]
    fn valid_kpks_short() {
        assert_eq!(
            validate_kipukas_url("kpks.us/some-card"),
            Ok("https://www.kipukas.cards/some-card".to_string())
        );
    }

    #[test]
    fn valid_kpks_https() {
        assert_eq!(
            validate_kipukas_url("https://www.kpks.us/some-card"),
            Ok("https://www.kipukas.cards/some-card".to_string())
        );
    }

    #[test]
    fn valid_kpks_no_www() {
        assert_eq!(
            validate_kipukas_url("https://kpks.us/some-card"),
            Ok("https://www.kipukas.cards/some-card".to_string())
        );
    }

    #[test]
    fn valid_kipukas_cards() {
        assert_eq!(
            validate_kipukas_url("https://www.kipukas.cards/some-card"),
            Ok("https://www.kipukas.cards/some-card".to_string())
        );
    }

    #[test]
    fn valid_kipukas_no_www() {
        assert_eq!(
            validate_kipukas_url("https://kipukas.cards/some-card"),
            Ok("https://www.kipukas.cards/some-card".to_string())
        );
    }

    #[test]
    fn invalid_url_rejected() {
        assert!(validate_kipukas_url("https://evil.com/phish").is_err());
    }

    #[test]
    fn empty_url_rejected() {
        assert!(validate_kipukas_url("").is_err());
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

    #[test]
    fn found_valid_url_redirects() {
        let html = handle_found("?url=https%3A%2F%2Fkpks.us%2Fmy-card");
        assert!(html.contains("QR Code Found"));
        assert!(html.contains("https://www.kipukas.cards/my-card"));
        assert!(html.contains("window.location.href"));
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

    #[test]
    fn found_short_url() {
        let html = handle_found("?url=kpks.us%2Fa-card");
        assert!(html.contains("https://www.kipukas.cards/a-card"));
    }
}
