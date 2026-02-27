//! Cryptographic signing for tamper-resistant player data exports.
//!
//! Uses HMAC-SHA256 with a user-chosen passphrase to sign the PLAYER_DOC
//! base64 data. The HMAC prevents editing the exported blob without
//! knowledge of the passphrase — modifying data or using the wrong
//! passphrase causes verification to fail.
//!
//! This is the Rust-owned integrity layer. An optional JS-side AES-GCM
//! encryption layer (via Web Crypto API) provides confidentiality on top.

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

#[cfg(test)]
mod tests {
    use super::*;

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
