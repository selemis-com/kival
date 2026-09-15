//! Security helpers shared by Kival crates.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Prefix applied to generated API key credentials.
pub const API_KEY_PREFIX: &str = "kvl_";
/// Number of random bytes used for bearer credentials and one-time capabilities.
pub const SECRET_TOKEN_BYTES: usize = 32;
/// Security helper result type.
pub type Result<T> = std::result::Result<T, SecurityError>;

/// Security helper errors.
#[derive(Debug, Clone, Copy, Error)]
pub enum SecurityError {
    /// Random byte generation failed.
    #[error("random generation failed: {0}")]
    Random(#[from] getrandom::Error),
}

/// Newly generated API-key credential.
///
/// `Debug` output is intentionally opaque because this value contains credential material.
#[must_use]
pub struct GeneratedApiKey {
    /// Plaintext bearer token shown to the caller once.
    pub token: String,
    /// Fixed-size verifier persisted for authentication.
    pub token_hash: [u8; 32],
}

impl std::fmt::Debug for GeneratedApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeneratedApiKey").finish_non_exhaustive()
    }
}

/// Generates a Kival API key and its stored verifier.
///
/// # Errors
///
/// Returns an error if the operating-system random source fails.
pub fn generate_api_key() -> Result<GeneratedApiKey> {
    let token = format!("{API_KEY_PREFIX}{}", generate_secret_token()?);
    let token_hash = hash_token(&token);
    Ok(GeneratedApiKey { token, token_hash })
}

/// Generates a 256-bit unpadded base64url authentication secret.
///
/// # Errors
///
/// Returns an error if the operating-system random source fails.
pub fn generate_secret_token() -> Result<String> {
    let mut token = [0_u8; SECRET_TOKEN_BYTES];
    getrandom::fill(&mut token)?;
    Ok(URL_SAFE_NO_PAD.encode(token))
}

/// Derives the fixed-size verifier stored for a bearer credential or capability.
#[must_use]
pub fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_api_key_contains_matching_verifier() {
        let api_key = generate_api_key().expect("API key generation should succeed");

        assert!(api_key.token.starts_with(API_KEY_PREFIX));
        assert_eq!(api_key.token_hash, hash_token(&api_key.token));
    }
}
