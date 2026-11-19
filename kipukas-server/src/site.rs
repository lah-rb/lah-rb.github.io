//! The card pages of the built site, written by the engine at build time.
//!
//! Every card has a page of its own at a fixed address (`/timebattle/`), a
//! short address that redirects to it (`/yDLL`, the QR code printed on the
//! card), and an entry in the sitemap. All of these are plain files. The host
//! serves them without running anything, search engines and link previews
//! read them without running scripts, and an address with no file is a real
//! "not found".
//!
//! This module produces those files from the card catalog. It is the same
//! engine that runs in the browser, built natively and run after every Jekyll
//! build (`src/bin/card-pages.rs`, started by `_plugins/card_pages.rb`), so a
//! card's page holds exactly what the engine renders for that card:
//!
//! - The detail panel is [`card_panel::render_panel`] with no player state.
//!   In the browser the page mounts the panel as an engine-owned view, the
//!   engine renders it again with the player's state, and the difference
//!   (damage marks) is patched in.
//! - The head tags ([`render_head`]) are what search engines and link
//!   previews read: title, description, address, structured data, and the
//!   address of the card's preview picture (`preview.rs` draws it).
//!
//! ## The shell
//!
//! The rest of a card page (scripts, styles, toolbar, the frame around the
//! art) is the same for every card and is shared with the site's other pages,
//! which Jekyll builds. Jekyll renders it once, from `card-shell.html`: a
//! page with holes where a card goes. A hole is written `[[card.<name>]]`;
//! the names are in [`fill`]. [`render_page`] fills a copy of the shell for
//! one card. A hole this module does not know, or one the shell lacks, is an
//! error, so the shell and this module cannot drift apart unnoticed.
//!
//! ## Pure by design
//!
//! Everything here is text in, text out. [`build`] takes the shell and the
//! two files Jekyll wrote that cards are added to, and returns every file to
//! write, and which preview pictures the pages name. Reading and writing
//! files, and making the pictures, is left to the binary.
//!
//! ## Data files
//!
//! One small file tells the site's scripts about the cards: the list of art
//! the service worker fetches for offline play ([`offline_manifest`]). It is
//! written with the pages.
//!
//! Building twice gives the same result as building once: adding the cards
//! to a sitemap or an index of redirects that already has them changes
//! nothing. Jekyll's watch mode rebuilds the site over and over (and the
//! project's dev setup runs two watchers at once), so this matters.

use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

use crate::catalog::{Card, CARDS};
use crate::game::{card_panel, player_doc};
use crate::preview;
use crate::routes::util::escape_html;

/// The site's address and name. Also in `_config.yml` (`url`, `title`), which
/// Jekyll uses for the pages it builds.
pub const SITE_URL: &str = "https://www.kipukas.cards";
pub const SITE_NAME: &str = "Kipukas";

/// The two files Jekyll writes that cards are added to.
pub const SITEMAP_PATH: &str = "sitemap.xml";
pub const REDIRECTS_PATH: &str = "redirects.json";

/// The data file, relative to the site folder. The script that reads it
/// (`sw-src.js`) knows it by this address.
pub const OFFLINE_MANIFEST_PATH: &str = "offline-manifest.json";

/// What the service worker fetches for offline play besides the card art:
/// the QR reader, which is left out of the precache because only the
/// scanner needs it.
const OFFLINE_EXTRA: [&str; 1] = [crate::routes::qr::READER];

const HOLE_OPEN: &str = "[[card.";
const HOLE_CLOSE: &str = "]]";

/// Every hole a shell must have. See [`fill`] for what goes in each.
const HOLES: [&str; 5] = ["head", "slug", "image", "landscape", "panel"];

// ── Addresses ──────────────────────────────────────────────────────

/// A card's full address, e.g. `https://www.kipukas.cards/timebattle/`.
pub fn address(card: &Card) -> String {
    format!("{}{}", SITE_URL, card.url)
}

/// The file a card's page is written to, relative to the site folder.
///
/// `/timebattle/` is served from `timebattle/index.html`; an address without
/// a trailing slash (`/fourohfour`) from `fourohfour.html`; an address that
/// names a file (`/404.html`) from that file.
pub fn page_path(card: &Card) -> String {
    let path = card.url.trim_start_matches('/');
    if path.ends_with('/') {
        format!("{}index.html", path)
    } else if path.ends_with(".html") {
        path.to_string()
    } else {
        format!("{}.html", path)
    }
}

/// The file a card's short address is served from (`yDLL.html` for `/yDLL`),
/// or `None` if the card has no short code.
pub fn redirect_path(card: &Card) -> Option<String> {
    if card.code.is_empty() {
        None
    } else {
        Some(format!("{}.html", card.code))
    }
}

/// The file a card's preview picture is written to, relative to the site
/// folder. See `preview.rs` for what the picture is.
pub fn preview_path(card: &Card) -> String {
    format!("assets/previews/{}.jpg", card.slug)
}

/// The file a card's art is read from, relative to the site folder.
pub fn art_path(card: &Card) -> String {
    format!("assets/images/{}", card.img_name)
}

