//! Server implementation for Kival.

#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg))]

use std::{io::Result, net::SocketAddr, sync::Arc, time::Duration};

use axum::Router;
use kival_sdk::API_PREFIX;
use kival_storage::BlobStore;
use kival_tasks::DurableTasks;
use kival_tracing::{error, info};
use sqlx::PgPool;
use tokio::{net::TcpListener, task::JoinError};
use tokio_util::sync::CancellationToken;

/// Root namespace reserved for Kival HTTP APIs.
const API_ROOT: &str = "/api";
/// Interval between low-frequency Steda retention and recovery maintenance passes.
const DURABLE_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(60 * 60);
pub mod api;
pub mod layers;
mod web;
mod webauthn;
pub use webauthn::{WebAuthnConfig, WebAuthnConfigError};

/// Runtime settings used by request handlers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerSettings {
    /// Maximum accepted attachment upload size in bytes.
    pub attachment_max_bytes: u64,
    /// Passkey-ceremony start requests accepted per direct peer and minute.
    pub authentication_start_requests_per_minute: u32,
    /// Passkey-ceremony completion requests accepted per direct peer and minute.
    pub authentication_finish_requests_per_minute: u32,
    /// Authenticated requests accepted per user and minute.
    pub authenticated_user_requests_per_minute: u32,
    /// Bearer-authentication attempts accepted per direct peer and minute. Zero disables this
    /// pre-authentication limit.
    pub api_key_authentication_attempts_per_minute: u32,
    /// Requests accepted per API key and minute.
    pub api_key_requests_per_minute: u32,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            attachment_max_bytes: 100 * 1024 * 1024,
            authentication_start_requests_per_minute: 30,
            authentication_finish_requests_per_minute: 15,
            authenticated_user_requests_per_minute: 1_200,
            api_key_authentication_attempts_per_minute: 1_200,
            api_key_requests_per_minute: 1_200,
        }
    }
}

/// Shared state available to server handlers.
#[derive(Debug, Clone)]
pub struct ServerState {
    /// Database pool shared by request handlers.
    db: PgPool,
    /// Blob store shared by request handlers.
    blob_store: BlobStore,
    /// Durable task handles shared by request handlers.
    durable_tasks: DurableTasks,
    /// Exact `WebAuthn` relying-party expectations.
    webauthn: WebAuthnConfig,
    /// Maximum accepted attachment upload size in bytes.
    attachment_max_bytes: u64,
    /// In-process request limiter.
    rate_limiter: api::RateLimiter,
    /// In-process realtime invalidation fan-out.
    realtime: api::RealtimeHub,
}

impl ServerState {
    /// Creates shared server state with derived `WebAuthn` relying-party settings.
    #[must_use]
    pub fn with_webauthn(
        db: PgPool,
        blob_store: BlobStore,
        durable_tasks: DurableTasks,
        webauthn: WebAuthnConfig,
    ) -> Self {
        Self::with_settings(db, blob_store, durable_tasks, webauthn, ServerSettings::default())
    }

    /// Creates shared server state with explicit runtime settings.
    #[must_use]
    pub fn with_settings(
        db: PgPool,
        blob_store: BlobStore,
        durable_tasks: DurableTasks,
        webauthn: WebAuthnConfig,
        settings: ServerSettings,
    ) -> Self {
        Self {
            db,
            blob_store,
            durable_tasks,
            webauthn,
            attachment_max_bytes: settings.attachment_max_bytes,
            rate_limiter: api::RateLimiter::new(
                settings.authentication_start_requests_per_minute,
                settings.authentication_finish_requests_per_minute,
                settings.authenticated_user_requests_per_minute,
                settings.api_key_authentication_attempts_per_minute,
                settings.api_key_requests_per_minute,
            ),
            realtime: api::RealtimeHub::new(),
        }
    }

    /// Returns the `PostgreSQL` pool.
    #[must_use]
    pub const fn db(&self) -> &PgPool {
        &self.db
    }

    /// Returns the blob store.
    #[must_use]
    pub const fn blob_store(&self) -> &BlobStore {
        &self.blob_store
    }

