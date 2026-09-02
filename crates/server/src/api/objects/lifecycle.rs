//! Object creation, retrieval, and lifecycle transitions.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{
    CreateInitialObject, EventKind, ListObjects, ObjectListEntry, archive_object,
    create_initial_object, fetch_object, fetch_object_in_tx, list_objects,
    maintain_object_references, re_resolve_current_wikilinks_for_titles, unarchive_object,
};
use kival_sdk::{
    CreateObjectRequest, ListParams, ListResponse, ObjectListItem, ObjectListParams, ObjectResponse,
};
use kival_types::{ObjectListOrder, ObjectRole};
use serde_json::json;
use uuid::Uuid;

use super::{
    api_object, emit_wikilink_reresolution_event, validate_metadata,
    versions::{
        api_object_version, fetch_version, fetch_version_for_mutation,
        hydrate_version_creator_for_mutation,
    },
};
use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        authz::{ensure_workspace_member, require_archived_object_admin_role, require_object_role},
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
        pagination,
        query::QueryParams,
        validate::required_trimmed,
    },
};

/// Converts a kernel object-list projection into the HTTP wire representation.
fn api_object_list_entry(entry: ObjectListEntry) -> ObjectListItem {
    ObjectListItem {
        object: api_object(entry.object),
        updated_by_username: entry.updated_by_username,
        updated_by_display_name: entry.updated_by_display_name,
        updated_by_workspace_role: entry.updated_by_workspace_role,
        updated_by_object_role: entry.updated_by_object_role,
        connection_count: entry.connection_count,
        unresolved_thread_count: entry.unresolved_thread_count,
        favorited: entry.favorited,
        pinned: entry.pinned,
        pinned_at: entry.pinned_at,
    }
}

/// Lists visible objects in a workspace by archive status.
pub(crate) async fn handle_list_objects(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
    QueryParams(params): QueryParams<ObjectListParams>,
) -> ApiResult<Json<ListResponse<ObjectListItem>>> {
    let list_params = ListParams { limit: params.limit, cursor: params.cursor.clone() };
    let cursor_subject = actor.api_key_id().unwrap_or(actor.id);
    let limit = list_params.checked_limit().map_err(ApiError::bad_request)?;
    let cursor_kind = pagination::filtered_kind(
        "objects",
        &(
            workspace_id,
            params.status.as_str(),
            params.order.as_str(),
            params.favorited,
            params.pinned,
        ),
    )?;
    let (cursor_at, cursor_id) = match params.order {
        ObjectListOrder::Created => {
            let cursor =
                pagination::decode_created_at(&list_params, &cursor_kind, Some(cursor_subject))?;
            (cursor.map(|cursor| cursor.created_at), cursor.map(|cursor| cursor.id))
        }
        ObjectListOrder::Updated => {
            let cursor =
                pagination::decode_updated_at(&list_params, &cursor_kind, Some(cursor_subject))?;
            (cursor.map(|cursor| cursor.updated_at), cursor.map(|cursor| cursor.id))
        }
    };

    let objects = list_objects(
        state.db(),
        ListObjects {
            workspace_id,
            actor_id: actor.id,
            cursor_at,
            cursor_id,
            limit: limit + 1,
            status: params.status,
            order: params.order,
            favorited: params.favorited,
            pinned: params.pinned,
        },
    )
    .await?
    .into_iter()
    .map(api_object_list_entry)
    .collect::<Vec<_>>();

    let response = match params.order {
        ObjectListOrder::Created => pagination::created_at_page(
            objects,
            limit,
            &cursor_kind,
            Some(cursor_subject),
            |object| (object.created_at, object.id),
        )?,
        ObjectListOrder::Updated => pagination::updated_at_page(
            objects,
            limit,
            &cursor_kind,
            Some(cursor_subject),
            |object| (object.updated_at, object.id),
        )?,
    };

    Ok(Json(response))
}

