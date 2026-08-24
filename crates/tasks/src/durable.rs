//! Durable task execution backed by Steda.

use sqlx::PgPool;
use steda::{Queue, Steda};

/// Queue reserved for Kival's default durable background work.
const KIVAL_QUEUE_NAME: &str = "kival";

/// Process-local handles for Kival's Steda installation.
///
/// This type owns only bootstrap and handle lifetime. Kival code uses the native
/// [`Steda`] and [`Queue`] APIs directly, including Steda's transactional task
/// submission and control operations.
#[derive(Clone, Debug)]
pub struct DurableTasks {
    /// Root Steda handle sharing Kival's `PostgreSQL` pool.
    steda: Steda,
    /// Default Kival queue.
    queue: Queue,
}

impl DurableTasks {
    /// Bootstrap Steda over Kival's existing `PostgreSQL` pool.
    ///
    /// The Steda database schema must already have been applied by Kival's
    /// kernel migrations. Queue creation is idempotent and verifies an existing
    /// queue before returning it.
    ///
    /// # Errors
    ///
    /// Returns an error if the queue name is invalid or its durable storage
    /// cannot be created or verified.
    pub async fn bootstrap(pool: PgPool) -> steda::Result<Self> {
        let steda = Steda::from_pool(pool);
        let queue = steda.queue(KIVAL_QUEUE_NAME)?;
        queue.create().await?;

        Ok(Self { steda, queue })
    }

    /// Returns Kival's root Steda handle.
    #[must_use]
    pub const fn steda(&self) -> &Steda {
        &self.steda
    }

    /// Returns Kival's default durable task queue.
    #[must_use]
    pub const fn queue(&self) -> &Queue {
        &self.queue
    }
}

#[cfg(test)]
mod tests {
    use sqlx::PgPool;

    use super::DurableTasks;

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn bootstrap_provisions_default_queue(pool: PgPool) -> steda::Result<()> {
        let first = DurableTasks::bootstrap(pool.clone()).await?;
        assert_eq!(first.queue().name(), "kival");

        let second = DurableTasks::bootstrap(pool).await?;
        assert_eq!(second.queue().name(), "kival");

        Ok(())
    }
}
