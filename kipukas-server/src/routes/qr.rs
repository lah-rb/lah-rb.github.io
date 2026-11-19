//! `/api/qr/status` — the QR scanner, an engine-owned view (Pattern 13 in
//! CONTRIBUTING.md).
//!
//! The engine holds where the scanner stands (closed, asking for the camera
//! privacy notice to be accepted, scanning, failed) and its two switches
//! (the screen light, and finding codes by computer vision), and draws it.
//! Every control in it posts back here.
//!
//! The camera is not the engine's: the page runs it for as long as the
//! scanner's video is drawn (qr-camera.js) and hands its frames to the
//! engine's worker. Nor are the model that finds codes and the reader that
//! reads them: the worker holds those two and passes the frames between
//! them and the engine (kipukas-worker.js, `crate::scan`). It says here
//! when either cannot be loaded, which the scanner shows. What a code says
//! is posted here too, and the engine decides where it leads: a card's
//! page, another page of this site, or nowhere, with the reason shown in
//! the scanner.
//!
//! For a player who has turned it on in the settings
//! (`settings::SCANNER_STATUS`), the scanner writes over the camera's
//! picture what its frames take. Only the worker can time them, so it
//! reports each frame's times here, and the engine keeps the count and
//! draws the line.

use std::cell::RefCell;

use crate::catalog::CARDS;
use crate::page;
use crate::routes::util::{escape_html, get_param, parse_form_body};
use crate::settings;
use crate::store;

/// Storage key: `true` once the camera privacy notice has been accepted.
/// The privacy policy page's own Accept link writes it too.
pub const PRIVACY: &str = "qr-privacy-accepted";
/// Storage key: `true` while finding codes by computer vision is turned on.
pub const CV: &str = "kipukas-cv-enabled";

/// The QR reader: zxing-cpp compiled to one WebAssembly file by
/// `zxing-reader/build.sh`, which the worker fetches and starts
/// (`assets/js/zxing-decode.js`). A newer build is another file, named here.
pub const READER: &str = "/assets/js-wasm/zxing-3.2.0-rc1.wasm";

/// The model that finds codes in a frame when computer vision is on: YOLO26n
/// trained on the card photos (CONTRIBUTING.md, The model that finds codes),
/// which the worker fetches and runs (`assets/js/yolo-inference.js`). A model
/// trained again is another file, named here.
pub const MODEL: &str = "/assets/js-wasm/yolo26n-qr-wide.onnx";

/// What runs the model: ONNX Runtime Web, served by the site from a folder
/// that carries its version (`scripts/fetch-onnxruntime.ts` puts it there
/// at build time, and stops the build if this names another folder). The
/// worker imports it when the model is first wanted
/// (`assets/js/yolo-inference.js`).
pub const RUNNER: &str = "/assets/js-wasm/onnxruntime-web-1.21.0/ort.all.min.mjs";

/// Where the scanner stands.
#[derive(Debug, Clone, PartialEq, Default)]
enum Face {
    #[default]
    Closed,
    /// The camera privacy notice, to accept or decline.
    Privacy,
    /// The camera's picture, with the scanner's controls.
    Scanning,
    /// The camera could not be started; why.
    Failed(String),
}

/// Where the worker stands with the model that finds codes, as it last
/// said.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum Finder {
    /// Nothing said yet: the model has not been asked for on this page.
    #[default]
    Unsaid,
    /// Being fetched and made ready. Frames are read whole meanwhile.
    Loading,
    Ready,
    /// It could not be fetched or could not be run. Frames are read whole.
    Failed,
}

/// What the worker spent on one frame, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Took {
    /// The engine's part: the frame taken in and thresholded, the model's
    /// input made and its output read, each picture cut out (`crate::scan`).
    engine: f64,
    /// The reader's, over every picture of the frame it was offered.
    reader: f64,
    /// The model's, if it was run on the frame.
    model: Option<f64>,
}

/// What the frames have taken since the scanner opened, or since the way
/// codes are found was last changed.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Tally {
    frames: u32,
    /// The frames the model was run on.
    with_model: u32,
    /// The sum over those frames of each part.
    engine: f64,
    reader: f64,
    model: f64,
    /// The frame counted last.
    last: Took,
}

impl Tally {
    fn count(&mut self, took: Took) {
        self.frames += 1;
        self.engine += took.engine;
        self.reader += took.reader;
        if let Some(model) = took.model {
            self.with_model += 1;
            self.model += model;
        }
        self.last = took;
    }
}

#[derive(Default)]
struct Scanner {
    face: Face,
    /// The camera privacy notice has been accepted.
    privacy: bool,
    /// The screen is lit white, to light a card held to a front camera.
    light: bool,
    /// Codes are found by computer vision (YOLO) before they are read.
    cv: bool,
    /// Where the worker stands with the model that does it.
    finder: Finder,
    /// The worker could not load the QR reader, so no frame is read.
    no_reader: bool,
    /// What the frames take, as the worker reports it while the status
    /// line is wanted.
    tally: Tally,
    /// What came of the last code read: where it leads, or why it is
    /// refused.
    read: Option<Result<String, &'static str>>,
}

thread_local! {
    static SCANNER: RefCell<Scanner> = RefCell::new(Scanner::default());
}

fn with_scanner<R>(f: impl FnOnce(&mut Scanner) -> R) -> R {
    SCANNER.with(|scanner| f(&mut scanner.borrow_mut()))
}

