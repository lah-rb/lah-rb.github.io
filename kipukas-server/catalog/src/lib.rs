//! Reads the card files and writes the engine's card catalog.
//!
//! Every card is one file in `cards/`, named after the card's address, holding
//! YAML front matter and nothing else. The engine's build script (`build.rs`)
//! calls this crate each time the engine is compiled: [`read_cards`] reads the
//! files and [`emit`] writes them out as Rust, which the engine includes
//! (`src/catalog.rs`). So a compiled engine, in the browser or at build time,
//! always holds the card files as they were when it was compiled, and there is
//! no generated source to keep in step by hand.
//!
//! This crate runs on the machine that builds the engine. Neither it nor its
//! YAML parser is part of the engine.
//!
//! ## The field decides the type
//!
//! YAML guesses a value's type from how it looks: `404` is a number, `no` may
//! be a boolean, `1e3` is a thousand. Here the field decides instead. A text
//! field takes its value as it is written, whatever it looks like; a number
//! field must hold a whole number; a yes-or-no field must hold `true` or
//! `false`. Anything else is an error, not a silent zero.
//!
//! ## The house style for long text
//!
//! Long text in a card file is written in quotes over several lines, with the
//! later lines starting at the left edge:
//!
//! ```yaml
//! description: "The days grew long and the king looked longingly
//! out into a city, once grand, now in dismay."
//! ```
//!
//! Jekyll's YAML parser accepted that, and so did the one this crate replaced,
//! but YAML proper wants the later lines indented, and the parser used here
//! holds to it. Nearly every card is written this way, so the reader accepts
//! it: [`indent_continued_values`] indents those lines before parsing. What
//! the value says is the same either way.
//!
//! ## A card file that is wrong stops the build
//!
//! [`read_cards`] returns every problem it finds, each naming its file (and
//! line, where there is one), and the build script fails with them. A card is
//! never left out quietly.

use std::fs;
use std::path::Path;

use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::{Marker, TScalarStyle};

/// Where on a card a kind of damage is taken, and how much of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KealMeans {
    pub name: String,
    pub genetics: Vec<String>,
    pub count: u8,
}

/// A card as its file describes it. The fields are those of the engine's
/// `Card` (`src/catalog.rs`), in the same order; what each one is for is
/// written there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    /// The card's file, e.g. `timebattle.html`.
    pub file: String,
    pub slug: String,
    pub title: String,
    pub layout: String,
    pub img_name: String,
    pub img_alt: String,
    pub thumbnail: String,
    pub tags: String,
    pub genetic_disposition: Option<String>,
    pub motivation: Option<String>,
    pub habitat: Option<String>,
    pub url: String,
    pub code: String,
    pub variation: Option<String>,
    pub movement_text: Option<String>,
    pub play_style: Option<String>,
    pub atk_die: Option<String>,
    pub def_die: Option<String>,
    pub criteria: Option<String>,
    pub success: Option<String>,
    pub failure: Option<String>,
    pub activation: Option<String>,
    pub detection: Option<String>,
    pub scarcity: Option<String>,
    pub description: Option<String>,
    pub promotion: Option<String>,
    pub promotion_conditions: Option<String>,
    pub injury_tolerance: u8,
    pub keal_means: Vec<KealMeans>,
    pub movement: u8,
    pub die: String,
    pub brawl_sequence: String,
    pub tamability: Option<u32>,
    pub loyalty_days: Option<u32>,
    pub hidden: bool,
    pub landscape: bool,
}

/// Where a card's grid picture is cut from when it does not fit.
const THUMBNAILS: [&str; 3] = ["top", "center", "bottom"];

/// The length of a short code. The hand view reads several from one address
/// by cutting it into pieces of this length.
const CODE_LENGTH: usize = 4;

// ── YAML, as written ───────────────────────────────────────────────

/// A YAML value with nothing guessed about it: a scalar keeps its text and
/// whether it was written bare (`plain`) or in quotes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Scalar { text: String, plain: bool, line: usize },
    List(Vec<Node>),
    Map(Vec<(String, Node)>),
}

impl Node {
    /// A value left empty (`habitat:`) or written as null.
    fn is_absent(&self) -> bool {
        matches!(
            self,
            Node::Scalar { text, plain: true, .. }
                if matches!(text.as_str(), "" | "~" | "null" | "Null" | "NULL")
        )
    }
}

/// Builds a [`Node`] from the parser's events.
#[derive(Default)]
struct Tree {
    /// The lists and maps being read, innermost last. A map holds its keys
    /// and values alternately until it ends.
    open: Vec<(bool, Vec<Node>)>,
    root: Option<Node>,
    problem: Option<String>,
}

impl Tree {
    fn add(&mut self, node: Node) {
        match self.open.last_mut() {
            Some((_, items)) => items.push(node),
            None => self.root = Some(node),
        }
    }
}