    /// Returns the durable task handles.
    #[must_use]
    pub const fn durable_tasks(&self) -> &DurableTasks {
        &self.durable_tasks
    }

    /// Returns the maximum accepted attachment upload size.
    #[must_use]
    pub const fn attachment_max_bytes(&self) -> u64 {
        self.attachment_max_bytes
    }

    /// Returns the in-process request limiter.
    pub(crate) const fn rate_limiter(&self) -> &api::RateLimiter {
        &self.rate_limiter
    }

    /// Returns the configured `WebAuthn` relying-party settings.
    pub(crate) const fn webauthn(&self) -> &WebAuthnConfig {
        &self.webauthn
    }

    /// Returns the in-process realtime invalidation hub.
    pub(crate) const fn realtime(&self) -> &api::RealtimeHub {
        &self.realtime
    }
}

/// Kival HTTP server.
#[derive(Debug, Clone)]
pub struct Server {
    /// Shared server state used to build request routers.
    state: Arc<ServerState>,
}

impl Server {
    /// Creates a new server instance with the given shared state.
    pub const fn new(state: Arc<ServerState>) -> Self {
        Self { state }
    }

    /// Returns the shared server state.
    pub fn state(&self) -> Arc<ServerState> {
        Arc::clone(&self.state)
    }

    /// Runs the HTTP server until the task is cancelled or the listener fails.
    ///
    /// # Errors
    ///
    /// Returns an error if the TCP listener cannot bind, notification projection cannot be
    /// initialized or continue running, a required background subsystem terminates unexpectedly,
    /// or the HTTP server fails while serving requests.
    pub async fn run(self, bind_addr: SocketAddr) -> Result<()> {
        self.run_with_graceful_shutdown(bind_addr, std::future::pending()).await
    }

