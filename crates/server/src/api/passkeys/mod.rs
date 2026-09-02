//! Passkey authentication and credential management.

use std::time::{Duration, Instant};

use axum::{
    Json,
    http::{HeaderValue, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use kival_kernel::prune_terminal_ceremonies;
use kival_tracing::warn;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    ServerState,
    api::{
        error::{ApiError, ApiResult},
        maintenance::record_cleanup_rows,
    },
};

mod authentication;
mod credentials;
mod enrollment;
mod sessions;

pub(crate) use authentication::{
    handle_finish_authentication, handle_finish_fresh_authentication, handle_start_authentication,
    handle_start_fresh_authentication,
};
pub(crate) use credentials::{
    handle_finish_registration, handle_list_passkeys, handle_revoke_passkey,
    handle_start_registration,
};
pub(crate) use enrollment::{handle_finish_enrollment, handle_start_enrollment};
pub(crate) use sessions::require_fresh_session_in_tx;

/// Lifetime of registration and assertion challenges.
const CEREMONY_TTL: Duration = Duration::from_secs(5 * 60);
/// Maximum terminal ceremonies deleted by one opportunistic cleanup.
const CEREMONY_CLEANUP_BATCH_SIZE: i64 = 128;

/// Builds browser-compatible public-key registration options.
fn registration_options(
    state: &ServerState,
    ceremony_id: Uuid,
    challenge: &[u8],
    user_id: Uuid,
    username: &str,
    display_name: &str,
    excluded: Vec<Vec<u8>>,
) -> Value {
    let mut rp = json!({ "name": state.webauthn().rp_name() });
    if !state.webauthn().uses_implicit_rp_id() {
        rp["id"] = json!(state.webauthn().rp_id());
    }
    json!({
        "ceremonyId": ceremony_id,
        "publicKey": {
            "challenge": URL_SAFE_NO_PAD.encode(challenge),
            "rp": rp,
            "user": {
                "id": URL_SAFE_NO_PAD.encode(user_id.as_bytes()),
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
    })
}

/// Builds browser-compatible discoverable-credential assertion options.
fn authentication_options(state: &ServerState, ceremony_id: Uuid, challenge: &[u8]) -> Value {
    let mut options = json!({
        "ceremonyId": ceremony_id,
        "publicKey": {
            "challenge": URL_SAFE_NO_PAD.encode(challenge),
            "allowCredentials": [],
            "timeout": CEREMONY_TTL.as_millis(),
            "userVerification": "required"
        }
    });
    if !state.webauthn().uses_implicit_rp_id() {
        options["publicKey"]["rpId"] = json!(state.webauthn().rp_id());
    }
    options
}

/// Converts opaque credential identifiers into browser descriptors.
fn descriptors(ids: Vec<Vec<u8>>) -> Vec<Value> {
    ids.into_iter()
        .map(|id| json!({ "type": "public-key", "id": URL_SAFE_NO_PAD.encode(id) }))
        .collect()
}

/// Returns a JSON response that prevents storage of ceremony secrets and options.
fn no_store_json(value: Value) -> ApiResult<Response> {
    let mut response = Json(value).into_response();
    response.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Ok(response)
}

/// Best-effort pruning keeps short-lived ceremony state from accumulating forever.
async fn prune_terminal_ceremonies_best_effort(state: &ServerState, user_id: Uuid) {
    let started_at = Instant::now();
    let result = prune_terminal_ceremonies(state.db(), user_id, CEREMONY_CLEANUP_BATCH_SIZE).await;

    record_cleanup_rows("webauthn_ceremonies", started_at, &result);

    if let Err(error) = result {
        warn!(
            target: "kival::server::passkeys",
            %error,
            "failed to prune terminal WebAuthn ceremonies"
        );
    }
}

/// Generates one cryptographically random 32-byte ceremony challenge.
fn challenge() -> ApiResult<Vec<u8>> {
    let mut challenge = [0_u8; 32];
    SystemRandom::new()
        .fill(&mut challenge)
        .map_err(|_| ApiError::internal("challenge generation failed"))?;
    Ok(challenge.to_vec())
}

/// Formats a trusted duration as a `PostgreSQL` interval parameter.
fn interval(duration: Duration) -> String {
    format!("{} seconds", duration.as_secs())
}

/// Returns the single public failure used by passkey login completion.
fn authentication_failed() -> ApiError {
    ApiError::unauthorized("authentication failed")
}

#[cfg(test)]
use authentication::{FinishAuthenticationRequest, validate_signature_count};
#[cfg(test)]
use credentials::normalized_label;
#[cfg(test)]
use enrollment::FinishEnrollmentRequest;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_requests_accept_browser_json_casing() {
        let ceremony_id = Uuid::nil();
        let authentication = json!({
            "ceremonyId": ceremony_id,
            "credential": {
                "id": "credential",
                "rawId": "credential",
                "type": "public-key",
                "response": {
                    "authenticatorData": "authenticator-data",
                    "clientDataJSON": "client-data",
                    "signature": "signature",
                    "userHandle": null
                }
            }
        });
        assert!(serde_json::from_value::<FinishAuthenticationRequest>(authentication).is_ok());

        let enrollment = json!({
            "username": "kival-user",
            "code": "kvl_enroll_code",
            "ceremonyId": ceremony_id,
            "label": "Work laptop",
            "credential": {
                "id": "credential",
                "rawId": "credential",
                "type": "public-key",
                "response": {
                    "clientDataJSON": "client-data",
                    "attestationObject": "attestation"
                }
            }
        });
        assert!(serde_json::from_value::<FinishEnrollmentRequest>(enrollment).is_ok());
    }

    #[test]
    fn passkey_labels_are_required_and_bounded() {
        assert!(normalized_label("").is_err());
        assert!(normalized_label("   ").is_err());
        assert_eq!(normalized_label("  Work laptop  ").unwrap(), "Work laptop");
        assert!(normalized_label(&"x".repeat(65)).is_err());
    }

    #[test]
    fn signature_counter_policy_allows_zero_and_requires_strict_advancement() {
        assert!(validate_signature_count(0, 0).is_ok());
        assert!(validate_signature_count(0, 1).is_ok());
        assert!(validate_signature_count(7, 8).is_ok());

        assert!(validate_signature_count(7, 7).is_err());
        assert!(validate_signature_count(7, 6).is_err());
        assert!(validate_signature_count(7, 0).is_err());
    }
}