impl MarkedEventReceiver for Tree {
    fn on_event(&mut self, event: Event, mark: Marker) {
        match event {
            Event::Scalar(text, style, _, _) => self.add(Node::Scalar {
                text,
                plain: style == TScalarStyle::Plain,
                line: mark.line(),
            }),
            Event::SequenceStart(..) => self.open.push((false, Vec::new())),
            Event::MappingStart(..) => self.open.push((true, Vec::new())),
            Event::SequenceEnd | Event::MappingEnd => {
                let Some((is_map, items)) = self.open.pop() else {
                    return;
                };
                if !is_map {
                    self.add(Node::List(items));
                    return;
                }
                let mut pairs = Vec::with_capacity(items.len() / 2);
                let mut items = items.into_iter();
                while let (Some(key), Some(value)) = (items.next(), items.next()) {
                    match key {
                        Node::Scalar { text, .. } => pairs.push((text, value)),
                        _ => self.problem = Some("a key that is not plain text".to_string()),
                    }
                }
                self.add(Node::Map(pairs));
            }
            Event::Alias(_) => {
                self.problem = Some(format!("line {}: an alias (*name), which is not supported", mark.line()));
            }
            _ => {}
        }
    }
}

/// The front matter of a card file: what is between its first line, `---`,
/// and the next line that is `---`.
fn front_matter(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---")?;
    let first_line_end = rest.find('\n')?;
    if !rest[..first_line_end].trim().is_empty() {
        return None;
    }
    let body = &rest[first_line_end + 1..];
    let mut at = 0;
    for line in body.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some(&body[..at]);
        }
        at += line.len();
    }
    None
}

/// If a line starts a top-level field whose value opens with a quote
/// (`description: "The days…`), the quote and where the value's text starts.
fn quoted_value_start(line: &str) -> Option<(char, usize)> {
    let key_end = line.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))?;
    if key_end == 0 || !line[key_end..].starts_with(':') {
        return None;
    }
    let after_colon = &line[key_end + 1..];
    let value = after_colon.trim_start_matches([' ', '\t']);
    if value.len() == after_colon.len() {
        return None; // no space after the colon: not a field
    }
    let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    Some((quote, line.len() - value.len() + 1))
}

/// Whether a quoted value's closing quote is in `text`, which starts inside
/// the value. In double quotes a backslash protects the next character; in
/// single quotes two quotes together stand for one.
fn closes_quote(text: &str, quote: char) -> bool {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quote == '"' && c == '\\' {
            chars.next();
        } else if c == quote {
            if quote == '\'' && chars.peek() == Some(&'\'') {
                chars.next();
            } else {
                return true;
            }
        }
    }
    false
}

/// Accept the house style for long text (see the top of this file): where a
/// top-level field's quoted value runs over several lines, indent its later
/// lines, which is how YAML wants them written. Nothing else is touched, and
/// no line is added or removed, so line numbers still point into the file.
fn indent_continued_values(yaml: &str) -> String {
    let mut out = String::with_capacity(yaml.len() + 512);
    // The quote of a value that has not closed yet.
    let mut open: Option<char> = None;
    for line in yaml.split_inclusive('\n') {
        let (quote, text_start) = match open {
            Some(quote) => {
                if !line.trim().is_empty() {
                    out.push_str("  ");
                }
                (quote, 0)
            }
            None => match quoted_value_start(line) {
                Some(start) => start,
                None => {
                    out.push_str(line);
                    continue;
                }
            },
        };
        out.push_str(line);
        open = if closes_quote(&line[text_start..], quote) { None } else { Some(quote) };
    }
    out
}

/// A card file's fields, in the order written.
fn fields(text: &str) -> Result<Vec<(String, Node)>, String> {
    let yaml = front_matter(text)
        .ok_or("it does not start with front matter (a `---` line, the fields, a `---` line)")?;
    let yaml = indent_continued_values(yaml);
    let mut tree = Tree::default();
    Parser::new_from_str(&yaml).load(&mut tree, false).map_err(|e| {
        // The front matter starts on the file's second line.
        format!("line {}: its front matter is not valid YAML: {}", e.marker().line() + 1, e.info())
    })?;
    if let Some(problem) = tree.problem {
        return Err(format!("its front matter has {}", problem));
    }
    match tree.root {
        Some(Node::Map(pairs)) => Ok(pairs),
        _ => Err("its front matter is not a list of `field: value` lines".to_string()),
    }
}

// ── One card ───────────────────────────────────────────────────────

/// Reads one card's fields, collecting what is wrong with them.
struct Reader<'a> {
    file: &'a str,
    fields: &'a [(String, Node)],
    problems: Vec<String>,
}

impl<'a> Reader<'a> {
    fn problem(&mut self, line: Option<usize>, what: String) {
        // Line numbers count from the top of the file; the front matter
        // starts on line 2.
        let place = match line {
            Some(line) => format!("cards/{} line {}", self.file, line + 1),
            None => format!("cards/{}", self.file),
        };
        self.problems.push(format!("{}: {}", place, what));
    }

