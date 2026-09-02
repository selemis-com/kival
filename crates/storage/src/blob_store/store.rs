//! Backend-agnostic content-addressed blob storage.

use std::{
    error::Error as StdError,
    fmt,
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Instant,
};

use bytes::Bytes;
use futures_util::{Stream, StreamExt, stream::BoxStream};
use kival_metrics::{
    counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram,
};
use object_store::{
    DynObjectStore, ObjectStoreExt, PutMode, PutPayload, PutPayloadMut, local::LocalFileSystem,
    path::Path,
};
use sha2::{Digest as _, Sha256};

use crate::blob_store::{BlobRef, BlobStoreError};

/// Maximum attempts to commit after racing with concurrent erase.
const MAX_COMMIT_RACE_ATTEMPTS: usize = 16;
/// Fixed multipart part size accepted by all supported backends (5 MiB).
const MULTIPART_CHUNK_SIZE: usize = 5 * 1024 * 1024;

/// Stable physical metadata for a stored blob.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlobMetadata {
    /// Content reference whose object was inspected.
    pub reference: BlobRef,
    /// Length of the stored bytes.
    pub len: u64,
}

/// Result of checking stored bytes against a content reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifyResult {
    /// No object exists for the requested reference.
    Missing,
    /// The object exists and hashes to the requested reference.
    Valid,
    /// The object exists but hashes to a different reference.
    Corrupt,
}

/// Content-addressed blob storage over an [`object_store`] backend.
///
/// Kival owns blob identity, verification, ingestion limits, and metrics. The
/// wrapped backend owns physical storage and may be a local filesystem or another
/// [`object_store::ObjectStore`] implementation.
#[derive(Clone, Debug)]
pub struct BlobStore {
    /// Physical object-storage implementation.
    backend: Arc<DynObjectStore>,
}

impl BlobStore {
    /// Wrap an arbitrary [`object_store::ObjectStore`] backend.
    #[must_use]
    pub fn new(backend: Arc<DynObjectStore>) -> Self {
        Self { backend }
    }

    /// Open a durable local-filesystem backend rooted at `root`.
    ///
    /// Local writes are fsynced before success is reported. Empty directories
    /// left after deletes are cleaned up automatically.
    ///
    /// # Errors
    ///
    /// Returns an error if the root cannot be created or initialized as a local
    /// object store.
    pub fn filesystem(root: impl Into<PathBuf>) -> Result<Self, BlobStoreError> {
        let root = root.into();
        std::fs::create_dir_all(&root)
            .map_err(|source| BlobStoreError::Io { path: root.clone(), source })?;

        let backend = LocalFileSystem::new_with_prefix(&root)
            .map_err(BlobStoreError::object_store)?
            .with_fsync(true)
            .with_automatic_cleanup(true);

        Ok(Self::new(Arc::new(backend)))
    }

    /// Store original bytes and return their stable content reference.
    ///
    /// Repeating `put` with identical bytes is idempotent. If an object already
    /// occupies the content-addressed key but does not match its digest, the
    /// corruption is reported rather than overwritten.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend fails or an existing object is corrupt.
    pub async fn put(&self, bytes: Bytes) -> Result<BlobRef, BlobStoreError> {
        let reference = BlobRef::from_bytes(&bytes);
        let bytes_len = bytes.len() as u64;
        let started_at = Instant::now();
        let result = self.put_payload(reference, bytes.into()).await;

        match &result {
            Ok(()) => {
                record_blob_operation("put", "completed", started_at);
                record_blob_bytes("put", "completed", bytes_len);
            }
            Err(_) => record_blob_operation("put", "error", started_at),
        }

        result.map(|()| reference)
    }

    /// Store original bytes, then verify the committed object by reading it back.
    ///
    /// # Errors
    ///
    /// Returns an error if the write or verification read fails, or if the
    /// committed object does not match its content reference.
    pub async fn put_verified(&self, bytes: Bytes) -> Result<BlobRef, BlobStoreError> {
        let reference = self.put(bytes).await?;

        match self.verify(&reference).await? {
            VerifyResult::Valid => Ok(reference),
            result => Err(BlobStoreError::PutVerificationFailed { reference, result }),
        }
    }

