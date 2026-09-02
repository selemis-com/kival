//! Object notification preferences, personal inbox, and retention maintenance.

use std::{
    sync::{Arc, Once},
    time::Duration,
};

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{
    EventKind, InboxEntryRow, apply_notification_retention, inbox_unread_count, list_inbox_entries,
    mark_inbox_read, object_notification_preference, publish_inbox_updated,
    publish_inbox_updated_for_user, set_object_notification_preference,
    update_inbox_entry_read_state,
};
use kival_metrics::{counter, describe_counter};
use kival_sdk::{
    InboxEntry, InboxListParams, InboxUnreadCountResponse, InboxUpdatedResponse, ListParams,
    ListResponse, MarkInboxReadRequest, ObjectNotificationPreference, UpdateInboxEntryRequest,
    UpdateObjectNotificationPreferenceRequest,
};
use kival_tracing::error;
use serde_json::json;
use sqlx::PgPool;
use tokio::time::sleep;
use uuid::Uuid;

use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        authz::require_object_readable,
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
        pagination,
        query::QueryParams,
    },
};

/// Interval between bounded notification-retention passes.
const RETENTION_INTERVAL: Duration = Duration::from_secs(60);
/// Maximum rows deleted from each notification table in one retention pass.
const RETENTION_BATCH_SIZE: i32 = 256;
/// Cursor kind used for inbox pagination.
const INBOX_CURSOR_KIND: &str = "inbox_notifications";
/// Ensures notification metric descriptions are emitted once.
static DESCRIBE_RETENTION_METRICS: Once = Once::new();

