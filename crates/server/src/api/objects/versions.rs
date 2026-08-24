//! Object version creation, updates, and history.

use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    extract::{Path, State},
};
use kival_kernel::{
    EventKind, ObjectVersion, UpdateObjectVersion, fetch_object_in_tx, fetch_object_version,
    fetch_object_version_by_number, fetch_object_version_creator_for_mutation,
    fetch_object_version_in_tx, list_object_version_creators, list_object_version_wikilinks,
    list_object_versions, maintain_object_references, update_object_version,
};
use kival_sdk::{
    ListParams, ListResponse, ObjectResponse, ObjectVersion as ApiObjectVersion,
    ObjectVersionResponse, ObjectVersionWikilink, ObjectVersionWikilinksResponse,
    UpdateObjectRequest,
};
use kival_types::ObjectRole;
use serde_json::json;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{api_object, emit_wikilink_reresolution_event, validate_metadata};
use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        authz::require_object_role,
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
        pagination,
        query::QueryParams,
        validate::required_trimmed,
    },
};

/// Updates an object, appending a new version only when versioned state changes.
pub(crate) async fn handle_update_object(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    JsonBody(request): JsonBody<UpdateObjectRequest>,
) -> ApiResult<Json<ObjectResponse>> {
    let effective_role =
        require_object_role(state.db(), actor.id, workspace_id, object_id, ObjectRole::Editor)
            .await?;

    if request.title.is_none() && request.body.is_none() && request.metadata.is_none() {
        return Err(ApiError::bad_request("at least one field must be provided"));
    }
    if let Some(metadata) = request.metadata.as_ref() {
        validate_metadata(metadata)?;
    }

    let title = request
        .title
        .as_deref()
        .map(|title| required_trimmed(title, "title"))
        .transpose()?
        .map(str::to_owned);

    let mut tx = state.db().begin().await?;
    let updated = update_object_version(
        &mut tx,
        UpdateObjectVersion {
            workspace_id,
            object_id,
            expected_current_version_id: request.expected_current_version_id,
            title,
            body: request.body,
            metadata: request.metadata,
            created_by: actor.id,
        },
    )
    .await?;
    let old_title = updated.previous_title;
    let changed = updated.changed;
    let version = updated.version;

    let object = api_object(fetch_object_in_tx(&mut tx, workspace_id, object_id).await?);
    if !changed {
        let mut version = api_object_version(version);
        hydrate_version_creator_for_mutation(&mut tx, workspace_id, object_id, &mut version)
            .await?;
        tx.commit().await?;
        return Ok(Json(ObjectResponse { effective_role, object, current_version: Some(version) }));
    }

    let new_title = version.title.clone();
    let affected_titles = if old_title == new_title {
        Vec::new()
    } else {
        vec![old_title.clone(), new_title.clone()]
    };
    let maintenance =
        maintain_object_references(&mut tx, workspace_id, object_id, version.id, &affected_titles)
            .await?;
    let reference_update = maintenance.reference_update;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::ObjectVersionAppended,
                json!({ "object_id": object_id, "object_version_id": version.id }),
            )
            .workspace(workspace_id)
            .object(object_id)
            .object_version(version.id),
    )
    .await?;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(EventKind::ObjectUpdated, json!({ "object_id": object.id }))
            .workspace(workspace_id)
            .object(object.id)
            .object_version(version.id),
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
                        "object_id": object_id,
                        "version_id": version.id,
                        "resolved_count": reference_update.resolved_count,
                        "unresolved_count": reference_update.unresolved_count,
                        "ambiguous_count": reference_update.ambiguous_count,
                        "stale_count": reference_update.stale_count,
                    }),
                )
                .workspace(workspace_id)
                .object(object_id)
                .object_version(version.id),
        )
        .await?;
    }

    if !affected_titles.is_empty() {
        emit_wikilink_reresolution_event(
            &mut tx,
            state.durable_tasks().queue(),
            &actor,
            workspace_id,
            object.id,
            &affected_titles,
            maintenance.reresolution,
        )
        .await?;
    }

    let mut version = api_object_version(version);
    hydrate_version_creator_for_mutation(&mut tx, workspace_id, object_id, &mut version).await?;

    tx.commit().await?;

    Ok(Json(ObjectResponse { effective_role, object, current_version: Some(version) }))
}

/// Lists object versions.
pub(crate) async fn handle_list_versions(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    QueryParams(params): QueryParams<ListParams>,
) -> ApiResult<Json<ListResponse<ApiObjectVersion>>> {
    let cursor = pagination::decode_version(&params, "object_versions", object_id)?;
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;

    let mut versions = list_object_versions(
        state.db(),
        actor.id,
        workspace_id,
        object_id,
        cursor.map(|cursor| cursor.version_number),
        limit + 1,
    )
    .await?
    .into_iter()
    .map(api_object_version)
    .collect::<Vec<_>>();
    hydrate_version_creators(state.db(), actor.id, workspace_id, object_id, &mut versions).await?;

    Ok(Json(pagination::version_page(versions, limit, "object_versions", object_id, |version| {
        version.version_number
    })?))
}

