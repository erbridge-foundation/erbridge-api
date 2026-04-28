use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use anyhow::{Result, anyhow};
use rand::Rng as _;
use sha2::Digest as _;
use subtle::ConstantTimeEq as _;

/// Encrypts `plaintext` with AES-256-GCM using `key`.
///
/// Output format: `[12-byte nonce][ciphertext]`.
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(key.into());

    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| anyhow!("encryption failed: {e}"))?;

    let mut output = Vec::with_capacity(12 + ciphertext.len());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Decrypts a value produced by [`encrypt`].
///
/// Expects `[12-byte nonce][ciphertext]`. Always propagates errors — never
/// returns an empty vec on failure.
pub fn decrypt(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 12 {
        return Err(anyhow!("ciphertext too short"));
    }

    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new(key.into());

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow!("decryption failed: {e}"))
}

/// Generates a new API key: `erbridge_<32 lowercase hex chars>` (128 bits of entropy).
/// Output is always exactly 41 chars. The returned string is the plaintext — hash it
/// with `sha256_hex` before storing.
pub fn generate_api_key() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("erbridge_{hex}")
}

/// Returns the lowercase hex-encoded SHA-256 digest of `input`.
/// Used to hash API keys before storage — never store the plaintext.
pub fn sha256_hex(input: &[u8]) -> String {
    let hash = sha2::Sha256::digest(input);
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares two byte slices in constant time.
///
/// Use this wherever secret bytes must be compared. Never use `==` on token
/// or key bytes — timing side-channels can leak secret values.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> [u8; 32] {
        [0x42u8; 32]
    }

    #[test]
    fn round_trip() {
        let key = test_key();
        let plaintext = b"hello, erbridge";
        let ciphertext = encrypt(&key, plaintext).unwrap();
        let decrypted = decrypt(&key, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn different_nonces_produce_different_ciphertexts() {
        let key = test_key();
        let plaintext = b"same plaintext";
        let ct1 = encrypt(&key, plaintext).unwrap();
        let ct2 = encrypt(&key, plaintext).unwrap();
        // Nonces are random; ciphertexts should differ.
        assert_ne!(ct1, ct2);
    }

    #[test]
    fn wrong_key_fails() {
        let key = test_key();
        let wrong_key = [0x99u8; 32];
        let ciphertext = encrypt(&key, b"secret").unwrap();
        assert!(decrypt(&wrong_key, &ciphertext).is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = test_key();
        let mut ciphertext = encrypt(&key, b"secret").unwrap();
        // Flip a byte in the ciphertext (after the 12-byte nonce).
        ciphertext[20] ^= 0xff;
        assert!(decrypt(&key, &ciphertext).is_err());
    }

    #[test]
    fn too_short_fails() {
        let key = test_key();
        assert!(decrypt(&key, &[0u8; 11]).is_err());
    }

    #[test]
    fn empty_plaintext_round_trip() {
        let key = test_key();
        let ct = encrypt(&key, b"").unwrap();
        let pt = decrypt(&key, &ct).unwrap();
        assert_eq!(pt, b"");
    }

    #[test]
    fn constant_time_eq_equal_slices() {
        assert!(constant_time_eq(b"token123", b"token123"));
    }

    #[test]
    fn constant_time_eq_different_slices() {
        assert!(!constant_time_eq(b"token123", b"token456"));
    }

    #[test]
    fn constant_time_eq_different_lengths() {
        assert!(!constant_time_eq(b"short", b"longer_value"));
    }

    #[test]
    fn constant_time_eq_empty_slices() {
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn constant_time_eq_one_empty() {
        assert!(!constant_time_eq(b"", b"x"));
    }

    #[test]
    fn generate_api_key_has_correct_prefix_and_length() {
        let key = generate_api_key();
        assert!(key.starts_with("erbridge_"));
        assert_eq!(key.len(), 41);
    }

    #[test]
    fn generate_api_key_suffix_is_lowercase_hex() {
        let key = generate_api_key();
        let suffix = key.strip_prefix("erbridge_").unwrap();
        assert_eq!(suffix.len(), 32);
        assert!(
            suffix
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn generate_api_key_produces_unique_values() {
        let a = generate_api_key();
        let b = generate_api_key();
        assert_ne!(a, b);
    }

    #[test]
    fn sha256_hex_empty_input() {
        let h = sha256_hex(b"");
        assert_eq!(h.len(), 64);
        assert!(h.starts_with("e3b0c44298fc1c149afb"));
    }
}
