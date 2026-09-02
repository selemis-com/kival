use std::{collections::BTreeMap, sync::Mutex};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use reqwest::{
    Client, StatusCode,
    header::{HeaderMap, SET_COOKIE},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

use crate::actors::{Actor, FixtureUsers};

/// Default origin used by the local integration-test client.
pub const TEST_ORIGIN: &str = "http://localhost";
/// Relying-party ID used by deterministic test credentials.
pub const TEST_RP_ID: &str = "localhost";
/// Label assigned to passkey credentials installed by this fixture.
pub const TEST_PASSKEY_LABEL: &str = "Kival integration fixture";

/// Deterministic P-256 private scalar for the administrator fixture.
const ADMIN_PRIVATE_SCALAR_HEX: &str =
    "c5998f19b79d71429d6afe1cf78b48387c38d55e7583d3863bf02c866151155d";
/// Deterministic P-256 private scalar for Alice's fixture.
const ALICE_PRIVATE_SCALAR_HEX: &str =
    "faf5594b887a625ff26c14c27d8788b66990adaee933cf656c399c549878209e";
/// Deterministic P-256 private scalar for Bob's fixture.
const BOB_PRIVATE_SCALAR_HEX: &str =
    "3b28a36305f072a0255e3ba6ed79b0144459ac2037a6e4a04241f9023d239aa1";
/// Deterministic P-256 private scalar for Charlie's fixture.
const CHARLIE_PRIVATE_SCALAR_HEX: &str =
    "6098513d75b99ace709f9e9ae67d80afc1bf29b224bd4580aa679e81112a7f8e";
/// Deterministic P-256 private scalar for Dave's fixture.
const DAVE_PRIVATE_SCALAR_HEX: &str =
    "a61d5486c8b657b989523dd63393a524c143e8c95dcdfc84373761adafa04111";

/// Stable credential ID paired with the administrator's private scalar.
const ADMIN_CREDENTIAL_ID: &str = "7nzJlsD50dzO3Ztqi1GxMcvPkVHhT-GoByJwgLw0-pQ";
/// Stable credential ID paired with Alice's private scalar.
const ALICE_CREDENTIAL_ID: &str = "rfT0FjtQTgFDWI0ntTYCbiX7WtvzeEyh_HOZKVRQWpg";
/// Stable credential ID paired with Bob's private scalar.
const BOB_CREDENTIAL_ID: &str = "-p2DBJoFMbhtYfq5J68W3l7ATnmhXhBcnOr0SGrboLA";
/// Stable credential ID paired with Charlie's private scalar.
const CHARLIE_CREDENTIAL_ID: &str = "jykCNHfKcOwhAomK5zyGfSHATMYRYOTC4Uj3kS5vh98";
/// Stable credential ID paired with Dave's private scalar.
const DAVE_CREDENTIAL_ID: &str = "NMsaoa_Z3RbKaBSk5Zh_X1QTsfM1-mRrklUm96tAgeU";

impl Actor {
    /// Returns the deterministic private scalar assigned to this actor.
    pub(crate) const fn private_scalar_hex(self) -> &'static str {
        match self {
            Self::Admin => ADMIN_PRIVATE_SCALAR_HEX,
            Self::Alice => ALICE_PRIVATE_SCALAR_HEX,
            Self::Bob => BOB_PRIVATE_SCALAR_HEX,
            Self::Charlie => CHARLIE_PRIVATE_SCALAR_HEX,
            Self::Dave => DAVE_PRIVATE_SCALAR_HEX,
        }
    }

    /// Returns the stable base64url credential ID assigned to this actor.
    pub(crate) const fn credential_id_base64url(self) -> &'static str {
        match self {
            Self::Admin => ADMIN_CREDENTIAL_ID,
            Self::Alice => ALICE_CREDENTIAL_ID,
            Self::Bob => BOB_CREDENTIAL_ID,
            Self::Charlie => CHARLIE_CREDENTIAL_ID,
            Self::Dave => DAVE_CREDENTIAL_ID,
        }
    }
}

#[derive(Debug, Clone)]
/// Identity metadata written to the test database.
pub struct InstalledIdentity {
    /// Actor represented by the identity.
    pub actor: Actor,
    /// Database ID of the actor's user.
    pub user_id: Uuid,
    /// Canonical username used during passkey authentication.
    pub username: String,
    /// Binary `WebAuthn` credential ID.
    pub credential_id: Vec<u8>,
}