    /// Store a bounded asynchronous stream of byte chunks.
    ///
    /// Inputs smaller than 5 MiB are buffered and committed directly. Larger
    /// inputs are streamed in fixed 5 MiB multipart parts to a temporary object,
    /// then promoted to the digest-derived key once the hash is known. Owned
    /// [`Bytes`] chunks are appended without copying where their boundaries permit.
    /// Memory usage therefore stays bounded independently of `max_len`.
    ///
    /// # Errors
    ///
    /// Returns an error if the input stream fails, exceeds `max_len`, random
    /// staging-key generation fails, the backend fails, or the final key is
    /// already occupied by corrupt data.
    pub async fn put_stream<S, E>(
        &self,
        stream: S,
        max_len: u64,
    ) -> Result<BlobMetadata, BlobStoreError>
    where
        S: Stream<Item = Result<Bytes, E>> + Send,
        E: StdError + Send + Sync + 'static,
    {
        let mut metrics = BlobStreamMetrics::start("put_stream");
        let result = async {
            let mut hasher = Sha256::new();
            let mut payload = PutPayloadMut::new();
            let mut staging = None;
            let mut len = 0_u64;
            let mut stream = Box::pin(stream);

            while let Some(chunk) = stream.next().await {
                let mut chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(source) => {
                        abort_staged_upload(staging.take()).await;
                        return Err(BlobStoreError::Input { source: Box::new(source) });
                    }
                };

                if chunk.is_empty() {
                    continue;
                }

                let next_len = match len.checked_add(chunk.len() as u64) {
                    Some(next_len) if next_len <= max_len => next_len,
                    _ => {
                        abort_staged_upload(staging.take()).await;
                        return Err(BlobStoreError::SizeLimitExceeded { limit: max_len });
                    }
                };

                hasher.update(&chunk);
                metrics.add_bytes(chunk.len() as u64);
                len = next_len;

                while !chunk.is_empty() {
                    let remaining = MULTIPART_CHUNK_SIZE - payload.content_length();
                    if chunk.len() < remaining {
                        payload.push(chunk);
                        break;
                    }

                    payload.push(chunk.split_to(remaining));
                    let part = std::mem::take(&mut payload).into();
                    match staging.as_mut() {
                        Some((_, upload)) => {
                            if let Err(error) = upload.put_part(part).await {
                                abort_staged_upload(staging.take()).await;
                                return Err(BlobStoreError::object_store(error));
                            }
                        }
                        None => {
                            let path = temporary_blob_path()?;
                            let mut upload = self
                                .backend
                                .put_multipart(&path)
                                .await
                                .map_err(BlobStoreError::object_store)?;
                            if let Err(error) = upload.put_part(part).await {
                                let _ = upload.abort().await;
                                return Err(BlobStoreError::object_store(error));
                            }
                            staging = Some((path, upload));
                        }
                    }
                }
            }

            let reference = BlobRef::from_digest(hasher.finalize().into());

            match staging {
                None => self.put_payload(reference, payload.freeze()).await?,
                Some((temporary_path, mut upload)) => {
                    if !payload.is_empty()
                        && let Err(error) = upload.put_part(payload.freeze()).await
                    {
                        let _ = upload.abort().await;
                        return Err(BlobStoreError::object_store(error));
                    }

                    if let Err(error) = upload.complete().await {
                        let _ = upload.abort().await;
                        return Err(BlobStoreError::object_store(error));
                    }

                    let promotion = self.promote_staged(&temporary_path, reference).await;
                    let _ = self.backend.delete(&temporary_path).await;
                    promotion?;
                }
            }

            Ok(BlobMetadata { reference, len })
        }
        .await;

        let outcome = match &result {
            Ok(_) => "completed",
            Err(BlobStoreError::SizeLimitExceeded { .. }) => "size_limit",
            Err(_) => "error",
        };
        metrics.finish(outcome);

