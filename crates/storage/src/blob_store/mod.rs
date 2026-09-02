//! Content-addressed storage of opaque byte blobs.

mod blob_ref;
mod error;
mod store;

pub use blob_ref::BlobRef;
pub use error::BlobStoreError;
pub use store::{BlobMetadata, BlobStore, BlobStream, VerifyResult};