#[derive(Debug, Clone)]
/// Identities installed for every fixture actor.
pub struct InstalledIdentities {
    /// Installed identities indexed by actor.
    users: BTreeMap<Actor, InstalledIdentity>,
}

impl InstalledIdentities {
    /// Returns the identity installed for `actor`.
    ///
    /// # Panics
    ///
    /// Panics if this value was constructed without one of the fixture actors.
    #[must_use]
    pub fn get(&self, actor: Actor) -> &InstalledIdentity {
        self.users.get(&actor).expect("the fixture always installs all actors")
    }

    /// Iterates over actors and their installed identities.
    pub fn iter(&self) -> impl Iterator<Item = (&Actor, &InstalledIdentity)> {
        self.users.iter()
    }
}

#[derive(Debug)]
/// A deterministic software authenticator for integration tests.
pub struct TestPasskey {
    /// Actor that owns this passkey.
    actor: Actor,
    /// Deterministic signing key used for assertions.
    signing_key: SigningKey,
    /// Binary `WebAuthn` credential ID.
    credential_id: Vec<u8>,
    /// Monotonically increasing `WebAuthn` signature counter.
    signature_count: u32,
}

impl TestPasskey {
    /// Constructs the deterministic passkey assigned to `actor`.
    ///
    /// # Errors
    ///
    /// Returns an error if the embedded private scalar or credential ID is invalid.
    pub fn new(actor: Actor) -> Result<Self, PasskeyFixtureError> {
        let credential_id = URL_SAFE_NO_PAD.decode(actor.credential_id_base64url())?;
        Self::from_credential_id(actor, credential_id)
    }

    /// Constructs a deterministic passkey scoped to one fixture user.
    ///
    /// The derived credential ID prevents separate test cases from attempting
    /// to reassign immutable credential rows between newly provisioned users.
    ///
    /// # Errors
    ///
    /// Returns an error if the embedded private scalar is invalid.
    pub fn for_user(actor: Actor, user_id: Uuid) -> Result<Self, PasskeyFixtureError> {
        let base_credential_id = URL_SAFE_NO_PAD.decode(actor.credential_id_base64url())?;
        let mut material = Vec::with_capacity(base_credential_id.len() + user_id.as_bytes().len());
        material.extend_from_slice(&base_credential_id);
        material.extend_from_slice(user_id.as_bytes());
        Self::from_credential_id(actor, Sha256::digest(material).to_vec())
    }

    /// Constructs a passkey from deterministic key and credential material.
    fn from_credential_id(
        actor: Actor,
        credential_id: Vec<u8>,
    ) -> Result<Self, PasskeyFixtureError> {
        let scalar = hex::decode(actor.private_scalar_hex())?;
        let signing_key = SigningKey::from_slice(&scalar)
            .map_err(|_| PasskeyFixtureError::InvalidPrivateKey(actor))?;

        if credential_id.is_empty() {
            return Err(PasskeyFixtureError::EmptyCredentialId(actor));
        }

        Ok(Self { actor, signing_key, credential_id, signature_count: 0 })
    }

    /// Returns the actor that owns this passkey.
    #[must_use]
    pub const fn actor(&self) -> Actor {
        self.actor
    }

    /// Returns the binary credential ID.
    #[must_use]
    pub fn credential_id(&self) -> &[u8] {
        &self.credential_id
    }

    /// Returns the credential ID encoded as unpadded base64url.
    #[must_use]
    pub fn credential_id_base64url(&self) -> String {
        URL_SAFE_NO_PAD.encode(&self.credential_id)
    }

    /// Returns the SEC1 uncompressed public key.
    ///
    /// # Panics
    ///
    /// Panics if the P-256 library does not encode an uncompressed public key
    /// as exactly 65 bytes.
    #[must_use]
    pub fn uncompressed_public_key(&self) -> [u8; 65] {
        self.signing_key
            .verifying_key()
            .to_sec1_point(false)
            .as_bytes()
            .try_into()
            .expect("an uncompressed P-256 public key is always 65 bytes")
    }

