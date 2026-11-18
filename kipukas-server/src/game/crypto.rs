//! What protects a backup of the player's data: a signature and a seal,
//! both from a passphrase the player chooses.
//!
//! - **The signature** is HMAC-SHA256 of the player's state under the
//!   passphrase ([`sign_export`], [`verify_export`]). It is kept inside the
//!   backup with the state, and checked before a backup is restored.
//! - **The seal** is what a backup file is ([`seal`], [`open`]): the signed
//!   state encrypted with AES-256-GCM under a key made from the passphrase
//!   by PBKDF2-HMAC-SHA256 over a salt drawn for that file. Without the
//!   passphrase the file says nothing of the player, and one that has been
//!   altered does not open.
//!
//! A backup file is JSON:
//!
//! ```text
//! { "version": 1, "format": "kipukas-player-export",
//!   "salt": <16 bytes>, "iv": <12 bytes>,
//!   "data": <the ciphertext, then its 16-byte tag> }
//! ```
//!
//! with the bytes in URL-safe base64 without padding. This is the format the
//! site's page script wrote with the browser's Web Crypto before the engine
//! took the seal over: every backup a player already has opens here, and a
//! test below holds one made by that script.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// What a backup file says it is.
const FORMAT: &str = "kipukas-player-export";
const VERSION: u64 = 1;

/// How many rounds of PBKDF2 make the key. Part of the file format: files
/// do not say it, so it cannot change without a new [`VERSION`].
const ITERATIONS: u32 = 100_000;

/// Why a backup file did not open.
#[derive(Debug, PartialEq)]
pub enum Sealed {
    /// It is not a backup file of this site.
    NotABackup,
    /// It is one, of a version this engine does not read.
    Version(String),
    /// The passphrase is not the one it was sealed with, or the file has
    /// been altered. The two cannot be told apart.
    WrongOrAltered,
}

impl std::fmt::Display for Sealed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Sealed::NotABackup => write!(f, "Not a valid Kipukas backup file"),
            Sealed::Version(version) => write!(f, "Unsupported backup version: {}", version),
            Sealed::WrongOrAltered => {
                write!(f, "Decryption failed — wrong passphrase or file has been tampered with")
            }
        }
    }
}

/// The key a passphrase and a salt make: PBKDF2 (RFC 8018) with HMAC-SHA256,
/// one block of output.
fn derive_key(passphrase: &str, salt: &[u8], iterations: u32) -> [u8; 32] {
    // Keyed once; each round starts from a copy.
    let keyed = hmac_sha256::HMAC::new(passphrase.as_bytes());
    let mut first = keyed.clone();
    first.update(salt);
    first.update(1u32.to_be_bytes());
    let mut round = first.finalize();
    let mut key = round;
    for _ in 1..iterations {
        let mut next = keyed.clone();
        next.update(round);
        round = next.finalize();
        for (k, r) in key.iter_mut().zip(round) {
            *k ^= r;
        }
    }
    key
}

/// Bytes for a new file's salt and nonce, from the system (in the browser,
/// `crypto.getRandomValues`).
pub fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("No random numbers: {}", e))?;
    Ok(bytes)
}

/// Seal `text` under the passphrase: the contents of a backup file. `salt`
/// and `iv` must be new for every file ([`random_bytes`]).
pub fn seal(passphrase: &str, text: &str, salt: [u8; 16], iv: [u8; 12]) -> Result<String, String> {
    if passphrase.is_empty() {
        return Err("Passphrase must not be empty".to_string());
    }
    let key = derive_key(passphrase, &salt, ITERATIONS);
    let sealed = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key))
        .encrypt(Nonce::from_slice(&iv), text.as_bytes())
        .map_err(|_| "Could not encrypt the backup".to_string())?;
    Ok(serde_json::json!({
        "version": VERSION,
        "format": FORMAT,
        "salt": URL_SAFE_NO_PAD.encode(salt),
        "iv": URL_SAFE_NO_PAD.encode(iv),
        "data": URL_SAFE_NO_PAD.encode(sealed),
    })
    .to_string())
}

/// The bytes of one of a file's fields. Ordinary base64 and padding are
/// taken too, as the page script that read these files took them.
fn field(file: &serde_json::Value, name: &str) -> Result<Vec<u8>, Sealed> {
    let text = file[name].as_str().ok_or(Sealed::NotABackup)?;
    let url_safe: String = text
        .trim_end_matches('=')
        .chars()
        .map(|c| match c {
            '+' => '-',
            '/' => '_',
            other => other,
        })
        .collect();
    URL_SAFE_NO_PAD.decode(url_safe).map_err(|_| Sealed::NotABackup)
}