/// Creates an object and its first version.
pub(crate) async fn handle_create_object(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path(workspace_id): Path<Uuid>,
    JsonBody(request): JsonBody<CreateObjectRequest>,
) -> ApiResult<Json<ObjectResponse>> {
    ensure_workspace_member(state.db(), actor.id, workspace_id).await?;

    validate_metadata(&request.metadata)?;

    let title = required_trimmed(&request.title, "title")?;

    let mut tx = state.db().begin().await?;

    let created = create_initial_object(
        &mut tx,
        CreateInitialObject {
            workspace_id,
            title: title.to_owned(),
            body: request.body,
            metadata: request.metadata,
            created_by: actor.id,
        },
    )
    .await?;
    let object_id = created.object_id;
    let creator_grant_id = created.creator_grant_id;
    let version = created.version;

    let object = api_object(
        fetch_object_in_tx(&mut tx, workspace_id, object_id)
            .await
            .map_err(ApiError::from_object_kernel)?,
    );

    let affected_titles = vec![object.title.clone()];
    let maintenance =
        maintain_object_references(&mut tx, workspace_id, object.id, version.id, &affected_titles)
            .await?;
    let reference_update = maintenance.reference_update;
    let reresolution = maintenance.reresolution;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::ObjectCreated,
                json!({ "object_id": object.id, "object_version_id": version.id }),
            )
            .workspace(workspace_id)
            .object(object.id)
            .object_version(version.id),
    )
    .await?;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::ObjectGrantCreated,
                json!({
                    "object_grant_id": creator_grant_id,
                    "object_role": "admin",
                }),
            )
            .workspace(workspace_id)
            .object(object.id)
            .object_grant(creator_grant_id)
            .target_user(actor.id),
    )
    .await?;

    if reference_update.changed() {
        emit_event(
            &mut tx,
            state.durable_tasks().queue(),
            actor
                .event(
                    EventKind::ObjectReferencesUpdated,
                    json!({
                        "object_id": object.id,
                        "version_id": version.id,
                        "resolved_count": reference_update.resolved_count,
                        "unresolved_count": reference_update.unresolved_count,
                        "ambiguous_count": reference_update.ambiguous_count,
                        "stale_count": reference_update.stale_count,
                    }),
                )
                .workspace(workspace_id)
                .object(object.id)
                .object_version(version.id),
        )
        .await?;
    }

    emit_wikilink_reresolution_event(
        &mut tx,
        state.durable_tasks().queue(),
        &actor,
        workspace_id,
        object.id,
        &affected_titles,
        reresolution,
    )
    .await?;

    let mut version = api_object_version(version);
    hydrate_version_creator_for_mutation(&mut tx, workspace_id, object.id, &mut version).await?;

    tx.commit().await?;

    Ok(Json(ObjectResponse {
        effective_role: ObjectRole::Admin,
        object,
        current_version: Some(version),
    }))
}

/// Gets an object and its current version.
pub(crate) async fn handle_get_object(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ObjectResponse>> {
    let readable = fetch_object(state.db(), actor.id, workspace_id, object_id)
        .await
        .map_err(ApiError::from_object_kernel)?;
    let effective_role = readable.effective_role;
    let object = api_object(readable.object);
    let current_version = match object.current_version_id {
        Some(version_id) => Some(
            fetch_version(state.as_ref(), actor.id, workspace_id, object.id, version_id).await?,
        ),
        None => None,
    };

    Ok(Json(ObjectResponse { effective_role, object, current_version }))
}

/// Archives an object.
pub(crate) async fn handle_archive_object(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ObjectResponse>> {
    let effective_role =
        require_object_role(state.db(), actor.id, workspace_id, object_id, ObjectRole::Admin)
            .await?;

    let mut tx = state.db().begin().await?;
    archive_object(&mut tx, workspace_id, object_id, actor.id).await?;

    let object = api_object(
        fetch_object_in_tx(&mut tx, workspace_id, object_id)
            .await
            .map_err(ApiError::from_object_kernel)?,
    );

    let affected_titles = vec![object.title.clone()];
    let reresolution =
        re_resolve_current_wikilinks_for_titles(&mut tx, workspace_id, &affected_titles).await?;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(EventKind::ObjectArchived, json!({ "object_id": object.id }))
            .workspace(workspace_id)
            .object(object.id),
    )
    .await?;

    emit_wikilink_reresolution_event(
        &mut tx,
        state.durable_tasks().queue(),
        &actor,
        workspace_id,
        object.id,
        &affected_titles,
        reresolution,
    )
    .await?;

    let current_version = match object.current_version_id {
        Some(version_id) => {
            Some(fetch_version_for_mutation(&mut tx, workspace_id, object.id, version_id).await?)
        }
        None => None,
    };

    tx.commit().await?;

    Ok(Json(ObjectResponse { effective_role, object, current_version }))
}

/// Unarchives an object.
pub(crate) async fn handle_unarchive_object(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ObjectResponse>> {
    let effective_role =
        require_archived_object_admin_role(state.db(), actor.id, workspace_id, object_id).await?;

    let mut tx = state.db().begin().await?;
    unarchive_object(&mut tx, workspace_id, object_id).await?;

    let object = api_object(
        fetch_object_in_tx(&mut tx, workspace_id, object_id)
            .await
            .map_err(ApiError::from_object_kernel)?,
    );

    let affected_titles = vec![object.title.clone()];
    let reresolution =
        re_resolve_current_wikilinks_for_titles(&mut tx, workspace_id, &affected_titles).await?;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(EventKind::ObjectUnarchived, json!({ "object_id": object.id }))
            .workspace(workspace_id)
            .object(object.id),
    )
    .await?;

    emit_wikilink_reresolution_event(
        &mut tx,
        state.durable_tasks().queue(),
        &actor,
        workspace_id,
        object.id,
        &affected_titles,
        reresolution,
    )
    .await?;

    let current_version = match object.current_version_id {
        Some(version_id) => {
            Some(fetch_version_for_mutation(&mut tx, workspace_id, object.id, version_id).await?)
        }
        None => None,
    };

    tx.commit().await?;

    Ok(Json(ObjectResponse { effective_role, object, current_version }))
}
