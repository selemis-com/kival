//! Passkey login and fresh-authentication workflows.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{ConnectInfo, State},
    http::HeaderMap,
    response::Response,
};
use kival_common::security;
use kival_kernel::{
    EventInsert, EventKind, FreshAuthenticationSessionRotation, active_user_id_by_username,
    consume_ceremony, create_authentication_ceremony, create_fresh_authentication_ceremony,
    has_active_passkey, lock_fresh_authentication_ceremony, lock_login_ceremony,
    login_ceremony_user_id, record_passkey_use, rotate_session_after_fresh_authentication,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::{
    CEREMONY_TTL, authentication_failed, authentication_options, challenge,
    credentials::fetch_passkey,
    interval, no_store_json, prune_terminal_ceremonies_best_effort,
    sessions::{
        authenticated_session_response, insert_session, require_active_user_locked,
        require_current_session_id, rotated_session_response,
    },
};
use crate::{
    ServerState,
    api::{
        auth::{AuthenticatedUser, generate_secret_token, prune_terminal_sessions_best_effort},
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
    },
    webauthn::{AuthenticationCredential, CeremonyExpectation, verify_authentication},
};

/// Username-first passkey authentication start request.
#[derive(Debug, Deserialize)]
pub(crate) struct AuthenticationOptionsRequest {
    /// Username identifying the interactive user.
    username: String,
}

/// Browser assertion paired with its server ceremony.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FinishAuthenticationRequest {
    /// Single-use ceremony identifier.
    ceremony_id: Uuid,
    /// Browser-produced public-key assertion.
    credential: AuthenticationCredential,
}

/// Starts username-first passkey authentication without disclosing account existence.
pub(crate) async fn handle_start_authentication(
    State(state): State<Arc<ServerState>>,
    JsonBody(request): JsonBody<AuthenticationOptionsRequest>,
) -> ApiResult<Response> {
    let challenge = challenge()?;
    let ceremony_id = Uuid::new_v4();
    let username = request.username.trim();

    let user = if username.is_empty() {
        None
    } else {
        active_user_id_by_username(state.db(), username).await?
    };

    if let Some(user_id) = user {
        let has_passkey = has_active_passkey(state.db(), user_id).await?;

        if has_passkey {
            prune_terminal_ceremonies_best_effort(&state, user_id).await;
            create_authentication_ceremony(
                state.db(),
                ceremony_id,
                user_id,
                &challenge,
                &interval(CEREMONY_TTL),
            )
            .await?;
        }
    }

    no_store_json(authentication_options(&state, ceremony_id, &challenge))
}

/// Completes passkey authentication and creates the normal browser session.
pub(crate) async fn handle_finish_authentication(
    State(state): State<Arc<ServerState>>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    JsonBody(request): JsonBody<FinishAuthenticationRequest>,
) -> ApiResult<Response> {
    let credential_id =
        request.credential.credential_id().map_err(|error| ApiError::bad_request(error.0))?;
    let session_token = generate_secret_token()?;
    let csrf_token = generate_secret_token()?;
    let mut tx = state.db().begin().await?;

    let user_id = require_login_ceremony_user_id(&mut tx, request.ceremony_id).await?;
    let user = require_active_user_locked(&mut tx, user_id).await?;
    let challenge = require_login_ceremony_locked(&mut tx, request.ceremony_id, user_id).await?;
    let stored = fetch_passkey(&mut tx, user_id, &credential_id).await?;
    let verified = verify_authentication(
        &request.credential,
        &stored.public_key,
        CeremonyExpectation {
            challenge: &challenge,
            origin: state.webauthn().origin_value(),
            rp_id: state.webauthn().rp_id(),
            alternate_origins: state.webauthn().alternate_origins(),
        },
    )
    .map_err(|_| authentication_failed())?;
    validate_user_handle(verified.user_handle.as_deref(), user_id)
        .map_err(|_| authentication_failed())?;
    validate_signature_count(stored.signature_count, verified.signature_count)
        .map_err(|_| authentication_failed())?;

    record_passkey_use(&mut tx, stored.id, i64::from(verified.signature_count)).await?;
    consume_ceremony(&mut tx, request.ceremony_id).await?;

    let expires_at =
        insert_session(&mut tx, user_id, &session_token, &csrf_token, &headers, peer_addr).await?;
    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        EventInsert::new(user_id, EventKind::AuthPasskeyLogin, json!({ "passkey_id": stored.id }))
            .target_user(user_id),
    )
    .await?;
    tx.commit().await?;
    prune_terminal_sessions_best_effort(&state).await;

    authenticated_session_response(user, expires_at, &session_token, &csrf_token)
}

