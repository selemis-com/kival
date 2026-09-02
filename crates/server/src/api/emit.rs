//! Helpers for appending audit events inside command transactions.

use kival_kernel::{EventInsert, append_event};
use sqlx::{Postgres, Transaction};
use steda::Queue;

use crate::api::{error::ApiResult, notification_tasks};

/// Appends an event using the caller's open transaction.
pub(crate) async fn emit_event(
    tx: &mut Transaction<'_, Postgres>,
    durable_queue: &Queue,
    event: EventInsert,
) -> ApiResult<()> {
    let event_id = append_event(tx, event).await?;

    notification_tasks::enqueue_if_needed(tx, durable_queue, event_id).await?;

    Ok(())
}
