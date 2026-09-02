//! Durable notification projection tasks.

use std::{sync::Once, time::Duration};

use kival_kernel::{
    NotificationProjectionBatch, notification_candidates_exist_for_event,
    pending_notification_candidates_exist, process_notification_projection_batch,
};
use kival_metrics::{counter, describe_counter, describe_gauge, gauge};
use kival_tracing::error;
use sqlx::{PgPool, Postgres, Transaction};
use steda::{Queue, RetryStrategy, Task, TaskContext, Worker};
use tokio::time::sleep;
use uuid::Uuid;

/// Maximum durable notification candidates projected in one transaction.
const PROJECTION_BATCH_SIZE: i32 = 100;
/// Delay before rechecking a backlog whose remaining candidates are currently locked elsewhere.
const PROJECTION_CONTENDED_DELAY: Duration = Duration::from_secs(1);
/// Small yield between non-empty projection batches.
const PROJECTION_ACTIVE_DELAY: Duration = Duration::from_millis(25);
/// Retry budget spanning roughly ninety minutes once the backoff reaches its cap.
const PROJECTION_MAX_ATTEMPTS: u32 = 100;
/// Retry policy for transient projection failures.
const PROJECTION_RETRY_STRATEGY: RetryStrategy =
    RetryStrategy::exponential(Duration::from_secs(1), 2.0, Some(Duration::from_secs(60)));
/// Stable durable task contract for draining pending notification candidates.
pub(crate) const PROJECT_NOTIFICATIONS: Task<(), ()> = Task::new("project-notifications");
/// Ensures projection metric descriptions are emitted once.
static DESCRIBE_PROJECTION_METRICS: Once = Once::new();

/// Submits projection work in the same transaction when an event created notification candidates.
///
/// # Errors
///
/// Returns an error if candidate state cannot be inspected or the durable task cannot be submitted.
pub(crate) async fn enqueue_if_needed(
    tx: &mut Transaction<'_, Postgres>,
    queue: &Queue,
    event_id: Uuid,
) -> steda::Result<()> {
    let has_candidates = notification_candidates_exist_for_event(tx, event_id).await?;

    if !has_candidates {
        return Ok(());
    }

    queue
        .spawn(PROJECT_NOTIFICATIONS, ())
        .idempotency_key(format!("notification-event:{event_id}"))
        .max_attempts(PROJECTION_MAX_ATTEMPTS)
        .retry_strategy(PROJECTION_RETRY_STRATEGY)
        .submit(tx)
        .await?;

    Ok(())
}

/// Submits one recovery task when notification candidates remain unprojected.
///
/// # Errors
///
/// Returns an error if candidate state cannot be inspected or recovery work cannot be submitted.
pub(crate) async fn enqueue_backlog_if_needed(queue: &Queue, pool: &PgPool) -> steda::Result<()> {
    let has_pending = pending_notification_candidates_exist(pool).await?;

    if !has_pending {
        return Ok(());
    }

    queue
        .spawn(PROJECT_NOTIFICATIONS, ())
        .max_attempts(PROJECTION_MAX_ATTEMPTS)
        .retry_strategy(PROJECTION_RETRY_STRATEGY)
        .await?;

    Ok(())
}

/// Builds the Kival notification projection worker for the default durable queue.
///
/// # Errors
///
/// Returns an error if the Steda worker configuration is invalid.
pub(crate) fn worker(queue: &Queue, pool: PgPool) -> steda::Result<Worker> {
    queue
        .worker()
        .task(PROJECT_NOTIFICATIONS, move |(), _ctx: TaskContext| project_backlog(pool.clone()))
        .build()
}

/// Projects bounded candidate batches until the durable candidate backlog is empty.
async fn project_backlog(pool: PgPool) -> steda::Result<()> {
    describe_projection_metrics();
    let result = project_backlog_inner(&pool).await;
    if let Err(error) = &result {
        counter!("notifications.projection_failures_total").increment(1);
        error!(
            target: "kival::server::notifications",
            error = ?error,
            "durable notification projection attempt failed",
        );
    }
    result
}

/// Performs one durable backlog projection attempt.
async fn project_backlog_inner(pool: &PgPool) -> steda::Result<()> {
    loop {
        let batch = process_notification_projection_batch(pool, PROJECTION_BATCH_SIZE).await?;
        record_projection_batch(&batch);

        if batch.remaining_candidate_lag == 0 {
            return Ok(());
        }

        if batch.candidates_processed == 0 {
            // Another worker may currently own every remaining candidate row. The SQL
            // projector uses SKIP LOCKED, so wait for those transactions to finish.
            sleep(PROJECTION_CONTENDED_DELAY).await;
        } else {
            sleep(PROJECTION_ACTIVE_DELAY).await;
        }
    }
}

/// Records one successful projection batch.
fn record_projection_batch(batch: &NotificationProjectionBatch) {
    counter!("notifications.candidates_processed_total")
        .increment(u64::from(batch.candidates_processed.unsigned_abs()));
    counter!("notifications.inbox_rows_changed_total")
        .increment(u64::from(batch.notifications_changed.unsigned_abs()));
    gauge!("notifications.candidate_processing_lag").set(batch.remaining_candidate_lag as f64);
}

