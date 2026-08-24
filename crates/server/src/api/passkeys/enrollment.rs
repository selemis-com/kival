//! Administrator-authorized passkey enrollment and recovery.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{ConnectInfo, State},
    http::HeaderMap,
    response::Response,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use kival_common::security;
use kival_kernel::{
    EventInsert, EventKind, PasskeyEnrollmentPurpose, active_credential_ids_in_tx,
    active_enrollment_ceremony, consume_ceremony, consume_enrollment_code,
    consume_expired_enrollment_ceremonies, create_enrollment_ceremony, create_passkey,
    enrollment_completion_user_id, has_active_passkey_in_tx, lock_active_user_by_id,
    lock_enrollment_completion, lock_enrollment_identity,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::{
    CEREMONY_TTL, challenge,
    credentials::normalized_label,
    interval, no_store_json, prune_terminal_ceremonies_best_effort, registration_options,
    sessions::{authenticated_session_response, insert_session},
};
use crate::{
    ServerState,
    api::{
        auth::{generate_secret_token, prune_terminal_sessions_best_effort},
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
        users::user_into_wire,
    },
    webauthn::{CeremonyExpectation, RegistrationCredential, verify_registration},
};

/// Prefix distinguishing enrollment capabilities from other Kival secrets.
const ENROLLMENT_CODE_PREFIX: &str = "kvl_enroll_";

/// Username and one-time code used to start administrator-authorized enrollment.
#[derive(Debug, Deserialize)]
pub(crate) struct EnrollmentOptionsRequest {
    /// Username to which the administrator bound the code.
    username: String,
    /// Raw enrollment code shown once to the administrator.
    code: String,
}

/// Browser registration response authorized by a username-bound enrollment code.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FinishEnrollmentRequest {
    /// Username to which the administrator bound the code.
    username: String,
    /// Raw enrollment code required again when committing the passkey.
    code: String,
    /// Single-use ceremony identifier returned by the options endpoint.
    ceremony_id: Uuid,
    /// Required user-visible passkey label.
    label: String,
    /// Browser-produced public-key registration response.
    credential: RegistrationCredential,
}

/// Starts passkey registration using an administrator-issued username-bound code.
pub(crate) async fn handle_start_enrollment(
    State(state): State<Arc<ServerState>>,
    JsonBody(request): JsonBody<EnrollmentOptionsRequest>,
) -> ApiResult<Response> {
    let username = request.username.trim();
    let code_hash = enrollment_code_hash(&request.code)?;
    if username.is_empty() {
        return Err(invalid_enrollment_code());
    }

    let mut tx = state.db().begin().await?;
    let identity = lock_enrollment_identity(&mut tx, code_hash.as_slice(), username)
        .await?
        .ok_or_else(invalid_enrollment_code)?;

    ensure_non_destructive_enrollment_allowed(&mut tx, identity.user_id, identity.purpose).await?;

    consume_expired_enrollment_ceremonies(&mut tx, identity.code_id).await?;

    let existing = active_enrollment_ceremony(&mut tx, identity.code_id).await?;
    let (ceremony_id, ceremony_challenge) = if let Some(existing) = existing {
        existing
    } else {
        let ceremony_challenge = challenge()?;
        let ceremony_id = create_enrollment_ceremony(
            &mut tx,
            identity.user_id,
            identity.code_id,
            &ceremony_challenge,
            &interval(CEREMONY_TTL),
        )
        .await?;
        (ceremony_id, ceremony_challenge)
    };

    let excluded = active_credential_ids_in_tx(&mut tx, identity.user_id).await?;
    tx.commit().await?;
    prune_terminal_ceremonies_best_effort(&state, identity.user_id).await;

    no_store_json(registration_options(
        &state,
        ceremony_id,
        &ceremony_challenge,
        identity.user_id,
        &identity.username,
        &identity.display_name,
        excluded,
    ))
}

