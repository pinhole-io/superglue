//! Seal a proxy upstream API key with the gateway master key.
//!
//! The cipher key is SHA-256 of the master key. The nonce is random and stored
//! beside the ciphertext. Callers must not log the plaintext or the header.

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use secrecy::SecretString;
use sha2::{Digest, Sha256};

use crate::gateway::error::{GatewayError, GatewayResult};

const NONCE_LEN: usize = 12;

/// Ciphertext and the nonce required to open it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedKey {
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

/// Encrypt `plaintext` for storage. An empty master key or empty secret is rejected.
pub fn seal_api_key(master_key: &str, plaintext: &str) -> GatewayResult<SealedKey> {
    if master_key.is_empty() {
        return Err(GatewayError::bad_request(
            "master key is required to store a proxy API key",
        ));
    }
    if plaintext.is_empty() {
        return Err(GatewayError::bad_request("proxy API key is empty"));
    }
    let cipher = cipher(master_key);
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|_| GatewayError::Internal("failed to encrypt proxy API key".into()))?;
    Ok(SealedKey {
        nonce: nonce.to_vec(),
        ciphertext,
    })
}

/// Decrypt a stored proxy API key. The error text does not include the secret.
pub fn open_api_key(
    master_key: &str,
    nonce: &[u8],
    ciphertext: &[u8],
) -> GatewayResult<SecretString> {
    if master_key.is_empty() {
        return Err(GatewayError::Internal(
            "master key is required to read a proxy API key".into(),
        ));
    }
    if nonce.len() != NONCE_LEN {
        return Err(GatewayError::Internal(
            "proxy API key nonce is invalid".into(),
        ));
    }
    let cipher = cipher(master_key);
    let nonce = Nonce::from_slice(nonce);
    let plain = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| GatewayError::Internal("failed to decrypt proxy API key".into()))?;
    let text = String::from_utf8(plain)
        .map_err(|_| GatewayError::Internal("proxy API key is not valid text".into()))?;
    Ok(SecretString::from(text))
}

/// Last four characters, prefixed when the secret is longer. Matches provider status.
#[must_use]
pub fn key_suffix(secret: &str) -> String {
    if secret.len() <= 4 {
        secret.to_string()
    } else {
        format!("...{}", &secret[secret.len() - 4..])
    }
}

fn cipher(master_key: &str) -> ChaCha20Poly1305 {
    let digest = Sha256::digest(master_key.as_bytes());
    let key = Key::from_slice(digest.as_slice());
    ChaCha20Poly1305::new(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn seal_round_trip_hides_the_plaintext() {
        let sealed = seal_api_key("master-secret", "sk-proxy-token").unwrap();
        assert!(!sealed.ciphertext.windows(4).any(|w| w == b"sk-p"));
        let opened = open_api_key("master-secret", &sealed.nonce, &sealed.ciphertext).unwrap();
        assert_eq!(opened.expose_secret(), "sk-proxy-token");
    }

    #[test]
    fn wrong_master_key_does_not_open() {
        let sealed = seal_api_key("master-secret", "sk-proxy-token").unwrap();
        let err = open_api_key("other-secret", &sealed.nonce, &sealed.ciphertext).unwrap_err();
        assert!(!err.to_string().contains("sk-proxy"));
    }

    #[test]
    fn suffix_keeps_four_characters() {
        assert_eq!(key_suffix("abcd"), "abcd");
        assert_eq!(key_suffix("sk-proxy-token"), "...oken");
    }
}
