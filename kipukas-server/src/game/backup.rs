//! Backups of the player's data: a file the player keeps, and restoring
//! the player's data from one.
//!
//! The Player Data page's backup section is an engine-owned view (Pattern
//! 13) of `/api/player/backup` (`routes::backup`). What it shows is held
//! here: which screen it is on, what was typed, and what came of the last
//! thing asked.
//!
//! A backup is the player's state, signed and sealed with a passphrase the
//! player chooses (`game::crypto`). Making one hands the page a file to save
//! (`page::download`). Restoring from one opens the file with the
//! passphrase, checks the signature, and takes its state over as a game
//! sent by another device is taken (`player_doc::take_over`): the backup's
//! game takes the place of the one here, and this device's record is added
//! to the backup's, so that restoring loses no day of play and restoring
//! twice counts none twice. Every view is then drawn again. Nothing is
//! restored from a file that does not open or whose signature does not hold.
//!
//! The settings page's switches travel with it (`crate::settings`): they
//! are sealed in the file beside the state, and restoring puts each as the
//! backup has it. A backup made before they were carried changes none.
//!
//! The passphrase is held only while something asked was refused, so that
//! the field keeps it (the engine draws the form again). It is forgotten
//! when the backup is made or restored, and when the player goes back to
//! the section's first screen.

use std::cell::RefCell;

use crate::game::{crypto, player_doc};
use crate::{clock, page, settings};

/// The fewest characters a passphrase may have.
pub const SHORTEST: usize = 4;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum Screen {
    /// The choice of making a backup or restoring from one.
    #[default]
    Menu,
    Export,
    Import,
}

/// What the dialog shows.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Dialog {
    pub screen: Screen,
    /// What came of the last thing asked, if anything.
    pub notice: String,
    /// Whether that was what was wanted.
    pub good: bool,
    /// What was typed, kept while a form that was refused is shown again.
    pub passphrase: String,
    pub confirm: String,
}

thread_local! {
    static DIALOG: RefCell<Dialog> = RefCell::new(Dialog::default());
}

pub fn dialog() -> Dialog {
    DIALOG.with(|dialog| dialog.borrow().clone())
}

/// Go to a screen, with nothing typed and nothing said.
pub fn show(screen: Screen) {
    DIALOG.with(|dialog| *dialog.borrow_mut() = Dialog { screen, ..Dialog::default() });
}

/// Back to how the section opens, with nothing typed and nothing said.
pub fn close() {
    show(Screen::Menu);
}

/// Stay on the screen and say why what was asked was not done.
fn refuse(why: impl Into<String>, passphrase: &str, confirm: &str) {
    DIALOG.with(|dialog| {
        let mut dialog = dialog.borrow_mut();
        dialog.notice = why.into();
        dialog.good = false;
        dialog.passphrase = passphrase.to_string();
        dialog.confirm = confirm.to_string();
    });
}

/// Back to the menu, saying what was done.
fn done(what: &str) {
    DIALOG.with(|dialog| {
        *dialog.borrow_mut() = Dialog { notice: what.to_string(), good: true, ..Dialog::default() };
    });
}

/// What a backup file seals: the player's state, signed, and the settings.
/// The signature is of the state alone. The seal is over all of it, and
/// does not open if any of it was changed.
fn signed(passphrase: &str, now: f64) -> Result<String, String> {
    let state = player_doc::encode_full_state();
    let mac = crypto::sign_export(passphrase, &state)?;
    let sealed = serde_json::json!({
        "player_doc": state,
        "mac": mac,
        "exported_at": clock::moment_of(now),
        "settings": settings::for_backup(),
    });
    Ok(sealed.to_string())
}

/// Make a backup at `now` and hand it to the player as a file.
pub fn export(now: f64, passphrase: &str, confirm: &str) {
    if passphrase.chars().count() < SHORTEST {
        return refuse(format!("Passphrase must be at least {} characters", SHORTEST), passphrase, confirm);
    }
    if passphrase != confirm {
        return refuse("Passphrases do not match", passphrase, confirm);
    }
    let file = signed(passphrase, now).and_then(|signed| {
        crypto::seal(passphrase, &signed, crypto::random_bytes()?, crypto::random_bytes()?)
    });
    match file {
        Ok(file) => {
            page::download(&format!("kipukas-player-backup-{}.kipukas", clock::date_of(now)), &file);
            done("✓ Backup exported successfully");
        }
        Err(problem) => refuse(format!("Export failed: {}", problem), passphrase, confirm),
    }
}