    /// Creates a signed `WebAuthn` authentication assertion.
    ///
    /// # Errors
    ///
    /// Returns an error if the signature counter overflows or the client data
    /// cannot be serialized.
    pub fn assertion(
        &mut self,
        challenge_base64url: &str,
        rp_id: &str,
        origin: &str,
        user_id: Uuid,
    ) -> Result<AuthenticationCredential, PasskeyFixtureError> {
        self.signature_count = self
            .signature_count
            .checked_add(1)
            .ok_or(PasskeyFixtureError::SignatureCounterOverflow)?;

        let client_data_json = serde_json::to_vec(&CollectedClientData {
            ceremony_type: "webauthn.get",
            challenge: challenge_base64url,
            origin,
            cross_origin: false,
        })?;

        let mut authenticator_data = Vec::with_capacity(37);
        authenticator_data.extend_from_slice(&Sha256::digest(rp_id.as_bytes()));
        authenticator_data.push(0x05); // user present | user verified
        authenticator_data.extend_from_slice(&self.signature_count.to_be_bytes());

        let client_data_hash = Sha256::digest(&client_data_json);
        let mut signed_data = Vec::with_capacity(authenticator_data.len() + client_data_hash.len());
        signed_data.extend_from_slice(&authenticator_data);
        signed_data.extend_from_slice(&client_data_hash);

        let signature: Signature = self.signing_key.sign(&signed_data);
        let credential_id = self.credential_id_base64url();

        Ok(AuthenticationCredential {
            id: credential_id.clone(),
            raw_id: credential_id,
            credential_type: "public-key",
            response: AuthenticationResponse {
                authenticator_data: URL_SAFE_NO_PAD.encode(authenticator_data),
                client_data_json: URL_SAFE_NO_PAD.encode(client_data_json),
                signature: URL_SAFE_NO_PAD.encode(signature.to_der().as_bytes()),
                user_handle: Some(URL_SAFE_NO_PAD.encode(user_id.as_bytes())),
            },
        })
    }

    /// Performs the start and finish authentication requests for this passkey.
    ///
    /// # Errors
    ///
    /// Returns an error if an HTTP request fails, the server returns a non-success
    /// response, a response cannot be decoded, or assertion creation fails.
    pub async fn authenticate(
        &mut self,
        client: &Client,
        base_url: &str,
        user_id: Uuid,
        origin: &str,
    ) -> Result<AuthenticatedSessionResponse, PasskeyFixtureError> {
        self.authenticate_as(client, base_url, self.actor.username(), user_id, origin).await
    }

    /// Performs passkey authentication using an explicitly assigned username.
    ///
    /// # Errors
    ///
    /// Returns an error if an HTTP request fails, the server returns a non-success
    /// response, a response cannot be decoded, assertion creation fails, or the
    /// response omits its CSRF cookie.
    pub async fn authenticate_as(
        &mut self,
        client: &Client,
        base_url: &str,
        username: &str,
        user_id: Uuid,
        origin: &str,
    ) -> Result<AuthenticatedSessionResponse, PasskeyFixtureError> {
        let api = format!("{}/api/v1", base_url.trim_end_matches('/'));

        let options_response = client
            .post(format!("{api}/auth/passkey/authentication/options"))
            .json(&StartAuthenticationInput { username })
            .send()
            .await?;

        let options = decode_success::<AuthenticationOptions>(
            "start passkey authentication",
            options_response,
        )
        .await?;

        let rp_id = match options.public_key.rp_id {
            Some(rp_id) => rp_id,
            None => Url::parse(origin)?
                .host_str()
                .map(str::to_owned)
                .ok_or_else(|| PasskeyFixtureError::MissingOriginHost(origin.to_owned()))?,
        };
        let credential = self.assertion(&options.public_key.challenge, &rp_id, origin, user_id)?;

        let finish_response = client
            .post(format!("{api}/auth/passkey/authentication/finish"))
            .json(&FinishAuthenticationInput { ceremony_id: options.ceremony_id, credential })
            .send()
            .await?;

        let csrf_token = response_cookie(finish_response.headers(), "__Host-kival_csrf")
            .ok_or(PasskeyFixtureError::MissingCsrfCookie)?;
        let mut session: AuthenticatedSessionResponse =
            decode_success("finish passkey authentication", finish_response).await?;
        session.csrf_token = csrf_token;
        Ok(session)
    }
}

