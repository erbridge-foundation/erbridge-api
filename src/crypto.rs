use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use anyhow::{Result, anyhow};
use rand::Rng as _;

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
}