        result
    }

    /// Buffer all bytes for a reference, if present.
    ///
    /// `get_bytes` is a convenience for callers that explicitly need the whole
    /// object in memory. Streaming consumers should prefer [`Self::get`]. Neither
    /// read path verifies the digest automatically; use [`Self::verify`] when
    /// corruption detection is required.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured backend fails.
    pub async fn get_bytes(&self, reference: &BlobRef) -> Result<Option<Bytes>, BlobStoreError> {
        let started_at = Instant::now();
        let path = blob_path(reference);
        let result = match self.backend.get(&path).await {
            Ok(result) => result.bytes().await.map(Some).map_err(BlobStoreError::object_store),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(BlobStoreError::object_store(error)),
        };

        match &result {
            Ok(Some(bytes)) => {
                record_blob_operation("get_bytes", "hit", started_at);
                record_blob_bytes("get_bytes", "hit", bytes.len() as u64);
            }
            Ok(None) => record_blob_operation("get_bytes", "miss", started_at),
            Err(_) => record_blob_operation("get_bytes", "error", started_at),
        }

        result
    }

    /// Get a blob as a stream of byte chunks, if present.
    ///
    /// The returned stream preserves the backend's native chunked read model
    /// while mapping backend failures into [`BlobStoreError`]. Use
    /// [`Self::get_bytes`] only when the caller explicitly needs a buffered read.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured backend fails while opening the object.
    pub async fn get(
        &self,
        reference: &BlobRef,
    ) -> Result<Option<(BlobStream, BlobMetadata)>, BlobStoreError> {
        let started_at = Instant::now();
        let path = blob_path(reference);
        let reference = *reference;
        let result = match self.backend.get(&path).await {
            Ok(result) => {
                let len = result.meta.size;
                let metadata = BlobMetadata { reference, len };
                Ok(Some((BlobStream::new(result.into_stream(), len), metadata)))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(BlobStoreError::object_store(error)),
        };

        match &result {
            Ok(Some(_)) => record_blob_operation("get", "hit", started_at),
            Ok(None) => record_blob_operation("get", "miss", started_at),
            Err(_) => record_blob_operation("get", "error", started_at),
        }

        result
    }

    /// Return whether a blob exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured backend fails.
    pub async fn exists(&self, reference: &BlobRef) -> Result<bool, BlobStoreError> {
        let started_at = Instant::now();
        let path = blob_path(reference);
        let result = match self.backend.head(&path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(error) => Err(BlobStoreError::object_store(error)),
        };

        match &result {
            Ok(true) => record_blob_operation("exists", "hit", started_at),
            Ok(false) => record_blob_operation("exists", "miss", started_at),
            Err(_) => record_blob_operation("exists", "error", started_at),
        }

        result
    }

    /// Return metadata for a stored blob, if present.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured backend fails.
    pub async fn stat(&self, reference: &BlobRef) -> Result<Option<BlobMetadata>, BlobStoreError> {
        let started_at = Instant::now();
        let path = blob_path(reference);
        let result = match self.backend.head(&path).await {
            Ok(metadata) => Ok(Some(BlobMetadata { reference: *reference, len: metadata.size })),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(BlobStoreError::object_store(error)),
        };

        match &result {
            Ok(Some(_)) => record_blob_operation("stat", "hit", started_at),
            Ok(None) => record_blob_operation("stat", "miss", started_at),
            Err(_) => record_blob_operation("stat", "error", started_at),
        }

        result
    }

    /// Remove a blob if present.
    ///
    /// This is idempotent for missing blobs. Reference and retention policy
    /// remain the caller's responsibility.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured backend fails.
    pub async fn erase(&self, reference: &BlobRef) -> Result<(), BlobStoreError> {
        let started_at = Instant::now();
        let path = blob_path(reference);
        let result = match self.backend.delete(&path).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(error) => Err(BlobStoreError::object_store(error)),
        };

        match &result {
            Ok(()) => record_blob_operation("erase", "completed", started_at),
            Err(_) => record_blob_operation("erase", "error", started_at),
        }

        result
    }

    /// Verify stored bytes against their content reference.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured backend fails while reading the object.
    pub async fn verify(&self, reference: &BlobRef) -> Result<VerifyResult, BlobStoreError> {
        let started_at = Instant::now();
        let result = self.verify_uninstrumented(reference).await;

        match &result {
            Ok(VerifyResult::Valid) => record_blob_operation("verify", "valid", started_at),
            Ok(VerifyResult::Missing) => record_blob_operation("verify", "missing", started_at),
            Ok(VerifyResult::Corrupt) => record_blob_operation("verify", "corrupt", started_at),
            Err(_) => record_blob_operation("verify", "error", started_at),
        }

        result
    }

    /// Commit a complete payload without replacing an existing content-addressed object.
    async fn put_payload(
        &self,
        reference: BlobRef,
        payload: PutPayload,
    ) -> Result<(), BlobStoreError> {
        let path = blob_path(&reference);

        for _ in 0..MAX_COMMIT_RACE_ATTEMPTS {
            match self.backend.put_opts(&path, payload.clone(), PutMode::Create.into()).await {
                Ok(_) => return Ok(()),
                Err(object_store::Error::AlreadyExists { .. }) => {
                    match self.verify_uninstrumented(&reference).await? {
                        VerifyResult::Valid => return Ok(()),
                        VerifyResult::Corrupt => {
                            return Err(BlobStoreError::CorruptExistingBlob { reference });
                        }
                        VerifyResult::Missing => {}
                    }
                }
                Err(error) => return Err(BlobStoreError::object_store(error)),
            }
        }

        Err(BlobStoreError::CommitRaceExhausted { reference })
    }

    /// Promote a completed staging object to its content-addressed key.
    async fn promote_staged(
        &self,
        temporary_path: &Path,
        reference: BlobRef,
    ) -> Result<(), BlobStoreError> {
        match self.verify_uninstrumented(&reference).await? {
            VerifyResult::Valid => return Ok(()),
            VerifyResult::Corrupt => {
                return Err(BlobStoreError::CorruptExistingBlob { reference });
            }
            VerifyResult::Missing => {}
        }

        // `copy_if_not_exists` is not universally available on S3-compatible
        // stores. A normal copy is portable. Racing legitimate writers derive
        // the same key from the same bytes, so overwriting that identical object
        // is harmless; a pre-existing corrupt object was rejected above.
        self.backend
            .copy(temporary_path, &blob_path(&reference))
            .await
            .map_err(BlobStoreError::object_store)
    }

    /// Verify a blob without emitting a separate top-level verification metric.
    async fn verify_uninstrumented(
        &self,
        reference: &BlobRef,
    ) -> Result<VerifyResult, BlobStoreError> {
        let path = blob_path(reference);
        let result = match self.backend.get(&path).await {
            Ok(result) => result,
            Err(object_store::Error::NotFound { .. }) => return Ok(VerifyResult::Missing),
            Err(error) => return Err(BlobStoreError::object_store(error)),
        };

        let mut hasher = Sha256::new();
        let mut stream = result.into_stream();
        while let Some(chunk) = stream.next().await {
            hasher.update(&chunk.map_err(BlobStoreError::object_store)?);
        }

        let digest: [u8; 32] = hasher.finalize().into();
        Ok(if &digest == reference.digest() { VerifyResult::Valid } else { VerifyResult::Corrupt })
    }
}

