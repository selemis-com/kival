//! Errors produced by Kival's `PostgreSQL` state bindings.

use sqlx::migrate::MigrateError;
use thiserror::Error;

/// Result type for Kival kernel operations.
pub type Result<T> = std::result::Result<T, KernelError>;

/// Errors produced while opening, migrating, or operating on Kival state.
#[derive(Debug, Error)]
pub enum KernelError {
    /// `PostgreSQL` operation failed.
    #[error("database error: {0}")]
    Database(#[source] sqlx::Error),

    /// `PostgreSQL` schema migration failed.
    #[error("PostgreSQL migration error: {source}")]
    Migrate {
        /// Underlying migration failure.
        #[source]
        source: MigrateError,
    },

    /// Stored constrained vocabulary no longer matches the kernel bindings.
    #[error("invalid stored {kind}: {value}")]
    InvalidStoredValue {
        /// Vocabulary whose stored representation was invalid.
        kind: &'static str,
        /// Unexpected stored value.
        value: String,
    },

    /// Target resource does not exist in the lifecycle state required by the operation.
    #[error("resource not found")]
    ResourceNotFound,

    /// Actor does not hold the capability required by the operation.
    #[error("required capability not held")]
    CapabilityRequired,

    /// Attachment version does not belong to the target object.
    #[error("attachment version must belong to target object")]
    InvalidAttachmentVersion,

    /// Object grant user principal is not an active workspace member.
    #[error("object grant user principal must be an active workspace member")]
    InvalidObjectGrantUserPrincipal,

    /// Object grant group principal is not actively linked to the workspace.
    #[error("object grant group principal must be linked to the workspace")]
    InvalidObjectGrantGroupPrincipal,

    /// Object has no current immutable version to update from.
    #[error("object has no current version")]
    ObjectHasNoCurrentVersion,

    /// Object current version no longer matches the caller's optimistic expectation.
    #[error("object changed since the expected version")]
    ObjectVersionConflict,

    /// Object grant transition would remove the final active administrator.
    #[error("object must retain at least one active administrator grant")]
    ObjectMustRetainAdminGrant,
}

impl From<sqlx::Error> for KernelError {
    fn from(error: sqlx::Error) -> Self {
        if let sqlx::Error::Database(database_error) = &error {
            match database_error.code().as_deref() {
                Some("KRNFD") => return Self::ResourceNotFound,
                Some("KCAPR") => return Self::CapabilityRequired,
                _ => {}
            }
        }

        Self::Database(error)
    }
}
