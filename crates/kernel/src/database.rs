//! `PostgreSQL` bootstrap for Kival's state machine.

use std::{num::NonZeroU32, time::Duration};

use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

use crate::{KernelError, Result};

/// Runtime settings for one PostgreSQL connection pool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatabasePoolSettings {
    /// Maximum number of connections owned by this Kival process.
    pub max_connections: NonZeroU32,
    /// Maximum time a request waits for an available connection.
    pub acquire_timeout: Duration,
}

impl Default for DatabasePoolSettings {
    fn default() -> Self {
        Self {
            max_connections: NonZeroU32::new(8).expect("non-zero default"),
            acquire_timeout: Duration::from_secs(5),
        }
    }
}

/// Opens a `PostgreSQL` pool and migrates it to Kival's current state machine.
///
/// # Errors
///
/// Returns an error if the pool cannot connect or migrations fail.
pub async fn open_pool_with_options(options: PgConnectOptions) -> Result<PgPool> {
    open_pool_with_settings(options, DatabasePoolSettings::default()).await
}

/// Opens a `PostgreSQL` pool with explicit per-process pool settings and runs migrations.
///
/// # Errors
///
/// Returns an error if the pool cannot connect or migrations fail.
pub async fn open_pool_with_settings(
    options: PgConnectOptions,
    settings: DatabasePoolSettings,
) -> Result<PgPool> {
    let options = options.options([("client_min_messages", "warning")]);
    let pool = PgPoolOptions::new()
        .max_connections(settings.max_connections.get())
        .acquire_timeout(settings.acquire_timeout)
        .connect_with(options)
        .await?;

    configure(&pool).await?;
    Ok(pool)
}

/// Migrates an existing `PostgreSQL` pool to Kival's current state machine.
///
/// # Errors
///
/// Returns an error if migrations fail.
async fn configure(pool: &PgPool) -> Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|source| KernelError::Migrate { source })?;

    Ok(())
}

/// Checks whether the configured `PostgreSQL` pool can execute a trivial query.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` is unavailable.
pub async fn database_ready(pool: &PgPool) -> Result<()> {
    sqlx::query(
        r#"
        SELECT 1
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}