/// Open a backup file with the passphrase: the text that was sealed.
pub fn open(passphrase: &str, file: &str) -> Result<String, Sealed> {
    let file: serde_json::Value = serde_json::from_str(file).map_err(|_| Sealed::NotABackup)?;
    if file["format"] != FORMAT {
        return Err(Sealed::NotABackup);
    }
    if file["version"] != VERSION {
        return Err(Sealed::Version(file["version"].to_string()));
    }
    let (salt, iv, data) = (field(&file, "salt")?, field(&file, "iv")?, field(&file, "data")?);
    if iv.len() != 12 {
        return Err(Sealed::NotABackup);
    }
    let key = derive_key(passphrase, &salt, ITERATIONS);
    let text = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key))
        .decrypt(Nonce::from_slice(&iv), data.as_slice())
        .map_err(|_| Sealed::WrongOrAltered)?;
    String::from_utf8(text).map_err(|_| Sealed::WrongOrAltered)
}

/// Compute HMAC-SHA256 of `data` using `passphrase` as the key.
/// Returns the MAC as a lowercase hex string (64 chars).
///
/// # Errors
/// Returns `Err` if the passphrase is empty.
pub fn sign_export(passphrase: &str, data: &str) -> Result<String, String> {
    if passphrase.is_empty() {
        return Err("Passphrase must not be empty".to_string());
    }

    let mac = hmac_sha256::HMAC::mac(data.as_bytes(), passphrase.as_bytes());
    Ok(hex_encode(&mac))
}

/// Verify that `mac_hex` is a valid HMAC-SHA256 of `data` under `passphrase`.
/// Uses constant-time comparison to prevent timing attacks.
///
/// # Errors
/// Returns `Err` if the passphrase is empty or the MAC hex string is invalid.
pub fn verify_export(passphrase: &str, data: &str, mac_hex: &str) -> Result<bool, String> {
    if passphrase.is_empty() {
        return Err("Passphrase must not be empty".to_string());
    }

    let expected_bytes = hex_decode(mac_hex).map_err(|e| format!("Invalid MAC hex: {}", e))?;
    if expected_bytes.len() != 32 {
        return Err(format!(
            "Invalid MAC length: expected 32 bytes, got {}",
            expected_bytes.len()
        ));
    }

    let mut expected = [0u8; 32];
    expected.copy_from_slice(&expected_bytes);

    // Recompute HMAC and use constant-time comparison
    let computed = hmac_sha256::HMAC::mac(data.as_bytes(), passphrase.as_bytes());
    // Constant-time comparison to prevent timing attacks
    Ok(ct_eq(&computed, &expected))
}

/// Constant-time byte comparison to prevent timing attacks.
/// Returns true if `a` and `b` have identical contents.
fn ct_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff: u8 = 0;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Encode bytes as lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Decode a hex string into bytes.
fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("Odd-length hex string".to_string());
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[i..i + 2], 16)
            .map_err(|e| format!("Invalid hex at position {}: {}", i, e))?;
        bytes.push(byte);
    }
    Ok(bytes)
}

/// A backup made on 2026-11-18 by the site as it then was: the engine
/// signed the state and the page's script (kipukas-crypto.js, the browser's
/// Web Crypto) sealed it. Files like it are on players' devices.
#[cfg(test)]
pub(crate) mod made_by_the_page_script {
    pub(crate) const FILE: &str = r#"{"version":1,"format":"kipukas-player-export","salt":"5hhOvoxSshq9oewAxN-gvw","iv":"nvXL_FrqW6GbwWqc","data":"H85hKcUFdXK3LY-OMm9lLGYNMRHlMWcN8E3Xam1I5pBJ6ZoUQgjB9ccAwBSVNC1zEj5d3X4hv5u6m4SxA9RWp3X2ofiYsOcdWm88RIv5aTY1vVT95KhmYXpuI8hFsk59PIxXR8PnRZJQOrNCFSKa56ukjPd8gDqVEWI7uIGjkAV-YiOTKhH33nLPqZ2pN-Crl_kgjIXlTB2GJ4BISuuqNbmPoiOU0Z02d1QwvWzHt0QsELAjYPzAnulJmk8UMGRIfGLafn8ANrJKzQ0ZaB6rW1LSSep0hPVr3zku2IU2wvZS33JxDcoQ5Y23fbYrr-P3e98KtwXzKB6nJ--Yp6qaHOqbEBskKn4AHNg17UtX8kisTO2r5mUZPgzKdyuRqvdofgVQR6ZFwK4iNE6lY54djEnahhV4esoYWCTOQkfADFf-JlKejbYyz7y-FSfAIS7YiHT1LqBbTcoSB2NlTUWshfbhJ90_ZZ_sU1MDGPlvz9jdtXF9hbipBDSGbr_CXht0jpfjG9tf4H9yGbyQv9vEXxcyei7iJ6LAOcO3kU6-5tFFMbFMBFkRx8FS4Nrl6HfqHg59WBYyZEPqMqRR5JGuCC6bWZP7QunePv23kes-6_UHIrGY8kwuBfo4n72iAIZrUXSmyWqt8FryMzrKozpyQpWXIKIEdaxcvQrC3EU"}"#;
    pub(crate) const PASSPHRASE: &str = "correct horse battery";
}