/// Converts a kernel inbox row into its API representation.
fn inbox_into_wire(row: InboxEntryRow) -> InboxEntry {
    InboxEntry {
        id: row.id,
        sequence_number: row.sequence_number,
        recipient_user_id: row.recipient_user_id,
        workspace_id: row.workspace_id,
        workspace_name: row.workspace_name,
        object_id: row.object_id,
        object_title: row.object_title,
        source_event_id: row.source_event_id,
        latest_event_id: row.latest_event_id,
        actor_user_id: row.actor_user_id,
        actor_username: row.actor_username,
        notification_type: row.notification_type,
        reason: row.reason,
        event_count: row.event_count,
        thread_id: row.thread_id,
        comment_id: row.comment_id,
        comment_excerpt: row.comment_excerpt,
        read_at: row.read_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/// Returns the effective object notification preference.
pub(crate) async fn handle_get_object_notification_preference(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ObjectNotificationPreference>> {
    actor.require_session()?;
    let row = object_notification_preference(state.db(), actor.id, workspace_id, object_id).await?;

    Ok(Json(ObjectNotificationPreference {
        workspace_id,
        object_id,
        ordinary_notifications_enabled: row.0.unwrap_or(true),
        explicit: row.0.is_some(),
        updated_at: row.1,
    }))
}

/// Creates or updates an explicit object notification preference.
pub(crate) async fn handle_update_object_notification_preference(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    JsonBody(input): JsonBody<UpdateObjectNotificationPreferenceRequest>,
) -> ApiResult<Json<ObjectNotificationPreference>> {
    actor.require_session()?;
    require_object_readable(state.db(), actor.id, workspace_id, object_id).await?;

    let mut tx = state.db().begin().await?;
    let updated_at = set_object_notification_preference(
        &mut tx,
        actor.id,
        workspace_id,
        object_id,
        input.ordinary_notifications_enabled,
    )
    .await?;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::ObjectNotificationPreferenceChanged,
                json!({
                    "ordinary_notifications_enabled": input.ordinary_notifications_enabled,
                }),
            )
            .workspace(workspace_id)
            .object(object_id)
            .target_user(actor.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(ObjectNotificationPreference {
        workspace_id,
        object_id,
        ordinary_notifications_enabled: input.ordinary_notifications_enabled,
        explicit: true,
        updated_at: Some(updated_at),
    }))
}

/// Lists currently visible personal inbox entries.
pub(crate) async fn handle_list_inbox(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    QueryParams(params): QueryParams<InboxListParams>,
) -> ApiResult<Json<ListResponse<InboxEntry>>> {
    actor.require_session()?;
    let list_params = ListParams { limit: params.limit, cursor: params.cursor.clone() };
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;
    let cursor_kind =
        pagination::filtered_kind(INBOX_CURSOR_KIND, &(params.workspace_id, params.unread_only))?;
    let cursor = pagination::decode_sequence(&list_params, &cursor_kind, Some(actor.id))?;

    let rows = list_inbox_entries(
        state.db(),
        actor.id,
        cursor.map(|cursor| cursor.sequence_number),
        params.unread_only,
        params.workspace_id,
        limit + 1,
    )
    .await?;

    let entries = rows.into_iter().map(inbox_into_wire).collect();
    let page = pagination::sequence_page(entries, limit, &cursor_kind, Some(actor.id), |entry| {
        entry.sequence_number
    })?;

    Ok(Json(page))
}

/// Returns the current unread inbox count under current authorization.
pub(crate) async fn handle_get_inbox_unread_count(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
) -> ApiResult<Json<InboxUnreadCountResponse>> {
    actor.require_session()?;

    let unread_count = inbox_unread_count(state.db(), actor.id).await?;

    Ok(Json(InboxUnreadCountResponse { unread_count }))
}

/// Changes one inbox entry's read state.
pub(crate) async fn handle_update_inbox_entry(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(inbox_entry_id): Path<Uuid>,
    JsonBody(input): JsonBody<UpdateInboxEntryRequest>,
) -> ApiResult<Json<InboxEntry>> {
    actor.require_session()?;
    let mut tx = state.db().begin().await?;

    let entry = update_inbox_entry_read_state(&mut tx, inbox_entry_id, actor.id, input.read)
        .await?
        .ok_or_else(|| ApiError::not_found("inbox entry not found"))?;

    publish_inbox_updated(
        &mut tx,
        actor.id,
        entry.workspace_id,
        entry.object_id,
        entry.latest_event_id,
        entry.id,
    )
    .await?;
    tx.commit().await?;

    Ok(Json(inbox_into_wire(entry)))
}

/// Marks a currently authorized inbox range read.
pub(crate) async fn handle_mark_inbox_read(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    JsonBody(input): JsonBody<MarkInboxReadRequest>,
) -> ApiResult<Json<InboxUpdatedResponse>> {
    actor.require_session()?;
    if input.through_sequence.is_some_and(|sequence| sequence < 0) {
        return Err(ApiError::bad_request("through_sequence must be at least 0"));
    }

    let mut tx = state.db().begin().await?;
    let updated =
        mark_inbox_read(&mut tx, actor.id, input.workspace_id, input.through_sequence).await?;

    if updated > 0 {
        publish_inbox_updated_for_user(&mut tx, actor.id).await?;
    }
    tx.commit().await?;

    Ok(Json(InboxUpdatedResponse { updated: u64::try_from(updated).unwrap_or(0) }))
}

/// Runs bounded notification retention until cancelled.
pub(crate) async fn run_retention(pool: PgPool) {
    describe_retention_metrics();

    loop {
        match apply_notification_retention(&pool, RETENTION_BATCH_SIZE).await {
            Ok(batch) => {
                counter!(
                    "notifications.retention_rows_deleted_total",
                    "table" => "candidates"
                )
                .increment(u64::from(batch.candidates_deleted.unsigned_abs()));
                counter!(
                    "notifications.retention_rows_deleted_total",
                    "table" => "inbox"
                )
                .increment(u64::from(batch.inbox_deleted.unsigned_abs()));
            }
            Err(error) => {
                counter!("notifications.retention_failures_total").increment(1);
                error!(
                    target: "kival::server::notifications",
                    error = ?error,
                    "notification retention cleanup failed",
                );
            }
        }

        sleep(RETENTION_INTERVAL).await;
    }
}

/// Registers notification retention metric descriptions once.
fn describe_retention_metrics() {
    DESCRIBE_RETENTION_METRICS.call_once(|| {
        describe_counter!(
            "notifications.retention_rows_deleted_total",
            "Expired notification rows deleted by bounded retention cleanup."
        );
        describe_counter!(
            "notifications.retention_failures_total",
            "Failed bounded notification retention passes."
        );
    });
}
