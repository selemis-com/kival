//! API key management handlers.

use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use kival_common::security;
use kival_kernel::{
    ApiKeyRow, EventKind, KernelError, api_key_workspaces_accessible, create_api_key,
    fetch_api_key, list_api_key_scopes, list_api_key_workspaces, list_api_keys,
    lock_active_api_key, replace_api_key_authorization, revoke_api_key, set_api_key_delegation,
};
use kival_sdk::{
    ApiKey, ApiKeyListResponse, ApiKeyResponse, CreateApiKeyRequest, CreateApiKeyResponse,
    ListParams, UpdateApiKeyRequest,
};
use kival_types::ApiKeyScope;
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::{
        auth::{API_KEY_PREFIX, AuthenticatedUser, generate_secret_token},
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
        pagination,
        passkeys::require_fresh_session_in_tx,
        query::QueryParams,
        validate::required_trimmed,
    },
};

/// Maximum API key label length.
const API_KEY_LABEL_MAX_CHARS: usize = 64;
/// Maximum number of explicit workspaces that may be delegated to one key.
const API_KEY_WORKSPACE_LIMIT: usize = 256;

/// Creates a labeled API key for the authenticated user.
pub(crate) async fn handle_create_api_key(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    headers: HeaderMap,
    JsonBody(request): JsonBody<CreateApiKeyRequest>,
) -> ApiResult<Response> {
    actor.require_session()?;

    let label = required_trimmed(&request.label, "API key label")?;
    if label.chars().count() > API_KEY_LABEL_MAX_CHARS {
        return Err(ApiError::bad_request("API key label is too long"));
    }
    if request.scopes.is_empty() {
        return Err(ApiError::bad_request("API key must have at least one scope"));
    }
    let scopes = dedupe_scopes(&request.scopes);
    let workspace_ids = dedupe_workspaces(&request.workspace_ids);
    if workspace_ids.len() > API_KEY_WORKSPACE_LIMIT {
        return Err(ApiError::bad_request("API key has too many workspace restrictions"));
    }

    let token = format!("{API_KEY_PREFIX}{}", generate_secret_token()?);
    let token_hash = security::hash_token(&token);
    let mut tx = state.db().begin().await?;
    require_fresh_session_in_tx(&mut tx, actor.id, &headers).await?;

    let row = create_api_key(&mut tx, actor.id, label, token_hash.as_slice(), request.expires_at)
        .await?
        .ok_or_else(|| ApiError::bad_request("API key expiration must be in the future"))?;

    ensure_accessible_workspaces(&mut tx, actor.id, &workspace_ids).await?;
    set_api_key_delegation(&mut tx, row.id, &scopes, &workspace_ids).await?;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::AuthApiKeyCreated,
                json!({
                    "api_key_id": row.id,
                    "api_key_label": &row.label,
                    "scopes": scopes.iter().map(|scope| scope.as_str()).collect::<Vec<_>>(),
                    "workspace_ids": &workspace_ids,
                    "expires_at": request.expires_at,
                }),
            )
            .target_user(actor.id),
    )
    .await?;

    let api_key = api_key_from_row(row, scopes, workspace_ids);
    tx.commit().await?;

    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));

    Ok((headers, Json(CreateApiKeyResponse { api_key, token })).into_response())
}

/// Lists API keys created by the authenticated user.
pub(crate) async fn handle_list_api_keys(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    QueryParams(params): QueryParams<ListParams>,
) -> ApiResult<Json<ApiKeyListResponse>> {
    actor.require_session()?;

    let cursor = pagination::decode_created_at(&params, "api_keys", Some(actor.id))?;
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;
    let rows = list_api_keys(
        state.db(),
        actor.id,
        cursor.map(|cursor| cursor.created_at),
        cursor.map(|cursor| cursor.id),
        limit + 1,
    )
    .await?;

    let api_keys = hydrate_api_keys(state.db(), rows).await?;

    Ok(Json(pagination::created_at_page(api_keys, limit, "api_keys", Some(actor.id), |api_key| {
        (api_key.created_at, api_key.id)
    })?))
}

/// Replaces the delegated scopes and workspaces of one active API key without rotating its token.
pub(crate) async fn handle_update_api_key(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    headers: HeaderMap,
    axum::extract::Path(api_key_id): axum::extract::Path<Uuid>,
    JsonBody(request): JsonBody<UpdateApiKeyRequest>,
) -> ApiResult<Json<ApiKeyResponse>> {
    actor.require_session()?;

    if request.scopes.is_empty() {
        return Err(ApiError::bad_request("API key must have at least one scope"));
    }
    let scopes = dedupe_scopes(&request.scopes);
    let workspace_ids = dedupe_workspaces(&request.workspace_ids);
    if workspace_ids.len() > API_KEY_WORKSPACE_LIMIT {
        return Err(ApiError::bad_request("API key has too many workspace restrictions"));
    }

    let mut tx = state.db().begin().await?;
    require_fresh_session_in_tx(&mut tx, actor.id, &headers).await?;

    let existing = lock_active_api_key(&mut tx, api_key_id, actor.id)
        .await?
        .ok_or_else(|| ApiError::not_found("active API key not found"))?;

    if existing.authorization_revision != request.authorization_revision {
        return Err(ApiError::conflict("API key authorization changed; reload and try again"));
    }

    ensure_accessible_workspaces(&mut tx, actor.id, &workspace_ids).await?;
    let update =
        replace_api_key_authorization(&mut tx, api_key_id, &scopes, &workspace_ids).await?;
    let previous_scopes = update.previous_scopes;
    let previous_workspace_ids = update.previous_workspace_ids;
    let row = update.row;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::AuthApiKeyUpdated,
                json!({
                    "api_key_id": row.id,
                    "api_key_label": &row.label,
                    "previous_authorization_revision": existing.authorization_revision,
                    "authorization_revision": row.authorization_revision,
                    "previous_scopes": previous_scopes
                        .iter()
                        .map(|scope| scope.as_str())
                        .collect::<Vec<_>>(),
                    "scopes": scopes.iter().map(|scope| scope.as_str()).collect::<Vec<_>>(),
                    "previous_workspace_ids": previous_workspace_ids,
                    "workspace_ids": &workspace_ids,
                }),
            )
            .target_user(actor.id),
    )
    .await?;

    let api_key = api_key_from_row(row, scopes, workspace_ids);
    tx.commit().await?;

    Ok(Json(ApiKeyResponse { api_key }))
}

