//! Content references for stored blobs.

use std::str::FromStr;

use sha2::{Digest, Sha256};

use crate::BlobStoreError;

/// A stable SHA-256 content reference for a blob.
///
/// The textual representation is exactly 64 lowercase hexadecimal characters.
/// `BlobRef` is content identity, not an authorization token. Possession of a
/// reference must not be treated as permission to read content.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct BlobRef {
    /// Raw SHA-256 digest bytes.
    digest: [u8; 32],
}

impl BlobRef {
    /// Construct a reference from raw SHA-256 digest bytes.
    #[must_use]
    pub const fn from_digest(digest: [u8; 32]) -> Self {
        Self { digest }
    }

    /// Construct a reference by hashing original blob bytes with SHA-256.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_digest(Sha256::digest(bytes).into())
    }

    /// Return the raw digest bytes.
    #[must_use]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

impl std::fmt::Debug for BlobRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

impl std::fmt::Display for BlobRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut encoded = [0_u8; 64];
        hex::encode_to_slice(self.digest, &mut encoded).map_err(|_error| std::fmt::Error)?;
        let encoded = str::from_utf8(&encoded).map_err(|_error| std::fmt::Error)?;

        formatter.write_str(encoded)
    }
}

impl FromStr for BlobRef {
    type Err = BlobStoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 64 {
            return Err(BlobStoreError::InvalidBlobRefLength { expected: 64, actual: value.len() });
        }

        let mut digest = [0_u8; 32];
        hex::decode_to_slice(value, &mut digest)
            .map_err(|_error| BlobStoreError::InvalidDigestHex)?;

        Ok(Self { digest })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{BlobRef, BlobStoreError};

    #[test]
    fn same_bytes_produce_same_ref() {
        assert_eq!(BlobRef::from_bytes(b"same"), BlobRef::from_bytes(b"same"));
    }

    #[test]
    fn different_bytes_produce_different_refs() {
        assert_ne!(BlobRef::from_bytes(b"left"), BlobRef::from_bytes(b"right"));
    }

    #[test]
    fn bytes_are_hashed_with_sha256() {
        assert_eq!(
            BlobRef::from_bytes(b"hello").to_string(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        );
    }

    #[test]
    fn from_digest_preserves_digest() {
        let digest = [7_u8; 32];
        let reference = BlobRef::from_digest(digest);

        assert_eq!(reference.digest(), &digest);
        assert_eq!(reference.to_string(), "07".repeat(32));
    }

    #[test]
    fn display_from_str_roundtrip() {
        let reference = BlobRef::from_bytes(b"hello");
        let parsed = BlobRef::from_str(&reference.to_string()).expect("valid ref");

        assert_eq!(parsed, reference);
        assert_eq!(parsed.digest(), reference.digest());
    }

    #[test]
    fn debug_matches_display() {
        let reference = BlobRef::from_bytes(b"hello");

        assert_eq!(format!("{reference:?}"), reference.to_string());
    }

    #[test]
    fn display_is_canonical_lowercase() {
        let reference = BlobRef::from_bytes(b"hello");
        let text = reference.to_string();

        assert_eq!(text.len(), 64);
        assert!(
            text.chars().all(|character| {
                character.is_ascii_digit() || ('a'..='f').contains(&character)
            })
        );
    }

    #[test]
    fn empty_digest_is_rejected() {
        assert!(matches!(
            BlobRef::from_str(""),
            Err(BlobStoreError::InvalidBlobRefLength { expected: 64, actual: 0 })
        ));
    }

    #[test]
    fn short_digest_is_rejected() {
        assert!(matches!(
            BlobRef::from_str("abc"),
            Err(BlobStoreError::InvalidBlobRefLength { expected: 64, actual: 3 })
        ));
    }

    #[test]
    fn long_digest_is_rejected() {
        let value = "0".repeat(65);

        assert!(matches!(
            BlobRef::from_str(&value),
            Err(BlobStoreError::InvalidBlobRefLength { expected: 64, actual: 65 })
        ));
    }

    #[test]
    fn invalid_hex_is_rejected() {
        assert!(matches!(
            BlobRef::from_str("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"),
            Err(BlobStoreError::InvalidDigestHex)
        ));
    }

    #[test]
    fn uppercase_hex_is_accepted_and_displayed_lowercase() {
        let lower = BlobRef::from_bytes(b"hello").to_string();
        let upper_digest = lower.to_uppercase();
        let parsed = BlobRef::from_str(&upper_digest).expect("valid ref");

        assert_eq!(parsed.to_string(), lower);
    }
}
