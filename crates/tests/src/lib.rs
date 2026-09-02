//! Test harness for Kival.

#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg))]

/// Authenticated actors and complete multi-actor fixtures.
mod actors;
mod clients;
mod db;
mod fixtures;
mod http;
mod names;
/// Deterministic passkeys used by integration tests.
mod passkeys;
/// Reproducible stateful-test primitives.
mod stateful;

use std::sync::Arc;

pub use actors::{Actor, ActorClient, Actors, Fixture, FixtureError, FixtureUser, FixtureUsers};
use axum::Router;
pub use clients::{ApiKeyClient, BrowserClient, HttpResponse, TestClientError};
pub use fixtures::{
    TestFixtureExt, TestGroup, TestObject, TestObjectSpace, TestWorkspace, object_metadata,
    test_body,
};
pub use http::{TestActor, TestJsonResponse, TestRawResponseExt, TestResponseExt};
use kival_sdk::API_PREFIX;
use kival_server::{
    ServerSettings, ServerState, WebAuthnConfig, api::router, layers::build_layers,
};
use kival_storage::BlobStore;
use kival_tasks::DurableTasks;
pub use names::unique_name;
pub use passkeys::{
    AuthenticatedSessionResponse, AuthenticatedUser, AuthenticationCredential,
    AuthenticationResponse, InstalledIdentities, InstalledIdentity, PasskeyFixtureError,
    TEST_ORIGIN, TEST_PASSKEY_LABEL, TEST_RP_ID, TestPasskey, install_test_identities,
    install_test_identities_for,
};
use sqlx::PgPool;
pub use stateful::{
    Handle, KivalStateMachine, Lifecycle, Model, ModeledApiKey, ModeledAttachment, ModeledComment,
    ModeledCommentThread, ModeledEvent, Operation, OperationError, Principal, ResourceKind,
    ResourceMap,
};
use tempfile::TempDir;

/// Test result type used by the Kival test harness.
pub type TestResult<T> = eyre::Result<T>;

/// Shared Kival test application.
///
/// This owns a migrated database pool, a temporary blob store, an Axum app, and
/// an authenticated global admin actor.
#[derive(Debug)]
pub struct TestKival {
    /// Database pool used by the test app.
    pub pool: PgPool,

    /// Server state used by the test app.
    pub state: Arc<ServerState>,

    /// Fully layered Axum app rooted at [`API_PREFIX`].
    pub app: Router,

    /// Authenticated global admin actor.
    pub admin: TestActor,

    /// Temporary blob directory kept alive for the lifetime of the test app.
    _blob_dir: TempDir,
}

impl TestKival {
    /// Creates a test Kival using an isolated, migrated test database pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the blob store cannot be created or the bootstrap
    /// admin session cannot be created.
    pub async fn new(pool: PgPool) -> TestResult<Self> {
        let blob_dir = tempfile::tempdir()?;
        let blob_store = BlobStore::filesystem(blob_dir.path())?;
        let webauthn = WebAuthnConfig::from_canonical_url("http://localhost:5173")?;
        let durable_tasks = DurableTasks::bootstrap(pool.clone()).await?;
        let settings = ServerSettings {
            authentication_start_requests_per_minute: 0,
            authentication_finish_requests_per_minute: 0,
            authenticated_user_requests_per_minute: 0,
            api_key_authentication_attempts_per_minute: 0,
            api_key_requests_per_minute: 0,
            ..ServerSettings::default()
        };
        let state = Arc::new(ServerState::with_settings(
            pool.clone(),
            blob_store,
            durable_tasks,
            webauthn,
            settings,
        ));

        let app = Router::new().nest(API_PREFIX, build_layers(router(Arc::clone(&state))));

        let admin_session = db::insert_global_admin(&pool).await?;
        let admin = http::actor_from_session(admin_session)?;

        Ok(Self { pool, state, app, admin, _blob_dir: blob_dir })
    }
}