/// The full address of a card's preview picture.
fn preview_address(card: &Card) -> String {
    format!("{}/{}", SITE_URL, preview_path(card))
}

/// The site's not-found page is a card page too (the 404 Glitch). The host
/// serves it for any address that has no file.
pub fn is_not_found_page(card: &Card) -> bool {
    card.url == "/404.html"
}

// ── Head tags ──────────────────────────────────────────────────────

/// Card text as plain text: markup removed, whitespace collapsed.
fn plain_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            // A tag separates words ("one<br>two").
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if in_tag => {}
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What a search result or link preview says about a card: its History.
pub fn description(card: &Card) -> Option<String> {
    card.description.map(plain_text).filter(|text| !text.is_empty())
}

/// A card's structured data (schema.org, JSON-LD).
///
/// A card is described as a web page. It is not an article and carries no
/// date: when Jekyll built these pages it treated each card file as a blog
/// post, and said so here.
fn structured_data(card: &Card) -> Value {
    let mut data = Map::new();
    data.insert("@context".into(), json!("https://schema.org"));
    data.insert("@type".into(), json!("WebPage"));
    data.insert("headline".into(), json!(card.title));
    data.insert("url".into(), json!(address(card)));
    data.insert("image".into(), json!(preview_address(card)));
    if let Some(text) = description(card) {
        data.insert("description".into(), json!(text));
    }
    Value::Object(data)
}