/// Instrumented chunk stream for blob delivery.
pub struct BlobStream {
    /// Backend-native object stream.
    stream: BoxStream<'static, object_store::Result<Bytes>>,
    /// Object length observed when the stream was opened.
    expected_len: u64,
    /// Stream lifecycle metrics.
    metrics: BlobStreamMetrics,
}

impl fmt::Debug for BlobStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BlobStream")
            .field("expected_len", &self.expected_len)
            .finish_non_exhaustive()
    }
}

impl BlobStream {
    /// Wrap one backend-native object stream.
    fn new(stream: BoxStream<'static, object_store::Result<Bytes>>, expected_len: u64) -> Self {
        Self { stream, expected_len, metrics: BlobStreamMetrics::start("stream_read") }
    }
}

impl Stream for BlobStream {
    type Item = Result<Bytes, BlobStoreError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.stream.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                self.metrics.add_bytes(chunk.len() as u64);
                if !self.metrics.is_finished() && self.metrics.bytes() > self.expected_len {
                    self.metrics.finish("length_mismatch");
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(error))) => {
                self.metrics.finish("error");
                Poll::Ready(Some(Err(BlobStoreError::object_store(error))))
            }
            Poll::Ready(None) => {
                if !self.metrics.is_finished() {
                    let outcome = if self.metrics.bytes() == self.expected_len {
                        "completed"
                    } else {
                        "length_mismatch"
                    };
                    self.metrics.finish(outcome);
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for BlobStream {
    fn drop(&mut self) {
        if !self.metrics.is_finished() {
            self.metrics.finish("cancelled");
        }
    }
}

/// Best-effort cleanup for an unfinished multipart upload.
async fn abort_staged_upload(mut staging: Option<(Path, Box<dyn object_store::MultipartUpload>)>) {
    if let Some((_, upload)) = staging.as_mut() {
        let _ = upload.abort().await;
    }
}

/// Generate an unguessable object path for multipart staging.
fn temporary_blob_path() -> Result<Path, BlobStoreError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|source| BlobStoreError::Random { source })?;
    Ok(Path::from(format!("_kival/tmp/{}", hex::encode(random))))
}