/// Performs login authentication with a passkey shared by multiple browser clients.
///
/// # Errors
///
/// Returns an error if the HTTP ceremony, assertion construction, or response decoding fails.
pub(crate) async fn authenticate_shared_as(
    passkey: &Mutex<TestPasskey>,
    client: &Client,
    base_url: &str,
    username: &str,
    user_id: Uuid,
    origin: &str,
) -> Result<AuthenticatedSessionResponse, PasskeyFixtureError> {
    let api = format!("{}/api/v1", base_url.trim_end_matches('/'));
    let options_response = client
        .post(format!("{api}/auth/passkey/authentication/options"))
        .json(&StartAuthenticationInput { username })
        .send()
        .await?;
    let options =
        decode_success::<AuthenticationOptions>("start passkey authentication", options_response)
            .await?;
    let credential = shared_assertion(passkey, &options, origin, user_id)?;
    let finish_response = client
        .post(format!("{api}/auth/passkey/authentication/finish"))
        .json(&FinishAuthenticationInput { ceremony_id: options.ceremony_id, credential })
        .send()
        .await?;
    let csrf_token = response_cookie(finish_response.headers(), "__Host-kival_csrf")
        .ok_or(PasskeyFixtureError::MissingCsrfCookie)?;
    let mut session: AuthenticatedSessionResponse =
        decode_success("finish passkey authentication", finish_response).await?;
    session.csrf_token = csrf_token;
    Ok(session)
}

/// Performs fresh authentication for an existing browser session and lets the server rotate it.
///
/// # Errors
///
/// Returns an error if the HTTP ceremony, assertion construction, or response decoding fails.
pub(crate) async fn fresh_authenticate_shared(
    passkey: &Mutex<TestPasskey>,
    client: &Client,
    base_url: &str,
    user_id: Uuid,
    origin: &str,
    csrf_token: &str,
) -> Result<(), PasskeyFixtureError> {
    let api = format!("{}/api/v1", base_url.trim_end_matches('/'));
    let options_response = client
        .post(format!("{api}/auth/passkeys/fresh/options"))
        .header("x-csrf-token", csrf_token)
        .send()
        .await?;
    let options = decode_success::<AuthenticationOptions>(
        "start fresh passkey authentication",
        options_response,
    )
    .await?;
    let credential = shared_assertion(passkey, &options, origin, user_id)?;
    let finish_response = client
        .post(format!("{api}/auth/passkeys/fresh/finish"))
        .header("x-csrf-token", csrf_token)
        .json(&FinishAuthenticationInput { ceremony_id: options.ceremony_id, credential })
        .send()
        .await?;
    let status = finish_response.status();
    if status != StatusCode::NO_CONTENT {
        let body = finish_response.bytes().await?;
        return Err(PasskeyFixtureError::UnexpectedHttpResponse {
            operation: "finish fresh passkey authentication",
            status,
            body: String::from_utf8_lossy(&body).into_owned(),
        });
    }
    Ok(())
}

/// Builds one assertion while serializing access to the shared signature counter.
fn shared_assertion(
    passkey: &Mutex<TestPasskey>,
    options: &AuthenticationOptions,
    origin: &str,
    user_id: Uuid,
) -> Result<AuthenticationCredential, PasskeyFixtureError> {
    let rp_id = match &options.public_key.rp_id {
        Some(rp_id) => rp_id.clone(),
        None => Url::parse(origin)?
            .host_str()
            .map(str::to_owned)
            .ok_or_else(|| PasskeyFixtureError::MissingOriginHost(origin.to_owned()))?,
    };
    let mut passkey = passkey.lock().map_err(|_| PasskeyFixtureError::AuthenticatorPoisoned)?;
    passkey.assertion(&options.public_key.challenge, &rp_id, origin, user_id)
}

/// Installs deterministic passkeys for all fixture actors.
///
/// Existing credentials with the same IDs are reset to the deterministic
/// fixture values.
///
/// # Errors
///
/// Returns an error if a fixture user is missing, deterministic credential
/// construction fails, or a database operation fails.
pub async fn install_test_identities(
    pool: &PgPool,
) -> Result<InstalledIdentities, PasskeyFixtureError> {
    let mut users = Vec::with_capacity(Actor::ALL.len());
    let mut transaction = pool.begin().await?;

    for actor in Actor::ALL {
        let username = actor.username();
        let user_id = find_active_user(&mut transaction, username).await?;
        users.push(crate::actors::FixtureUser::new(actor, user_id, username));
    }
    transaction.rollback().await?;

    install_test_identities_for(pool, &FixtureUsers::new(users)).await
}