/// At load: what is stored of the player's two choices.
pub fn restore(privacy: bool, cv: bool) {
    with_scanner(|scanner| {
        *scanner = Scanner { privacy, cv, ..Scanner::default() };
    });
}

/// Forget everything, as before any page has loaded.
#[cfg(test)]
pub fn reset() {
    restore(false, false);
}

// ── SVG icon constants ─────────────────────────────────────────────

const SVG_EYE_OFF: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-slate-100 hover:stroke-kip-red active:stroke-kip-drk-sienna stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="M3.98 8.223A10.477 10.477 0 0 0 1.934 12C3.226 16.338 7.244 19.5 12 19.5c.993 0 1.953-.138 2.863-.395M6.228 6.228A10.451 10.451 0 0 1 12 4.5c4.756 0 8.773 3.162 10.065 7.498a10.522 10.522 0 0 1-4.293 5.774M6.228 6.228 3 3m3.228 3.228 3.65 3.65m7.894 7.894L21 21m-3.228-3.228-3.65-3.65m0 0a3 3 0 1 0-4.243-4.243m4.242 4.242L9.88 9.88" /></svg>"#;

const SVG_FLASH_OFF: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-slate-100 hover:stroke-kip-red active:stroke-kip-drk-sienna stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="M11.412 15.655 9.75 21.75l3.745-4.012M9.257 13.5H3.75l2.659-2.849m2.048-2.194L14.25 2.25 12 10.5h8.25l-4.707 5.043M8.457 8.457 3 3m5.457 5.457 7.086 7.086m0 0L21 21" /></svg>"#;

const SVG_FLASH_ON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-slate-100 hover:stroke-kip-red active:stroke-kip-drk-sienna stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="m3.75 13.5 10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75Z" /></svg>"#;

const SVG_CAMERA_SWITCH: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-slate-100 hover:stroke-kip-red active:stroke-kip-drk-sienna stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0 3.181 3.183a8.25 8.25 0 0 0 13.803-3.7M4.031 9.865a8.25 8.25 0 0 1 13.803-3.7l3.181 3.182m0-4.991v4.99" /></svg>"#;

// CV toggle: "cpu-chip" icon — represents computer vision / neural network
const SVG_CV_OFF: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-slate-400 stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="M8.25 3v1.5M4.5 8.25H3m18 0h-1.5M4.5 12H3m18 0h-1.5m-15 3.75H3m18 0h-1.5M8.25 19.5V21M12 3v1.5m0 15V21m3.75-18v1.5m0 15V21m-9-1.5h10.5a2.25 2.25 0 0 0 2.25-2.25V6.75a2.25 2.25 0 0 0-2.25-2.25H6.75A2.25 2.25 0 0 0 4.5 6.75v10.5a2.25 2.25 0 0 0 2.25 2.25Zm.75-12h9v9h-9v-9Z" /></svg>"#;

const SVG_CV_ON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="w-9 h-9 scale-95 fill-transparent stroke-emerald-400 stroke-2 m-2"><path stroke-linecap="round" stroke-linejoin="round" d="M8.25 3v1.5M4.5 8.25H3m18 0h-1.5M4.5 12H3m18 0h-1.5m-15 3.75H3m18 0h-1.5M8.25 19.5V21M12 3v1.5m0 15V21m3.75-18v1.5m0 15V21m-9-1.5h10.5a2.25 2.25 0 0 0 2.25-2.25V6.75a2.25 2.25 0 0 0-2.25-2.25H6.75A2.25 2.25 0 0 0 4.5 6.75v10.5a2.25 2.25 0 0 0 2.25 2.25Zm.75-12h9v9h-9v-9Z" /></svg>"#;

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
/// it is made only of plain path characters. The address comes from whatever
/// QR code the camera saw, so it is not trusted.
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

/// Characters allowed in a pass-through path.
fn is_safe_path_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-_./?=&#%".contains(&b)
}

// ── Route handlers ─────────────────────────────────────────────────

/// GET /api/qr/status — the scanner as it stands. Nothing while it is closed.
pub fn handle_status(_query: &str) -> String {
    SCANNER.with(|scanner| render(&scanner.borrow()))
}