    /// Runs the HTTP server until shutdown is requested, then drains in-flight requests.
    ///
    /// # Errors
    ///
    /// Returns an error if the TCP listener cannot bind, notification projection cannot be
    /// initialized or continue running, a required background subsystem terminates unexpectedly,
    /// or the HTTP server fails while serving requests.
    ///
    /// # Panics
    ///
    /// Panics if the compile-time API prefix is not contained within the `/api` namespace.
    pub async fn run_with_graceful_shutdown(
        self,
        bind_addr: SocketAddr,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> Result<()> {
        let listener = TcpListener::bind(bind_addr).await?;

        let api_version_prefix = API_PREFIX
            .strip_prefix(API_ROOT)
            .expect("API_PREFIX must remain inside the /api namespace");
        let api_namespace = Router::new()
            .nest(api_version_prefix, api::router(self.state()))
            .fallback(api::status::handle_get_fallback);
        let router = Router::new()
            .route(API_ROOT, axum::routing::any(api::status::handle_get_fallback))
            .route("/api/", axum::routing::any(api::status::handle_get_fallback))
            .nest(API_ROOT, api_namespace)
            .merge(web::router());

        let notification_queue = self.state.durable_tasks().queue().clone();
        api::enqueue_notification_backlog_if_needed(&notification_queue, self.state.db())
            .await
            .map_err(|error| {
                std::io::Error::other(format!(
                    "failed to enqueue notification backlog recovery: {error}"
                ))
            })?;
        let notification_worker =
            api::notification_worker(&notification_queue, self.state.db().clone()).map_err(
                |error| {
                    std::io::Error::other(format!("failed to build notification worker: {error}"))
                },
            )?;

        let cancellation = CancellationToken::new();
        let shutdown_cancellation = cancellation.clone();
        let shutdown_forwarder = tokio::spawn(async move {
            shutdown.await;
            shutdown_cancellation.cancel();
        });

        let mut durable_maintenance = tokio::spawn(run_durable_maintenance(
            self.state.durable_tasks().steda().clone(),
            self.state.durable_tasks().queue().clone(),
            self.state.db().clone(),
        ));
        let mut notification_retention =
            tokio::spawn(api::run_notification_retention(self.state.db().clone()));
        let mut realtime_listener = tokio::spawn(api::run_realtime_listener(
            self.state.db().clone(),
            self.state.realtime().clone(),
        ));

        let mut serve = Box::pin(
            axum::serve(
                listener,
                layers::build_layers(router).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(cancellation.clone().cancelled_owned())
            .into_future(),
        );
        let mut worker =
            Box::pin(notification_worker.run_until(cancellation.clone().cancelled_owned()));

        info!(
            target: "kival::server",
            "Kival available at: {}",
            self.state.webauthn().origin()
        );

        let result = {
            let background_failure = async {
                tokio::select! {
                    result = &mut durable_maintenance => {
                        background_task_failure("durable maintenance", result)
                    }
                    result = &mut notification_retention => {
                        background_task_failure("notification retention", result)
                    }
                    result = &mut realtime_listener => {
                        background_task_failure("realtime listener", result)
                    }
                }
            };
            tokio::pin!(background_failure);

            tokio::select! {
                server_result = &mut serve => {
                    cancellation.cancel();
                    let worker_result = worker.await;
                    match server_result {
                        Ok(()) => worker_result.map_err(|error| {
                            std::io::Error::other(format!("notification worker failed: {error}"))
                        }),
                        Err(error) => {
                            if let Err(worker_error) = worker_result {
                                error!(
                                    target: "kival::server::durable_tasks",
                                    error = ?worker_error,
                                    "notification worker also failed while the HTTP server was stopping",
                                );
                            }
                            Err(error)
                        }
                    }
                }
                worker_result = &mut worker => {
                    let shutdown_requested = cancellation.is_cancelled();
                    cancellation.cancel();
                    let server_result = serve.await;
                    match worker_result {
                        Ok(()) if shutdown_requested => server_result,
                        Ok(()) => {
                            if let Err(server_error) = server_result {
                                error!(
                                    target: "kival::server",
                                    error = ?server_error,
                                    "HTTP server also failed while the notification worker was stopping",
                                );
                            }
                            Err(std::io::Error::other(
                                "notification worker stopped unexpectedly",
                            ))
                        }
                        Err(error) => Err(std::io::Error::other(format!(
                            "notification worker failed: {error}"
                        ))),
                    }
                }
                background_error = &mut background_failure => {
                    cancellation.cancel();
                    let (server_result, worker_result) = tokio::join!(&mut serve, &mut worker);
                    if let Err(server_error) = server_result {
                        error!(
                            target: "kival::server",
                            error = ?server_error,
                            "HTTP server also failed while a background subsystem was stopping",
                        );
                    }
                    if let Err(worker_error) = worker_result {
                        error!(
                            target: "kival::server::durable_tasks",
                            error = ?worker_error,
                            "notification worker also failed while a background subsystem was stopping",
                        );
                    }
                    Err(background_error)
                }
            }
        };

        shutdown_forwarder.abort();
        durable_maintenance.abort();
        notification_retention.abort();
        realtime_listener.abort();
        let _ = shutdown_forwarder.await;
        let _ = durable_maintenance.await;
        let _ = notification_retention.await;
        let _ = realtime_listener.await;

        result
    }
}

/// Converts unexpected completion of a required background subsystem into a server error.
fn background_task_failure(
    name: &'static str,
    result: std::result::Result<(), JoinError>,
) -> std::io::Error {
    match result {
        Ok(()) => std::io::Error::other(format!("{name} stopped unexpectedly")),
        Err(error) => std::io::Error::other(format!("{name} task failed: {error}")),
    }
}

/// Runs low-frequency durable-task retention and notification recovery maintenance.
async fn run_durable_maintenance(steda: steda::Steda, queue: steda::Queue, pool: PgPool) {
    loop {
        tokio::time::sleep(DURABLE_MAINTENANCE_INTERVAL).await;

        if let Err(error) = steda.cleanup().await {
            error!(
                target: "kival::server::durable_tasks",
                error = ?error,
                "durable task retention cleanup failed",
            );
        }

        if let Err(error) = api::enqueue_notification_backlog_if_needed(&queue, &pool).await {
            error!(
                target: "kival::server::durable_tasks",
                error = ?error,
                "failed to enqueue notification projection reconciliation",
            );
        }
    }
}