/// Restore the player's data from a backup file's text, at `now`.
pub fn import(now: f64, passphrase: &str, file: &str) {
    if file.trim().is_empty() {
        return refuse("Please select a .kipukas file", passphrase, "");
    }
    if passphrase.is_empty() {
        return refuse("Please enter the passphrase", passphrase, "");
    }
    match restore(now, passphrase, file) {
        Ok(taken) => {
            // Everything on the page that shows the player's state shows
            // the restored one. It is stored with the next handover.
            page::refresh();
            done(&restored(&taken));
        }
        Err(problem) => refuse(problem, passphrase, ""),
    }
}

/// What is said of a backup that was restored.
fn restored(taken: &player_doc::Taken) -> String {
    let days = |n: u32| format!("{} day{} of play", n, if n == 1 { "" } else { "s" });
    let record = match (taken.days_new_here, taken.days_new_there) {
        (0, 0) => String::new(),
        (brought, 0) => format!(" Your record gained {}.", days(brought)),
        (0, kept) => format!(" This device's {} were kept.", days(kept)),
        (brought, kept) => {
            format!(" Your record gained {}, and this device's {} were kept.", days(brought), days(kept))
        }
    };
    format!("✓ Backup restored: this device has its game.{}", record)
}

fn restore(now: f64, passphrase: &str, file: &str) -> Result<player_doc::Taken, String> {
    let signed = crypto::open(passphrase, file).map_err(|sealed| sealed.to_string())?;
    let signed: serde_json::Value =
        serde_json::from_str(&signed).map_err(|_| crypto::Sealed::NotABackup.to_string())?;
    let (Some(state), Some(mac)) = (signed["player_doc"].as_str(), signed["mac"].as_str()) else {
        return Err(crypto::Sealed::NotABackup.to_string());
    };
    match crypto::verify_export(passphrase, state, mac) {
        Ok(true) => {}
        Ok(false) | Err(_) => {
            return Err("Verification failed — wrong passphrase or data has been tampered with".to_string());
        }
    }
    let taken =
        player_doc::take_over(state, now, "file").map_err(|e| format!("The backup could not be restored: {}", e))?;
    settings::from_backup(&signed["settings"]);
    Ok(taken)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::crypto::made_by_the_page_script::{FILE as MADE_BY_THE_PAGE_SCRIPT, PASSPHRASE as ITS_PASSPHRASE};

    const NOW: f64 = 1_795_032_930_123.0; // 2026-11-18T20:15:30.123Z

    fn fresh() {
        player_doc::init_player_doc();
        settings::reset();
        crate::store::reset();
        crate::routes::qr::reset();
        page::reset();
        close();
    }

    /// A player with one damage mark and one timer.
    fn a_player() {
        player_doc::ensure_card_state("brox_the_defiant", 3);
        player_doc::toggle_slot("brox_the_defiant", 2);
        player_doc::add_alarm(NOW, 5, "siege", "red");
    }

    #[test]
    fn the_dialog_opens_on_the_menu_with_nothing_said() {
        fresh();
        assert_eq!(dialog(), Dialog::default());
        show(Screen::Export);
        assert_eq!(dialog().screen, Screen::Export);
        close();
        assert_eq!(dialog(), Dialog::default());
    }

    #[test]
    fn a_passphrase_too_short_or_not_confirmed_makes_no_backup_and_stays_in_its_field() {
        fresh();
        show(Screen::Export);
        export(NOW, "abc", "abc");
        let refused = dialog();
        assert_eq!(refused.screen, Screen::Export);
        assert_eq!(refused.notice, "Passphrase must be at least 4 characters");
        assert!(!refused.good);
        assert_eq!((refused.passphrase.as_str(), refused.confirm.as_str()), ("abc", "abc"));

        export(NOW, "correct horse", "correct house");
        assert_eq!(dialog().notice, "Passphrases do not match");
        assert_eq!(dialog().confirm, "correct house");
        assert!(page::take_download().is_none());
        // Four characters, not four bytes.
        export(NOW, "añoñ", "añoñ");
        assert!(page::take_download().is_some());
        fresh();
    }

    #[test]
    fn a_backup_is_a_file_handed_to_the_player_that_restores_their_state() {
        fresh();
        a_player();
        let state = player_doc::encode_full_state();
        show(Screen::Export);
        export(NOW, "correct horse", "correct horse");

        let (name, file) = page::take_download().expect("a file is handed over");
        assert_eq!(name, "kipukas-player-backup-2026-11-18.kipukas");
        let said = dialog();
        assert_eq!((said.screen, said.notice.as_str(), said.good), (Screen::Menu, "✓ Backup exported successfully", true));
        assert!(said.passphrase.is_empty() && said.confirm.is_empty(), "the passphrase is forgotten");
        assert!(!file.contains(&state), "the state is sealed");
        let signed: serde_json::Value = serde_json::from_str(&crypto::open("correct horse", &file).unwrap()).unwrap();
        assert_eq!(signed["player_doc"], state.as_str());
        assert_eq!(signed["exported_at"], "2026-11-18T20:15:30.123Z");

        // The player starts over, then restores from the file.
        player_doc::init_player_doc();
        assert!(player_doc::get_alarms().is_empty());
        show(Screen::Import);
        import(NOW, "correct horse", &file);
        assert_eq!((dialog().screen, dialog().notice.as_str(), dialog().good), (Screen::Menu, "✓ Backup restored: this device has its game.", true));
        assert!(player_doc::get_slot("brox_the_defiant", 2));
        assert_eq!(player_doc::get_alarms()[0].name, "siege");
        assert!(page::take_refresh(), "every view is drawn again");
        fresh();
    }

    #[test]
    fn restoring_takes_the_backups_game_and_keeps_the_record_of_this_device() {
        fresh();
        a_player();
        player_doc::increment_loyalty("brox_the_defiant", "2026-11-17").unwrap();
        export(NOW, "correct horse", "correct horse");
        let (_, file) = page::take_download().unwrap();

        // Another device, with a game and plays of its own.
        player_doc::init_player_doc();
        player_doc::add_alarm(NOW, 2, "another game", "blue");
        player_doc::increment_loyalty("brox_the_defiant", "2026-11-18").unwrap();
        player_doc::increment_loyalty("liliel", "2026-11-18").unwrap();
        import(NOW, "correct horse", &file);
        assert_eq!(
            dialog().notice,
            "✓ Backup restored: this device has its game. Your record gained 1 day of play, and this device's 2 days of play were kept."
        );
        let timers: Vec<String> = player_doc::get_alarms().into_iter().map(|alarm| alarm.name).collect();
        assert_eq!(timers, ["siege"], "the game is the backup's");
        assert_eq!(player_doc::get_loyalty("brox_the_defiant").unwrap().0, 2);
        assert_eq!(player_doc::get_loyalty("liliel").unwrap().0, 1);
        let merges = player_doc::merges();
        assert_eq!((merges.len(), merges[0].by.as_str(), merges[0].at), (1, "file", NOW));

        // The same file again: nothing is counted twice.
        import(NOW + 1000.0, "correct horse", &file);
        assert_eq!(dialog().notice, "✓ Backup restored: this device has its game. This device's 2 days of play were kept.");
        assert_eq!(player_doc::get_loyalty("brox_the_defiant").unwrap().0, 2);
        assert_eq!(player_doc::merges().len(), 1);
        fresh();
    }

    #[test]
    fn a_backup_carries_the_settings_and_restoring_puts_them_as_it_has_them() {
        fresh();
        a_player();
        settings::handle_post("toggle=kipukas-scanner-status");
        export(NOW, "correct horse", "correct horse");
        let (_, file) = page::take_download().unwrap();
        let signed: serde_json::Value = serde_json::from_str(&crypto::open("correct horse", &file).unwrap()).unwrap();
        assert_eq!(signed["settings"], serde_json::json!({ "qr-privacy-accepted": false, "kipukas-scanner-status": true }));

        // Another device, where the player has chosen otherwise.
        fresh();
        settings::handle_post("toggle=qr-privacy-accepted");
        page::handle_handover_post("");
        import(NOW, "correct horse", &file);
        assert_eq!(dialog().notice, "✓ Backup restored: this device has its game.");
        assert!(settings::is_on(&settings::SCANNER_STATUS));
        assert!(!settings::is_on(&settings::CAMERA_PRIVACY));
        // They are stored with the player's state, and every view is drawn
        // again, the settings page with them.
        let handed: serde_json::Value = serde_json::from_str(&page::handle_handover_post("")).unwrap();
        assert_eq!(handed["save"]["kipukas-scanner-status"], "true");
        assert_eq!(handed["save"]["qr-privacy-accepted"], "false");
        assert_eq!(handed["refresh"], true, "{}", handed);
        fresh();
    }

    #[test]
    fn a_file_that_is_not_restored_changes_no_setting() {
        fresh();
        settings::handle_post("toggle=kipukas-scanner-status");
        export(NOW, "correct horse", "correct horse");
        let (_, file) = page::take_download().unwrap();
        fresh();
        import(NOW, "wrong horse", &file);
        assert!(!dialog().good);
        assert!(!settings::is_on(&settings::SCANNER_STATUS));
        fresh();
    }

    #[test]
    fn two_backups_of_the_same_state_are_different_files() {
        fresh();
        a_player();
        export(NOW, "correct horse", "correct horse");
        let (_, first) = page::take_download().unwrap();
        export(NOW, "correct horse", "correct horse");
        let (_, second) = page::take_download().unwrap();
        let (first, second): (serde_json::Value, serde_json::Value) =
            (serde_json::from_str(&first).unwrap(), serde_json::from_str(&second).unwrap());
        for field in ["salt", "iv", "data"] {
            assert_ne!(first[field], second[field], "{}", field);
        }
        fresh();
    }

    #[test]
    fn nothing_is_restored_from_a_file_that_does_not_open() {
        fresh();
        a_player();
        export(NOW, "correct horse", "correct horse");
        let (_, file) = page::take_download().unwrap();
        player_doc::init_player_doc();
        player_doc::add_alarm(NOW, 2, "kept", "blue");
        show(Screen::Import);

        for (passphrase, text, why) in [
            ("correct horse", "", "Please select a .kipukas file"),
            ("", file.as_str(), "Please enter the passphrase"),
            ("wrong horse", file.as_str(), "Decryption failed — wrong passphrase or file has been tampered with"),
            ("correct horse", "a holiday photo", "Not a valid Kipukas backup file"),
            ("correct horse", r#"{"format":"kipukas-player-export","version":7}"#, "Unsupported backup version: 7"),
        ] {
            import(NOW, passphrase, text);
            let refused = dialog();
            assert_eq!((refused.screen, refused.notice.as_str(), refused.good), (Screen::Import, why, false));
            assert_eq!(refused.passphrase, passphrase, "the passphrase stays in its field");
            assert_eq!(player_doc::get_alarms()[0].name, "kept", "{}", why);
            assert!(!page::take_refresh());
        }
        fresh();
    }

    #[test]
    fn a_file_that_opens_but_is_not_signed_by_its_passphrase_is_not_restored() {
        fresh();
        a_player();
        let state = player_doc::encode_full_state();
        player_doc::init_player_doc();
        // Sealed with one passphrase around a state signed with another,
        // and around things that are not a signed state at all.
        let forged = serde_json::json!({
            "player_doc": state, "mac": crypto::sign_export("another passphrase", &state).unwrap(), "exported_at": "",
        });
        for (inner, why) in [
            (forged.to_string(), "Verification failed — wrong passphrase or data has been tampered with"),
            (r#"{"player_doc":"AAAA"}"#.to_string(), "Not a valid Kipukas backup file"),
            ("not json".to_string(), "Not a valid Kipukas backup file"),
        ] {
            let file = crypto::seal("correct horse", &inner, [1; 16], [2; 12]).unwrap();
            import(NOW, "correct horse", &file);
            assert_eq!(dialog().notice, why);
            assert!(!player_doc::get_slot("brox_the_defiant", 2));
        }
        fresh();
    }

    #[test]
    fn a_backup_made_before_the_engine_sealed_them_is_restored() {
        fresh();
        show(Screen::Import);
        import(NOW, ITS_PASSPHRASE, MADE_BY_THE_PAGE_SCRIPT);
        assert_eq!((dialog().notice.as_str(), dialog().good), ("✓ Backup restored: this device has its game.", true));
        // What that player had: a mark on Brox's second slot and a timer.
        assert!(player_doc::get_slot("brox_the_defiant", 2));
        assert!(!player_doc::get_slot("brox_the_defiant", 1));
        let timers = player_doc::get_alarms();
        assert_eq!((timers[0].name.as_str(), timers[0].remaining), ("fixture", 7));
        fresh();

        // It carries no settings, so those of this device stay as they are.
        settings::handle_post("toggle=kipukas-scanner-status");
        import(NOW, ITS_PASSPHRASE, MADE_BY_THE_PAGE_SCRIPT);
        assert!(dialog().good);
        assert!(settings::is_on(&settings::SCANNER_STATUS));
        fresh();
    }
}