/// Compute the backend object path for a content reference.
fn blob_path(reference: &BlobRef) -> Path {
    let digest = reference.to_string();
    Path::from(format!("{}/{}/{}", &digest[..2], &digest[2..4], digest))
}

/// Tracks one active streaming blob operation through its terminal outcome.
#[derive(Debug)]
struct BlobStreamMetrics {
    /// Stable stream operation label.
    operation: &'static str,
    /// Time at which streaming started.
    started_at: Instant,
    /// Bytes transferred before completion, failure, or cancellation.
    bytes: u64,
    /// Whether a terminal outcome has already been recorded.
    finished: bool,
}

impl BlobStreamMetrics {
    /// Start tracking one active blob stream.
    fn start(operation: &'static str) -> Self {
        describe_blob_metrics();
        gauge!("blob.streams_in_flight", "operation" => operation).increment(1.0);
        Self { operation, started_at: Instant::now(), bytes: 0, finished: false }
    }

    /// Add bytes transferred by this stream.
    const fn add_bytes(&mut self, bytes: u64) {
        self.bytes = self.bytes.saturating_add(bytes);
    }

    /// Return the number of bytes transferred by this stream.
    const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Return whether a terminal stream outcome has already been recorded.
    const fn is_finished(&self) -> bool {
        self.finished
    }

    /// Record one terminal stream outcome exactly once.
    fn finish(&mut self, outcome: &'static str) {
        if self.finished {
            return;
        }

        self.finished = true;
        gauge!("blob.streams_in_flight", "operation" => self.operation).decrement(1.0);
        record_blob_operation(self.operation, outcome, self.started_at);
        record_blob_bytes(self.operation, outcome, self.bytes);
    }
}

impl Drop for BlobStreamMetrics {
    fn drop(&mut self) {
        if !self.finished {
            self.finish("cancelled");
        }
    }
}

/// Register blob-store metric descriptions.
fn describe_blob_metrics() {
    describe_counter!("blob.operations_total", "Blob store operations.");
    describe_counter!(
        "blob.bytes_total",
        "Blob store bytes transferred by terminal operation outcome."
    );
    describe_gauge!("blob.streams_in_flight", "Active streaming blob operations.");
    describe_histogram!("blob.operation_duration_seconds", "Blob store operation duration.");
}

/// Record one terminal blob operation and its duration.
fn record_blob_operation(operation: &'static str, outcome: &'static str, started_at: Instant) {
    describe_blob_metrics();
    counter!(
        "blob.operations_total",
        "operation" => operation,
        "outcome" => outcome
    )
    .increment(1);
    histogram!(
        "blob.operation_duration_seconds",
        "operation" => operation,
        "outcome" => outcome
    )
    .record(started_at.elapsed().as_secs_f64());
}

/// Record bytes transferred by one terminal blob operation.
fn record_blob_bytes(operation: &'static str, outcome: &'static str, bytes: u64) {
    describe_blob_metrics();
    counter!(
        "blob.bytes_total",
        "operation" => operation,
        "outcome" => outcome
    )
    .increment(bytes);
}

#[cfg(test)]
mod tests {
    use std::io;

    use futures_util::TryStreamExt;
    use object_store::{ObjectStore as _, memory::InMemory};
    use tempfile::tempdir;

    use super::*;

    fn memory_store() -> (BlobStore, Arc<InMemory>) {
        let backend = Arc::new(InMemory::new());
        (BlobStore::new(backend.clone()), backend)
    }

    fn byte_stream(bytes: Bytes) -> impl Stream<Item = Result<Bytes, io::Error>> {
        futures_util::stream::iter([Ok(bytes)])
    }