/// Installs deterministic passkeys for explicitly assigned fixture users.
///
/// Existing credentials with the same IDs are reset to the deterministic
/// fixture values.
///
/// # Errors
///
/// Returns an error if an actor assignment is missing, deterministic credential
/// construction fails, or a database operation fails.
pub async fn install_test_identities_for(
    pool: &PgPool,
    fixture_users: &FixtureUsers,
) -> Result<InstalledIdentities, PasskeyFixtureError> {
    let mut transaction = pool.begin().await?;
    let mut users = BTreeMap::new();

    for actor in Actor::ALL {
        let fixture_user =
            fixture_users.get(actor).ok_or(PasskeyFixtureError::MissingActor(actor))?;
        let user_id = fixture_user.user_id;
        let passkey = TestPasskey::for_user(actor, user_id)?;
        let credential_id = passkey.credential_id().to_vec();
        let public_key = passkey.uncompressed_public_key();

        sqlx::query(
            r#"
            INSERT INTO kival.passkey_credentials (
                user_id,
                credential_id,
                public_key,
                label,
                signature_count
            )
            VALUES ($1, $2, $3, $4, 0)
            ON CONFLICT (credential_id) DO UPDATE
            SET
                user_id = EXCLUDED.user_id,
                public_key = EXCLUDED.public_key,
                label = EXCLUDED.label,
                signature_count = 0,
                updated_at = now(),
                last_used_at = NULL,
                revoked_at = NULL,
                revoked_by = NULL,
                revoked_by_operator = false,
                revocation_reason = NULL
            "#,
        )
        .bind(user_id)
        .bind(&credential_id)
        .bind(public_key.as_slice())
        .bind(TEST_PASSKEY_LABEL)
        .execute(&mut *transaction)
        .await?;

        users.insert(
            actor,
            InstalledIdentity {
                actor,
                user_id,
                username: fixture_user.username.clone(),
                credential_id,
            },
        );
    }

    transaction.commit().await?;
    Ok(InstalledIdentities { users })
}

/// Finds the active database user with `username`.
async fn find_active_user(
    transaction: &mut Transaction<'_, Postgres>,
    username: &'static str,
) -> Result<Uuid, PasskeyFixtureError> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM kival.users
        WHERE username = $1
            AND disabled_at IS NULL
        "#,
    )
    .bind(username)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(PasskeyFixtureError::MissingUser(username))
}

/// Decodes a successful JSON response or preserves its error body.
async fn decode_success<T>(
    operation: &'static str,
    response: reqwest::Response,
) -> Result<T, PasskeyFixtureError>
where
    T: for<'de> Deserialize<'de>,
{
    let status = response.status();
    let body = response.bytes().await?;

    if status != StatusCode::OK {
        return Err(PasskeyFixtureError::UnexpectedHttpResponse {
            operation,
            status,
            body: String::from_utf8_lossy(&body).into_owned(),
        });
    }

    Ok(serde_json::from_slice(&body)?)
}

/// Extracts one cookie value from response `Set-Cookie` headers.
fn response_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get_all(SET_COOKIE).iter().filter_map(|value| value.to_str().ok()).find_map(|cookie| {
        cookie
            .split(';')
            .next()
            .and_then(|pair| pair.split_once('='))
            .filter(|(cookie_name, _)| *cookie_name == name)
            .map(|(_, value)| value.to_owned())
    })
}

#[derive(Debug, Serialize)]
/// Request body that begins passkey authentication.
struct StartAuthenticationInput<'a> {
    /// Username whose authentication options are requested.
    username: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// Authentication options returned by the server.