    /// A field's value, or `None` if the card does not have the field or
    /// leaves it empty. The last one wins if a field is written twice.
    fn get(&self, field: &str) -> Option<&'a Node> {
        self.fields
            .iter()
            .rev()
            .find(|(name, _)| name == field)
            .map(|(_, node)| node)
            .filter(|node| !node.is_absent())
    }

    /// A field's text as written, or `None` if absent.
    fn text(&mut self, field: &str) -> Option<String> {
        match self.get(field)? {
            Node::Scalar { text, .. } => Some(text.clone()),
            _ => {
                self.problem(None, format!("`{}` must be text, not a list or a group of fields", field));
                None
            }
        }
    }

    /// A field's text without the space around it, or `None` if absent or
    /// only space.
    fn trimmed(&mut self, field: &str) -> Option<String> {
        self.text(field)
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
    }

    /// A field the card must have.
    fn required(&mut self, field: &str) -> String {
        match self.text(field) {
            Some(text) if !text.trim().is_empty() => text,
            Some(_) => {
                self.problem(None, format!("its `{}` is empty, and every card needs one", field));
                String::new()
            }
            None => {
                // If the field is there but is not text, `text` has said so.
                if self.get(field).is_none() {
                    self.problem(None, format!("it has no `{}`, which every card needs", field));
                }
                String::new()
            }
        }
    }

    /// A whole number of at most `max`, or `None` if absent.
    fn whole(&mut self, field: &str, max: u32) -> Option<u32> {
        let node = self.get(field)?;
        let number = whole_number(node).filter(|n| *n <= max);
        if number.is_none() {
            self.problem(
                line_of(node),
                format!("`{}` must be a whole number from 0 to {}, written without quotes", field, max),
            );
        }
        number
    }

    /// `true` or `false`; `false` if absent.
    fn flag(&mut self, field: &str) -> bool {
        let Some(node) = self.get(field) else {
            return false;
        };
        match node {
            Node::Scalar { text, plain: true, .. } if text == "true" => true,
            Node::Scalar { text, plain: true, .. } if text == "false" => false,
            _ => {
                self.problem(line_of(node), format!("`{}` must be `true` or `false`, written without quotes", field));
                false
            }
        }
    }
}

fn line_of(node: &Node) -> Option<usize> {
    match node {
        Node::Scalar { line, .. } => Some(*line),
        _ => None,
    }
}

/// A scalar written bare and made of digits only, as a number.
fn whole_number(node: &Node) -> Option<u32> {
    match node {
        Node::Scalar { text, plain: true, .. }
            if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) =>
        {
            text.parse().ok()
        }
        _ => None,
    }
}

/// A card's address without its slashes or a closing `.html`: `/timebattle/`
/// is `timebattle`, `/404.html` is `404`.
fn slug_of(permalink: &str) -> String {
    let slug = permalink.replace('/', "");
    slug.strip_suffix(".html").unwrap_or(&slug).to_string()
}

fn keal_means(reader: &mut Reader) -> Vec<KealMeans> {
    let Some(node) = reader.get("keal_means") else {
        return Vec::new();
    };
    let Node::Map(entries) = node else {
        reader.problem(
            line_of(node),
            "`keal_means` must be a group of named keal means, each with `genetics` and `count`".to_string(),
        );
        return Vec::new();
    };
    let mut all = Vec::new();
    for (name, entry) in entries {
        let Node::Map(parts) = entry else {
            reader.problem(
                line_of(entry),
                format!("the keal means {:?} must have `genetics` and `count` under it", name),
            );
            continue;
        };
        let part = |field: &str| {
            parts
                .iter()
                .rev()
                .find(|(key, _)| key == field)
                .map(|(_, node)| node)
                .filter(|node| !node.is_absent())
        };
        let genetics = match part("genetics") {
            None => Vec::new(),
            Some(Node::List(items)) => items
                .iter()
                .filter_map(|item| match item {
                    Node::Scalar { text, .. } => Some(text.clone()),
                    _ => {
                        reader.problem(None, format!("the genetics of the keal means {:?} must each be text", name));
                        None
                    }
                })
                .collect(),
            Some(other) => {
                reader.problem(
                    line_of(other),
                    format!("the `genetics` of the keal means {:?} must be a list, like [\"Avian\", \"Magic\"]", name),
                );
                Vec::new()
            }
        };
        let count = match part("count") {
            None => 0,
            Some(node) => match whole_number(node).and_then(|n| u8::try_from(n).ok()) {
                Some(count) => count,
                None => {
                    reader.problem(
                        line_of(node),
                        format!("the `count` of the keal means {:?} must be a whole number from 0 to 255", name),
                    );
                    0
                }
            },
        };
        all.push(KealMeans { name: name.clone(), genetics, count });
    }
    all
}