/// The head tags of a card's page: what search engines and link previews
/// read. One tag per line.
pub fn render_head(card: &Card) -> String {
    let title = escape_html(card.title);
    let url = escape_html(&address(card));
    let mut tags: Vec<String> = Vec::new();

    tags.push(format!("<title>{} | {}</title>", title, SITE_NAME));
    tags.push(format!(r#"<meta property="og:title" content="{}" />"#, title));
    tags.push(r#"<meta property="og:locale" content="en_US" />"#.to_string());
    if let Some(text) = description(card) {
        let text = escape_html(&text);
        tags.push(format!(r#"<meta name="description" content="{}" />"#, text));
        tags.push(format!(r#"<meta property="og:description" content="{}" />"#, text));
    }
    tags.push(format!(r#"<link rel="canonical" href="{}" />"#, url));
    tags.push(format!(r#"<meta property="og:url" content="{}" />"#, url));
    tags.push(format!(r#"<meta property="og:site_name" content="{}" />"#, SITE_NAME));
    tags.push(r#"<meta property="og:type" content="website" />"#.to_string());

    // The picture a shared link shows. Its size is told so that an app can
    // lay the preview out before it has fetched the picture.
    let picture = escape_html(&preview_address(card));
    tags.push(format!(r#"<meta property="og:image" content="{}" />"#, picture));
    tags.push(r#"<meta property="og:image:type" content="image/jpeg" />"#.to_string());
    tags.push(format!(
        r#"<meta property="og:image:width" content="{}" />"#,
        preview::WIDTH
    ));
    tags.push(format!(
        r#"<meta property="og:image:height" content="{}" />"#,
        preview::HEIGHT
    ));
    if !card.img_alt.trim().is_empty() {
        tags.push(format!(
            r#"<meta property="og:image:alt" content="{}" />"#,
            escape_html(card.img_alt.trim())
        ));
    }
    tags.push(r#"<meta name="twitter:card" content="summary_large_image" />"#.to_string());
    tags.push(format!(r#"<meta property="twitter:image" content="{}" />"#, picture));
    tags.push(format!(r#"<meta property="twitter:title" content="{}" />"#, title));

    // Inside a script element only "</" could end it early; JSON allows the
    // slash to be written "\/".
    let data = structured_data(card).to_string().replace("</", "<\\/");
    tags.push(format!(r#"<script type="application/ld+json">{}</script>"#, data));

    tags.join("\n")
}

// ── The page ───────────────────────────────────────────────────────

/// What goes in a hole, or `None` if there is no hole of that name.
fn fill(hole: &str, card: &Card) -> Option<String> {
    Some(match hole {
        // Title, description, address and structured data.
        "head" => render_head(card),
        // Checked by `check_card` to be safe wherever the shell puts them:
        // in an address, an attribute or a quoted script string.
        "slug" => card.slug.to_string(),
        "image" => card.img_name.to_string(),
        // Whether the art is wider than tall, as a script value.
        "landscape" => card.landscape.to_string(),
        // The detail panel, as the engine renders it.
        "panel" => card_panel::render_panel(card),
        _ => return None,
    })
}

/// Check that a text is a card shell: it has every hole, and no other.
pub fn check_shell(shell: &str) -> Result<(), String> {
    let mut found: BTreeSet<&str> = BTreeSet::new();
    let mut rest = shell;
    while let Some(start) = rest.find(HOLE_OPEN) {
        let after = &rest[start + HOLE_OPEN.len()..];
        let end = after
            .find(HOLE_CLOSE)
            .ok_or("the card shell has a hole that is not closed with ]]")?;
        let name = &after[..end];
        if !HOLES.contains(&name) {
            return Err(format!(
                "the card shell has a hole this engine does not fill: [[card.{}]]",
                name
            ));
        }
        found.insert(name);
        rest = &after[end + HOLE_CLOSE.len()..];
    }
    for hole in HOLES {
        if !found.contains(hole) {
            return Err(format!("the card shell has no [[card.{}]] hole", hole));
        }
    }
    Ok(())
}

/// A card's page: the shell with its holes filled for this card.
///
/// What is put in a hole is not searched for holes again.
pub fn render_page(shell: &str, card: &Card) -> Result<String, String> {
    let mut page = String::with_capacity(shell.len() + 8192);
    let mut rest = shell;
    while let Some(start) = rest.find(HOLE_OPEN) {
        page.push_str(&rest[..start]);
        let after = &rest[start + HOLE_OPEN.len()..];
        let end = after
            .find(HOLE_CLOSE)
            .ok_or("the card shell has a hole that is not closed with ]]")?;
        let name = &after[..end];
        let content = fill(name, card).ok_or_else(|| {
            format!(
                "the card shell has a hole this engine does not fill: [[card.{}]]",
                name
            )
        })?;
        page.push_str(&content);
        rest = &after[end + HOLE_CLOSE.len()..];
    }
    page.push_str(rest);
    Ok(page)
}

// ── Short addresses ────────────────────────────────────────────────

/// The page at a card's short address: it sends the visitor on to the card.
///
/// It works without scripts (the refresh tag), tells search engines which
/// address is the real one (canonical) and keeps itself out of their results
/// (noindex).
pub fn render_redirect(card: &Card) -> String {
    let url = escape_html(&address(card));
    format!(
        concat!(
            "<!DOCTYPE html>\n",
            "<html lang=\"en-US\">\n",
            "  <meta charset=\"utf-8\">\n",
            "  <title>Redirecting&hellip;</title>\n",
            "  <link rel=\"canonical\" href=\"{url}\">\n",
            "  <script>location=\"{url}\"</script>\n",
            "  <meta http-equiv=\"refresh\" content=\"0; url={url}\">\n",
            "  <meta name=\"robots\" content=\"noindex\">\n",
            "  <h1>Redirecting&hellip;</h1>\n",
            "  <a href=\"{url}\">Click here if you are not redirected.</a>\n",
            "</html>\n",
        ),
        url = url
    )
}

/// Add the cards' short addresses to the site's index of redirects
/// (`{"/yDLL": "https://…/timebattle/", …}`). A short address that is
/// already there and leads to the same card is left alone; one that leads
/// somewhere else is an error.
pub fn merge_redirects(existing: &str, cards: &[Card]) -> Result<String, String> {
    let mut index: Map<String, Value> = if existing.trim().is_empty() {
        Map::new()
    } else {
        serde_json::from_str(existing)
            .map_err(|e| format!("{} is not a JSON object: {}", REDIRECTS_PATH, e))?
    };
    for card in cards.iter().filter(|c| !c.code.is_empty()) {
        let from = format!("/{}", card.code);
        let to = json!(address(card));
        match index.get(&from) {
            Some(already) if *already == to => {}
            Some(other) => {
                return Err(format!(
                    "{} already redirects {} to {} (wanted for {})",
                    REDIRECTS_PATH, from, other, card.slug
                ));
            }
            None => {
                index.insert(from, to);
            }
        }
    }
    Ok(Value::Object(index).to_string())
}

// ── Sitemap ────────────────────────────────────────────────────────

/// A card's sitemap entry, or `None` if it has none (the not-found page).
pub fn sitemap_entry(card: &Card) -> Option<String> {
    if is_not_found_page(card) {
        return None;
    }
    // No date: nothing records when a card's page last changed.
    Some(format!("<url>\n<loc>{}</loc>\n</url>\n", escape_html(&address(card))))
}

/// Add the cards to the sitemap Jekyll wrote for the rest of the site.
/// Cards go first, in the order of their slugs. A card that is already listed
/// is left as it is.
pub fn merge_sitemap(existing: &str, cards: &[Card]) -> Result<String, String> {
    let open = existing
        .find("<urlset")
        .ok_or_else(|| format!("{} has no <urlset>", SITEMAP_PATH))?;
    let open_end = open
        + existing[open..]
            .find('>')
            .ok_or_else(|| format!("{} has an unfinished <urlset>", SITEMAP_PATH))?
        + 1;

    let mut listed: Vec<&Card> = cards.iter().collect();
    listed.sort_by_key(|c| c.slug);

    let mut entries = String::new();
    for card in listed {
        let Some(entry) = sitemap_entry(card) else {
            continue;
        };
        let loc = format!("<loc>{}</loc>", escape_html(&address(card)));
        if !existing.contains(&loc) {
            entries.push_str(&entry);
        }
    }
    if entries.is_empty() {
        return Ok(existing.to_string());
    }

    let (head, tail) = existing.split_at(open_end);
    Ok(format!("{}\n{}{}", head, entries, tail.trim_start_matches('\n')))
}

// ── Data files ─────────────────────────────────────────────────────

/// A string as JSON.
fn json_text(text: &str) -> String {
    Value::String(text.to_string()).to_string()
}

/// What the service worker fetches once the site is installed, so that it
/// works offline: every shown card's art, and [`OFFLINE_EXTRA`].
pub fn offline_manifest(cards: &[Card]) -> String {
    let images: Vec<String> = cards
        .iter()
        .filter(|card| !card.hidden)
        .map(|card| json_text(&format!("/{}", art_path(card))))
        .collect();
    let extra: Vec<String> = OFFLINE_EXTRA.iter().map(|path| json_text(path)).collect();
    format!(r#"{{"images":[{}],"extra":[{}]}}"#, images.join(","), extra.join(","))
}

// ── The whole build ────────────────────────────────────────────────

/// Everything the build writes for the cards.
pub struct Output {
    /// The cards' files: path relative to the site folder, and contents.
    pub files: Vec<(String, String)>,
    /// The data files: path relative to the site folder, and contents.
    pub data: Vec<(String, String)>,
    /// The sitemap, with the cards added.
    pub sitemap: String,
    /// The index of redirects, with the cards' short addresses added.
    pub redirects: String,
    /// The preview pictures the pages name, for the binary to make.
    pub previews: Vec<Preview>,
}

/// A preview picture a card's page names: where it goes and what it is made
/// from, both relative to the site folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub path: String,
    pub art: String,
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Check that a card's slug, short code, art file and address can be put
/// where the shell and the file system put them, as they are.
fn check_card(card: &Card) -> Result<(), String> {
    let bad = |what: &str, value: &str| {
        Err(format!(
            "card {:?}: its {} {:?} has characters that are not allowed there",
            card.slug, what, value
        ))
    };
    if card.slug.is_empty() || !card.slug.chars().all(is_name_char) {
        return bad("slug", card.slug);
    }
    if !card.code.chars().all(is_name_char) {
        return bad("short code", card.code);
    }
    if card.img_name.is_empty()
        || card.img_name.starts_with('.')
        || !card.img_name.chars().all(|c| is_name_char(c) || c == '.')
    {
        return bad("art file name", card.img_name);
    }
    let path = card.url.strip_prefix('/').unwrap_or("");
    if path.is_empty()
        || path.starts_with('.')
        || path.contains("..")
        || path.contains("//")
        || !path.chars().all(|c| is_name_char(c) || c == '.' || c == '/')
    {
        return bad("address", card.url);
    }
    Ok(())
}

/// Produce every card's page and short address, and add the cards to the
/// sitemap and the index of redirects.
///
/// `shell` is the card shell as Jekyll rendered it; `sitemap` and
/// `redirects` are the files Jekyll wrote for the rest of the site
/// (`redirects` may be empty if it wrote none).
pub fn build(cards: &[Card], shell: &str, sitemap: &str, redirects: &str) -> Result<Output, String> {
    check_shell(shell)?;

    // Pages are rendered with no player state: nobody's damage belongs in a
    // file served to everyone.
    player_doc::init_player_doc();

    let mut files: Vec<(String, String)> = Vec::new();
    let mut previews: Vec<Preview> = Vec::new();
    let mut paths: BTreeSet<String> = BTreeSet::new();
    let mut claim = |path: String, card: &Card| -> Result<String, String> {
        if !paths.insert(path.to_lowercase()) {
            // Compared without case: some file systems (and this project's
            // build machine) cannot hold two names that differ only by case.
            return Err(format!(
                "card {:?} would be written to {}, which another card already uses",
                card.slug, path
            ));
        }
        Ok(path)
    };

    for card in cards {
        check_card(card)?;
        let path = claim(page_path(card), card)?;
        files.push((path, render_page(shell, card)?));
        if let Some(path) = redirect_path(card) {
            let path = claim(path, card)?;
            files.push((path, render_redirect(card)));
        }
        previews.push(Preview {
            path: claim(preview_path(card), card)?,
            art: art_path(card),
        });
    }

    let data = vec![(
        claim_data(&mut paths, OFFLINE_MANIFEST_PATH)?,
        offline_manifest(cards),
    )];

    Ok(Output {
        files,
        data,
        previews,
        sitemap: merge_sitemap(sitemap, cards)?,
        redirects: merge_redirects(redirects, cards)?,
    })
}

/// Take a data file's path, unless a card's file is already written there.
fn claim_data(paths: &mut BTreeSet<String>, path: &str) -> Result<String, String> {
    if !paths.insert(path.to_lowercase()) {
        return Err(format!("a card would be written to {}, which is a data file", path));
    }
    Ok(path.to_string())
}

/// [`build`] for the cards compiled into this engine.
pub fn build_catalog(shell: &str, sitemap: &str, redirects: &str) -> Result<Output, String> {
    build(&CARDS, shell, sitemap, redirects)
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::damage;

    /// A small shell with every hole, shaped like the real one.
    const SHELL: &str = concat!(
        "<html><head>\n[[card.head]]\n<meta charset=\"utf-8\"></head>",
        "<body><div x-data=\"{ landscapeArt: [[card.landscape]] }\">",
        "<div data-engine-view=\"/api/room/fists?card=[[card.slug]]\"></div>",
        "<img src=\"/assets/images/[[card.image]]?d=512\">",
        "<img src=\"/assets/images/[[card.image]]\">",
        "<div data-engine-view=\"/api/card/panel?card=[[card.slug]]\">[[card.panel]]</div>",
        "</div></body></html>",
    );

    const SITEMAP: &str = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
        "<url>\n<loc>https://www.kipukas.cards/hand/</loc>\n</url>\n",
        "</urlset>\n",
    );

    fn card(slug: &str) -> &'static Card {
        CARDS
            .iter()
            .find(|c| c.slug == slug)
            .unwrap_or_else(|| panic!("{} is in the catalog", slug))
    }

    fn built() -> Output {
        build_catalog(SHELL, SITEMAP, "").expect("the catalog builds")
    }

    fn file<'a>(output: &'a Output, path: &str) -> &'a str {
        output
            .files
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, html)| html.as_str())
            .unwrap_or_else(|| panic!("{} was written", path))
    }

    // ── Addresses ──────────────────────────────────────────────────

    #[test]
    fn a_page_is_written_where_the_host_serves_its_address_from() {
        assert_eq!(page_path(card("timebattle")), "timebattle/index.html");
        assert_eq!(page_path(card("fourohfour")), "fourohfour.html");
        assert_eq!(page_path(card("404")), "404.html");
        assert_eq!(address(card("timebattle")), "https://www.kipukas.cards/timebattle/");
    }

    #[test]
    fn a_short_code_is_served_from_a_file_of_its_name() {
        assert_eq!(redirect_path(card("timebattle")), Some("yDLL.html".to_string()));
        // The not-found page has no short code.
        assert_eq!(redirect_path(card("404")), None);
    }

    // ── Head tags ──────────────────────────────────────────────────

    #[test]
    fn the_head_names_the_card_and_its_address() {
        let head = render_head(card("timebattle"));
        assert!(head.starts_with("<title>Timebattle | Kipukas</title>\n"));
        assert!(head.contains(r#"<meta property="og:title" content="Timebattle" />"#));
        assert!(head.contains(
            r#"<link rel="canonical" href="https://www.kipukas.cards/timebattle/" />"#
        ));
        assert!(head.contains(
            r#"<meta property="og:url" content="https://www.kipukas.cards/timebattle/" />"#
        ));
        assert!(head.contains(r#"<meta property="og:site_name" content="Kipukas" />"#));
        assert_eq!(head.matches("<title>").count(), 1);
    }

    #[test]
    fn the_description_is_the_cards_history_as_plain_text() {
        let c = card("timebattle");
        let text = description(c).unwrap();
        assert!(text.starts_with("The days grew long and the king looked longingly"));
        assert!(!text.contains('\n'));
        assert!(!text.contains("  "));
        let head = render_head(c);
        assert!(head.contains(&format!(
            r#"<meta name="description" content="{}" />"#,
            escape_html(&text)
        )));
        assert!(head.contains(&format!(
            r#"<meta property="og:description" content="{}" />"#,
            escape_html(&text)
        )));
    }

    #[test]
    fn markup_in_card_text_is_left_out_of_plain_text() {
        assert_eq!(plain_text("one<br>two  <b>three</b>\n four"), "one two three four");
        assert_eq!(plain_text("  spaced   out  "), "spaced out");
        assert_eq!(plain_text("a > b"), "a > b");
    }

    #[test]
    fn no_description_in_a_head_can_end_its_attribute() {
        for c in CARDS.iter() {
            let head = render_head(c);
            for line in head.lines().filter(|l| l.starts_with("<meta") || l.starts_with("<link")) {
                // Each tag is one line with an even number of quotes.
                assert_eq!(line.matches('"').count() % 2, 0, "{}: {}", c.slug, line);
                assert!(line.ends_with(" />"), "{}: {}", c.slug, line);
            }
        }
    }

    #[test]
    fn a_card_is_a_page_not_a_dated_article() {
        for c in CARDS.iter() {
            let head = render_head(c);
            assert!(
                head.contains(r#"<meta property="og:type" content="website" />"#),
                "{}",
                c.slug
            );
            assert!(!head.contains("article"), "{}", c.slug);
            assert!(!head.contains("published_time"), "{}", c.slug);
        }
    }

    #[test]
    fn the_head_names_the_cards_preview_picture_and_its_size() {
        let head = render_head(card("timebattle"));
        let picture = "https://www.kipukas.cards/assets/previews/timebattle.jpg";
        for tag in [
            format!(r#"<meta property="og:image" content="{}" />"#, picture),
            r#"<meta property="og:image:type" content="image/jpeg" />"#.to_string(),
            r#"<meta property="og:image:width" content="1200" />"#.to_string(),
            r#"<meta property="og:image:height" content="630" />"#.to_string(),
            concat!(
                r#"<meta property="og:image:alt" content=""#,
                r#"Horse, dragon, horse, horse, and imp. The battle wages... horse." />"#
            )
            .to_string(),
            r#"<meta name="twitter:card" content="summary_large_image" />"#.to_string(),
            format!(r#"<meta property="twitter:image" content="{}" />"#, picture),
        ] {
            assert!(head.contains(&tag), "missing {}", tag);
        }
        assert_eq!(structured(&head)["image"], picture);
    }

    #[test]
    fn art_with_no_description_gets_no_empty_alt_text() {
        let silent = Card { img_alt: "  ", ..card("cloth").clone() };
        let head = render_head(&silent);
        assert!(!head.contains("og:image:alt"));
        assert!(head.contains(r#"<meta property="og:image" content="#));
    }

    /// The structured data of a rendered head, parsed back.
    fn structured(head: &str) -> Value {
        let open = r#"<script type="application/ld+json">"#;
        let start = head.find(open).expect("structured data present") + open.len();
        let end = start + head[start..].find("</script>").expect("script closed");
        serde_json::from_str(&head[start..end]).expect("structured data is valid JSON")
    }

    #[test]
    fn structured_data_is_valid_and_describes_the_card() {
        for c in CARDS.iter() {
            let data = structured(&render_head(c));
            assert_eq!(data["@context"], "https://schema.org", "{}", c.slug);
            assert_eq!(data["headline"], c.title, "{}", c.slug);
            assert_eq!(data["url"], address(c), "{}", c.slug);
            assert_eq!(data["description"], description(c).unwrap(), "{}", c.slug);
            assert_eq!(data["@type"], "WebPage", "{}", c.slug);
            assert!(data.get("datePublished").is_none(), "{}", c.slug);
            assert!(data.get("dateModified").is_none(), "{}", c.slug);
        }
    }

    #[test]
    fn card_text_cannot_end_the_structured_data_script() {
        let hostile = Card {
            description: Some("before </script><script>alert(1)</script> after"),
            title: "A </script> title",
            ..card("cloth").clone()
        };
        let head = render_head(&hostile);
        // One script element, closed once, and still valid JSON inside.
        assert_eq!(head.matches("</script>").count(), 1);
        assert_eq!(structured(&head)["headline"], "A </script> title");
        assert!(head.contains("<title>A &lt;/script&gt; title | Kipukas</title>"));
    }

    // ── The page ───────────────────────────────────────────────────

    #[test]
    fn a_page_is_the_shell_with_every_hole_filled() {
        let c = card("brox_the_defiant");
        let page = render_page(SHELL, c).unwrap();
        assert!(!page.contains(HOLE_OPEN));
        assert!(page.contains("<title>Brox The Defiant | Kipukas</title>"));
        assert!(page.contains(r#"data-engine-view="/api/room/fists?card=brox_the_defiant""#));
        assert!(page.contains(&format!(r#"src="/assets/images/{}?d=512""#, c.img_name)));
        assert!(page.contains(&format!(r#"src="/assets/images/{}">"#, c.img_name)));
        assert!(page.contains("landscapeArt: false"));
        assert!(page.contains(r#"data-engine-view="/api/card/panel?card=brox_the_defiant""#));
        // What the shell says outside its holes is kept as it is.
        assert!(page.starts_with("<html><head>\n<title>"));
        assert!(page.ends_with("</div></div></body></html>"));
    }

    #[test]
    fn the_panel_in_a_page_is_what_the_engine_renders_with_no_player_state() {
        player_doc::init_player_doc();
        for c in CARDS.iter() {
            let page = render_page(SHELL, c).unwrap();
            let view = format!(
                r#"<div data-engine-view="/api/card/panel?card={}">{}</div>"#,
                c.slug,
                card_panel::render_card_panel(c.slug)
            );
            assert!(page.contains(&view), "{}", c.slug);
        }
        player_doc::init_player_doc();
    }

    #[test]
    fn a_fighting_cards_page_has_its_keal_means_in_the_markup() {
        // Readable without running anything: by a crawler, or before the
        // engine has started.
        let output = built();
        let page = file(&output, "brox_the_defiant/index.html");
        assert!(page.contains("Crushing Hope"));
        assert!(page.contains(r#"data-slot="1" aria-pressed="false""#));
    }

    #[test]
    fn landscape_art_is_told_to_the_page() {
        let wide = CARDS.iter().find(|c| c.landscape).expect("a landscape card");
        assert!(render_page(SHELL, wide).unwrap().contains("landscapeArt: true"));
        let tall = CARDS.iter().find(|c| !c.landscape).expect("a portrait card");
        assert!(render_page(SHELL, tall).unwrap().contains("landscapeArt: false"));
    }

    #[test]
    fn what_fills_a_hole_is_not_searched_for_holes() {
        let odd = Card {
            title: "About [[card.slug]] and [[card.nonsense]]",
            ..card("cloth").clone()
        };
        let page = render_page(SHELL, &odd).unwrap();
        assert!(page.contains("About [[card.slug]] and [[card.nonsense]]"));
    }

    #[test]
    fn a_shell_with_an_unknown_hole_is_refused() {
        let shell = SHELL.replace("[[card.image]]", "[[card.picture]]");
        let error = check_shell(&shell).unwrap_err();
        assert!(error.contains("[[card.picture]]"), "{}", error);
        assert!(render_page(&shell, card("cloth")).is_err());
        assert!(build_catalog(&shell, SITEMAP, "").is_err());
    }

    #[test]
    fn a_shell_missing_a_hole_is_refused() {
        for hole in HOLES {
            let shell = SHELL.replace(&format!("[[card.{}]]", hole), "");
            let error = check_shell(&shell).unwrap_err();
            assert!(error.contains(&format!("no [[card.{}]] hole", hole)), "{}", error);
        }
        assert!(check_shell("<html></html>").is_err());
        assert!(check_shell(SHELL).is_ok());
    }

    #[test]
    fn a_hole_that_is_not_closed_is_refused() {
        let shell = format!("{}[[card.slug", SHELL);
        assert!(check_shell(&shell).is_err());
        assert!(render_page(&shell, card("cloth")).is_err());
    }

    // ── Short addresses ────────────────────────────────────────────

    #[test]
    fn a_short_address_sends_the_visitor_to_the_card() {
        let html = render_redirect(card("timebattle"));
        let url = "https://www.kipukas.cards/timebattle/";
        assert!(html.contains(&format!(r#"<link rel="canonical" href="{}">"#, url)));
        assert!(html.contains(&format!(r#"<script>location="{}"</script>"#, url)));
        assert!(html.contains(&format!(
            r#"<meta http-equiv="refresh" content="0; url={}">"#,
            url
        )));
        assert!(html.contains(r#"<meta name="robots" content="noindex">"#));
        assert!(html.contains(&format!(r#"<a href="{}">"#, url)));
    }

    #[test]
    fn every_short_code_is_in_the_index_of_redirects() {
        let output = built();
        let index: Map<String, Value> = serde_json::from_str(&output.redirects).unwrap();
        let coded = CARDS.iter().filter(|c| !c.code.is_empty()).count();
        assert_eq!(index.len(), coded);
        assert_eq!(index["/yDLL"], "https://www.kipukas.cards/timebattle/");
        assert_eq!(index["/eUN0"], "https://www.kipukas.cards/fourohfour");
    }

    #[test]
    fn redirects_already_in_the_index_are_kept_and_never_replaced() {
        let merged = merge_redirects(r#"{"/old":"https://example.org/"}"#, &CARDS).unwrap();
        let index: Map<String, Value> = serde_json::from_str(&merged).unwrap();
        assert_eq!(index["/old"], "https://example.org/");
        assert_eq!(index["/yDLL"], "https://www.kipukas.cards/timebattle/");

        let clash = merge_redirects(r#"{"/yDLL":"https://example.org/"}"#, &CARDS);
        assert!(clash.unwrap_err().contains("/yDLL"));
        assert!(merge_redirects("not json", &CARDS).is_err());
    }

    #[test]
    fn adding_the_cards_twice_is_the_same_as_adding_them_once() {
        let once = built();
        let twice = build_catalog(SHELL, &once.sitemap, &once.redirects).expect("builds again");
        assert_eq!(twice.sitemap, once.sitemap);
        assert_eq!(twice.redirects, once.redirects);
        assert_eq!(twice.files, once.files);
        assert_eq!(twice.previews, once.previews);
        assert_eq!(twice.data, once.data);
    }

    // ── Sitemap ────────────────────────────────────────────────────

    #[test]
    fn every_card_but_the_not_found_page_is_in_the_sitemap() {
        let output = built();
        for c in CARDS.iter() {
            let loc = format!("<loc>{}</loc>", address(c));
            assert_eq!(
                output.sitemap.matches(&loc).count(),
                if is_not_found_page(c) { 0 } else { 1 },
                "{}",
                c.slug
            );
        }
        // What Jekyll listed is still there, after the cards, and the file is
        // still one well-formed list.
        assert!(output.sitemap.contains("<loc>https://www.kipukas.cards/hand/</loc>"));
        assert!(output.sitemap.starts_with("<?xml"));
        assert!(output.sitemap.trim_end().ends_with("</urlset>"));
        assert_eq!(
            output.sitemap.matches("<url>").count(),
            output.sitemap.matches("</url>").count()
        );
        // Every card but the not-found page, and the one entry Jekyll wrote.
        let cards_listed = CARDS.iter().filter(|c| !is_not_found_page(c)).count();
        assert_eq!(output.sitemap.matches("<url>").count(), cards_listed + 1);
    }

    #[test]
    fn a_sitemap_entry_is_the_cards_address_and_nothing_else() {
        assert_eq!(
            sitemap_entry(card("timebattle")).unwrap(),
            "<url>\n<loc>https://www.kipukas.cards/timebattle/</loc>\n</url>\n"
        );
        assert_eq!(
            sitemap_entry(card("fourohfour")).unwrap(),
            "<url>\n<loc>https://www.kipukas.cards/fourohfour</loc>\n</url>\n"
        );
        assert_eq!(sitemap_entry(card("404")), None);
    }

    #[test]
    fn a_card_the_sitemap_already_lists_is_not_listed_twice() {
        let listed = SITEMAP.replace("/hand/", "/timebattle/");
        let merged = merge_sitemap(&listed, &CARDS).unwrap();
        assert_eq!(
            merged.matches("<loc>https://www.kipukas.cards/timebattle/</loc>").count(),
            1
        );
        assert!(merged.contains("<loc>https://www.kipukas.cards/cloth/</loc>"));
    }

    #[test]
    fn a_file_that_is_not_a_sitemap_is_refused() {
        assert!(merge_sitemap("<html></html>", &CARDS).is_err());
        assert!(merge_sitemap("<urlset", &CARDS).is_err());
    }

    // ── Data files ─────────────────────────────────────────────────

    #[test]
    fn the_offline_list_has_every_shown_cards_art_and_the_qr_decoder() {
        let manifest: Value = serde_json::from_str(&offline_manifest(&CARDS)).unwrap();
        let images = manifest["images"].as_array().unwrap();
        assert_eq!(images.len(), CARDS.iter().filter(|c| !c.hidden).count());
        assert!(images.contains(&json!("/assets/images/timebattle.jxl")));
        assert!(!images.contains(&json!("/assets/images/404.jxl")));
        assert_eq!(manifest["extra"], json!(["/assets/js-wasm/zxing-3.2.0-rc1.wasm"]));
        assert!(offline_manifest(&CARDS).starts_with(r#"{"images":["#));
    }

    #[test]
    fn the_data_file_is_written_with_the_pages() {
        let output = built();
        let paths: Vec<&str> = output.data.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(paths, ["offline-manifest.json"]);
        assert_eq!(output.data[0].1, offline_manifest(&CARDS));
    }

    // ── The whole build ────────────────────────────────────────────

    #[test]
    fn every_card_gets_a_page_and_every_code_a_redirect() {
        let output = built();
        let coded = CARDS.iter().filter(|c| !c.code.is_empty()).count();
        assert_eq!(output.files.len(), CARDS.len() + coded);
        for c in CARDS.iter() {
            let page = file(&output, &page_path(c));
            assert!(page.contains(&format!("<title>{} | Kipukas</title>", escape_html(c.title))));
            assert!(!page.contains(HOLE_OPEN), "{}", c.slug);
            if let Some(path) = redirect_path(c) {
                assert!(file(&output, &path).contains(&address(c)));
            }
        }
    }

    #[test]
    fn every_card_has_a_preview_made_from_its_art_and_named_by_its_page() {
        let output = built();
        assert_eq!(output.previews.len(), CARDS.len());
        for c in CARDS.iter() {
            let preview = output
                .previews
                .iter()
                .find(|p| p.path == preview_path(c))
                .unwrap_or_else(|| panic!("{} has a preview", c.slug));
            assert_eq!(preview.path, format!("assets/previews/{}.jpg", c.slug));
            assert_eq!(preview.art, format!("assets/images/{}", c.img_name));
            let named = format!(r#"content="{}/{}""#, SITE_URL, preview.path);
            assert!(file(&output, &page_path(c)).contains(&named), "{}", c.slug);
        }
    }

    #[test]
    fn no_two_files_share_a_path() {
        let output = built();
        let mut paths: Vec<String> = output.files.iter().map(|(p, _)| p.to_lowercase()).collect();
        paths.sort();
        let before = paths.len();
        paths.dedup();
        assert_eq!(paths.len(), before);
    }

    #[test]
    fn two_cards_at_one_address_are_refused() {
        let twin = Card {
            slug: "cloth_again",
            code: "",
            ..card("cloth").clone()
        };
        let cards = [card("cloth").clone(), twin];
        let error = build(&cards, SHELL, SITEMAP, "").err().unwrap();
        assert!(error.contains("cloth_again"), "{}", error);
    }

    #[test]
    fn a_short_code_that_differs_from_another_only_by_case_is_refused() {
        let a = Card { code: "abCD", ..card("cloth").clone() };
        let b = Card { code: "ABcd", ..card("timebattle").clone() };
        assert!(build(&[a, b], SHELL, SITEMAP, "").is_err());
    }

    #[test]
    fn a_card_whose_names_do_not_fit_is_refused() {
        let cloth = card("cloth");
        let cases = [
            Card { slug: "bad slug", ..cloth.clone() },
            Card { slug: "x'); alert(1); ('", ..cloth.clone() },
            Card { code: "a/b", ..cloth.clone() },
            Card { img_name: "../secret.jxl", ..cloth.clone() },
            Card { img_name: "a b.jxl", ..cloth.clone() },
            Card { url: "/../outside/", ..cloth.clone() },
            Card { url: "no-leading-slash/", ..cloth.clone() },
            Card { url: "/", ..cloth.clone() },
        ];
        for bad in cases {
            assert!(
                build(std::slice::from_ref(&bad), SHELL, SITEMAP, "").is_err(),
                "{:?} {:?} {:?} {:?}",
                bad.slug,
                bad.code,
                bad.img_name,
                bad.url
            );
        }
        assert!(build(std::slice::from_ref(cloth), SHELL, SITEMAP, "").is_ok());
    }

    #[test]
    fn nobodys_damage_is_written_into_a_page() {
        player_doc::init_player_doc();
        damage::toggle_slot("brox_the_defiant", 1);
        let output = built();
        let page = file(&output, "brox_the_defiant/index.html");
        assert!(!page.contains(r#"aria-pressed="true""#));
        player_doc::init_player_doc();
    }

    #[test]
    fn the_same_inputs_build_the_same_site() {
        let a = built();
        let b = built();
        assert_eq!(a.files, b.files);
        assert_eq!(a.sitemap, b.sitemap);
        assert_eq!(a.redirects, b.redirects);
    }
}
