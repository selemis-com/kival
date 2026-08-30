//! Registration and management of stored passkey credentials.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
    response::Response,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::SecondsFormat;
use kival_kernel::{
    EventKind, PasskeyRow, consume_ceremony, create_passkey, create_registration_ceremony,
    list_passkeys, lock_active_passkey_ids, lock_passkey, lock_registration_ceremony,
    registration_identity, revoke_passkey,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    CEREMONY_TTL, authentication_failed, challenge, descriptors, interval, no_store_json,
    prune_terminal_ceremonies_best_effort,
    sessions::{
        require_active_user_locked, require_current_session_id, require_fresh_session,
        require_fresh_session_in_tx,
    },
};
use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
    },
    webauthn::{CeremonyExpectation, RegistrationCredential, verify_registration},
};

/// Browser registration response paired with user metadata.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FinishRegistrationRequest {
    /// Single-use ceremony identifier.
    ceremony_id: Uuid,
    /// Required user-visible passkey label.
    label: String,
    /// Browser-produced public-key registration response.
    credential: RegistrationCredential,
}

/// Starts passkey enrollment for the current browser session.
pub(crate) async fn handle_start_registration(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    headers: HeaderMap,
) -> ApiResult<Response> {
    actor.require_session()?;
    let session_id = require_fresh_session(&state, actor.id, &headers).await?;
    prune_terminal_ceremonies_best_effort(&state, actor.id).await;
    let challenge = challenge()?;

    let (username, display_name, excluded) =
        registration_identity(state.db(), actor.id).await?.ok_or_else(authentication_failed)?;
    let ceremony_id = create_registration_ceremony(
        state.db(),
        actor.id,
        session_id,
        &challenge,
        &interval(CEREMONY_TTL),
    )
    .await?;

    let mut rp = json!({ "name": state.webauthn().rp_name() });
    if !state.webauthn().uses_implicit_rp_id() {
        rp["id"] = json!(state.webauthn().rp_id());
    }
    no_store_json(json!({
        "ceremonyId": ceremony_id,
        "publicKey": {
            "challenge": URL_SAFE_NO_PAD.encode(&challenge),
            "rp": rp,
            "user": {
                "id": URL_SAFE_NO_PAD.encode(actor.id.as_bytes()),
                "name": username,
                "displayName": display_name
            },
            "pubKeyCredParams": [{ "type": "public-key", "alg": -7 }],
            "timeout": CEREMONY_TTL.as_millis(),
            "attestation": "none",
            "authenticatorSelection": {
                "residentKey": "required",
                "requireResidentKey": true,
                "userVerification": "required"
            },
            "excludeCredentials": descriptors(excluded)
        }
    }))
}

/// Completes passkey enrollment for the current browser session.
pub(crate) async fn handle_finish_registration(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    headers: HeaderMap,
    JsonBody(request): JsonBody<FinishRegistrationRequest>,
) -> ApiResult<Json<Value>> {
    actor.require_session()?;
    let session_id = require_current_session_id(&state, actor.id, &headers).await?;
    let label = normalized_label(&request.label)?;
    let mut tx = state.db().begin().await?;

    let _locked_user = require_active_user_locked(&mut tx, actor.id).await?;
    let challenge = lock_registration_ceremony(&mut tx, request.ceremony_id, actor.id, session_id)
        .await?
        .ok_or_else(|| ApiError::bad_request("invalid or expired ceremony"))?;
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
        actor.id,
        Some(label),
        &verified.credential_id,
        verified.public_key.as_slice(),
        i64::from(verified.signature_count),
    )
    .await?;
    consume_ceremony(&mut tx, request.ceremony_id).await?;
    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(EventKind::AuthPasskeyEnrolled, json!({ "passkey_id": row.id }))
            .target_user(actor.id),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "passkey": passkey_json(&row)? })))
}

/// Lists the current user's passkeys without exposing public-key material.
pub(crate) async fn handle_list_passkeys(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
) -> ApiResult<Json<Value>> {
    actor.require_session()?;
    let rows = list_passkeys(state.db(), actor.id).await?;
    let items = rows.iter().map(passkey_json).collect::<ApiResult<Vec<_>>>()?;
    Ok(Json(json!({ "items": items })))
}

/// Revokes one passkey while preventing accidental removal of the last credential.
pub(crate) async fn handle_revoke_passkey(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    headers: HeaderMap,
    Path(passkey_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    actor.require_session()?;
    let mut tx = state.db().begin().await?;
    require_fresh_session_in_tx(&mut tx, actor.id, &headers).await?;
    let active_ids = lock_active_passkey_ids(&mut tx, actor.id).await?;
    if active_ids.len() <= 1 {
        return Err(ApiError::conflict("the last passkey cannot be removed"));
    }
    let row = revoke_passkey(&mut tx, passkey_id, actor.id).await?;
    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(EventKind::AuthPasskeyRevoked, json!({ "passkey_id": row.id }))
            .target_user(actor.id),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "passkey": passkey_json(&row)? })))
}

/// Locks one active passkey by owner and credential identifier.
pub(super) async fn fetch_passkey(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    credential_id: &[u8],
) -> ApiResult<PasskeyRow> {
    lock_passkey(tx, user_id, credential_id).await?.ok_or_else(authentication_failed)
}

/// Trims and validates a required user-visible passkey label.
pub(super) fn normalized_label(label: &str) -> ApiResult<&str> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 64 {
        Err(ApiError::bad_request("label must contain 1 to 64 characters"))
    } else {
        Ok(label)
    }
}

/// Serializes safe passkey metadata without public-key bytes.
fn passkey_json(row: &PasskeyRow) -> ApiResult<Value> {
    let created_at = row.created_at.to_rfc3339_opts(SecondsFormat::AutoSi, true);
    let updated_at = row.updated_at.to_rfc3339_opts(SecondsFormat::AutoSi, true);
    let last_used_at =
        row.last_used_at.map(|value| value.to_rfc3339_opts(SecondsFormat::AutoSi, true));
    let revoked_at = row.revoked_at.map(|value| value.to_rfc3339_opts(SecondsFormat::AutoSi, true));

    Ok(json!({
        "id": row.id,
        "userId": row.user_id,
        "credentialId": URL_SAFE_NO_PAD.encode(&row.credential_id),
        "label": row.label,
        "createdAt": created_at,
        "updatedAt": updated_at,
        "lastUsedAt": last_used_at,
        "revokedAt": revoked_at
    }))
}