/// Revokes one API key owned by the authenticated user.
pub(crate) async fn handle_revoke_api_key(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    axum::extract::Path(api_key_id): axum::extract::Path<Uuid>,
) -> ApiResult<Json<ApiKeyResponse>> {
    actor.require_session()?;

    let mut tx = state.db().begin().await?;
    let updated = revoke_api_key(&mut tx, api_key_id, actor.id).await?;

    let row = if let Some(row) = updated {
        emit_event(
            &mut tx,
            state.durable_tasks().queue(),
            actor
                .event(
                    EventKind::AuthApiKeyRevoked,
                    json!({ "api_key_id": row.id, "api_key_label": &row.label }),
                )
                .target_user(actor.id),
        )
        .await?;
        row
    } else {
        fetch_api_key(&mut tx, api_key_id, actor.id).await.map_err(|error| match error {
            KernelError::Database(sqlx::Error::RowNotFound) => {
                ApiError::not_found("API key not found")
            }
            error => ApiError::from(error),
        })?
    };

    tx.commit().await?;

    Ok(Json(ApiKeyResponse { api_key: hydrate_api_key(state.db(), row).await? }))
}

/// Converts an API key row and its already-known delegation data into the wire resource.
fn api_key_from_row(row: ApiKeyRow, scopes: Vec<ApiKeyScope>, workspace_ids: Vec<Uuid>) -> ApiKey {
    ApiKey {
        id: row.id,
        user_id: row.user_id,
        label: row.label,
        authorization_revision: row.authorization_revision,
        scopes,
        workspace_ids,
        created_at: row.created_at,
        updated_at: row.updated_at,
        expires_at: row.expires_at,
        revoked_at: row.revoked_at,
        last_used_at: row.last_used_at,
    }
}

/// Loads scopes and workspace restrictions for one API key row.
async fn hydrate_api_key(pool: &sqlx::PgPool, row: ApiKeyRow) -> ApiResult<ApiKey> {
    let mut api_keys = hydrate_api_keys(pool, vec![row]).await?;
    api_keys.pop().ok_or_else(|| ApiError::internal("API key hydration returned no row"))
}

/// Loads scopes and workspace restrictions for API key rows without per-key queries.
async fn hydrate_api_keys(pool: &sqlx::PgPool, rows: Vec<ApiKeyRow>) -> ApiResult<Vec<ApiKey>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let ids = rows.iter().map(|row| row.id).collect::<Vec<_>>();
    let scope_rows = list_api_key_scopes(pool, &ids).await?;
    let workspace_rows = list_api_key_workspaces(pool, &ids).await?;

    let mut scopes_by_key: HashMap<Uuid, Vec<ApiKeyScope>> = HashMap::new();
    for (api_key_id, scope) in scope_rows {
        scopes_by_key.entry(api_key_id).or_default().push(scope);
    }

    let mut workspaces_by_key: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (api_key_id, workspace_id) in workspace_rows {
        workspaces_by_key.entry(api_key_id).or_default().push(workspace_id);
    }
    Ok(rows
        .into_iter()
        .map(|row| {
            let id = row.id;
            api_key_from_row(
                row,
                scopes_by_key.remove(&id).unwrap_or_default(),
                workspaces_by_key.remove(&id).unwrap_or_default(),
            )
        })
        .collect())
}

/// Sorts and deduplicates API key scopes.
fn dedupe_scopes(scopes: &[ApiKeyScope]) -> Vec<ApiKeyScope> {
    let mut scopes = scopes.to_vec();
    scopes.sort_unstable_by_key(|scope| scope.as_str());
    scopes.dedup();
    scopes
}

/// Sorts and deduplicates workspace identifiers.
fn dedupe_workspaces(workspace_ids: &[Uuid]) -> Vec<Uuid> {
    let mut workspace_ids = workspace_ids.to_vec();
    workspace_ids.sort_unstable();
    workspace_ids.dedup();
    workspace_ids
}

/// Ensures the owner may delegate every requested workspace.
async fn ensure_accessible_workspaces(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor_id: Uuid,
    workspace_ids: &[Uuid],
) -> ApiResult<()> {
    if api_key_workspaces_accessible(tx, actor_id, workspace_ids).await? {
        Ok(())
    } else {
        Err(ApiError::forbidden("cannot delegate access to an inaccessible workspace"))
    }
}
