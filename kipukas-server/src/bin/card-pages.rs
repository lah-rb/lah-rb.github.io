//! What the site's build needs from the engine's card catalog. Jekyll runs
//! it twice in every build (`_plugins/card_pages.rb`):
//!
//!   card-pages count
//!   card-pages write --site <built site folder> --cache <cache folder>
//!                    [--kippa <file>]  < card shell
//!
//! `count` prints how many cards the binder shows. Jekyll asks before it
//! renders, for the binder's filter menu.
//!
//! `write` adds the cards to the site Jekyll has built. Jekyll renders one
//! card shell (`card-shell.html`) and hands it over on standard input. This
//! fills a copy of the shell for each card in the engine's catalog and writes
//! it at the card's address, writes the page behind each card's short
//! address, writes the data files the site's scripts read, and adds the cards
//! to the sitemap and the index of redirects that Jekyll wrote. What is
//! written is decided in `src/site.rs`; this file only reads and writes.
//!
//! It also writes each card's preview picture, the JPEG a shared link shows
//! (`src/preview.rs`), made from the card's art in the built site. Making one
//! takes far longer than everything else here, so each is kept in the cache
//! folder under a name that changes when the art or the way previews are made
//! changes, and is made only when it is not there. The cache folder can be
//! deleted at any time; the next run fills it again.
//!
//! With `--kippa`, it also keeps Kippa's copy of the catalog (`src/kippa.rs`)
//! up to date at that path, which is outside the site.
//!
//! It never replaces someone else's file: if a file with different contents
//! already exists where a card's page, short address, preview or data file
//! goes, something else built that address, and nothing is written. A file that is
//! already what this would write is left alone, so running it again (or twice
//! at once, as two Jekyll watchers do) is harmless.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kipukas_server::catalog::CARDS;
use kipukas_server::{kippa, preview, site};

/// Write a file so that it is never seen half-written: write beside it, then
/// move into place.
fn write_whole(target: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(folder) = target.parent() {
        fs::create_dir_all(folder)
            .map_err(|e| format!("cannot create {}: {}", folder.display(), e))?;
    }
    let mut beside = target.as_os_str().to_owned();
    beside.push(format!(".{}.tmp", std::process::id()));
    let beside = PathBuf::from(beside);
    fs::write(&beside, contents)
        .and_then(|_| fs::rename(&beside, target))
        .map_err(|e| {
            let _ = fs::remove_file(&beside);
            format!("cannot write {}: {}", target.display(), e)
        })
}

/// A card's preview picture: from the cache if this art's preview is there,
/// otherwise made now and kept. Returns the JPEG and whether it was made.
fn preview_of(art_file: &Path, cache_dir: &Path) -> Result<(Vec<u8>, bool), String> {
    let art = fs::read(art_file)
        .map_err(|e| format!("cannot read the card art {}: {}", art_file.display(), e))?;
    let kept = cache_dir.join(format!("{}.jpg", preview::cache_key(&art)));
    if let Ok(jpeg) = fs::read(&kept) {
        return Ok((jpeg, false));
    }
    let jpeg = preview::render(&art)
        .map_err(|e| format!("cannot make a preview of {}: {}", art_file.display(), e))?;
    write_whole(&kept, &jpeg)?;
    Ok((jpeg, true))
}