/// Completes code-authorized registration and creates the normal browser session.
pub(crate) async fn handle_finish_enrollment(
    State(state): State<Arc<ServerState>>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    JsonBody(request): JsonBody<FinishEnrollmentRequest>,
) -> ApiResult<Response> {
    let username = request.username.trim();
    let code_hash = enrollment_code_hash(&request.code)?;
    if username.is_empty() {
        return Err(invalid_enrollment_code());
    }
    let label = normalized_label(&request.label)?;
    let session_token = generate_secret_token()?;
    let csrf_token = generate_secret_token()?;
    let mut tx = state.db().begin().await?;

    let user_id =
        enrollment_completion_user_id(&mut tx, code_hash.as_slice(), username, request.ceremony_id)
            .await?
            .ok_or_else(invalid_enrollment_code)?;
    let user = user_into_wire(
        lock_active_user_by_id(&mut tx, user_id).await?.ok_or_else(invalid_enrollment_code)?,
    );
    let completion = lock_enrollment_completion(
        &mut tx,
        user_id,
        code_hash.as_slice(),
        username,
        request.ceremony_id,
    )
    .await?
    .ok_or_else(invalid_enrollment_code)?;
    let code_id = completion.code_id;
    let challenge = completion.challenge;
    let purpose = completion.purpose;

    ensure_non_destructive_enrollment_allowed(&mut tx, user_id, purpose).await?;

    let verified = verify_registration(
        &request.credential,
        CeremonyExpectation {
            challenge: &challenge,
            origin: state.webauthn().origin_value(),
            rp_id: state.webauthn().rp_id(),
            alternate_origins: state.webauthn().alternate_origins(),
        },
    )
    .map_err(|error| ApiError::bad_request(error.0))?;
    let row = create_passkey(
        &mut tx,
        user_id,
        Some(label),
        &verified.credential_id,
        verified.public_key.as_slice(),
        i64::from(verified.signature_count),
    )
    .await?;
    consume_ceremony(&mut tx, request.ceremony_id).await?;
    consume_enrollment_code(&mut tx, code_id).await?;

    let expires_at =
        insert_session(&mut tx, user_id, &session_token, &csrf_token, &headers, peer_addr).await?;
    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        EventInsert::new(
            user_id,
            EventKind::AuthPasskeyEnrollmentCodeConsumed,
            json!({
                "enrollment_code_id": code_id,
                "passkey_id": row.id,
                "purpose": purpose.as_str()
            }),
        )
        .target_user(user_id),
    )
    .await?;
    tx.commit().await?;
    prune_terminal_sessions_best_effort(&state).await;

    authenticated_session_response(user, expires_at, &session_token, &csrf_token)
}

/// Rejects non-destructive administrative enrollment once a passkey exists.
async fn ensure_non_destructive_enrollment_allowed(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    purpose: PasskeyEnrollmentPurpose,
) -> ApiResult<()> {
    if purpose != PasskeyEnrollmentPurpose::Enrollment {
        return Ok(());
    }

    let has_passkey = has_active_passkey_in_tx(tx, user_id).await?;
    if has_passkey {
        Err(ApiError::conflict("user already has an active passkey; use passkey reset"))
    } else {
        Ok(())
    }
}

/// Validates an enrollment code's shape before deriving its database verifier.
fn enrollment_code_hash(code: &str) -> ApiResult<[u8; 32]> {
    if code.trim() != code {
        return Err(invalid_enrollment_code());
    }
    let encoded = code.strip_prefix(ENROLLMENT_CODE_PREFIX).ok_or_else(invalid_enrollment_code)?;
    let decoded = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| invalid_enrollment_code())?;
    if decoded.len() != 32 {
        return Err(invalid_enrollment_code());
    }
    Ok(security::hash_token(code))
}

/// Returns the non-enumerating error used for all invalid enrollment capabilities.
fn invalid_enrollment_code() -> ApiError {
    ApiError::unauthorized("invalid or expired enrollment code")
}