struct AuthenticationOptions {
    /// Server-side authentication ceremony identifier.
    ceremony_id: String,
    /// `WebAuthn` public-key request options.
    public_key: AuthenticationPublicKeyOptions,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// Public-key fields needed to construct an assertion.
struct AuthenticationPublicKeyOptions {
    /// Base64url challenge issued by the server.
    challenge: String,
    /// Optional relying-party ID covered by authenticator data.
    ///
    /// Browsers derive this from the current origin when the server omits it.
    rp_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// Request body that finishes passkey authentication.
struct FinishAuthenticationInput {
    /// Server-side authentication ceremony identifier.
    ceremony_id: String,
    /// Signed credential response.
    credential: AuthenticationCredential,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// `WebAuthn` credential submitted to finish authentication.
pub struct AuthenticationCredential {
    /// Base64url credential ID.
    pub id: String,
    /// Base64url raw credential ID.
    pub raw_id: String,
    /// `WebAuthn` credential type, always `public-key`.
    #[serde(rename = "type")]
    pub credential_type: &'static str,
    /// Signed authenticator response.
    pub response: AuthenticationResponse,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// Authenticator response fields in a `WebAuthn` assertion.
pub struct AuthenticationResponse {
    /// Base64url authenticator data.
    pub authenticator_data: String,
    /// Base64url collected client data JSON.
    #[serde(rename = "clientDataJSON")]
    pub client_data_json: String,
    /// Base64url DER-encoded assertion signature.
    pub signature: String,
    /// Optional base64url user handle.
    pub user_handle: Option<String>,
}

#[derive(Debug, Serialize)]
/// Browser client data covered by the assertion signature.
struct CollectedClientData<'a> {
    /// `WebAuthn` ceremony type.
    #[serde(rename = "type")]
    ceremony_type: &'static str,
    /// Challenge issued by the server.
    challenge: &'a str,
    /// Origin from which the request is made.
    origin: &'a str,
    /// Whether the ceremony crosses origins.
    #[serde(rename = "crossOrigin")]
    cross_origin: bool,
}

#[derive(Debug, Deserialize)]
/// Successful authenticated-session response.
pub struct AuthenticatedSessionResponse {
    /// Server-formatted session expiry timestamp.
    pub expires_at: String,
    /// User authenticated by the new session.
    pub user: AuthenticatedUser,
    /// CSRF token extracted from the authentication response cookie.
    #[serde(skip)]
    pub csrf_token: String,
}

#[derive(Debug, Deserialize)]
/// User details returned after successful authentication.
pub struct AuthenticatedUser {
    /// Database user ID.
    pub id: Uuid,
    /// Login name.
    pub username: String,
    /// Human-readable name.
    pub display_name: String,
}

#[derive(Debug, Error)]
/// Failure while installing or using a deterministic passkey.
pub enum PasskeyFixtureError {
    /// An explicit fixture assignment omitted an actor.
    #[error("fixture user assignment is missing {0:?}")]
    MissingActor(Actor),

    /// Authentication succeeded without issuing the required CSRF cookie.
    #[error("passkey authentication response omitted its CSRF cookie")]
    MissingCsrfCookie,

    /// A shared deterministic authenticator was poisoned by a prior panic.
    #[error("shared deterministic authenticator is poisoned")]
    AuthenticatorPoisoned,

    /// An embedded private scalar is not valid hexadecimal.
    #[error("invalid hex in deterministic test credential")]
    InvalidHex(#[from] hex::FromHexError),

    /// An embedded credential ID is not valid base64url.
    #[error("invalid base64url in deterministic test credential")]
    InvalidBase64(#[from] base64::DecodeError),

    /// An embedded scalar is not a valid P-256 private key.
    #[error("invalid deterministic P-256 private key for {0:?}")]
    InvalidPrivateKey(Actor),

    /// An embedded credential ID decoded to an empty value.
    #[error("credential ID for {0:?} is empty")]
    EmptyCredentialId(Actor),

    /// The authenticator signature counter cannot be incremented.
    #[error("signature counter overflow")]
    SignatureCounterOverflow,

    /// An expected active fixture user was not found.
    #[error("expected active test user `{0}` does not exist")]
    MissingUser(&'static str),

    /// A browser origin did not contain the host needed to derive an omitted RP ID.
    #[error("test origin does not contain a host: {0}")]
    MissingOriginHost(String),

    /// A browser origin was not a valid URL.
    #[error("invalid test origin URL")]
    InvalidOrigin(#[from] url::ParseError),

    /// The server returned a non-success response.
    #[error("{operation} returned HTTP {status}: {body}")]
    UnexpectedHttpResponse {
        /// Operation that received the response.
        operation: &'static str,
        /// Unexpected HTTP status.
        status: StatusCode,
        /// Response body retained for diagnostics.
        body: String,
    },

    /// A database operation failed.
    #[error("database error")]
    Database(#[from] sqlx::Error),

    /// An HTTP operation failed.
    #[error("HTTP error")]
    Http(#[from] reqwest::Error),

    /// JSON serialization or deserialization failed.
    #[error("JSON error")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_credential_ids_are_distinct() {
        let credentials = Actor::ALL.map(|actor| TestPasskey::new(actor).expect("valid fixture"));

        for credential in &credentials {
            assert_eq!(credential.uncompressed_public_key()[0], 0x04);
            assert_eq!(credential.uncompressed_public_key().len(), 65);
            assert!(!credential.credential_id().is_empty());
        }

        for (index, credential) in credentials.iter().enumerate() {
            for other in &credentials[index + 1..] {
                assert_ne!(credential.uncompressed_public_key(), other.uncompressed_public_key());
                assert_ne!(credential.credential_id(), other.credential_id());
            }
        }
    }
}