fn write(site_dir: &Path, cache_dir: &Path, kippa_file: Option<&Path>, shell: &str) -> Result<String, String> {
    let read = |name: &str| -> Result<String, String> {
        fs::read_to_string(site_dir.join(name))
            .map_err(|e| format!("cannot read {}: {}", site_dir.join(name).display(), e))
    };
    let sitemap = read(site::SITEMAP_PATH)?;
    // Jekyll may not have written an index of redirects.
    let redirects = read(site::REDIRECTS_PATH).unwrap_or_default();

    let output = site::build_catalog(shell, &sitemap, &redirects)?;

    // Every file to write and what goes in it: the pages, the data files,
    // then the previews.
    let mut files: Vec<(PathBuf, &[u8], &str)> = output
        .files
        .iter()
        .map(|(path, html)| (site_dir.join(path), html.as_bytes(), "card's page"))
        .collect();
    files.extend(
        output
            .data
            .iter()
            .map(|(path, json)| (site_dir.join(path), json.as_bytes(), "data file of the cards")),
    );
    let mut pictures: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    let mut made = 0;
    for picture in &output.previews {
        let (jpeg, new) = preview_of(&site_dir.join(&picture.art), cache_dir)?;
        made += usize::from(new);
        pictures.push((site_dir.join(&picture.path), jpeg));
    }
    files.extend(
        pictures
            .iter()
            .map(|(target, jpeg)| (target.clone(), jpeg.as_slice(), "card's preview")),
    );

    // Look at every target before writing anything, so a refused build leaves
    // the site as it was.
    let mut to_write: Vec<(&Path, &[u8])> = Vec::new();
    for (target, contents, what) in &files {
        match fs::read(target) {
            Ok(existing) if existing == *contents => {} // already right
            Ok(_) => {
                return Err(format!(
                    "{} already exists and is not the {}: something else built this address",
                    target.display(),
                    what
                ));
            }
            Err(_) => to_write.push((target, contents)),
        }
    }

    for (target, contents) in &to_write {
        write_whole(target, contents)?;
    }
    if output.sitemap != sitemap {
        write_whole(&site_dir.join(site::SITEMAP_PATH), output.sitemap.as_bytes())?;
    }
    if output.redirects != redirects {
        write_whole(&site_dir.join(site::REDIRECTS_PATH), output.redirects.as_bytes())?;
    }

    // Kippa's copy of the catalog, kept in the source tree: written only
    // when it differs, so an unchanged catalog leaves the file untouched.
    let mut kippa_note = "";
    if let Some(file) = kippa_file {
        let catalog = kippa::catalog(&CARDS);
        if fs::read(file).ok().as_deref() != Some(catalog.as_bytes()) {
            write_whole(file, catalog.as_bytes())?;
            kippa_note = "; Kippa's catalog updated";
        }
    }

    let pages = output.files.iter().filter(|(_, html)| !is_redirect(html)).count();
    Ok(format!(
        "{} card pages, {} short addresses, {} previews and {} data files in {} ({} files written, {} previews made{})",
        pages,
        output.files.len() - pages,
        pictures.len(),
        output.data.len(),
        site_dir.display(),
        to_write.len(),
        made,
        kippa_note
    ))
}

/// A short-address page, as opposed to a card page.
fn is_redirect(html: &str) -> bool {
    html.contains("<title>Redirecting&hellip;</title>")
}

const USAGE: &str = "usage: card-pages count
       card-pages write --site <built site folder> --cache <cache folder> [--kippa <file>]  < card shell";

/// The folders and file `write` was given: `--name value` pairs.
fn write_arguments(args: &[String]) -> Option<(PathBuf, PathBuf, Option<PathBuf>)> {
    let (mut site_dir, mut cache_dir, mut kippa_file) = (None, None, None);
    let mut args = args.iter();
    while let Some(name) = args.next() {
        let value = PathBuf::from(args.next()?);
        match name.as_str() {
            "--site" => site_dir = Some(value),
            "--cache" => cache_dir = Some(value),
            "--kippa" => kippa_file = Some(value),
            _ => return None,
        }
    }
    Some((site_dir?, cache_dir?, kippa_file))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((command, [])) if command == "count" => Ok(site::shown_count(&CARDS).to_string()),
        Some((command, rest)) if command == "write" => match write_arguments(rest) {
            Some((site_dir, cache_dir, kippa_file)) => {
                let mut shell = String::new();
                std::io::stdin()
                    .read_to_string(&mut shell)
                    .map_err(|e| format!("cannot read the card shell from standard input: {}", e))
                    .and_then(|_| write(&site_dir, &cache_dir, kippa_file.as_deref(), &shell))
            }
            None => Err(USAGE.to_string()),
        },
        _ => Err(USAGE.to_string()),
    };

    match result {
        Ok(answer) => {
            println!("{}", answer);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", error);
            ExitCode::FAILURE
        }
    }
}
