//! Writes the card pages into a built site.
//!
//!   card-pages <built site folder> <cache folder>  < card shell
//!
//! Jekyll renders one card shell (`card-shell.html`) and hands it to this
//! program on standard input after every build (`_plugins/card_pages.rb`).
//! This fills a copy of the shell for each card in the engine's catalog and
//! writes it at the card's address, writes the page behind each card's short
//! address, and adds the cards to the sitemap and the index of redirects that
//! Jekyll wrote. What is written is decided in `src/site.rs`; this file only
//! reads and writes.
//!
//! It also writes each card's preview picture, the JPEG a shared link shows
//! (`src/preview.rs`), made from the card's art in the built site. Making one
//! takes far longer than everything else here, so each is kept in the cache
//! folder under a name that changes when the art or the way previews are made
//! changes, and is made only when it is not there. The cache folder can be
//! deleted at any time; the next run fills it again.
//!
//! It never replaces someone else's file: if a file with different contents
//! already exists where a card's page, short address or preview goes,
//! something else built that address, and nothing is written. A file that is
//! already what this would write is left alone, so running it again (or twice
//! at once, as two Jekyll watchers do) is harmless.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kipukas_server::{preview, site};

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

fn run(site_dir: &Path, cache_dir: &Path, shell: &str) -> Result<String, String> {
    let read = |name: &str| -> Result<String, String> {
        fs::read_to_string(site_dir.join(name))
            .map_err(|e| format!("cannot read {}: {}", site_dir.join(name).display(), e))
    };
    let sitemap = read(site::SITEMAP_PATH)?;
    // Jekyll may not have written an index of redirects.
    let redirects = read(site::REDIRECTS_PATH).unwrap_or_default();

    let output = site::build_catalog(shell, &sitemap, &redirects)?;

    // Every file to write and what goes in it: the pages, then the previews.
    let mut files: Vec<(PathBuf, &[u8], &str)> = output
        .files
        .iter()
        .map(|(path, html)| (site_dir.join(path), html.as_bytes(), "page"))
        .collect();
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
            .map(|(target, jpeg)| (target.clone(), jpeg.as_slice(), "preview")),
    );

    // Look at every target before writing anything, so a refused build leaves
    // the site as it was.
    let mut to_write: Vec<(&Path, &[u8])> = Vec::new();
    for (target, contents, what) in &files {
        match fs::read(target) {
            Ok(existing) if existing == *contents => {} // already right
            Ok(_) => {
                return Err(format!(
                    "{} already exists and is not this card's {}: something else built this address",
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

    let pages = output.files.iter().filter(|(_, html)| !is_redirect(html)).count();
    Ok(format!(
        "{} card pages, {} short addresses and {} previews in {} ({} files written, {} previews made)",
        pages,
        output.files.len() - pages,
        pictures.len(),
        site_dir.display(),
        to_write.len(),
        made
    ))
}

/// A short-address page, as opposed to a card page.
fn is_redirect(html: &str) -> bool {
    html.contains("<title>Redirecting&hellip;</title>")
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(site_dir), Some(cache_dir), None) = (args.next(), args.next(), args.next()) else {
        eprintln!("usage: card-pages <built site folder> <cache folder>  < card shell");
        return ExitCode::FAILURE;
    };

    let mut shell = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut shell) {
        eprintln!("[card-pages] cannot read the card shell from standard input: {}", error);
        return ExitCode::FAILURE;
    }

    match run(Path::new(&site_dir), Path::new(&cache_dir), &shell) {
        Ok(summary) => {
            println!("{}", summary);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", error);
            ExitCode::FAILURE
        }
    }
}