/// POST /api/qr/status — a control of the scanner, or a report to it.
///
/// Body:
///   - `action=toggle`: the toolbar's button. Opens the scanner (the
///     privacy notice first, until it has been accepted) or closes it.
///   - `action=accept`: the privacy notice was accepted
///   - `action=close`: close the scanner
///   - `action=light`: turn the screen light on or off
///   - `action=cv`: turn finding codes by computer vision on or off
///   - `action=found&url=<what the code says>`: the worker read a code
///   - `action=finder&state=<loading|ready|failed>`: where the worker
///     stands with the model that finds codes
///   - `action=reader&state=<ready|failed>`: whether the worker could load
///     the QR reader
///   - `action=frame&engine=<ms>&reader=<ms>[&model=<ms>]`: what the worker
///     spent on a frame, by whose work it was; `model` only if the model
///     was run on it. Sent while the status line is wanted
///   - `action=error&msg=<why>`: the page could not start the camera
pub fn handle_status_post(body: &str) -> String {
    let params = parse_form_body(body);
    match get_param(&params, "action").unwrap_or("") {
        "toggle" => {
            if with_scanner(|scanner| scanner.face == Face::Closed) {
                open();
            } else {
                close();
            }
        }
        "accept" => {
            with_scanner(|scanner| scanner.privacy = true);
            store::put(PRIVACY, Some("true".to_string()));
            open();
        }
        "close" => close(),
        "light" => with_scanner(|scanner| scanner.light = !scanner.light),
        "cv" => {
            let cv = with_scanner(|scanner| {
                scanner.cv = !scanner.cv;
                scanner.tally = Tally::default();
                scanner.cv
            });
            store::put(CV, Some(cv.to_string()));
            page::qr(cv);
        }
        "found" => found(get_param(&params, "url").unwrap_or("")),
        "finder" => {
            let finder = match get_param(&params, "state") {
                Some("loading") => Finder::Loading,
                Some("ready") => Finder::Ready,
                Some("failed") => Finder::Failed,
                _ => return String::new(),
            };
            with_scanner(|scanner| scanner.finder = finder);
            said_by_the_worker();
        }
        "reader" => {
            let no_reader = match get_param(&params, "state") {
                Some("ready") => false,
                Some("failed") => true,
                _ => return String::new(),
            };
            with_scanner(|scanner| scanner.no_reader = no_reader);
            said_by_the_worker();
        }
        "frame" => {
            // Times are the worker's own measurements; anything else is
            // not counted.
            let ms = |name: &str| {
                get_param(&params, name).and_then(|ms| ms.parse::<f64>().ok()).filter(|ms| ms.is_finite() && *ms >= 0.0)
            };
            let (Some(engine), Some(reader)) = (ms("engine"), ms("reader")) else {
                return String::new();
            };
            with_scanner(|scanner| scanner.tally.count(Took { engine, reader, model: ms("model") }));
            if settings::is_on(&settings::SCANNER_STATUS) {
                said_by_the_worker();
            }
        }
        "error" => {
            let why = get_param(&params, "msg").filter(|msg| !msg.is_empty()).unwrap_or("The camera could not be started");
            with_scanner(|scanner| {
                if scanner.face == Face::Scanning {
                    scanner.face = Face::Failed(why.to_string());
                }
            });
        }
        _ => {}
    }
    String::new()
}

/// Open the scanner: on the camera if the privacy notice has been accepted,
/// on the notice if not.
fn open() {
    let cv = with_scanner(|scanner| {
        scanner.face = if scanner.privacy { Face::Scanning } else { Face::Privacy };
        scanner.light = false;
        scanner.read = None;
        scanner.tally = Tally::default();
        scanner.cv
    });
    // The worker gets its decoder ready meanwhile, while the notice is read
    // or the camera starts.
    page::qr(cv);
}

fn close() {
    with_scanner(|scanner| {
        scanner.face = Face::Closed;
        scanner.light = false;
        scanner.read = None;
    });
}

/// The worker read a code. A code that leads somewhere on this site takes
/// the player there; any other is refused in the scanner, which goes on
/// looking.
fn found(url: &str) {
    if !with_scanner(|scanner| scanner.face == Face::Scanning) {
        return;
    }
    let read = kipukas_path(url).and_then(destination);
    if let Ok(path) = &read {
        page::go(path);
    }
    with_scanner(|scanner| scanner.read = Some(read));
    said_by_the_worker();
}

/// The worker has reported something the scanner shows. The report is not a
/// tap in the scanner, so nothing draws the scanner again unless it is
/// asked for. A closed scanner has nothing to draw.
fn said_by_the_worker() {
    if with_scanner(|scanner| scanner.face != Face::Closed) {
        page::redraw("/api/qr/status");
    }
}

/// The player has turned the scanner's status line on or off
/// (`crate::settings`). The count starts again, and an open scanner tells
/// its worker, which reports what the frames take only while that is
/// wanted.
pub fn status_setting_changed() {
    let open = with_scanner(|scanner| {
        scanner.tally = Tally::default();
        (scanner.face != Face::Closed).then_some(scanner.cv)
    });
    if let Some(cv) = open {
        page::qr(cv);
        page::redraw("/api/qr/status");
    }
}

// ── HTML fragment builders ─────────────────────────────────────────

const POST: &str = r#"data-engine-post="/api/qr/status""#;

fn render(scanner: &Scanner) -> String {
    match &scanner.face {
        Face::Closed => String::new(),
        Face::Privacy => privacy_modal(),
        Face::Scanning => scanning_ui(scanner),
        Face::Failed(why) => error_html(why),
    }
}

fn privacy_modal() -> String {
    format!(
        r##"<div class="fixed inset-0 flex items-center justify-center z-50" role="dialog" aria-modal="true" aria-label="Camera privacy notice">
  <div class="z-50 absolute inset-0 bg-slate-300 opacity-75"></div>
  <div class="bg-amber-50 z-50 p-6 rounded-lg shadow-xl w-full max-w-lg">
    <h2 class="text-xl font-semibold mb-4 text-kip-drk-sienna">Privacy Notice</h2>
    <p class="mb-4 text-kip-drk-sienna">
      By allowing camera access to our QR code scanner, you agree to our
      <a href="/privacy_policy" class="text-kip-red hover:text-emerald-600 underline">privacy policy</a>.
      Breath easy, we don&rsquo;t collect any data.
    </p>
    <button type="button" {post} data-engine-body="action=accept"
            class="bg-kip-red hover:bg-emerald-600 text-amber-50 font-bold py-2 my-2 px-4 rounded">
      Accept &amp; Continue
    </button>
    <button type="button" {post} data-engine-body="action=close"
            class="bg-kip-red hover:bg-emerald-600 text-amber-50 font-bold py-2 px-4 rounded">
      Don&rsquo;t Need It
    </button>
  </div>
</div>"##,
        post = POST
    )
}

