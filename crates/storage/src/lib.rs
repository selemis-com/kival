//! Storage infrastructure for Kival.

#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod blob_store;

pub use blob_store::{BlobMetadata, BlobRef, BlobStore, BlobStoreError, BlobStream, VerifyResult};
