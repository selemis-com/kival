//! Error types for blob storage operations.

use std::{error::Error as StdError, io, path::PathBuf};

use thiserror::Error;

use crate::{BlobRef, VerifyResult};

/// Errors returned by blob reference parsing and store operations.
#[derive(Debug, Error)]
pub enum BlobStoreError {
    /// A blob reference has the wrong encoded length.
    #[error("invalid BlobRef length: expected {expected} hex chars, got {actual}")]
    InvalidBlobRefLength {
        /// Expected hex character count.
        expected: usize,
        /// Actual hex character count.
        actual: usize,
    },

    /// A blob reference is not valid hexadecimal.
    #[error("invalid digest hex")]
    InvalidDigestHex,

    /// A streaming blob exceeded the configured ingestion limit.
    #[error("blob exceeds the configured size limit of {limit} bytes")]
    SizeLimitExceeded {
        /// Maximum accepted blob size in bytes.
        limit: u64,
    },

    /// Reading bytes from a caller-provided blob stream failed.
    #[error("blob input stream failed: {source}")]
    Input {
        /// Original stream error.
        #[source]
        source: Box<dyn StdError + Send + Sync>,
    },

    /// Local filesystem initialization failed.
    #[error("filesystem I/O error at {path}: {source}")]
    Io {
        /// Filesystem path involved in the failed operation.
        path: PathBuf,
        /// Original I/O error.
        #[source]
        source: io::Error,
    },

    /// Generating a unique temporary object key failed.
    #[error("failed to generate temporary object key: {source}")]
    Random {
        /// Randomness provider error.
        #[source]
        source: getrandom::Error,
    },

    /// The underlying object-store backend failed.
    #[error("object store error: {source}")]
    ObjectStore {
        /// Original backend error.
        #[source]
        source: object_store::Error,
    },

    /// A content-addressed key already exists with mismatching bytes.
    #[error("corrupt existing blob: {reference}")]
    CorruptExistingBlob {
        /// Reference whose object contains mismatching bytes.
        reference: BlobRef,
    },

    /// Commit repeatedly raced with concurrent erase.
    #[error("commit race exhausted for blob: {reference}")]
    CommitRaceExhausted {
        /// Reference that could not be committed after bounded retries.
        reference: BlobRef,
    },

    /// A verified put wrote a blob but read-back verification did not validate it.
    #[error("put verification failed for blob {reference}: {result:?}")]
    PutVerificationFailed {
        /// Reference returned by the write step.
        reference: BlobRef,
        /// Verification result observed after the write step.
        result: VerifyResult,
    },
}

impl BlobStoreError {
    /// Wrap one backend error in the storage error type.
    pub(crate) const fn object_store(source: object_store::Error) -> Self {
        Self::ObjectStore { source }
    }
}