/// The camera's picture and the scanner's controls.
///
/// The page runs the camera while the element marked `data-qr-camera` is
/// drawn, into the video in it, and reads frames through the canvas. Those
/// three, and the canvas the worker's findings are drawn on, are the page's
/// to change, so each is marked to be kept as it is when the rest is drawn
/// again. The button marked `data-qr-switch` is the page's too: which
/// camera is in use is a matter of the device, not of the scanner.
fn scanning_ui(scanner: &Scanner) -> String {
    // Lights the card for a front camera. A tap anywhere on it turns it off.
    let light = if scanner.light {
        format!(
            r#"<div role="button" aria-label="Turn the screen light off" {post} data-engine-body="action=light" class="fixed inset-0 z-40 bg-white w-screen h-screen"></div>"#,
            post = POST
        )
    } else {
        String::new()
    };
    let (light_label, light_icon) =
        if scanner.light { ("Turn the screen light off", SVG_FLASH_OFF) } else { ("Turn the screen light on", SVG_FLASH_ON) };
    let (cv_label, cv_icon) = if scanner.cv {
        ("Disable CV detection (YOLO)", SVG_CV_ON)
    } else {
        ("Enable CV detection (YOLO)", SVG_CV_OFF)
    };
    // What the worker has said it could not load. Without the reader no
    // frame is read. Without the model (asked for only with computer vision
    // on) frames are read whole.
    let note = |look: &str, said: &str| format!(r#"<p class="rounded bg-black/60 px-1.5 py-0.5 {}">{}</p>"#, look, said);
    let mut notes = String::new();
    if scanner.no_reader {
        notes += &note("text-red-300", "The QR reader could not be loaded. Codes cannot be read until it is.");
    }
    if scanner.cv {
        match scanner.finder {
            Finder::Loading => notes += &note("", "Loading computer vision. Reading without it meanwhile."),
            Finder::Failed => notes += &note("text-red-300", "Computer vision could not be loaded. Reading without it."),
            Finder::Unsaid | Finder::Ready => {}
        }
    }
    // What the frames take, for a player who has asked to see it. Its
    // place is drawn from the start, and filled once a frame is counted.
    let status = if settings::is_on(&settings::SCANNER_STATUS) {
        format!(
            r#"<p class="z-50 absolute top-14 left-3 right-3 rounded bg-black/60 px-1.5 py-0.5 text-xs text-center text-slate-100 empty:hidden">{}</p>"#,
            status_line(&scanner.tally)
        )
    } else {
        String::new()
    };
    let read = match &scanner.read {
        None => String::new(),
        Some(Ok(_)) => r#"<div class="p-2 text-center"><span class="text-kip-drk-sienna font-semibold">QR Code Found!</span></div>"#
            .to_string(),
        Some(Err(why)) => error_html(why),
    };

    format!(
        r##"<div>
  {light}
  <!-- Video container - higher z-index to stay above the screen light -->
  <div data-qr-camera class="z-50 aspect-square fixed w-80 md:w-1/2 lg:w-1/3 -translate-x-1/2 lg:-translate-y-1/2 bottom-8 left-1/2 rounded-lg transition delay-150">
    <canvas id="canvas" data-engine-keep class="-z-10 object-cover size-full scale-x-[-1] hidden"
            width="640" height="640"></canvas>

    <!-- Close button (left) -->
    <button type="button" aria-label="Close the scanner" class="z-50 absolute top-3 left-3 size-fit transition delay-150"
            {post} data-engine-body="action=close">
      {eye_off}
    </button>

    <!-- Switch camera button (center) -->
    <button type="button" aria-label="Switch camera" class="z-50 absolute top-3 left-1/2 -translate-x-1/2 size-fit transition delay-150"
            data-qr-switch>
      {camera_switch}
    </button>

    <!-- Screen light button (right) -->
    <button type="button" aria-label="{light_label}" class="z-50 absolute top-3 right-3 size-fit transition delay-150"
            {post} data-engine-body="action=light">
      {light_icon}
    </button>

    <video id="video" data-engine-keep
           class="z-50 object-cover size-full scale-x-[-1] rounded-lg transition delay-150"
           autoplay playsinline></video>
    <!-- CV toggle button (bottom-left): opt-in to YOLO neural network detection -->
    <button type="button" aria-label="{cv_label}" title="{cv_label}" class="z-50 absolute bottom-3 left-3 size-fit transition delay-150"
            {post} data-engine-body="action=cv">
      {cv_icon}
    </button>

    <!-- What the worker could not load (bottom-right) -->
    <div role="status" class="z-50 absolute bottom-3 right-3 max-w-[70%] flex flex-col items-end gap-1 text-xs text-right text-slate-100">{notes}</div>
    {status}

    <canvas id="bbox-overlay" data-engine-keep
            class="z-[60] absolute inset-0 size-full scale-x-[-1] rounded-lg pointer-events-none"
            width="640" height="640"></canvas>
    <div id="qr-result" role="status">{read}</div>
  </div>
</div>"##,
        light = light,
        post = POST,
        eye_off = SVG_EYE_OFF,
        camera_switch = SVG_CAMERA_SWITCH,
        light_label = light_label,
        light_icon = light_icon,
        cv_label = cv_label,
        cv_icon = cv_icon,
        notes = notes,
        status = status,
        read = read,
    )
}

/// What the last frame took and, in brackets, the average since the count
/// began: the engine's part, the reader's, and the model's if it was run on
/// that frame. Nothing until a frame has been counted.
fn status_line(tally: &Tally) -> String {
    if tally.frames == 0 {
        return String::new();
    }
    // To a tenth of a millisecond, without a tenth that is nought.
    let tenths = |ms: f64| {
        let said = format!("{:.1}", ms);
        said.strip_suffix(".0").map(str::to_string).unwrap_or(said)
    };
    let frames = f64::from(tally.frames);
    let model = match tally.last.model {
        Some(model) => format!(", model {:.0} ({:.0})", model, tally.model / f64::from(tally.with_model.max(1))),
        None => String::new(),
    };
    format!(
        "frame {}: engine {} ms ({} a frame), reader {} ({}){}",
        tally.frames,
        tenths(tally.last.engine),
        tenths(tally.engine / frames),
        tenths(tally.last.reader),
        tenths(tally.reader / frames),
        model
    )
}

/// `msg` may be the browser's words (why the camera did not start).
fn error_html(msg: &str) -> String {
    format!(
        r#"<div class="p-2 text-center">
  <span class="text-kip-red">{}</span>
</div>"#,
        escape_html(msg)
    )
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() {
        reset();
        settings::reset();
        store::reset();
        page::reset();
    }

    /// What the worker is asked for when its decoder is wanted.
    fn asked_of_the_worker(cv: bool, status: bool) -> serde_json::Value {
        serde_json::json!({ "cv": cv, "reader": READER, "model": MODEL, "runner": RUNNER, "status": status })
    }

    /// A tap on one of the scanner's controls, and the scanner as it is
    /// then drawn.
    fn tap(body: &str) -> String {
        assert_eq!(handle_status_post(body), "");
        handle_status("")
    }

    /// Encode every byte, as the worker's `encodeURIComponent` would at least.
    fn encoded(text: &str) -> String {
        text.bytes().map(|b| format!("%{:02X}", b)).collect()
    }

    /// The worker reads a code: where the page is sent, and the scanner as
    /// it is then drawn.
    fn read(code: &str) -> (Option<String>, String) {
        let html = tap(&format!("action=found&url={}", encoded(code)));
        (page::take_go(), html)
    }

    fn scanning() {
        fresh();
        restore(true, false);
        tap("action=toggle");
        page::reset();
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

    // -- the scanner --

    #[test]
    fn the_scanner_is_nothing_until_it_is_opened() {
        fresh();
        assert_eq!(handle_status(""), "");
        fresh();
    }

    #[test]
    fn the_first_opening_asks_for_the_privacy_notice_to_be_accepted() {
        fresh();
        let html = tap("action=toggle");
        assert!(html.contains("Privacy Notice"));
        assert!(html.contains(r#"data-engine-post="/api/qr/status" data-engine-body="action=accept""#));
        assert!(html.contains(r#"data-engine-post="/api/qr/status" data-engine-body="action=close""#));
        assert!(!html.contains("<video"));
        // The worker is told to get its decoder ready meanwhile.
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert_eq!(handed["qr"], asked_of_the_worker(false, false));

        // Declining closes it, and nothing is remembered.
        assert_eq!(tap("action=close"), "");
        assert!(tap("action=toggle").contains("Privacy Notice"));
        fresh();
    }

    #[test]
    fn accepting_opens_the_camera_and_is_remembered() {
        fresh();
        tap("action=toggle");
        let html = tap("action=accept");
        assert!(html.contains("<video"));
        assert!(html.contains("data-qr-camera"));
        assert_eq!(store::take_saves().get(PRIVACY), Some(&Some("true".to_string())));

        // From now on the button opens the camera straight away, on this
        // page and, once stored, on the next.
        tap("action=toggle");
        assert!(tap("action=toggle").contains("<video"));
        fresh();
        restore(true, false);
        assert!(tap("action=toggle").contains("<video"));
        fresh();
    }

    #[test]
    fn the_scanner_has_nothing_for_the_page_to_run() {
        scanning();
        tap("action=light");
        for html in [privacy_modal(), handle_status("")] {
            for forbidden in ["onclick", "htmx", "hx-", "x-data", "x-show", "@click", "<script", "kipukasQR", "localStorage"] {
                assert!(!html.contains(forbidden), "{} in {}", forbidden, html);
            }
        }
        fresh();
    }

    #[test]
    fn what_the_page_runs_the_camera_with_is_kept_when_the_scanner_is_drawn_again() {
        scanning();
        let html = handle_status("");
        for kept in [
            r#"<canvas id="canvas" data-engine-keep"#,
            r#"<video id="video" data-engine-keep"#,
            r#"<canvas id="bbox-overlay" data-engine-keep"#,
        ] {
            assert!(html.contains(kept), "{}", kept);
        }
        assert!(html.contains("data-qr-switch"));
        fresh();
    }

    #[test]
    fn the_buttons_toggle_closes_the_scanner_wherever_it_stands() {
        scanning();
        assert_eq!(tap("action=toggle"), "");
        tap("action=toggle");
        tap("action=error&msg=Camera%20denied");
        assert_eq!(tap("action=toggle"), "");
        fresh();
    }

    #[test]
    fn the_screen_light_is_a_switch_and_goes_out_when_the_scanner_closes() {
        scanning();
        let off = handle_status("");
        assert!(off.contains(r#"aria-label="Turn the screen light on""#));
        assert!(!off.contains("bg-white w-screen"));

        let on = tap("action=light");
        assert!(on.contains("fixed inset-0 z-40 bg-white w-screen h-screen"));
        assert_eq!(on.matches(r#"data-engine-body="action=light""#).count(), 2, "the light itself turns it off too");
        assert!(on.contains(r#"aria-label="Turn the screen light off""#));

        assert!(!tap("action=light").contains("bg-white w-screen"));
        tap("action=light");
        tap("action=close");
        assert!(!tap("action=toggle").contains("bg-white w-screen"));
        fresh();
    }

    #[test]
    fn finding_codes_by_computer_vision_is_a_switch_that_is_remembered_and_told_to_the_worker() {
        scanning();
        assert!(handle_status("").contains(r#"title="Enable CV detection (YOLO)""#));
        let on = tap("action=cv");
        assert!(on.contains(r#"title="Disable CV detection (YOLO)""#));
        assert!(on.contains("stroke-emerald-400"));
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert_eq!(handed["qr"], asked_of_the_worker(true, false));
        assert_eq!(handed["save"][CV], "true");

        tap("action=cv");
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert_eq!(handed["qr"], asked_of_the_worker(false, false));
        assert_eq!(handed["save"][CV], "false");

        // Stored, it is on when the scanner next opens.
        fresh();
        restore(true, true);
        assert!(tap("action=toggle").contains(r#"title="Disable CV detection (YOLO)""#));
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert_eq!(handed["qr"], asked_of_the_worker(true, false));
        fresh();
    }

    // -- what the frames take --

    /// The settings page's switch for the status line.
    fn turn_the_status_line(on: bool) {
        if settings::is_on(&settings::SCANNER_STATUS) != on {
            settings::handle_post("toggle=kipukas-scanner-status");
        }
    }

    const STATUS_PLACE: &str = "top-14 left-3 right-3";

    #[test]
    fn nothing_is_said_of_what_frames_take_unless_the_player_has_asked() {
        scanning();
        let before = handle_status("");
        assert!(!before.contains(STATUS_PLACE));
        // The worker is not asked to report, and if it does nothing is drawn for it.
        tap("action=toggle");
        tap("action=toggle");
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert_eq!(handed["qr"], asked_of_the_worker(false, false));
        assert_eq!(tap("action=frame&engine=5.94&reader=0.4"), before);
        assert!(page::take_redraws().is_empty());
        fresh();
    }

    #[test]
    fn with_the_status_line_on_the_worker_is_asked_to_report_and_the_scanner_shows_it() {
        fresh();
        restore(true, false);
        turn_the_status_line(true);
        page::reset();
        let html = tap("action=toggle");
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert_eq!(handed["qr"], asked_of_the_worker(false, true));
        // Its place is there from the start, empty until a frame is counted.
        assert!(html.contains(&format!(r#"{} rounded bg-black/60 px-1.5 py-0.5 text-xs text-center text-slate-100 empty:hidden"></p>"#, STATUS_PLACE)));

        let html = tap("action=frame&engine=5.94&reader=0.4");
        assert!(html.contains(">frame 1: engine 5.9 ms (5.9 a frame), reader 0.4 (0.4)</p>"), "{}", html);
        // The worker's report is not a tap: the scanner is drawn again for it.
        assert_eq!(page::take_redraws(), ["/api/qr/status"]);

        // The average is over every frame counted; a tenth that is nought is left out.
        let html = tap("action=frame&engine=8.06&reader=0");
        assert!(html.contains(">frame 2: engine 8.1 ms (7 a frame), reader 0 (0.2)</p>"), "{}", html);
        fresh();
    }

    #[test]
    fn the_models_time_is_shown_for_a_frame_it_was_run_on_and_averaged_over_those_frames() {
        fresh();
        restore(true, true);
        turn_the_status_line(true);
        tap("action=toggle");
        let html = tap("action=frame&engine=6&reader=9&model=40.4");
        assert!(html.contains(">frame 1: engine 6 ms (6 a frame), reader 9 (9), model 40 (40)</p>"), "{}", html);
        // A frame read whole (the model not ready) says nothing of the model.
        let html = tap("action=frame&engine=2&reader=3");
        assert!(html.contains(">frame 2: engine 2 ms (4 a frame), reader 3 (6)</p>"), "{}", html);
        // The model's average is over the frames it ran on, not all of them.
        let html = tap("action=frame&engine=4&reader=0&model=30");
        assert!(html.contains(">frame 3: engine 4 ms (4 a frame), reader 0 (4), model 30 (35)</p>"), "{}", html);
        fresh();
    }

    #[test]
    fn the_count_starts_again_when_the_scanner_opens_and_when_computer_vision_is_turned() {
        fresh();
        restore(true, false);
        turn_the_status_line(true);
        tap("action=toggle");
        tap("action=frame&engine=10&reader=10");
        assert!(tap("action=frame&engine=20&reader=20").contains(">frame 2: "));

        let html = tap("action=cv");
        assert!(!html.contains("frame 2"), "the count is of frames looked at one way");
        assert!(tap("action=frame&engine=1&reader=1&model=50").contains(">frame 1: engine 1 ms (1 a frame), reader 1 (1), model 50 (50)</p>"));

        tap("action=close");
        let html = tap("action=toggle");
        assert!(!html.contains("frame 1"));
        assert!(tap("action=frame&engine=3&reader=3").contains(">frame 1: engine 3 ms (3 a frame), reader 3 (3)</p>"));
        fresh();
    }

    #[test]
    fn a_report_that_is_not_times_is_not_counted() {
        fresh();
        restore(true, false);
        turn_the_status_line(true);
        tap("action=toggle");
        page::reset();
        let before = handle_status("");
        for body in [
            "action=frame",
            "action=frame&engine=5",
            "action=frame&reader=5",
            "action=frame&engine=abc&reader=5",
            "action=frame&engine=-1&reader=5",
            "action=frame&engine=NaN&reader=5",
            "action=frame&engine=inf&reader=5",
            "action=frame&engine=%3Cb%3E&reader=5",
        ] {
            assert_eq!(tap(body), before, "{}", body);
        }
        assert!(page::take_redraws().is_empty());
        // A model time that is not one is left out, and the frame still counted.
        assert!(tap("action=frame&engine=5&reader=5&model=fast").contains(">frame 1: engine 5 ms (5 a frame), reader 5 (5)</p>"));
        fresh();
    }

    #[test]
    fn turning_the_status_line_while_the_scanner_is_open_tells_the_worker_and_draws_the_scanner() {
        scanning();
        turn_the_status_line(true);
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert_eq!(handed["qr"], asked_of_the_worker(false, true));
        assert!(handed["redraw"].as_array().is_some_and(|routes| routes.contains(&"/api/qr/status".into())), "{}", handed);
        assert!(handle_status("").contains(STATUS_PLACE));

        turn_the_status_line(false);
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert_eq!(handed["qr"], asked_of_the_worker(false, false));
        assert!(!handle_status("").contains(STATUS_PLACE));

        // A closed scanner has nobody to tell.
        tap("action=close");
        page::reset();
        turn_the_status_line(true);
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert!(handed.get("qr").is_none(), "{}", handed);
        assert!(!handed["redraw"].as_array().is_some_and(|routes| routes.contains(&"/api/qr/status".into())), "{}", handed);
        fresh();
    }

    // -- what the worker could not load --

    #[test]
    fn while_the_model_is_loading_or_could_not_be_the_scanner_says_it_reads_without_it() {
        scanning();
        tap("action=cv");
        page::reset();
        assert!(!handle_status("").contains("computer vision"), "nothing is said before the worker says");

        let html = tap("action=finder&state=loading");
        assert!(html.contains("Loading computer vision. Reading without it meanwhile."));
        // The worker's report is not a tap: the scanner is drawn again for it.
        assert_eq!(page::take_redraws(), ["/api/qr/status"]);

        let html = tap("action=finder&state=failed");
        assert!(html.contains("Computer vision could not be loaded. Reading without it."));
        assert!(!html.contains("Loading computer vision"));
        assert!(html.contains("<video"), "the scanner goes on");

        let html = tap("action=finder&state=ready");
        assert!(!html.contains("omputer vision could not") && !html.contains("Loading computer vision"));
        fresh();
    }

    #[test]
    fn what_is_said_of_the_model_is_shown_only_with_computer_vision_on() {
        scanning();
        tap("action=cv");
        tap("action=finder&state=failed");
        assert!(!tap("action=cv").contains("Computer vision could not be loaded"));
        // On again, it is still what the worker last said, until it says otherwise.
        assert!(tap("action=cv").contains("Computer vision could not be loaded"));
        // A state that is not one is not taken.
        assert!(tap("action=finder&state=broken").contains("Computer vision could not be loaded"));
        assert!(tap("action=finder").contains("Computer vision could not be loaded"));
        fresh();
    }

    #[test]
    fn a_reader_that_could_not_be_loaded_is_said_until_it_is() {
        scanning();
        page::reset();
        let html = tap("action=reader&state=failed");
        assert!(html.contains("The QR reader could not be loaded."));
        assert_eq!(page::take_redraws(), ["/api/qr/status"]);
        assert!(tap("action=reader&state=nonsense").contains("The QR reader could not be loaded."));
        assert!(!tap("action=reader&state=ready").contains("The QR reader could not be loaded."));
        fresh();
    }

    #[test]
    fn a_report_from_the_worker_with_the_scanner_closed_draws_nothing() {
        fresh();
        assert_eq!(tap("action=finder&state=failed"), "");
        assert_eq!(tap("action=reader&state=failed"), "");
        assert!(page::take_redraws().is_empty());
        fresh();
    }

    #[test]
    fn a_camera_that_cannot_be_started_says_why_in_the_browsers_words_escaped() {
        scanning();
        let html = tap("action=error&msg=%3Cb%3EPermission%20denied%3C%2Fb%3E");
        assert!(html.contains("&lt;b&gt;Permission denied&lt;/b&gt;"));
        assert!(html.contains("text-kip-red"));
        assert!(!html.contains("<video"));

        scanning();
        assert!(tap("action=error").contains("The camera could not be started"));

        // A report with no scanner open opens nothing.
        fresh();
        assert_eq!(tap("action=error&msg=late"), "");
        fresh();
    }

    // -- a code is read --

    /// A real card and its short code, taken from the compiled catalog.
    fn a_card() -> &'static crate::catalog::Card {
        CARDS
            .iter()
            .find(|c| c.slug == "brox_the_defiant")
            .expect("brox_the_defiant is in the catalog")
    }

    #[test]
    fn every_card_has_a_unique_four_character_code() {
        // All but the site's not-found page, which is not a printed card and
        // has no QR code.
        let mut codes: Vec<&str> = CARDS
            .iter()
            .filter(|c| c.url != "/404.html")
            .map(|c| c.code)
            .collect();
        assert_eq!(codes.len(), CARDS.len() - 1);
        assert!(codes.iter().all(|c| c.len() == 4), "{:?}", codes);
        codes.sort();
        let total = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), total, "short codes must be unique");
    }

    #[test]
    fn a_scanned_short_code_goes_straight_to_the_card_page() {
        scanning();
        let card = a_card();
        let (go, html) = read(&format!("kpks.us/{}", card.code));
        // Not the redirect stub, and not a fixed domain.
        assert_eq!(go.as_deref(), Some("/brox_the_defiant/"));
        assert!(html.contains("QR Code Found"));
        assert!(html.contains("<video"), "the scanner stays as it is until the page goes");
        fresh();
    }

    #[test]
    fn every_address_form_of_a_short_code_resolves_the_same() {
        let card = a_card();
        for prefix in KIPUKAS_PREFIXES {
            scanning();
            let (go, _) = read(&format!("{}{}", prefix, card.code));
            assert_eq!(go.as_deref(), Some("/brox_the_defiant/"), "{}", prefix);
        }
        fresh();
    }

    #[test]
    fn a_scanned_slug_goes_to_the_card_page() {
        scanning();
        let (go, _) = read("https://www.kipukas.cards/brox_the_defiant/");
        assert_eq!(go.as_deref(), Some("/brox_the_defiant/"));
        fresh();
    }

    #[test]
    fn short_codes_are_case_sensitive() {
        scanning();
        let card = a_card();
        let wrong_case = if card.code == card.code.to_lowercase() {
            card.code.to_uppercase()
        } else {
            card.code.to_lowercase()
        };
        let (go, _) = read(&format!("kpks.us/{}", wrong_case));
        assert_ne!(go.as_deref(), Some("/brox_the_defiant/"));
        fresh();
    }

    #[test]
    fn another_page_on_the_site_is_reached_by_its_relative_path() {
        scanning();
        assert_eq!(read("https://kpks.us/game_rules/index.html").0.as_deref(), Some("/game_rules/index.html"));
        assert_eq!(read("kpks.us/a-card").0.as_deref(), Some("/a-card"));
        fresh();
    }

    #[test]
    fn an_address_with_anything_but_plain_path_characters_is_refused() {
        for path in ["');alert(1);//", "</script><script>alert(1)</script>", "a b", "a\\b", "a\"b", "é"] {
            scanning();
            let (go, html) = read(&format!("kpks.us/{}", path));
            assert_eq!(go, None, "should refuse {:?}", path);
            assert!(html.contains("unrecognised address"), "{:?}: {}", path, html);
        }
        // Still percent-encoded: harmless characters only, and passed on as such.
        scanning();
        assert_eq!(read("kpks.us/x%27%29").0.as_deref(), Some("/x%27%29"));
        fresh();
    }

    #[test]
    fn a_code_that_is_not_a_kipukas_address_is_refused_and_the_scanner_goes_on_looking() {
        scanning();
        let (go, html) = read("https://evil.com/phish");
        assert_eq!(go, None);
        assert!(html.contains("not a Kipukas URL"));
        assert!(html.contains("text-kip-red"));
        assert!(html.contains("<video"), "still scanning");

        // The next code read replaces what was said.
        let (go, html) = read("kpks.us/brox_the_defiant");
        assert_eq!(go.as_deref(), Some("/brox_the_defiant/"));
        assert!(!html.contains("not a Kipukas URL"));

        scanning();
        let (go, html) = read("");
        assert_eq!(go, None);
        assert!(html.contains("not a Kipukas URL"));
        fresh();
    }

    #[test]
    fn what_came_of_a_code_is_drawn_without_a_tap() {
        scanning();
        tap("action=found&url=nonsense");
        assert_eq!(page::take_redraws(), ["/api/qr/status"]);
        fresh();
    }

    #[test]
    fn a_code_read_with_no_scanner_open_leads_nowhere() {
        fresh();
        let (go, html) = read("kpks.us/brox_the_defiant");
        assert_eq!(go, None);
        assert_eq!(html, "");

        // Nor behind the privacy notice.
        tap("action=toggle");
        assert_eq!(read("kpks.us/brox_the_defiant").0, None);
        fresh();
    }

    #[test]
    fn what_a_code_said_is_forgotten_when_the_scanner_closes() {
        scanning();
        read("https://evil.com/phish");
        tap("action=close");
        assert!(!tap("action=toggle").contains("not a Kipukas URL"));
        fresh();
    }
}