    #[tokio::test]
    async fn generic_backend_roundtrips_blob_operations() {
        let (store, _) = memory_store();
        let bytes = Bytes::from_static(b"hello object store");
        let reference = store.put(bytes.clone()).await.expect("put");

        assert_eq!(store.get_bytes(&reference).await.expect("get"), Some(bytes.clone()));
        assert!(store.exists(&reference).await.expect("exists"));
        assert_eq!(
            store.stat(&reference).await.expect("stat"),
            Some(BlobMetadata { reference, len: bytes.len() as u64 })
        );
        assert_eq!(store.verify(&reference).await.expect("verify"), VerifyResult::Valid);

        store.erase(&reference).await.expect("erase");
        assert!(!store.exists(&reference).await.expect("exists after erase"));
        assert_eq!(
            store.verify(&reference).await.expect("verify after erase"),
            VerifyResult::Missing
        );
        store.erase(&reference).await.expect("idempotent erase");
    }

    #[tokio::test]
    async fn content_reference_preserves_existing_backend_key_layout() {
        let (store, backend) = memory_store();
        let reference = store.put(Bytes::from_static(b"path layout")).await.expect("put");
        let encoded = reference.to_string();
        let expected = Path::from(format!("{}/{}/{}", &encoded[..2], &encoded[2..4], encoded));

        assert!(backend.head(&expected).await.is_ok());
    }

    #[tokio::test]
    async fn repeated_put_is_idempotent() {
        let (store, _) = memory_store();
        let bytes = Bytes::from_static(b"same bytes");

        let first = store.put(bytes.clone()).await.expect("first put");
        let second = store.put(bytes.clone()).await.expect("second put");

        assert_eq!(first, second);
        assert_eq!(store.get_bytes(&first).await.expect("get"), Some(bytes));
    }

    #[tokio::test]
    async fn put_refuses_to_overwrite_corrupt_content_addressed_object() {
        let (store, backend) = memory_store();
        let expected = Bytes::from_static(b"expected bytes");
        let reference = BlobRef::from_bytes(&expected);
        let corrupted = Bytes::from_static(b"corrupted bytes");

        backend
            .put(&blob_path(&reference), corrupted.clone().into())
            .await
            .expect("inject corrupt object");

        let error = store.put(expected).await.expect_err("corruption must be rejected");
        assert!(matches!(
            error,
            BlobStoreError::CorruptExistingBlob { reference: observed } if observed == reference
        ));
        assert_eq!(store.get_bytes(&reference).await.expect("get"), Some(corrupted));
    }

    #[tokio::test]
    async fn put_stream_is_bounded_and_content_addressed() {
        let (store, _) = memory_store();
        let input = b"streamed bytes";
        let stored = store
            .put_stream(byte_stream(Bytes::copy_from_slice(input)), input.len() as u64)
            .await
            .expect("put stream");

        assert_eq!(stored.reference, BlobRef::from_bytes(input));
        assert_eq!(stored.len, input.len() as u64);
        assert_eq!(
            store.get_bytes(&stored.reference).await.expect("get"),
            Some(Bytes::copy_from_slice(input))
        );
    }

    #[tokio::test]
    async fn put_stream_preserves_chunk_order() {
        let (store, _) = memory_store();
        let input = futures_util::stream::iter([
            Ok::<_, io::Error>(Bytes::from_static(b"chunk one ")),
            Ok(Bytes::from_static(b"chunk two")),
        ]);
        let expected = Bytes::from_static(b"chunk one chunk two");

        let stored = store.put_stream(input, expected.len() as u64).await.expect("put stream");

        assert_eq!(stored.reference, BlobRef::from_bytes(&expected));
        assert_eq!(store.get_bytes(&stored.reference).await.expect("get"), Some(expected));
    }

    #[tokio::test]
    async fn put_stream_rejects_oversized_small_input_without_storing_it() {
        let (store, _) = memory_store();
        let input = b"too large";
        let result = store.put_stream(byte_stream(Bytes::copy_from_slice(input)), 3).await;

        assert!(matches!(result, Err(BlobStoreError::SizeLimitExceeded { limit: 3 })));
        assert!(!store.exists(&BlobRef::from_bytes(input)).await.expect("exists"));
    }