/// Read one card file. `file` is its name in `cards/`, `text` its contents.
/// On failure, every problem found, each starting with the file's name.
pub fn parse_card(file: &str, text: &str) -> Result<Card, Vec<String>> {
    let fields = fields(text).map_err(|problem| vec![format!("cards/{}: {}", file, problem)])?;
    let mut reader = Reader { file, fields: &fields, problems: Vec::new() };

    let url = reader.required("permalink");
    let slug = slug_of(&url);
    let title = reader.required("title");
    let layout = reader.required("layout");
    let img_name = reader.required("img_name");

    // The file is named after the address, so a card is found by where it
    // lives on the site, and two cards cannot share an address.
    if !slug.is_empty() && file != format!("{}.html", slug) {
        reader.problem(
            None,
            format!("its address is {}, so the file must be named {}.html", url, slug),
        );
    }

    let hidden = reader.flag("hidden");

    // The short code, as in the QR code printed on the card.
    let code = reader
        .text("redirect_from")
        .map(|text| text.replace('/', "").trim().to_string())
        .unwrap_or_default();
    if code.is_empty() {
        if !hidden {
            reader.problem(
                None,
                "it has no `redirect_from` (its short code), which every card the binder shows needs".to_string(),
            );
        }
    } else if code.chars().count() != CODE_LENGTH {
        reader.problem(
            None,
            format!("its short code {:?} must be {} characters long", code, CODE_LENGTH),
        );
    }

    let thumbnail = match reader.text("thumbnail") {
        None => "center".to_string(),
        Some(anchor) if THUMBNAILS.contains(&anchor.as_str()) => anchor,
        Some(other) => {
            reader.problem(
                None,
                format!("`thumbnail` must be top, center or bottom, not {:?}", other),
            );
            "center".to_string()
        }
    };

    // Movement is a number of spaces, or text for a card that moves some
    // other way. The text is shown; the number is what the game counts with.
    let (movement, movement_text) = match reader.get("movement") {
        None => (0, None),
        Some(node) => match whole_number(node).map(u8::try_from) {
            Some(Ok(spaces)) => (spaces, Some(spaces.to_string())),
            Some(Err(_)) => {
                reader.problem(line_of(node), "`movement` as a number must be at most 255".to_string());
                (0, None)
            }
            None => (0, reader.trimmed("movement")),
        },
    };

    let card = Card {
        file: file.to_string(),
        slug,
        title,
        layout,
        img_name,
        img_alt: reader.trimmed("img_alt").unwrap_or_default(),
        thumbnail,
        tags: reader.text("tags").unwrap_or_default(),
        genetic_disposition: reader.trimmed("genetic_disposition"),
        motivation: reader.trimmed("motivation"),
        habitat: reader.trimmed("habitat"),
        url,
        code,
        variation: reader.trimmed("variation"),
        movement_text,
        play_style: reader.trimmed("play_style"),
        atk_die: reader.trimmed("atk_die"),
        def_die: reader.trimmed("def_die"),
        criteria: reader.trimmed("criteria"),
        success: reader.trimmed("success"),
        failure: reader.trimmed("failure"),
        activation: reader.trimmed("activation"),
        detection: reader.trimmed("detection"),
        scarcity: reader.trimmed("scarcity"),
        description: reader.trimmed("description"),
        promotion: reader.trimmed("promotion"),
        promotion_conditions: reader.trimmed("promotion_conditions"),
        injury_tolerance: reader.whole("injury_tolerance", u8::MAX as u32).unwrap_or(0) as u8,
        keal_means: keal_means(&mut reader),
        movement,
        die: reader.text("die").unwrap_or_default(),
        brawl_sequence: reader.text("brawl_sequence").unwrap_or_default(),
        tamability: reader.whole("tamability", u32::MAX),
        loyalty_days: reader.whole("loyalty_days", u32::MAX),
        hidden,
        landscape: reader.flag("landscape"),
    };

    if reader.problems.is_empty() {
        Ok(card)
    } else {
        Err(reader.problems)
    }
}

// ── All the cards ──────────────────────────────────────────────────

/// Put cards in the catalog's order, which is the order the binder shows
/// them in: by title, capitals and small letters alike. Cards with the same
/// title keep the order they came in.
fn sort(cards: &mut [Card]) {
    cards.sort_by_key(|card| card.title.to_lowercase());
}

/// What is wrong between cards: two at one address, or two with one short
/// code.
fn clashes(cards: &[Card]) -> Vec<String> {
    let mut problems = Vec::new();
    for (i, card) in cards.iter().enumerate() {
        for earlier in &cards[..i] {
            if card.url == earlier.url {
                problems.push(format!(
                    "cards/{} and cards/{} have the same address, {}",
                    earlier.file, card.file, card.url
                ));
            }
            if !card.code.is_empty() && card.code == earlier.code {
                problems.push(format!(
                    "cards/{} and cards/{} have the same short code, {}",
                    earlier.file, card.file, card.code
                ));
            }
        }
    }
    problems
}