/// Gets a specific object version by immutable ID or monotonic version number.
pub(crate) async fn handle_get_version(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id, version)): Path<(Uuid, Uuid, String)>,
) -> ApiResult<Json<ObjectVersionResponse>> {
    let version = fetch_version_identifier(
        state.as_ref(),
        actor.id,
        workspace_id,
        object_id,
        &version,
    )
    .await?;
    let mut versions = vec![api_object_version(version)];
    hydrate_version_creators(state.db(), actor.id, workspace_id, object_id, &mut versions).await?;
    let version = versions.pop().expect("one fetched object version");

    Ok(Json(ObjectVersionResponse { version }))
}

/// Lists wikilinks derived from one immutable object version.
pub(crate) async fn handle_get_version_wikilinks(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id, version)): Path<(Uuid, Uuid, String)>,
) -> ApiResult<Json<ObjectVersionWikilinksResponse>> {
    let version = fetch_version_identifier(
        state.as_ref(),
        actor.id,
        workspace_id,
        object_id,
        &version,
    )
    .await?;
    let items = list_object_version_wikilinks(
        state.db(),
        actor.id,
        workspace_id,
        object_id,
        version.id,
    )
    .await?
    .into_iter()
    .map(|reference| ObjectVersionWikilink {
        raw_target: reference.raw_target,
        display_text: reference.display_text,
        target_object_id: reference.target_object_id,
    })
    .collect();

    Ok(Json(ObjectVersionWikilinksResponse { items }))
}

/// Resolves the version identifier accepted by object-version read endpoints.
async fn fetch_version_identifier(
    state: &ServerState,
    actor_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    version: &str,
) -> ApiResult<ObjectVersion> {
    if let Ok(version_id) = Uuid::parse_str(version) {
        return Ok(fetch_object_version(
            state.db(),
            actor_id,
            workspace_id,
            object_id,
            version_id,
        )
        .await?);
    }

    let version_number = version.parse::<i64>().map_err(|_error| {
        ApiError::bad_request("version must be a UUID or positive version number")
    })?;
    if version_number < 1 {
        return Err(ApiError::bad_request("version number must be at least 1"));
    }

    Ok(fetch_object_version_by_number(
        state.db(),
        actor_id,
        workspace_id,
        object_id,
        version_number,
    )
    .await?)
}

/// Fetches an object version inside an existing mutation transaction.
pub(super) async fn fetch_version_for_mutation(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    version_id: Uuid,
) -> ApiResult<ApiObjectVersion> {
    let mut version =
        api_object_version(fetch_object_version_in_tx(tx, object_id, version_id).await?);
    hydrate_version_creator_for_mutation(tx, workspace_id, object_id, &mut version).await?;
    Ok(version)
}

/// Fetches an object version by object and version ID.
pub(super) async fn fetch_version(
    state: &ServerState,
    actor_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    version_id: Uuid,
) -> ApiResult<ApiObjectVersion> {
    let version =
        fetch_object_version(state.db(), actor_id, workspace_id, object_id, version_id).await?;
    let mut versions = vec![api_object_version(version)];
    hydrate_version_creators(state.db(), actor_id, workspace_id, object_id, &mut versions).await?;
    Ok(versions.pop().expect("one fetched object version"))
}

/// Converts a kernel object version into the HTTP representation.
pub(super) fn api_object_version(version: ObjectVersion) -> ApiObjectVersion {
    ApiObjectVersion {
        id: version.id,
        object_id: version.object_id,
        version_number: version.version_number,
        title: version.title,
        body: version.body,
        metadata: version.metadata,
        created_by: version.created_by,
        created_by_username: None,
        created_by_display_name: None,
        created_by_workspace_role: None,
        created_by_object_role: None,
        created_at: version.created_at,
    }
}

/// Resolves creator identities for object versions without filtering disabled accounts.
pub(super) async fn hydrate_version_creators(
    pool: &sqlx::PgPool,
    actor_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    versions: &mut [ApiObjectVersion],
) -> ApiResult<()> {
    let version_ids = versions
        .iter()
        .filter(|version| version.created_by.is_some())
        .map(|version| version.id)
        .collect::<Vec<_>>();
    if version_ids.is_empty() {
        return Ok(());
    }

    let identities =
        list_object_version_creators(pool, actor_id, workspace_id, object_id, &version_ids)
            .await?
            .into_iter()
            .map(|creator| (creator.version_id, creator))
            .collect::<HashMap<_, _>>();

    for version in versions {
        let Some(identity) = identities.get(&version.id) else {
            continue;
        };
        version.created_by_username = Some(identity.username.clone());
        version.created_by_display_name = Some(identity.display_name.clone());
        version.created_by_workspace_role = identity.workspace_role;
        version.created_by_object_role = identity.object_role;
    }

    Ok(())
}

/// Resolves one version creator while an object mutation transaction is active.
pub(super) async fn hydrate_version_creator_for_mutation(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    version: &mut ApiObjectVersion,
) -> ApiResult<()> {
    if version.created_by.is_none() {
        return Ok(());
    }

    if let Some(identity) =
        fetch_object_version_creator_for_mutation(tx, workspace_id, object_id, version.id).await?
    {
        version.created_by_username = Some(identity.username);
        version.created_by_display_name = Some(identity.display_name);
        version.created_by_workspace_role = identity.workspace_role;
        version.created_by_object_role = identity.object_role;
    }

    Ok(())
}