    #[tokio::test]
    async fn get_preserves_chunk_stream_contract() {
        let (store, _) = memory_store();
        let bytes = Bytes::from_static(b"stream me");
        let reference = store.put(bytes.clone()).await.expect("put");
        let (stream, metadata) = store.get(&reference).await.expect("get").expect("present");
        let read = stream
            .try_fold(Vec::new(), async |mut buffer, chunk| {
                buffer.extend_from_slice(&chunk);
                Ok(buffer)
            })
            .await
            .expect("read stream");

        assert_eq!(metadata, BlobMetadata { reference, len: bytes.len() as u64 });
        assert_eq!(read, bytes);
    }

    #[tokio::test]
    async fn verify_detects_corrupt_backend_object() {
        let (store, backend) = memory_store();
        let reference = BlobRef::from_bytes(b"expected");

        backend
            .put(&blob_path(&reference), PutPayload::from_static(b"different"))
            .await
            .expect("inject corruption");

        assert_eq!(store.verify(&reference).await.expect("verify"), VerifyResult::Corrupt);
    }

    #[tokio::test]
    async fn large_stream_uses_backend_staging_and_removes_temporary_object() {
        let (store, backend) = memory_store();
        let input = vec![0x5a; MULTIPART_CHUNK_SIZE + 17];
        let stored = store
            .put_stream(byte_stream(Bytes::copy_from_slice(&input)), input.len() as u64)
            .await
            .expect("put stream");

        assert_eq!(stored.reference, BlobRef::from_bytes(&input));
        assert_eq!(
            store.get_bytes(&stored.reference).await.expect("get"),
            Some(Bytes::from(input))
        );
        assert_no_temporary_objects(&backend).await;
    }

    #[tokio::test]
    async fn oversized_large_stream_aborts_backend_staging() {
        let (store, backend) = memory_store();
        let first = Bytes::from(vec![0x4d; MULTIPART_CHUNK_SIZE]);
        let second = Bytes::from_static(&[0x4d]);
        let input = futures_util::stream::iter([Ok::<_, io::Error>(first), Ok(second)]);
        let result = store.put_stream(input, MULTIPART_CHUNK_SIZE as u64).await;

        assert!(matches!(
            result,
            Err(BlobStoreError::SizeLimitExceeded { limit })
                if limit == MULTIPART_CHUNK_SIZE as u64
        ));
        assert_no_temporary_objects(&backend).await;
    }

    #[tokio::test]
    async fn large_stream_does_not_replace_corrupt_final_object() {
        let (store, backend) = memory_store();
        let input = vec![0x33; MULTIPART_CHUNK_SIZE + 1];
        let reference = BlobRef::from_bytes(&input);
        let corrupted = Bytes::from_static(b"corrupted");

        backend
            .put(&blob_path(&reference), corrupted.clone().into())
            .await
            .expect("inject corrupt object");

        let error = store
            .put_stream(byte_stream(Bytes::copy_from_slice(&input)), input.len() as u64)
            .await
            .expect_err("corruption must be rejected");
        assert!(matches!(
            error,
            BlobStoreError::CorruptExistingBlob { reference: observed } if observed == reference
        ));
        assert_eq!(store.get_bytes(&reference).await.expect("get"), Some(corrupted));
        assert_no_temporary_objects(&backend).await;
    }

    #[tokio::test]
    async fn filesystem_backend_preserves_existing_disk_layout() {
        let root = tempdir().expect("tempdir");
        let store = BlobStore::filesystem(root.path()).expect("filesystem store");
        let bytes = Bytes::from_static(b"local filesystem");
        let reference = store.put(bytes.clone()).await.expect("put");
        let digest = reference.to_string();
        let path = root.path().join(&digest[..2]).join(&digest[2..4]).join(digest);

        assert_eq!(std::fs::read(path).expect("read stored file"), bytes);
        assert_eq!(store.get_bytes(&reference).await.expect("get"), Some(bytes));
    }

    async fn assert_no_temporary_objects(backend: &InMemory) {
        let prefix = Path::from("_kival/tmp");
        let temporary = backend.list(Some(&prefix)).collect::<Vec<_>>().await;
        assert!(temporary.is_empty(), "temporary staging objects remain: {temporary:?}");
    }
}