/// Check cards against each other and put them in the catalog's order.
/// `cards` are (file name, contents) pairs.
pub fn catalog(files: &[(String, String)]) -> Result<Vec<Card>, Vec<String>> {
    // In file name order, so the result does not depend on the order the
    // file system lists them in.
    let mut files: Vec<&(String, String)> = files.iter().collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut cards = Vec::new();
    let mut problems = Vec::new();
    for (file, text) in files {
        match parse_card(file, text) {
            Ok(card) => cards.push(card),
            Err(found) => problems.extend(found),
        }
    }
    problems.extend(clashes(&cards));
    if !problems.is_empty() {
        return Err(problems);
    }
    sort(&mut cards);
    Ok(cards)
}

/// Read every card file (`*.html`) in a folder into the catalog.
pub fn read_cards(folder: &Path) -> Result<Vec<Card>, Vec<String>> {
    let cannot = |what: String| vec![format!("{}: {}", folder.display(), what)];
    let entries = fs::read_dir(folder).map_err(|e| cannot(format!("cannot read the card folder: {}", e)))?;
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| cannot(e.to_string()))?.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("html") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return Err(cannot(format!("a card file's name is not text: {}", path.display())));
        };
        let text = fs::read_to_string(&path)
            .map_err(|e| vec![format!("cards/{}: cannot be read: {}", name, e)])?;
        files.push((name.to_string(), text));
    }
    if files.is_empty() {
        return Err(cannot("there are no card files (*.html) in it".to_string()));
    }
    catalog(&files)
}

// ── As Rust ────────────────────────────────────────────────────────

fn optional(value: &Option<String>) -> String {
    match value {
        Some(text) => format!("Some({:?})", text),
        None => "None".to_string(),
    }
}