/// Registers notification projection metric descriptions once.
fn describe_projection_metrics() {
    DESCRIBE_PROJECTION_METRICS.call_once(|| {
        describe_counter!(
            "notifications.candidates_processed_total",
            "Durable notification candidates processed by the inbox projection."
        );
        describe_counter!(
            "notifications.inbox_rows_changed_total",
            "Inbox rows inserted or updated by notification projection."
        );
        describe_counter!(
            "notifications.projection_failures_total",
            "Failed durable notification projection attempts."
        );
        describe_gauge!(
            "notifications.candidate_processing_lag",
            "Durable notification candidates still waiting for projection."
        );
    });
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use eyre::Result;
    use kival_tests::{TestFixtureExt, TestKival, object_metadata, test_body};
    use kival_types::{MembershipRole, ObjectRole};
    use sqlx::PgPool;
    use tokio::time::{sleep, timeout};
    use tokio_util::sync::CancellationToken;
    use uuid::Uuid;

    use super::worker;

    /// Runs the production notification worker until all currently submitted durable work is idle.
    async fn run_worker_until_idle(r: &TestKival) -> Result<()> {
        let worker = worker(r.state.durable_tasks().queue(), r.pool.clone())?;
        let cancellation = CancellationToken::new();
        let worker_cancellation = cancellation.clone();
        let worker_task =
            tokio::spawn(
                async move { worker.run_until(worker_cancellation.cancelled_owned()).await },
            );

        timeout(Duration::from_secs(10), async {
            loop {
                let active_tasks: i64 = sqlx::query_scalar(
                    r#"
                    SELECT count(*)
                    FROM steda.tasks_kival
                    WHERE state IN ('pending', 'running', 'sleeping')
                    "#,
                )
                .fetch_one(&r.pool)
                .await?;
                let pending_candidates: i64 = sqlx::query_scalar(
                    r#"
                    SELECT count(*)
                    FROM kival.notification_candidates
                    WHERE projected_at IS NULL
                    "#,
                )
                .fetch_one(&r.pool)
                .await?;

                if active_tasks == 0 && pending_candidates == 0 {
                    return Ok::<(), sqlx::Error>(());
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await??;

        cancellation.cancel();
        worker_task.await??;
        Ok(())
    }

    /// Proves the transactional event wake-up is actually claimed and executed by Steda.
    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn steda_worker_projects_transactionally_enqueued_notification(
        pool: PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("steda notification worker").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Steda Notification Worker",
                &test_body("Steda Notification Worker", "Version one."),
                object_metadata("steda-notification-worker-v1"),
            )
            .await?;
        let viewer = r
            .create_object_actor(
                space.workspace.id,
                object.id,
                "steda-notification-viewer",
                MembershipRole::Member,
                ObjectRole::Viewer,
            )
            .await?;

        // Drain fixture-setup wake-ups through the real worker so the assertion below starts from
        // a clean durable queue rather than bypassing Steda with the projection SQL function.
        run_worker_until_idle(&r).await?;
        sqlx::query(
            r#"
            DELETE FROM kival.inbox_notifications
            WHERE recipient_user_id = $1
            "#,
        )
        .bind(viewer.id)
        .execute(&r.pool)
        .await?;

        r.update_object(
            space.workspace.id,
            object.id,
            Some("Steda Notification Worker v2"),
            None,
            None,
        )
        .await?;

        let event_id: Uuid = sqlx::query_scalar(
            r#"
            SELECT event_id
            FROM kival.notification_candidates
            WHERE recipient_user_id = $1
                AND object_id = $2
                AND projected_at IS NULL
            ORDER BY sequence_number DESC
            LIMIT 1
            "#,
        )
        .bind(viewer.id)
        .bind(object.id)
        .fetch_one(&r.pool)
        .await?;
        let task_state: String = sqlx::query_scalar(
            r#"
            SELECT state
            FROM steda.tasks_kival
            WHERE name = 'project-notifications'
                AND idempotency_key = $1
            "#,
        )
        .bind(format!("notification-event:{event_id}"))
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(task_state, "pending");

        run_worker_until_idle(&r).await?;

        let task_state: String = sqlx::query_scalar(
            r#"
            SELECT state
            FROM steda.tasks_kival
            WHERE name = 'project-notifications'
                AND idempotency_key = $1
            "#,
        )
        .bind(format!("notification-event:{event_id}"))
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(task_state, "completed");

        let inbox_rows: i64 = sqlx::query_scalar(
            r#"
            SELECT count(*)
            FROM kival.inbox_notifications
            WHERE recipient_user_id = $1
                AND object_id = $2
            "#,
        )
        .bind(viewer.id)
        .bind(object.id)
        .fetch_one(&r.pool)
        .await?;
        assert_eq!(inbox_rows, 1);

        Ok(())
    }
}
