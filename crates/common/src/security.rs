//! Security helpers shared by Kival crates.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use thiserror::Error;

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