/// The catalog as Rust source: `CARD_COUNT` and `CARDS`, for the engine to
/// include beside its `Card` and `KealMeans` (`src/catalog.rs`).
pub fn emit(cards: &[Card]) -> String {
    let mut out = String::new();
    out.push_str("// Written by the engine's build script from the card files in cards/.\n");
    out.push_str("// It is made again whenever a card file changes; do not edit it.\n\n");
    out.push_str(&format!("pub const CARD_COUNT: usize = {};\n\n", cards.len()));
    out.push_str("pub static CARDS: [Card; CARD_COUNT] = [\n");
    for card in cards {
        let keal_means: Vec<String> = card
            .keal_means
            .iter()
            .map(|km| {
                format!(
                    "KealMeans {{ name: {:?}, genetics: &{:?}, count: {} }}",
                    km.name, km.genetics, km.count
                )
            })
            .collect();
        let number = |amount: Option<u32>| match amount {
            Some(amount) => format!("Some({})", amount),
            None => "None".to_string(),
        };
        let fields: Vec<(&str, String)> = vec![
            ("slug", format!("{:?}", card.slug)),
            ("title", format!("{:?}", card.title)),
            ("layout", format!("{:?}", card.layout)),
            ("img_name", format!("{:?}", card.img_name)),
            ("img_alt", format!("{:?}", card.img_alt)),
            ("thumbnail", format!("{:?}", card.thumbnail)),
            ("tags", format!("{:?}", card.tags)),
            ("genetic_disposition", optional(&card.genetic_disposition)),
            ("motivation", optional(&card.motivation)),
            ("habitat", optional(&card.habitat)),
            ("url", format!("{:?}", card.url)),
            ("code", format!("{:?}", card.code)),
            ("variation", optional(&card.variation)),
            ("movement_text", optional(&card.movement_text)),
            ("play_style", optional(&card.play_style)),
            ("atk_die", optional(&card.atk_die)),
            ("def_die", optional(&card.def_die)),
            ("criteria", optional(&card.criteria)),
            ("success", optional(&card.success)),
            ("failure", optional(&card.failure)),
            ("activation", optional(&card.activation)),
            ("detection", optional(&card.detection)),
            ("scarcity", optional(&card.scarcity)),
            ("description", optional(&card.description)),
            ("promotion", optional(&card.promotion)),
            ("promotion_conditions", optional(&card.promotion_conditions)),
            ("injury_tolerance", card.injury_tolerance.to_string()),
            ("keal_means", format!("&[{}]", keal_means.join(", "))),
            ("movement", card.movement.to_string()),
            ("die", format!("{:?}", card.die)),
            ("brawl_sequence", format!("{:?}", card.brawl_sequence)),
            ("tamability", number(card.tamability)),
            ("loyalty_days", number(card.loyalty_days)),
            ("hidden", card.hidden.to_string()),
            ("landscape", card.landscape.to_string()),
        ];
        out.push_str(&format!("    // cards/{}\n    Card {{\n", card.file));
        for (name, value) in fields {
            out.push_str(&format!("        {}: {},\n", name, value));
        }
        out.push_str("    },\n");
    }
    out.push_str("];\n");
    out
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A card file with the fields every card needs, and any others given.
    fn file(slug: &str, code: &str, more: &str) -> (String, String) {
        (
            format!("{}.html", slug),
            format!(
                "---\nlayout: Item\npermalink: /{}/\nredirect_from: /{}\ntitle: \"{}\"\nimg_name: {}.jxl\n{}\n---\n",
                slug, code, slug, slug, more
            ),
        )
    }

    fn card(more: &str) -> Card {
        let (name, text) = file("cloth", "AbCd", more);
        parse_card(&name, &text).unwrap_or_else(|problems| panic!("{:?}", problems))
    }

    fn problems(more: &str) -> String {
        let (name, text) = file("cloth", "AbCd", more);
        parse_card(&name, &text).expect_err("the card is refused").join("\n")
    }

    #[test]
    fn a_card_is_read_from_its_front_matter() {
        let c = card("tags: cloth fabric\nvariation: \"Item\"\nscarcity: \"Founder's Deck\"");
        assert_eq!(c.file, "cloth.html");
        assert_eq!(c.slug, "cloth");
        assert_eq!(c.url, "/cloth/");
        assert_eq!(c.code, "AbCd");
        assert_eq!(c.title, "cloth");
        assert_eq!(c.layout, "Item");
        assert_eq!(c.img_name, "cloth.jxl");
        assert_eq!(c.tags, "cloth fabric");
        assert_eq!(c.variation.as_deref(), Some("Item"));
        assert_eq!(c.scarcity.as_deref(), Some("Founder's Deck"));
        // What the card does not say.
        assert_eq!(c.thumbnail, "center");
        assert_eq!(c.habitat, None);
        assert_eq!((c.injury_tolerance, c.movement, c.tamability), (0, 0, None));
        assert!(!c.hidden && !c.landscape);
        assert!(c.keal_means.is_empty());
    }

    #[test]
    fn an_address_is_a_slug_without_slashes_or_html() {
        assert_eq!(slug_of("/timebattle/"), "timebattle");
        assert_eq!(slug_of("/fourohfour"), "fourohfour");
        assert_eq!(slug_of("/404.html"), "404");
    }

    #[test]
    fn text_over_several_lines_is_joined_and_trimmed() {
        let c = card("description: \"\n  The days grew long\n  and the king looked out.\n\"\nplay_style: one\n  two\n  three");
        assert_eq!(c.description.as_deref(), Some("The days grew long and the king looked out."));
        assert_eq!(c.play_style.as_deref(), Some("one two three"));
    }

    #[test]
    fn text_is_taken_as_written_whatever_it_looks_like() {
        let c = card("tags: no\ndie: 1e3\nbrawl_sequence: 0123\nhabitat: 2023-04-03\nmotivation: true");
        assert_eq!(c.tags, "no");
        assert_eq!(c.die, "1e3");
        assert_eq!(c.brawl_sequence, "0123");
        assert_eq!(c.habitat.as_deref(), Some("2023-04-03"));
        assert_eq!(c.motivation.as_deref(), Some("true"));
    }

    #[test]
    fn markup_and_quotes_in_text_are_kept() {
        let c = card("criteria: \"<b>Activation:</b> You lose <br>\n<b>Detection:</b> the \\\"capital\\\"\"");
        assert_eq!(
            c.criteria.as_deref(),
            Some("<b>Activation:</b> You lose <br> <b>Detection:</b> the \"capital\"")
        );
    }

    #[test]
    fn a_field_left_empty_is_a_field_not_there() {
        let c = card("habitat:\nmotivation: ~\nscarcity: \"  \"\ntags:");
        assert_eq!((c.habitat, c.motivation, c.scarcity), (None, None, None));
        assert_eq!(c.tags, "");
    }

    #[test]
    fn movement_is_a_number_of_spaces_or_text() {
        let spaces = card("movement: 3");
        assert_eq!((spaces.movement, spaces.movement_text.as_deref()), (3, Some("3")));
        let other = card("movement: \"\n<br>Random teleportation.\n\"");
        assert_eq!((other.movement, other.movement_text.as_deref()), (0, Some("<br>Random teleportation.")));
        let quoted = card("movement: \"3\"");
        assert_eq!((quoted.movement, quoted.movement_text.as_deref()), (0, Some("3")));
    }

    #[test]
    fn numbers_and_flags_are_read() {
        let c = card("injury_tolerance: 4\ntamability: 30\nhidden: true\nlandscape: false\nthumbnail: top");
        assert_eq!((c.injury_tolerance, c.tamability, c.loyalty_days), (4, Some(30), None));
        assert_eq!(card("loyalty_days: 40").loyalty_days, Some(40));
        assert!(c.hidden && !c.landscape);
        assert_eq!(c.thumbnail, "top");
    }

    #[test]
    fn keal_means_keep_their_order_names_and_genetics() {
        let c = card(
            "keal_means:\n  \"Scout's Vision\":\n    genetics: [\"Avian\", \"Cenozoic\"]\n    count: 1\n  Claws:\n    genetics: [\"Magic\"]\n    count: 2",
        );
        assert_eq!(
            c.keal_means,
            vec![
                KealMeans { name: "Scout's Vision".into(), genetics: vec!["Avian".into(), "Cenozoic".into()], count: 1 },
                KealMeans { name: "Claws".into(), genetics: vec!["Magic".into()], count: 2 },
            ]
        );
    }

    #[test]
    fn a_field_nothing_reads_is_ignored() {
        let c = card("minted: 2023-04-03\nold_keal: whatever");
        assert_eq!(c.slug, "cloth");
    }

    // ── The house style ────────────────────────────────────────────

    #[test]
    fn long_text_reads_the_same_with_its_later_lines_indented_or_not() {
        let house = card("description: \"The days grew long\nand the king looked out.\n\nA new day.\"\nscarcity: 'Founder''s\nDeck'");
        let proper = card("description: \"The days grew long\n  and the king looked out.\n\n  A new day.\"\nscarcity: 'Founder''s\n  Deck'");
        assert_eq!(house, proper);
        assert_eq!(house.description.as_deref(), Some("The days grew long and the king looked out.\nA new day."));
        assert_eq!(house.scarcity.as_deref(), Some("Founder's Deck"));
    }

    #[test]
    fn only_the_later_lines_of_an_open_quoted_value_are_indented() {
        let yaml = concat!(
            "title: \"Closed on its line\"\n",
            "img_alt: Lost? \"404\" will get you home\n",
            "criteria: \"<b>One:</b> a \\\"quoted\\\" word\n",
            "\n",
            "<b>Two:</b> the end\"\n",
            "keal_means:\n",
            "  \"Scout's Vision\":\n",
            "    genetics: [\"Avian\"]\n",
            "tags: a b\n",
        );
        let expected = yaml.replace("<b>Two:</b>", "  <b>Two:</b>");
        assert_eq!(indent_continued_values(yaml), expected);
        assert_eq!(indent_continued_values(yaml).lines().count(), yaml.lines().count());
    }

    #[test]
    fn a_quote_inside_the_text_does_not_end_the_value() {
        assert!(!closes_quote("an \\\"escaped\\\" quote", '"'));
        assert!(closes_quote("ends here\" # and more", '"'));
        assert!(!closes_quote("the king''s folly", '\''));
        assert!(closes_quote("the king''s folly'", '\''));
        // A double quote means nothing inside single quotes, and the reverse.
        assert!(!closes_quote("say \"hello\"", '\''));
        assert!(!closes_quote("the king's folly", '"'));
    }

    #[test]
    fn broken_yaml_is_reported_with_its_line_in_the_file() {
        let found = problems("habitat: \"never closed");
        assert!(found.starts_with("cards/cloth.html: line "), "{}", found);
        assert!(found.contains("not valid YAML"), "{}", found);
    }

    // ── What stops the build ───────────────────────────────────────

    #[test]
    fn a_card_missing_a_field_every_card_needs_is_refused() {
        for field in ["permalink", "title", "layout", "img_name"] {
            let (name, text) = file("cloth", "AbCd", "");
            let text: String = text.lines().filter(|l| !l.starts_with(field)).map(|l| format!("{}\n", l)).collect();
            let found = parse_card(&name, &text).expect_err(field).join("\n");
            assert!(found.contains(&format!("cards/cloth.html: it has no `{}`", field)), "{}", found);
        }
    }

    #[test]
    fn a_file_not_named_after_its_address_is_refused() {
        let (_, text) = file("cloth", "AbCd", "");
        let found = parse_card("cloht.html", &text).unwrap_err().join("\n");
        assert_eq!(found, "cards/cloht.html: its address is /cloth/, so the file must be named cloth.html");
    }

    #[test]
    fn a_short_code_of_the_wrong_length_is_refused() {
        let (name, text) = file("cloth", "AbC", "");
        let found = parse_card(&name, &text).unwrap_err().join("\n");
        assert!(found.contains("its short code \"AbC\" must be 4 characters long"), "{}", found);
    }

    #[test]
    fn a_shown_card_needs_a_short_code_and_a_hidden_one_does_not() {
        let (name, text) = file("cloth", "AbCd", "");
        let no_code: String = text.lines().filter(|l| !l.starts_with("redirect_from")).map(|l| format!("{}\n", l)).collect();
        let found = parse_card(&name, &no_code).unwrap_err().join("\n");
        assert!(found.contains("it has no `redirect_from`"), "{}", found);

        let hidden = no_code.replace("layout: Item", "layout: Item\nhidden: true");
        let c = parse_card(&name, &hidden).unwrap();
        assert!(c.hidden && c.code.is_empty());
    }

    #[test]
    fn a_number_that_is_not_one_is_refused_with_its_line() {
        let found = problems("injury_tolerance: three");
        assert_eq!(
            found,
            "cards/cloth.html line 7: `injury_tolerance` must be a whole number from 0 to 255, written without quotes"
        );
        assert!(problems("injury_tolerance: 300").contains("from 0 to 255"));
        assert!(problems("injury_tolerance: \"3\"").contains("written without quotes"));
        assert!(problems("tamability: -1").contains("`tamability` must be a whole number"));
        assert!(problems("movement: 300").contains("at most 255"));
    }

    #[test]
    fn a_flag_that_is_not_true_or_false_is_refused() {
        assert!(problems("hidden: yes").contains("`hidden` must be `true` or `false`"));
        assert!(problems("landscape: \"true\"").contains("`landscape` must be `true` or `false`"));
    }

    #[test]
    fn a_thumbnail_anchor_that_is_not_one_is_refused() {
        assert!(problems("thumbnail: middle").contains("`thumbnail` must be top, center or bottom, not \"middle\""));
    }

    #[test]
    fn keal_means_written_wrongly_are_refused() {
        assert!(problems("keal_means: claws").contains("`keal_means` must be a group"));
        assert!(problems("keal_means:\n  Claws: 2").contains("the keal means \"Claws\" must have"));
        assert!(problems("keal_means:\n  Claws:\n    genetics: Magic\n    count: 1").contains("must be a list"));
        assert!(problems("keal_means:\n  Claws:\n    genetics: [\"Magic\"]\n    count: many").contains("`count` of the keal means \"Claws\""));
    }

    #[test]
    fn every_problem_in_a_card_is_reported_at_once() {
        let found = problems("injury_tolerance: lots\nhidden: maybe\nthumbnail: middle");
        assert_eq!(found.lines().count(), 3, "{}", found);
    }

    #[test]
    fn a_file_without_front_matter_or_with_broken_yaml_is_refused() {
        let found = parse_card("cloth.html", "layout: Item\n").unwrap_err().join("\n");
        assert!(found.contains("cards/cloth.html: it does not start with front matter"), "{}", found);
        let found = parse_card("cloth.html", "---\ntitle: \"unclosed\n---\n").unwrap_err().join("\n");
        assert!(found.contains("not valid YAML"), "{}", found);
        let found = parse_card("cloth.html", "---\n- a\n- b\n---\n").unwrap_err().join("\n");
        assert!(found.contains("not a list of `field: value` lines"), "{}", found);
    }

    // ── All the cards ──────────────────────────────────────────────

    #[test]
    fn cards_are_in_title_order_whatever_order_the_files_come_in() {
        let files = vec![file("sticks", "AAAA", ""), file("Cloth", "BBBB", ""), file("honey", "CCCC", "")];
        let cards = catalog(&files).unwrap();
        let titles: Vec<&str> = cards.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["Cloth", "honey", "sticks"]);
    }

    #[test]
    fn cards_with_one_title_stay_in_file_name_order() {
        let mut second = file("fourohfour", "eUN0", "");
        second.1 = second.1.replace("title: \"fourohfour\"", "title: \"404 Glitch\"");
        let mut first = file("404", "", "hidden: true");
        first.1 = first.1.replace("title: \"404\"", "title: \"404 Glitch\"").replace("redirect_from: /\n", "");
        let cards = catalog(&[second, first]).unwrap();
        let slugs: Vec<&str> = cards.iter().map(|c| c.slug.as_str()).collect();
        assert_eq!(slugs, ["404", "fourohfour"]);
    }

    #[test]
    fn two_cards_with_one_short_code_are_refused() {
        let found = catalog(&[file("cloth", "AbCd", ""), file("honey", "AbCd", "")]).unwrap_err().join("\n");
        assert_eq!(found, "cards/cloth.html and cards/honey.html have the same short code, AbCd");
    }

    #[test]
    fn problems_in_several_files_are_all_reported() {
        let found = catalog(&[file("cloth", "AbC", ""), file("honey", "AbCd", "hidden: perhaps")]).unwrap_err();
        assert_eq!(found.len(), 2, "{:?}", found);
        assert!(found[0].starts_with("cards/cloth.html") && found[1].starts_with("cards/honey.html"));
    }

    // ── As Rust ────────────────────────────────────────────────────

    #[test]
    fn the_catalog_is_written_as_rust() {
        let cards = catalog(&[file(
            "cloth",
            "AbCd",
            "description: \"A \\\"fine\\\" cloth\\\\weave\"\nkeal_means:\n  Claws:\n    genetics: [\"Magic\", \"Avian\"]\n    count: 2\ntamability: 30\nhidden: true",
        )])
        .unwrap();
        let rust = emit(&cards);
        assert!(rust.contains("pub const CARD_COUNT: usize = 1;"));
        assert!(rust.contains("pub static CARDS: [Card; CARD_COUNT] = ["));
        assert!(rust.contains("        slug: \"cloth\",\n"));
        // Quotes and backslashes cannot end the string early.
        assert!(rust.contains(r#"        description: Some("A \"fine\" cloth\\weave"),"#), "{}", rust);
        assert!(rust.contains(
            r#"        keal_means: &[KealMeans { name: "Claws", genetics: &["Magic", "Avian"], count: 2 }],"#
        ));
        assert!(rust.contains("        tamability: Some(30),\n"));
        assert!(rust.contains("        loyalty_days: None,\n"));
        assert!(rust.contains("        habitat: None,\n"));
        assert!(rust.contains("        hidden: true,\n"));
    }
}