/// Starts fresh passkey authentication bound to the current session.
pub(crate) async fn handle_start_fresh_authentication(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    headers: HeaderMap,
) -> ApiResult<Response> {
    actor.require_session()?;
    let session_id = require_current_session_id(&state, actor.id, &headers).await?;
    if !has_active_passkey(state.db(), actor.id).await? {
        return Err(ApiError::conflict("no active passkey"));
    }
    prune_terminal_ceremonies_best_effort(&state, actor.id).await;
    let challenge = challenge()?;
    let ceremony_id = create_fresh_authentication_ceremony(
        state.db(),
        actor.id,
        session_id,
        &challenge,
        &interval(CEREMONY_TTL),
    )
    .await?;
    no_store_json(authentication_options(&state, ceremony_id, &challenge))
}

/// Completes fresh passkey authentication for destructive credential operations.
pub(crate) async fn handle_finish_fresh_authentication(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    headers: HeaderMap,
    JsonBody(request): JsonBody<FinishAuthenticationRequest>,
) -> ApiResult<Response> {
    actor.require_session()?;
    let session_id = require_current_session_id(&state, actor.id, &headers).await?;
    let credential_id =
        request.credential.credential_id().map_err(|error| ApiError::bad_request(error.0))?;
    let replacement_session_token = generate_secret_token()?;
    let replacement_csrf_token = generate_secret_token()?;
    let mut tx = state.db().begin().await?;

    let _locked_user = require_active_user_locked(&mut tx, actor.id).await?;
    let stored = fetch_passkey(&mut tx, actor.id, &credential_id).await?;
    let ceremony =
        lock_fresh_authentication_ceremony(&mut tx, request.ceremony_id, actor.id, session_id)
            .await?
            .ok_or_else(|| ApiError::bad_request("invalid or expired ceremony"))?;
    let challenge = ceremony.challenge;
    let session_expires_at = ceremony.session_expires_at;
    let user_agent = ceremony.user_agent;
    let ip_address = ceremony.ip_address;
    let verified = verify_authentication(
        &request.credential,
        &stored.public_key,
        CeremonyExpectation {
            challenge: &challenge,
            origin: state.webauthn().origin_value(),
            rp_id: state.webauthn().rp_id(),
            alternate_origins: state.webauthn().alternate_origins(),
        },
    )
    .map_err(|_| ApiError::unauthorized("invalid passkey"))?;
    validate_user_handle(verified.user_handle.as_deref(), actor.id)?;
    validate_signature_count(stored.signature_count, verified.signature_count)?;

    record_passkey_use(&mut tx, stored.id, i64::from(verified.signature_count)).await?;
    consume_ceremony(&mut tx, request.ceremony_id).await?;

    let replacement_session_id = rotate_session_after_fresh_authentication(
        &mut tx,
        FreshAuthenticationSessionRotation {
            user_id: actor.id,
            previous_session_id: session_id,
            session_token_hash: security::hash_token(&replacement_session_token).as_slice(),
            csrf_token_hash: security::hash_token(&replacement_csrf_token).as_slice(),
            expires_at: session_expires_at,
            user_agent: user_agent.as_deref(),
            ip_address: ip_address.as_deref(),
        },
    )
    .await?
    .ok_or_else(|| ApiError::unauthorized("invalid session"))?;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::AuthPasskeyFreshAuthenticated,
                json!({
                    "passkey_id": stored.id,
                    "previous_session_id": session_id,
                    "session_id": replacement_session_id
                }),
            )
            .target_user(actor.id),
    )
    .await?;
    tx.commit().await?;
    prune_terminal_sessions_best_effort(&state).await;

    rotated_session_response(
        session_expires_at,
        &replacement_session_token,
        &replacement_csrf_token,
    )
}

/// Resolves the immutable user binding of a potentially valid login ceremony.
async fn require_login_ceremony_user_id(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ceremony_id: Uuid,
) -> ApiResult<Uuid> {
    login_ceremony_user_id(tx, ceremony_id).await?.ok_or_else(authentication_failed)
}

/// Locks and revalidates a login ceremony after its bound user has been locked.
async fn require_login_ceremony_locked(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ceremony_id: Uuid,
    user_id: Uuid,
) -> ApiResult<Vec<u8>> {
    lock_login_ceremony(tx, ceremony_id, user_id).await?.ok_or_else(authentication_failed)
}

/// Binds the required discoverable credential handle to the ceremony user.
fn validate_user_handle(handle: Option<&[u8]>, user_id: Uuid) -> ApiResult<()> {
    match handle {
        Some(handle) if handle == user_id.as_bytes() => Ok(()),
        Some(_) | None => Err(ApiError::unauthorized("invalid passkey user handle")),
    }
}

/// Detects non-advancing counters when an authenticator uses counters.
pub(super) fn validate_signature_count(stored: i64, received: u32) -> ApiResult<()> {
    if (stored != 0 || received != 0) && i64::from(received) <= stored {
        Err(ApiError::unauthorized("passkey signature counter did not advance"))
    } else {
        Ok(())
    }
}