#[cfg(test)]
mod tests {
    use super::made_by_the_page_script::{FILE as MADE_BY_THE_PAGE_SCRIPT, PASSPHRASE as ITS_PASSPHRASE};
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        hex_encode(bytes)
    }

    #[test]
    fn the_key_is_pbkdf2_with_hmac_sha256() {
        // RFC 7914, section 11, and the vectors in common use for this
        // function (RFC 6070's inputs with SHA-256): the first 32 bytes.
        assert_eq!(
            hex(&derive_key("passwd", b"salt", 1)),
            "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc"
        );
        assert_eq!(
            hex(&derive_key("Password", b"NaCl", 80_000)),
            "4ddcd8f60b98be21830cee5ef22701f9641a4418d04c0414aeff08876b34ab56"
        );
        assert_eq!(
            hex(&derive_key("password", b"salt", 2)),
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
        );
        assert_eq!(
            hex(&derive_key("password", b"salt", 4096)),
            "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a"
        );
    }

    #[test]
    fn what_is_sealed_opens_with_its_passphrase_and_only_that() {
        let file = seal("correct horse", "the player's state", [7; 16], [9; 12]).unwrap();
        assert_eq!(open("correct horse", &file), Ok("the player's state".to_string()));
        assert_eq!(open("wrong horse", &file), Err(Sealed::WrongOrAltered));
        assert_eq!(open("", &file), Err(Sealed::WrongOrAltered));
        assert!(seal("", "text", [7; 16], [9; 12]).is_err());

        // The file is the format's five fields and says nothing of what it holds.
        let fields: serde_json::Value = serde_json::from_str(&file).unwrap();
        let names: Vec<&String> = fields.as_object().unwrap().keys().collect();
        assert_eq!(names, ["data", "format", "iv", "salt", "version"]);
        assert_eq!(fields["version"], 1);
        assert_eq!(fields["format"], "kipukas-player-export");
        assert_eq!(fields["salt"], "BwcHBwcHBwcHBwcHBwcHBw");
        assert_eq!(fields["iv"], "CQkJCQkJCQkJCQkJ");
        assert!(!file.contains("state"), "{}", file);
        // 18 bytes of text and the 16-byte tag.
        assert_eq!(URL_SAFE_NO_PAD.decode(fields["data"].as_str().unwrap()).unwrap().len(), 18 + 16);
    }

    #[test]
    fn a_file_that_has_been_altered_does_not_open() {
        let file = seal("correct horse", "the player's state", [7; 16], [9; 12]).unwrap();
        let mut fields: serde_json::Value = serde_json::from_str(&file).unwrap();
        for name in ["data", "iv", "salt"] {
            let was = fields[name].as_str().unwrap().to_string();
            // Change one character of the field.
            let changed: String =
                was.chars().enumerate().map(|(i, c)| if i == 3 { if c == 'A' { 'B' } else { 'A' } } else { c }).collect();
            fields[name] = serde_json::json!(changed);
            assert_eq!(open("correct horse", &fields.to_string()), Err(Sealed::WrongOrAltered), "{}", name);
            fields[name] = serde_json::json!(was);
        }
        assert_eq!(open("correct horse", &fields.to_string()), Ok("the player's state".to_string()));
    }

    #[test]
    fn what_is_not_a_backup_file_is_said_to_be_none() {
        let file = seal("correct horse", "text", [7; 16], [9; 12]).unwrap();
        let with = |name: &str, value: serde_json::Value| {
            let mut fields: serde_json::Value = serde_json::from_str(&file).unwrap();
            fields[name] = value;
            fields.to_string()
        };
        for not_one in [
            String::new(),
            "not json".to_string(),
            "[1, 2]".to_string(),
            "{}".to_string(),
            with("format", serde_json::json!("something-else")),
            with("salt", serde_json::Value::Null),
            with("iv", serde_json::json!("!!")),
            with("iv", serde_json::json!("CQkJ")),
            with("data", serde_json::json!(12)),
        ] {
            assert_eq!(open("correct horse", &not_one), Err(Sealed::NotABackup), "{}", not_one);
        }
        assert_eq!(open("correct horse", &with("version", serde_json::json!(2))), Err(Sealed::Version("2".to_string())));
        assert_eq!(Sealed::Version("2".to_string()).to_string(), "Unsupported backup version: 2");
    }

    #[test]
    fn ordinary_base64_with_padding_is_read_too() {
        let file = seal("correct horse", "the player's state?>", [0xfb; 16], [0xff; 12]).unwrap();
        let mut fields: serde_json::Value = serde_json::from_str(&file).unwrap();
        for name in ["salt", "iv", "data"] {
            let bytes = URL_SAFE_NO_PAD.decode(fields[name].as_str().unwrap()).unwrap();
            fields[name] = serde_json::json!(base64::engine::general_purpose::STANDARD.encode(bytes));
        }
        assert!(fields["salt"].as_str().unwrap().contains('+') && fields["salt"].as_str().unwrap().ends_with('='));
        assert_eq!(open("correct horse", &fields.to_string()), Ok("the player's state?>".to_string()));
    }

    #[test]
    fn two_draws_of_random_bytes_differ() {
        let (a, b): ([u8; 16], [u8; 16]) = (random_bytes().unwrap(), random_bytes().unwrap());
        assert_ne!(a, b);
    }

    #[test]
    fn a_backup_sealed_by_the_page_script_opens() {
        let inner = open(ITS_PASSPHRASE, MADE_BY_THE_PAGE_SCRIPT).unwrap();
        assert_eq!(inner, r#"{"player_doc":"AQ7Q3vHBAwAoAQhzZXR0aW5ncwtzaG93X2FsYXJtcwF4JwEFY2FyZHMQYnJveF90aGVfZGVmaWFudAEoANDe8cEDAQZ3YXN0ZWQBeScA0N7xwQMBBXNsb3RzAAgA0N7xwQMDAXmB0N7xwQMEAYjQ3vHBAwUBecjQ3vHBAwXQ3vHBAwYBeAcBBmFsYXJtcwEoANDe8cEDCAlyZW1haW5pbmcBfQcoANDe8cEDCAljb2xvcl9zZXQBdwRibHVlKADQ3vHBAwgHY2hhbmdlZAF9mufAhb5oKADQ3vHBAwgEbmFtZQF3B2ZpeHR1cmUoANDe8cEDCAJpZAF3DDZiYjE2ZmU5MjQ3MAHQ3vHBAwEFAQ","mac":"9bd308a81942b33580fb4133fe9a332e7b8c7378f3e48193cbfdd03ef156e629","exported_at":"2026-11-18T20:15:30.123Z"}"#);
        // And what is in it is signed with the same passphrase.
        let signed: serde_json::Value = serde_json::from_str(&inner).unwrap();
        assert_eq!(
            verify_export(ITS_PASSPHRASE, signed["player_doc"].as_str().unwrap(), signed["mac"].as_str().unwrap()),
            Ok(true)
        );
        assert_eq!(open("another passphrase", MADE_BY_THE_PAGE_SCRIPT), Err(Sealed::WrongOrAltered));
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let mac = sign_export("my-secret", "hello world").unwrap();
        assert_eq!(mac.len(), 64); // 32 bytes = 64 hex chars
        let valid = verify_export("my-secret", "hello world", &mac).unwrap();
        assert!(valid);
    }

    #[test]
    fn verify_rejects_wrong_passphrase() {
        let mac = sign_export("correct-passphrase", "data").unwrap();
        let valid = verify_export("wrong-passphrase", "data", &mac).unwrap();
        assert!(!valid);
    }

    #[test]
    fn verify_rejects_tampered_data() {
        let mac = sign_export("secret", "original data").unwrap();
        let valid = verify_export("secret", "tampered data", &mac).unwrap();
        assert!(!valid);
    }

    #[test]
    fn sign_rejects_empty_passphrase() {
        let result = sign_export("", "data");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn verify_rejects_empty_passphrase() {
        let result = verify_export("", "data", "aabbccdd");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn verify_rejects_invalid_hex_mac() {
        let result = verify_export("secret", "data", "not-valid-hex!");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid"));
    }

    #[test]
    fn verify_rejects_wrong_length_mac() {
        // Valid hex but too short (only 4 bytes instead of 32)
        let result = verify_export("secret", "data", "aabbccdd");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("length"));
    }

    #[test]
    fn sign_produces_consistent_results() {
        let mac1 = sign_export("key", "data").unwrap();
        let mac2 = sign_export("key", "data").unwrap();
        assert_eq!(mac1, mac2);
    }

    #[test]
    fn different_data_produces_different_macs() {
        let mac1 = sign_export("key", "data1").unwrap();
        let mac2 = sign_export("key", "data2").unwrap();
        assert_ne!(mac1, mac2);
    }

    #[test]
    fn different_keys_produce_different_macs() {
        let mac1 = sign_export("key1", "data").unwrap();
        let mac2 = sign_export("key2", "data").unwrap();
        assert_ne!(mac1, mac2);
    }
}
