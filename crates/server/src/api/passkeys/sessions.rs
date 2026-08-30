//! Browser-session persistence and fresh-authentication enforcement.

use std::{net::SocketAddr, time::Duration};

use axum::{
    Json,
    http::{HeaderMap, StatusCode, header::SET_COOKIE},
    response::{IntoResponse, Response},
};
use kival_common::security;
use kival_kernel::{
    create_session, current_session_id, lock_active_user_by_id, lock_fresh_session,
};
use kival_sdk::{AuthenticatedSessionResponse, User};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::{authentication_failed, interval};
use crate::{
    ServerState,
    api::{
        auth::{CSRF_COOKIE, SESSION_COOKIE, SESSION_TTL, session_cookie},
        error::{ApiError, ApiResult},
        users::user_into_wire,
    },
};

/// Maximum age accepted for a destructive credential-management action.
const FRESH_AUTH_TTL: Duration = Duration::from_secs(5 * 60);

/// Locks and returns the still-active user to serialize authentication with recovery.
pub(super) async fn require_active_user_locked(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
) -> ApiResult<User> {
    let row = lock_active_user_by_id(tx, user_id).await?.ok_or_else(authentication_failed)?;
    Ok(user_into_wire(row))
}

/// Inserts a fresh browser session after passkey verification.
pub(super) async fn insert_session(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    session_token: &str,
    csrf_token: &str,
    headers: &HeaderMap,
    peer_addr: SocketAddr,
) -> ApiResult<DateTime<Utc>> {
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let ip_address = peer_addr.ip().to_string();
    create_session(
        tx,
        user_id,
        security::hash_token(session_token).as_slice(),
        security::hash_token(csrf_token).as_slice(),
        &interval(SESSION_TTL),
        user_agent.as_deref(),
        &ip_address,
    )
    .await
    .map_err(ApiError::from)
}

/// Builds the authenticated-session envelope and secure session/CSRF cookies.
pub(super) fn authenticated_session_response(
    user: User,
    expires_at: DateTime<Utc>,
    session_token: &str,
    csrf_token: &str,
) -> ApiResult<Response> {
    let mut response =
        (StatusCode::OK, Json(AuthenticatedSessionResponse { expires_at, user })).into_response();
    append_auth_cookies(&mut response, session_token, csrf_token, SESSION_TTL.as_secs())?;
    Ok(response)
}

/// Returns fresh-authentication success while rotating both browser credentials.
pub(super) fn rotated_session_response(
    expires_at: DateTime<Utc>,
    session_token: &str,
    csrf_token: &str,
) -> ApiResult<Response> {
    let remaining_seconds = (expires_at - Utc::now()).num_seconds().max(1) as u64;
    let mut response = StatusCode::NO_CONTENT.into_response();
    append_auth_cookies(&mut response, session_token, csrf_token, remaining_seconds)?;
    Ok(response)
}

/// Appends the browser session and CSRF cookies with one shared lifetime.
fn append_auth_cookies(
    response: &mut Response,
    session_token: &str,
    csrf_token: &str,
    max_age_seconds: u64,
) -> ApiResult<()> {
    response.headers_mut().append(
        SET_COOKIE,
        format!(
            "{SESSION_COOKIE}={session_token}; Path=/; Max-Age={max_age_seconds}; HttpOnly; Secure; SameSite=Lax"
        )
        .parse()
        .map_err(|_| ApiError::internal("invalid cookie"))?,
    );
    response.headers_mut().append(
        SET_COOKIE,
        format!(
            "{CSRF_COOKIE}={csrf_token}; Path=/; Max-Age={max_age_seconds}; Secure; SameSite=Strict"
        )
        .parse()
        .map_err(|_| ApiError::internal("invalid cookie"))?,
    );
    Ok(())
}

/// Resolves the current active browser session from its cookie.
pub(super) async fn require_current_session_id(
    state: &ServerState,
    user_id: Uuid,
    headers: &HeaderMap,
) -> ApiResult<Uuid> {
    let token = session_cookie(headers).ok_or_else(|| ApiError::unauthorized("missing session"))?;
    current_session_id(state.db(), user_id, security::hash_token(token).as_slice())
        .await?
        .ok_or_else(|| ApiError::unauthorized("invalid session"))
}

/// Requires recent passkey verification.
pub(super) async fn require_fresh_session(
    state: &ServerState,
    user_id: Uuid,
    headers: &HeaderMap,
) -> ApiResult<Uuid> {
    let mut tx = state.db().begin().await?;
    let session_id = require_fresh_session_in_tx(&mut tx, user_id, headers).await?;
    tx.commit().await?;
    Ok(session_id)
}

/// Locks and validates the current fresh browser session inside an existing transaction.
pub(crate) async fn require_fresh_session_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    headers: &HeaderMap,
) -> ApiResult<Uuid> {
    let token = session_cookie(headers).ok_or_else(|| ApiError::unauthorized("missing session"))?;
    let session_token_hash = security::hash_token(token);

    let _locked_user = require_active_user_locked(tx, user_id).await?;
    let row =
        lock_fresh_session(tx, user_id, session_token_hash.as_slice(), &interval(FRESH_AUTH_TTL))
            .await?;

    let (session_id, allowed) = row.ok_or_else(|| ApiError::unauthorized("invalid session"))?;
    if !allowed {
        return Err(ApiError::forbidden("fresh passkey authentication required"));
    }
    Ok(session_id)
}
