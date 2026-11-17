//! Writes the card pages into a built site.
//!
//!   card-pages <built site folder>  < card shell
//!
//! Jekyll renders one card shell (`card-shell.html`) and hands it to this
//! program on standard input after every build (`_plugins/card_pages.rb`).
//! This fills a copy of the shell for each card in the engine's catalog and
//! writes it at the card's address, writes the page behind each card's short
//! address, and adds the cards to the sitemap and the index of redirects that
//! Jekyll wrote. What is written is decided in `src/site.rs`; this file only
//! reads and writes.
//!
//! It never replaces someone else's page: if a file with different contents
//! already exists where a card's page or short address goes, something else
//! built that address, and nothing is written. A file that is already what
//! this would write is left alone, so running it again (or twice at once, as
//! two Jekyll watchers do) is harmless.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kipukas_server::site;

/// Write a file so that it is never seen half-written: write beside it, then
/// move into place.
fn write_whole(target: &Path, contents: &str) -> Result<(), String> {
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

fn run(site_dir: &Path, shell: &str) -> Result<String, String> {
    let read = |name: &str| -> Result<String, String> {
        fs::read_to_string(site_dir.join(name))
            .map_err(|e| format!("cannot read {}: {}", site_dir.join(name).display(), e))
    };
    let sitemap = read(site::SITEMAP_PATH)?;
    // Jekyll may not have written an index of redirects.
    let redirects = read(site::REDIRECTS_PATH).unwrap_or_default();

    let output = site::build_catalog(shell, &sitemap, &redirects)?;

    // Look at every target before writing anything, so a refused build leaves
    // the site as it was.
    let mut to_write: Vec<(PathBuf, &str)> = Vec::new();
    for (path, contents) in &output.files {
        let target = site_dir.join(path);
        match fs::read(&target) {
            Ok(existing) if existing == contents.as_bytes() => {} // already right
            Ok(_) => {
                return Err(format!(
                    "{} already exists and is not this card's page: something else built this address",
                    target.display()
                ));
            }
            Err(_) => to_write.push((target, contents)),
        }
    }

    for (target, contents) in &to_write {
        write_whole(target, contents)?;
    }
    if output.sitemap != sitemap {
        write_whole(&site_dir.join(site::SITEMAP_PATH), &output.sitemap)?;
    }
    if output.redirects != redirects {
        write_whole(&site_dir.join(site::REDIRECTS_PATH), &output.redirects)?;
    }

    let pages = output.files.iter().filter(|(_, html)| !is_redirect(html)).count();
    Ok(format!(
        "{} card pages and {} short addresses in {} ({} files written)",
        pages,
        output.files.len() - pages,
        site_dir.display(),
        to_write.len()
    ))
}

/// A short-address page, as opposed to a card page.
fn is_redirect(html: &str) -> bool {
    html.contains("<title>Redirecting&hellip;</title>")
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(site_dir), None) = (args.next(), args.next()) else {
        eprintln!("usage: card-pages <built site folder>  < card shell");
        return ExitCode::FAILURE;
    };

    let mut shell = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut shell) {
        eprintln!("[card-pages] cannot read the card shell from standard input: {}", error);
        return ExitCode::FAILURE;
    }

    match run(Path::new(&site_dir), &shell) {
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
