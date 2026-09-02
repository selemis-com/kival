//! Narrow `WebAuthn` verification for Kival passkeys.
//!
//! Kival intentionally supports only the `WebAuthn` subset it needs: discoverable
//! P-256/ES256 credentials, no attestation, and required user verification.

use std::{error::Error, io::Cursor};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ciborium::Value;
use coset::{CborSerializable, CoseKey, Label, iana};
use ring::{digest, signature};
use serde::{Deserialize, Deserializer, de::IgnoredAny};
use url::{Host, Origin, Url};

/// `WebAuthn` relying-party settings used for every passkey ceremony.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebAuthnConfig {
    /// Canonical browser origin serialized for browser-facing responses.
    origin: String,
    /// Accepted browser origins and the relying-party identifier each must sign.
    origins: Vec<(Origin, String)>,
    /// Human-readable relying-party name displayed during enrollment.
    rp_name: String,
    /// Whether browser options should derive their RP ID from the current accepted host.
    implicit_rp_id: bool,
}

/// Fixed relying-party display name shown by authenticators.
const RP_NAME: &str = "Kival";

impl WebAuthnConfig {
    /// Derives and validates Kival `WebAuthn` settings from its canonical public URL.
    ///
    /// # Errors
    ///
    /// Returns an error when the public URL is not a plain HTTP(S) origin URL or its host cannot
    /// be used as Kival's exact relying-party identifier.
    pub fn from_canonical_url(canonical_url: &str) -> Result<Self, WebAuthnConfigError> {
        Self::from_canonical_url_with_allowed_origins(canonical_url, &[])
    }

    /// Derives settings from the canonical URL and additional exact browser origins.
    ///
    /// Local development deployments also include `localhost` on ports 3000 and 5173.
    /// IP-address origins are rejected because `WebAuthn` RP IDs must be valid domain strings.
    ///
    /// # Errors
    ///
    /// Returns an error when any configured value is not a supported HTTP(S) origin URL.
    pub fn from_canonical_url_with_allowed_origins(
        canonical_url: &str,
        allowed_origins: &[String],
    ) -> Result<Self, WebAuthnConfigError> {
        let canonical_url = validated_canonical_url(canonical_url)?;
        let rp_id = rp_id_for_url(&canonical_url)?;
        let loopback = is_loopback_url(&canonical_url);
        let origin_value = canonical_url.origin();
        let origin = origin_value.ascii_serialization();
        let mut origins = vec![(origin_value, rp_id)];

        if loopback {
            for value in ["http://localhost:3000", "http://localhost:5173"] {
                let url = validated_canonical_url(value)?;
                let rp_id = rp_id_for_url(&url)?;
                let candidate = url.origin();
                if origins.iter().all(|(accepted, _)| *accepted != candidate) {
                    origins.push((candidate, rp_id));
                }
            }
        }

        for value in allowed_origins {
            let url = validated_canonical_url(value)?;
            let rp_id = rp_id_for_url(&url)?;
            let candidate = url.origin();
            if origins.iter().all(|(accepted, _)| *accepted != candidate) {
                origins.push((candidate, rp_id));
            }
        }

        let implicit_rp_id = origins.len() > 1;

        Ok(Self { origin, origins, rp_name: RP_NAME.to_owned(), implicit_rp_id })
    }

    /// Returns the exact accepted browser origin.
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// Returns the parsed accepted browser origin.
    pub(crate) fn origin_value(&self) -> &Origin {
        &self.origins[0].0
    }

    /// Returns the relying-party identifier.
    pub(crate) fn rp_id(&self) -> &str {
        &self.origins[0].1
    }

    /// Returns additional accepted origins and their corresponding RP IDs.
    pub(crate) fn alternate_origins(&self) -> &[(Origin, String)] {
        &self.origins[1..]
    }

    /// Returns whether the browser should derive its RP ID from the current origin host.
    pub(crate) const fn uses_implicit_rp_id(&self) -> bool {
        self.implicit_rp_id
    }

    /// Returns the relying-party display name.
    pub(crate) fn rp_name(&self) -> &str {
        &self.rp_name
    }
}

/// Derives the exact RP ID signed for an accepted origin.
fn rp_id_for_url(url: &Url) -> Result<String, WebAuthnConfigError> {
    match url.host().ok_or(WebAuthnConfigError("Kival public URL must contain a hostname"))? {
        Host::Domain(hostname) => validated_hostname(hostname),
        Host::Ipv4(_) | Host::Ipv6(_) => {
            Err(WebAuthnConfigError("Kival public URL must use a hostname, not an IP address"))
        }
    }
}

/// Returns whether an origin uses one of the explicitly supported loopback hosts.
fn is_loopback_url(url: &Url) -> bool {
    matches!(url.host(), Some(Host::Domain("localhost")))
}

/// Invalid startup configuration for Kival's narrow `WebAuthn` profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebAuthnConfigError(&'static str);

impl std::fmt::Display for WebAuthnConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for WebAuthnConfigError {}

/// Parses the canonical public URL and rejects components outside an origin tuple.
fn validated_canonical_url(canonical_url: &str) -> Result<Url, WebAuthnConfigError> {
    if canonical_url.trim() != canonical_url {
        return Err(WebAuthnConfigError(
            "Kival public URL must not contain surrounding whitespace",
        ));
    }
    let parsed =
        Url::parse(canonical_url).map_err(|_| WebAuthnConfigError("invalid Kival public URL"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(WebAuthnConfigError(
            "Kival public URL must contain only an HTTP(S) scheme, hostname, and optional port",
        ));
    }
    if parsed.scheme() == "http" && parsed.host_str() != Some("localhost") {
        return Err(WebAuthnConfigError(
            "insecure Kival public URLs are supported only for loopback hosts",
        ));
    }
    Ok(parsed)
}

/// Validates a canonical URL hostname before using it as the RP ID.
fn validated_hostname(hostname: &str) -> Result<String, WebAuthnConfigError> {
    if hostname.is_empty()
        || hostname.len() > 253
        || hostname.starts_with('.')
        || hostname.ends_with('.')
        || hostname.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(WebAuthnConfigError("invalid Kival public URL hostname"));
    }
    Ok(hostname.to_owned())
}

/// Maximum decoded size accepted for general browser-supplied binary fields.
const MAX_FIELD_BYTES: usize = 16 * 1024;
/// `WebAuthn` recommended maximum credential identifier size.
const MAX_CREDENTIAL_ID_BYTES: usize = 1023;
/// Maximum discoverable credential user-handle size defined by `WebAuthn`.
const MAX_USER_HANDLE_BYTES: usize = 64;
/// Conservative bound for an ASN.1 DER encoded P-256 signature.
const MAX_SIGNATURE_BYTES: usize = 256;

/// Browser response returned by `navigator.credentials.create()`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RegistrationCredential {
    /// Base64url-encoded credential identifier.
    id: String,
    /// Base64url-encoded raw credential identifier.
    raw_id: String,
    /// Credential type, which must be `public-key`.
    #[serde(rename = "type")]
    kind: String,
    /// Authenticator registration response.
    response: RegistrationResponse,
}

/// Binary fields returned by a registration ceremony.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistrationResponse {
    /// Base64url-encoded collected client data JSON.
    #[serde(rename = "clientDataJSON")]
    client_data_json: String,
    /// Base64url-encoded CBOR attestation object.
    attestation_object: String,
}

/// Browser response returned by `navigator.credentials.get()`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AuthenticationCredential {
    /// Base64url-encoded credential identifier.
    id: String,
    /// Base64url-encoded raw credential identifier.
    raw_id: String,
    /// Credential type, which must be `public-key`.
    #[serde(rename = "type")]
    kind: String,
    /// Authenticator assertion response.
    response: AuthenticationResponse,
}

impl AuthenticationCredential {
    /// Decodes and cross-checks the two browser credential identifiers.
    pub(crate) fn credential_id(&self) -> Result<Vec<u8>, VerificationError> {
        validate_credential_ids(&self.id, &self.raw_id)
    }
}

/// Binary fields returned by an authentication ceremony.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthenticationResponse {
    /// Base64url-encoded authenticator data.
    authenticator_data: String,
    /// Base64url-encoded collected client data JSON.
    #[serde(rename = "clientDataJSON")]
    client_data_json: String,
    /// Base64url-encoded ASN.1 ECDSA signature.
    signature: String,
    /// Optional base64url-encoded discoverable credential user handle.
    user_handle: Option<String>,
}

/// Public credential material accepted from a registration ceremony.
#[derive(Debug)]
pub(crate) struct VerifiedRegistration {
    /// Authenticator-assigned opaque credential identifier.
    pub(crate) credential_id: Vec<u8>,
    /// Uncompressed SEC1 P-256 public key.
    pub(crate) public_key: [u8; 65],
    /// Authenticator signature counter at enrollment.
    pub(crate) signature_count: u32,
}

/// Security state accepted from an authentication ceremony.
#[derive(Debug)]
pub(crate) struct VerifiedAuthentication {
    /// Authenticator signature counter after this assertion.
    pub(crate) signature_count: u32,
    /// Optional discoverable credential user handle.
    pub(crate) user_handle: Option<Vec<u8>>,
}

/// Server values to which a ceremony response must be bound.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CeremonyExpectation<'a> {
    /// Random, single-use challenge issued by Kival.
    pub(crate) challenge: &'a [u8],
    /// Parsed browser origin configured for this deployment.
    pub(crate) origin: &'a Origin,
    /// Relying-party identifier configured for this deployment.
    pub(crate) rp_id: &'a str,
    /// Additional accepted origins and their corresponding RP IDs.
    pub(crate) alternate_origins: &'a [(Origin, String)],
}

/// Rejection of a malformed or cryptographically invalid ceremony response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VerificationError(pub(crate) &'static str);

impl std::fmt::Display for VerificationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for VerificationError {}

/// Tracks whether an unsupported client-data member was present, including as JSON null.
#[derive(Debug, Default)]
enum FieldPresence {
    /// The member was absent.
    #[default]
    Absent,
    /// The member was present with any JSON value.
    Present,
}

/// Marks an unsupported client-data member as present while discarding its value.
fn deserialize_presence<'de, D>(deserializer: D) -> Result<FieldPresence, D::Error>
where
    D: Deserializer<'de>,
{
    let _ignored = IgnoredAny::deserialize(deserializer)?;
    Ok(FieldPresence::Present)
}

/// Collected client data fields used by Kival's origin and challenge checks.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientData {
    /// Ceremony discriminator supplied by the browser.
    #[serde(rename = "type")]
    kind: String,
    /// Base64url-encoded server challenge.
    challenge: String,
    /// Browser origin that requested the ceremony.
    origin: String,
    /// Whether the browser reports a cross-origin request.
    #[serde(default)]
    cross_origin: bool,
    /// Presence of a cross-origin top-level origin, which Kival does not support.
    #[serde(default, rename = "topOrigin", deserialize_with = "deserialize_presence")]
    top_origin: FieldPresence,
}

/// Parsed fixed header and original bytes of authenticator data.
#[derive(Debug)]
struct AuthenticatorData<'a> {
    /// Complete authenticator data used for signature verification.
    bytes: &'a [u8],
    /// Authenticator flags byte.
    flags: u8,
    /// Big-endian authenticator signature counter.
    signature_count: u32,
}

/// Verifies a registration response and extracts its public credential material.
pub(crate) fn verify_registration(
    credential: &RegistrationCredential,
    expected: CeremonyExpectation<'_>,
) -> Result<VerifiedRegistration, VerificationError> {
    validate_credential_type(&credential.kind)?;
    let credential_id = validate_credential_ids(&credential.id, &credential.raw_id)?;
    let client_data_json = decode_field(
        &credential.response.client_data_json,
        MAX_FIELD_BYTES,
        "invalid client data encoding",
    )?;
    let rp_id = validate_client_data(&client_data_json, "webauthn.create", expected)?;

    let attestation_object = decode_field(
        &credential.response.attestation_object,
        MAX_FIELD_BYTES,
        "invalid attestation object encoding",
    )?;
    let attestation = parse_attestation_object(&attestation_object)?;
    let authenticator = parse_authenticator_data(&attestation.authenticator_data, rp_id)?;
    validate_authenticator_flags(authenticator.flags, true)?;

    let data = authenticator.bytes;
    let _aaguid =
        data.get(37..53).ok_or(VerificationError("attested credential data is truncated"))?;
    let credential_id_length_bytes: [u8; 2] = data
        .get(53..55)
        .ok_or(VerificationError("attested credential data is truncated"))?
        .try_into()
        .map_err(|_| VerificationError("attested credential data is truncated"))?;
    let credential_id_length = usize::from(u16::from_be_bytes(credential_id_length_bytes));
    if credential_id_length == 0 || credential_id_length > MAX_CREDENTIAL_ID_BYTES {
        return Err(VerificationError("attested credential ID length is invalid"));
    }
    let credential_id_end = 55_usize
        .checked_add(credential_id_length)
        .ok_or(VerificationError("credential ID length is invalid"))?;
    let attested_credential_id =
        data.get(55..credential_id_end).ok_or(VerificationError("credential ID is truncated"))?;
    require_equal_bytes(attested_credential_id, &credential_id, "credential ID mismatch")?;

    let cose_and_extensions = data
        .get(credential_id_end..)
        .ok_or(VerificationError("credential public key is missing"))?;
    let (public_key, key_length) = parse_es256_public_key(cose_and_extensions)?;
    validate_extensions_and_trailing(
        &cose_and_extensions[key_length..],
        authenticator.flags & FLAG_ED != 0,
    )?;

    Ok(VerifiedRegistration {
        credential_id,
        public_key,
        signature_count: authenticator.signature_count,
    })
}

/// Verifies an assertion response against the stored P-256 public key.
pub(crate) fn verify_authentication(
    credential: &AuthenticationCredential,
    public_key: &[u8],
    expected: CeremonyExpectation<'_>,
) -> Result<VerifiedAuthentication, VerificationError> {
    validate_credential_type(&credential.kind)?;
    validate_credential_ids(&credential.id, &credential.raw_id)?;
    if public_key.len() != 65 || public_key[0] != 0x04 {
        return Err(VerificationError("stored credential public key is invalid"));
    }

    let client_data_json = decode_field(
        &credential.response.client_data_json,
        MAX_FIELD_BYTES,
        "invalid client data encoding",
    )?;
    let rp_id = validate_client_data(&client_data_json, "webauthn.get", expected)?;
    let authenticator_data = decode_field(
        &credential.response.authenticator_data,
        MAX_FIELD_BYTES,
        "invalid authenticator data encoding",
    )?;
    let authenticator = parse_authenticator_data(&authenticator_data, rp_id)?;
    validate_authenticator_flags(authenticator.flags, false)?;
    validate_extensions_and_trailing(
        &authenticator.bytes[37..],
        authenticator.flags & FLAG_ED != 0,
    )?;

    let signature_bytes = decode_field(
        &credential.response.signature,
        MAX_SIGNATURE_BYTES,
        "invalid signature encoding",
    )?;
    let client_hash = digest::digest(&digest::SHA256, &client_data_json);
    let mut signed = Vec::with_capacity(authenticator_data.len() + client_hash.as_ref().len());
    signed.extend_from_slice(&authenticator_data);
    signed.extend_from_slice(client_hash.as_ref());
    signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, public_key)
        .verify(&signed, &signature_bytes)
        .map_err(|_| VerificationError("invalid WebAuthn signature"))?;

    let user_handle = credential
        .response
        .user_handle
        .as_deref()
        .map(|value| decode_field(value, MAX_USER_HANDLE_BYTES, "invalid user handle encoding"))
        .transpose()?;

    Ok(VerifiedAuthentication { signature_count: authenticator.signature_count, user_handle })
}

/// User-presence flag in authenticator data.
const FLAG_UP: u8 = 0x01;
/// User-verification flag in authenticator data.
const FLAG_UV: u8 = 0x04;
/// Backup-eligibility flag in authenticator data.
const FLAG_BE: u8 = 0x08;
/// Backup-state flag in authenticator data.
const FLAG_BS: u8 = 0x10;
/// Attested-credential-data flag in authenticator data.
const FLAG_AT: u8 = 0x40;
/// Extension-data flag in authenticator data.
const FLAG_ED: u8 = 0x80;
/// Flags whose semantics Kival explicitly understands.
const KNOWN_FLAGS: u8 = FLAG_UP | FLAG_UV | FLAG_BE | FLAG_BS | FLAG_AT | FLAG_ED;

/// Requires the public-key credential type used by `WebAuthn`.
fn validate_credential_type(kind: &str) -> Result<(), VerificationError> {
    if kind == "public-key" {
        Ok(())
    } else {
        Err(VerificationError("credential type is not public-key"))
    }
}

/// Decodes `id` and `rawId` and requires them to identify the same bounded credential.
fn validate_credential_ids(id: &str, raw_id: &str) -> Result<Vec<u8>, VerificationError> {
    let id = decode_field(id, MAX_CREDENTIAL_ID_BYTES, "invalid credential ID encoding")?;
    let raw_id =
        decode_field(raw_id, MAX_CREDENTIAL_ID_BYTES, "invalid raw credential ID encoding")?;
    if id.is_empty() {
        return Err(VerificationError("credential ID is empty"));
    }
    require_equal_bytes(&id, &raw_id, "credential ID mismatch")?;
    Ok(raw_id)
}

/// Binds collected client data to the expected type, origin, and challenge.
fn validate_client_data<'a>(
    encoded: &[u8],
    expected_type: &str,
    expected: CeremonyExpectation<'a>,
) -> Result<&'a str, VerificationError> {
    let client_data: ClientData = serde_json::from_slice(encoded)
        .map_err(|_| VerificationError("invalid client data JSON"))?;
    if client_data.kind != expected_type {
        return Err(VerificationError("unexpected WebAuthn ceremony type"));
    }
    let origin = parse_client_origin(&client_data.origin)?;
    let rp_id = if origin == *expected.origin {
        expected.rp_id
    } else {
        expected
            .alternate_origins
            .iter()
            .find_map(|(accepted, rp_id)| (origin == *accepted).then_some(rp_id.as_str()))
            .ok_or(VerificationError("WebAuthn origin mismatch"))?
    };
    if client_data.cross_origin {
        return Err(VerificationError("cross-origin WebAuthn is not allowed"));
    }
    if matches!(client_data.top_origin, FieldPresence::Present) {
        return Err(VerificationError("topOrigin is not supported"));
    }
    let challenge =
        decode_field(&client_data.challenge, MAX_USER_HANDLE_BYTES, "invalid challenge encoding")?;
    require_equal_bytes(&challenge, expected.challenge, "challenge mismatch")?;
    Ok(rp_id)
}

/// Parses an untrusted client origin as an origin tuple with no URL credentials or extra parts.
fn parse_client_origin(value: &str) -> Result<Origin, VerificationError> {
    let parsed = Url::parse(value).map_err(|_| VerificationError("invalid WebAuthn origin"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(VerificationError("invalid WebAuthn origin"));
    }
    Ok(parsed.origin())
}

/// Parses the fixed authenticator-data header and validates its RP-ID hash.
fn parse_authenticator_data<'a>(
    bytes: &'a [u8],
    rp_id: &str,
) -> Result<AuthenticatorData<'a>, VerificationError> {
    if bytes.len() < 37 {
        return Err(VerificationError("authenticator data is truncated"));
    }
    let expected_hash = digest::digest(&digest::SHA256, rp_id.as_bytes());
    require_equal_bytes(&bytes[..32], expected_hash.as_ref(), "RP ID hash mismatch")?;
    Ok(AuthenticatorData {
        bytes,
        flags: bytes[32],
        signature_count: u32::from_be_bytes([bytes[33], bytes[34], bytes[35], bytes[36]]),
    })
}

/// Enforces Kival's understood authenticator flags and ceremony-specific AT requirement.
const fn validate_authenticator_flags(
    flags: u8,
    registration: bool,
) -> Result<(), VerificationError> {
    if flags & !KNOWN_FLAGS != 0 {
        return Err(VerificationError("unsupported authenticator flags"));
    }
    if flags & FLAG_UP == 0 {
        return Err(VerificationError("user presence is required"));
    }
    if flags & FLAG_UV == 0 {
        return Err(VerificationError("user verification is required"));
    }
    if registration && flags & FLAG_AT == 0 {
        return Err(VerificationError("attested credential data is missing"));
    }
    if !registration && flags & FLAG_AT != 0 {
        return Err(VerificationError("authentication contains attested credential data"));
    }
    if flags & FLAG_BS != 0 && flags & FLAG_BE == 0 {
        return Err(VerificationError("backup state is set without backup eligibility"));
    }
    Ok(())
}

/// Compares public protocol values and maps mismatch to a stable verifier error.
fn require_equal_bytes(
    left: &[u8],
    right: &[u8],
    message: &'static str,
) -> Result<(), VerificationError> {
    if left == right { Ok(()) } else { Err(VerificationError(message)) }
}

/// Strictly decodes an unpadded base64url browser field within its decoded size limit.
fn decode_field(
    value: &str,
    maximum: usize,
    message: &'static str,
) -> Result<Vec<u8>, VerificationError> {
    let maximum_encoded = maximum
        .checked_add(2)
        .and_then(|value| value.checked_div(3))
        .and_then(|value| value.checked_mul(4))
        .ok_or(VerificationError(message))?;
    if value.len() > maximum_encoded || value.contains('=') {
        return Err(VerificationError(message));
    }
    let decoded = URL_SAFE_NO_PAD.decode(value).map_err(|_| VerificationError(message))?;
    if decoded.len() > maximum {
        return Err(VerificationError(message));
    }
    Ok(decoded)
}

/// Required members extracted from a none-format attestation object.
struct AttestationObject {
    /// Authenticator data embedded in the attestation object.
    authenticator_data: Vec<u8>,
}

/// Parses the top-level attestation object with ciborium and enforces none attestation.
///
/// Kival deliberately accepts only `fmt = "none"` with an empty statement. In particular, packed
/// self-attestation remains outside this narrow profile even when a client could return it after an
/// `attestation = "none"` request; supporting it would require an explicit policy expansion.
fn parse_attestation_object(bytes: &[u8]) -> Result<AttestationObject, VerificationError> {
    let (value, consumed) = parse_cbor_value(bytes, "invalid attestation object CBOR")?;
    if consumed != bytes.len() {
        return Err(VerificationError("trailing attestation object data"));
    }
    let Value::Map(entries) = value else {
        return Err(VerificationError("attestation object must be a CBOR map"));
    };

    let mut format = None;
    let mut authenticator_data = None;
    let mut statement_seen = false;
    for (key, value) in entries {
        let Value::Text(key) = key else {
            return Err(VerificationError("attestation object key must be text"));
        };
        match key.as_str() {
            "fmt" => {
                if format.is_some() {
                    return Err(VerificationError("duplicate attestation format"));
                }
                let Value::Text(value) = value else {
                    return Err(VerificationError("attestation format must be text"));
                };
                format = Some(value);
            }
            "authData" => {
                if authenticator_data.is_some() {
                    return Err(VerificationError("duplicate authenticator data"));
                }
                let Value::Bytes(value) = value else {
                    return Err(VerificationError("authenticator data must be bytes"));
                };
                authenticator_data = Some(value);
            }
            "attStmt" => {
                if statement_seen {
                    return Err(VerificationError("duplicate attestation statement"));
                }
                statement_seen = true;
                let Value::Map(statement) = value else {
                    return Err(VerificationError("attestation statement must be a map"));
                };
                if !statement.is_empty() {
                    return Err(VerificationError("attestation statement must be empty"));
                }
            }
            _ => return Err(VerificationError("unexpected attestation object member")),
        }
    }

    if format.as_deref() != Some("none") {
        return Err(VerificationError("attestation format must be none"));
    }
    if !statement_seen {
        return Err(VerificationError("attestation statement is missing"));
    }
    Ok(AttestationObject {
        authenticator_data: authenticator_data
            .ok_or(VerificationError("authenticator data is missing"))?,
    })
}

/// Parses an EC2 COSE key with coset and independently enforces P-256/ES256 public material.
fn parse_es256_public_key(bytes: &[u8]) -> Result<([u8; 65], usize), VerificationError> {
    let (value, consumed) = parse_cbor_value(bytes, "invalid credential public key CBOR")?;
    let Value::Map(entries) = value else {
        return Err(VerificationError("credential public key must be a CBOR map"));
    };
    for (position, (label, _)) in entries.iter().enumerate() {
        if entries[..position].iter().any(|(existing, _)| existing == label) {
            return Err(VerificationError("duplicate credential public key parameter"));
        }
    }
    let encoded =
        bytes.get(..consumed).ok_or(VerificationError("credential public key is truncated"))?;
    let key = CoseKey::from_slice(encoded)
        .map_err(|_| VerificationError("invalid credential public key COSE"))?;
    if key.kty != coset::KeyType::Assigned(iana::KeyType::EC2) {
        return Err(VerificationError("credential public key type must be EC2"));
    }
    if key.alg != Some(coset::Algorithm::Assigned(iana::Algorithm::ES256)) {
        return Err(VerificationError("credential public key algorithm must be ES256"));
    }
    if !key.key_id.is_empty() || !key.key_ops.is_empty() || !key.base_iv.is_empty() {
        return Err(VerificationError("credential public key contains unsupported parameters"));
    }

    let mut curve_valid = false;
    let mut x = None;
    let mut y = None;
    for (label, value) in &key.params {
        match label {
            Label::Int(-1) => {
                let Value::Integer(curve) = value else {
                    return Err(VerificationError("credential public key curve is malformed"));
                };
                if i128::from(*curve) != i128::from(iana::EllipticCurve::P_256 as i64) {
                    return Err(VerificationError("credential public key curve must be P-256"));
                }
                curve_valid = true;
            }
            Label::Int(-2) => {
                let Value::Bytes(coordinate) = value else {
                    return Err(VerificationError("credential public key x is malformed"));
                };
                x = Some(coordinate.as_slice());
            }
            Label::Int(-3) => {
                let Value::Bytes(coordinate) = value else {
                    return Err(VerificationError("credential public key y is malformed"));
                };
                y = Some(coordinate.as_slice());
            }
            Label::Int(-4) => {
                return Err(VerificationError("credential public key contains private material"));
            }
            _ => {
                return Err(VerificationError(
                    "credential public key contains unsupported parameters",
                ));
            }
        }
    }
    if !curve_valid {
        return Err(VerificationError("credential public key curve is missing"));
    }
    let x = x.ok_or(VerificationError("credential public key x is missing"))?;
    let y = y.ok_or(VerificationError("credential public key y is missing"))?;
    if x.len() != 32 || y.len() != 32 {
        return Err(VerificationError("credential public key coordinates are invalid"));
    }

    let mut public_key = [0_u8; 65];
    public_key[0] = 0x04;
    public_key[1..33].copy_from_slice(x);
    public_key[33..].copy_from_slice(y);
    Ok((public_key, consumed))
}

/// Parses extensions when flagged and otherwise rejects trailing authenticator data.
fn validate_extensions_and_trailing(
    bytes: &[u8],
    extensions_present: bool,
) -> Result<(), VerificationError> {
    if !extensions_present {
        return if bytes.is_empty() {
            Ok(())
        } else {
            Err(VerificationError("unexpected trailing authenticator data"))
        };
    }

    let (value, consumed) = parse_cbor_value(bytes, "invalid authenticator extension CBOR")?;
    if consumed != bytes.len() {
        return Err(VerificationError("trailing authenticator extension data"));
    }
    let Value::Map(entries) = value else {
        return Err(VerificationError("authenticator extensions must be a CBOR map"));
    };
    for (position, (key, _)) in entries.iter().enumerate() {
        if entries[..position].iter().any(|(existing, _)| existing == key) {
            return Err(VerificationError("duplicate authenticator extension"));
        }
    }
    Ok(())
}

/// Decodes exactly one bounded CBOR value and reports the number of bytes it consumed.
fn parse_cbor_value(
    bytes: &[u8],
    message: &'static str,
) -> Result<(Value, usize), VerificationError> {
    if bytes.is_empty() || bytes.len() > MAX_FIELD_BYTES {
        return Err(VerificationError(message));
    }
    let mut cursor = Cursor::new(bytes);
    let value = ciborium::de::from_reader(&mut cursor).map_err(|_| VerificationError(message))?;
    let consumed = usize::try_from(cursor.position()).map_err(|_| VerificationError(message))?;
    Ok((value, consumed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn integer(value: i64) -> Value {
        Value::Integer(value.into())
    }

    fn cbor(value: &Value) -> Vec<u8> {
        let mut encoded = Vec::new();
        ciborium::ser::into_writer(value, &mut encoded).expect("test CBOR must encode");
        encoded
    }

    fn valid_cose_entries() -> Vec<(Value, Value)> {
        vec![
            (integer(1), integer(2)),
            (integer(3), integer(-7)),
            (integer(-1), integer(1)),
            (integer(-2), Value::Bytes(vec![1_u8; 32])),
            (integer(-3), Value::Bytes(vec![2_u8; 32])),
        ]
    }

    fn valid_cose_key() -> Vec<u8> {
        cbor(&Value::Map(valid_cose_entries()))
    }

    fn client_data(
        kind: &str,
        challenge: &[u8],
        origin: &str,
        cross_origin: bool,
        top_origin: bool,
    ) -> Vec<u8> {
        let mut value = serde_json::json!({
            "type": kind,
            "challenge": URL_SAFE_NO_PAD.encode(challenge),
            "origin": origin,
            "crossOrigin": cross_origin
        });
        if top_origin {
            value["topOrigin"] = serde_json::Value::Null;
        }
        serde_json::to_vec(&value).expect("test client data must encode")
    }

    fn expected_origin() -> Origin {
        Url::parse("https://kival.example").expect("test origin must parse").origin()
    }

    fn authenticator_header(rp_id: &str, flags: u8, counter: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(digest::digest(&digest::SHA256, rp_id.as_bytes()).as_ref());
        data.push(flags);
        data.extend_from_slice(&counter.to_be_bytes());
        data
    }

    fn registration_authenticator_data(
        rp_id: &str,
        flags: u8,
        credential_id: &[u8],
        cose_key: &[u8],
    ) -> Vec<u8> {
        let mut data = authenticator_header(rp_id, flags, 0);
        data.extend_from_slice(&[0_u8; 16]);
        data.extend_from_slice(
            &u16::try_from(credential_id.len())
                .expect("test credential ID length must fit")
                .to_be_bytes(),
        );
        data.extend_from_slice(credential_id);
        data.extend_from_slice(cose_key);
        data
    }

    fn attestation(entries: Vec<(Value, Value)>) -> Vec<u8> {
        cbor(&Value::Map(entries))
    }

    fn none_attestation(authenticator_data: Vec<u8>) -> Vec<u8> {
        attestation(vec![
            (Value::Text("fmt".to_owned()), Value::Text("none".to_owned())),
            (Value::Text("authData".to_owned()), Value::Bytes(authenticator_data)),
            (Value::Text("attStmt".to_owned()), Value::Map(Vec::new())),
        ])
    }

    fn registration_credential(
        outer_id: &[u8],
        authenticator_data: Vec<u8>,
        challenge: &[u8],
    ) -> RegistrationCredential {
        let encoded_id = URL_SAFE_NO_PAD.encode(outer_id);
        RegistrationCredential {
            id: encoded_id.clone(),
            raw_id: encoded_id,
            kind: "public-key".to_owned(),
            response: RegistrationResponse {
                client_data_json: URL_SAFE_NO_PAD.encode(client_data(
                    "webauthn.create",
                    challenge,
                    "https://kival.example",
                    false,
                    false,
                )),
                attestation_object: URL_SAFE_NO_PAD.encode(none_attestation(authenticator_data)),
            },
        }
    }

    fn authentication_credential(
        credential_id: &[u8],
        authenticator_data: &[u8],
        client_data_json: &[u8],
        signature: &[u8],
        user_handle: Option<&[u8]>,
    ) -> AuthenticationCredential {
        let encoded_id = URL_SAFE_NO_PAD.encode(credential_id);
        AuthenticationCredential {
            id: encoded_id.clone(),
            raw_id: encoded_id,
            kind: "public-key".to_owned(),
            response: AuthenticationResponse {
                authenticator_data: URL_SAFE_NO_PAD.encode(authenticator_data),
                client_data_json: URL_SAFE_NO_PAD.encode(client_data_json),
                signature: URL_SAFE_NO_PAD.encode(signature),
                user_handle: user_handle.map(|value| URL_SAFE_NO_PAD.encode(value)),
            },
        }
    }

    #[test]
    fn client_data_requires_exact_type_challenge_and_normalized_origin() {
        let challenge = [7_u8; 32];
        let origin = expected_origin();
        let expected = CeremonyExpectation {
            challenge: &challenge,
            origin: &origin,
            rp_id: "kival.example",
            alternate_origins: &[],
        };

        let normalized =
            client_data("webauthn.get", &challenge, "https://kival.example:443", false, false);
        assert!(validate_client_data(&normalized, "webauthn.get", expected).is_ok());

        let wrong_type =
            client_data("webauthn.create", &challenge, "https://kival.example", false, false);
        assert!(validate_client_data(&wrong_type, "webauthn.get", expected).is_err());

        let wrong_challenge =
            client_data("webauthn.get", &[8_u8; 32], "https://kival.example", false, false);
        assert!(validate_client_data(&wrong_challenge, "webauthn.get", expected).is_err());
    }

    #[test]
    fn client_data_rejects_untrusted_origin_variants() {
        let challenge = [7_u8; 32];
        let origin = expected_origin();
        let expected = CeremonyExpectation {
            challenge: &challenge,
            origin: &origin,
            rp_id: "kival.example",
            alternate_origins: &[],
        };
        for candidate in [
            "http://kival.example",
            "https://other.example",
            "https://kival.example:444",
            "https://kival.example.attacker.test",
            "https://user@kival.example",
            "not an origin",
        ] {
            let encoded = client_data("webauthn.get", &challenge, candidate, false, false);
            assert!(
                validate_client_data(&encoded, "webauthn.get", expected).is_err(),
                "{candidate} should be rejected"
            );
        }
    }

    #[test]
    fn client_data_rejects_cross_origin_and_any_top_origin_member() {
        let challenge = [7_u8; 32];
        let origin = expected_origin();
        let expected = CeremonyExpectation {
            challenge: &challenge,
            origin: &origin,
            rp_id: "kival.example",
            alternate_origins: &[],
        };
        let cross_origin =
            client_data("webauthn.get", &challenge, "https://kival.example", true, false);
        assert!(validate_client_data(&cross_origin, "webauthn.get", expected).is_err());
        let top_origin =
            client_data("webauthn.get", &challenge, "https://kival.example", false, true);
        assert!(validate_client_data(&top_origin, "webauthn.get", expected).is_err());
    }

    #[test]
    fn attestation_object_intentionally_requires_exact_none_representation() {
        let valid = none_attestation(vec![0_u8; 37]);
        assert!(parse_attestation_object(&valid).is_ok());

        let malformed = [0xff];
        assert!(parse_attestation_object(&malformed).is_err());
        assert!(parse_attestation_object(&cbor(&Value::Array(Vec::new()))).is_err());
        assert!(parse_attestation_object(&cbor(&Value::Map(Vec::new()))).is_err());

        let wrong_format = attestation(vec![
            (Value::Text("fmt".to_owned()), Value::Text("packed".to_owned())),
            (Value::Text("authData".to_owned()), Value::Bytes(vec![0_u8; 37])),
            (Value::Text("attStmt".to_owned()), Value::Map(Vec::new())),
        ]);
        assert!(parse_attestation_object(&wrong_format).is_err());

        let nonempty_statement = attestation(vec![
            (Value::Text("fmt".to_owned()), Value::Text("none".to_owned())),
            (Value::Text("authData".to_owned()), Value::Bytes(vec![0_u8; 37])),
            (
                Value::Text("attStmt".to_owned()),
                Value::Map(vec![(Value::Text("unexpected".to_owned()), Value::Null)]),
            ),
        ]);
        assert!(parse_attestation_object(&nonempty_statement).is_err());
    }

    #[test]
    fn attestation_object_rejects_wrong_types_duplicates_and_trailing_data() {
        let duplicate = attestation(vec![
            (Value::Text("fmt".to_owned()), Value::Text("none".to_owned())),
            (Value::Text("fmt".to_owned()), Value::Text("none".to_owned())),
            (Value::Text("authData".to_owned()), Value::Bytes(vec![0_u8; 37])),
            (Value::Text("attStmt".to_owned()), Value::Map(Vec::new())),
        ]);
        assert!(parse_attestation_object(&duplicate).is_err());

        let wrong_type = attestation(vec![
            (Value::Text("fmt".to_owned()), Value::Integer(0.into())),
            (Value::Text("authData".to_owned()), Value::Text("nope".to_owned())),
            (Value::Text("attStmt".to_owned()), Value::Array(Vec::new())),
        ]);
        assert!(parse_attestation_object(&wrong_type).is_err());

        let mut trailing = none_attestation(vec![0_u8; 37]);
        trailing.push(0);
        assert!(parse_attestation_object(&trailing).is_err());
    }

    #[test]
    fn cose_key_accepts_only_the_exact_es256_p256_profile() {
        let key = valid_cose_key();
        let (public_key, consumed) = parse_es256_public_key(&key).expect("valid key");
        assert_eq!(consumed, key.len());
        assert_eq!(&public_key[1..33], &[1_u8; 32]);
        assert_eq!(&public_key[33..], &[2_u8; 32]);

        for (position, replacement) in [(0, integer(3)), (1, integer(-8)), (2, integer(2))] {
            let mut entries = valid_cose_entries();
            entries[position].1 = replacement;
            assert!(parse_es256_public_key(&cbor(&Value::Map(entries))).is_err());
        }
    }

    #[test]
    fn cose_key_rejects_missing_malformed_duplicate_and_private_parameters() {
        for missing_label in [-2_i64, -3_i64] {
            let entries = valid_cose_entries()
                .into_iter()
                .filter(|(label, _)| label != &integer(missing_label))
                .collect();
            assert!(parse_es256_public_key(&cbor(&Value::Map(entries))).is_err());
        }

        for coordinate_label in [-2_i64, -3_i64] {
            let mut entries = valid_cose_entries();
            let (_, coordinate) = entries
                .iter_mut()
                .find(|(label, _)| label == &integer(coordinate_label))
                .expect("coordinate must exist");
            *coordinate = Value::Bytes(vec![0_u8; 31]);
            assert!(parse_es256_public_key(&cbor(&Value::Map(entries))).is_err());
        }

        let mut malformed = valid_cose_entries();
        malformed[3].1 = Value::Text("not bytes".to_owned());
        assert!(parse_es256_public_key(&cbor(&Value::Map(malformed))).is_err());

        let mut duplicate = valid_cose_entries();
        duplicate.push((integer(-2), Value::Bytes(vec![3_u8; 32])));
        assert!(parse_es256_public_key(&cbor(&Value::Map(duplicate))).is_err());

        let mut private = valid_cose_entries();
        private.push((integer(-4), Value::Bytes(vec![4_u8; 32])));
        assert!(parse_es256_public_key(&cbor(&Value::Map(private))).is_err());

        assert!(parse_es256_public_key(&[0xff]).is_err());
    }

    #[test]
    fn authenticator_data_enforces_rp_hash_flags_and_extensions() {
        let valid = authenticator_header("kival.example", FLAG_UP | FLAG_UV, 0);
        let parsed = parse_authenticator_data(&valid, "kival.example").expect("valid header");
        assert!(validate_authenticator_flags(parsed.flags, false).is_ok());
        assert!(parse_authenticator_data(&valid, "other.example").is_err());
        assert!(parse_authenticator_data(&valid[..36], "kival.example").is_err());

        for flags in [FLAG_UV, FLAG_UP, FLAG_UP | FLAG_UV | 0x02, FLAG_UP | FLAG_UV | FLAG_BS] {
            assert!(validate_authenticator_flags(flags, false).is_err());
        }
        assert!(validate_authenticator_flags(FLAG_UP | FLAG_UV, true).is_err());
        assert!(validate_authenticator_flags(FLAG_UP | FLAG_UV | FLAG_AT, false).is_err());
        assert!(validate_authenticator_flags(FLAG_UP | FLAG_UV | FLAG_BE | FLAG_BS, false).is_ok());

        assert!(validate_extensions_and_trailing(&[], false).is_ok());
        assert!(validate_extensions_and_trailing(&[0], false).is_err());
        assert!(validate_extensions_and_trailing(&[0xff], true).is_err());
        let extension =
            cbor(&Value::Map(vec![(Value::Text("example".to_owned()), Value::Bool(true))]));
        assert!(validate_extensions_and_trailing(&extension, true).is_ok());
        let mut extension_trailing = extension;
        extension_trailing.push(0);
        assert!(validate_extensions_and_trailing(&extension_trailing, true).is_err());
    }

    #[test]
    fn registration_binds_outer_and_attested_credential_identifiers() {
        let challenge = [7_u8; 32];
        let origin = expected_origin();
        let credential_id = [9_u8; 32];
        let auth_data = registration_authenticator_data(
            "kival.example",
            FLAG_UP | FLAG_UV | FLAG_AT,
            &credential_id,
            &valid_cose_key(),
        );
        let credential = registration_credential(&credential_id, auth_data, &challenge);
        let expected = CeremonyExpectation {
            challenge: &challenge,
            origin: &origin,
            rp_id: "kival.example",
            alternate_origins: &[],
        };
        assert!(verify_registration(&credential, expected).is_ok());

        let mismatched = registration_authenticator_data(
            "kival.example",
            FLAG_UP | FLAG_UV | FLAG_AT,
            &[8_u8; 32],
            &valid_cose_key(),
        );
        let credential = registration_credential(&credential_id, mismatched, &challenge);
        assert!(verify_registration(&credential, expected).is_err());

        let mut trailing_key = valid_cose_key();
        trailing_key.push(0);
        let trailing = registration_authenticator_data(
            "kival.example",
            FLAG_UP | FLAG_UV | FLAG_AT,
            &credential_id,
            &trailing_key,
        );
        let credential = registration_credential(&credential_id, trailing, &challenge);
        assert!(verify_registration(&credential, expected).is_err());
    }

    #[test]
    fn credential_identifiers_are_bounded_and_cross_checked() {
        let id = URL_SAFE_NO_PAD.encode([1_u8; 32]);
        let other = URL_SAFE_NO_PAD.encode([2_u8; 32]);
        assert!(validate_credential_ids(&id, &id).is_ok());
        assert!(validate_credential_ids(&id, &other).is_err());
        assert!(validate_credential_ids("", "").is_err());
        let oversized = URL_SAFE_NO_PAD.encode(vec![0_u8; MAX_CREDENTIAL_ID_BYTES + 1]);
        assert!(validate_credential_ids(&oversized, &oversized).is_err());
    }
    #[test]
    fn authentication_verifies_the_exact_original_signed_bytes() {
        use ring::{
            rand::SystemRandom,
            signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair},
        };

        let random = SystemRandom::new();
        let document = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random)
            .expect("test key generation must succeed");
        let key_pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, document.as_ref(), &random)
                .expect("test key must parse");
        let challenge = [7_u8; 32];
        let origin = expected_origin();
        let expected = CeremonyExpectation {
            challenge: &challenge,
            origin: &origin,
            rp_id: "kival.example",
            alternate_origins: &[],
        };
        let authenticator_data = authenticator_header("kival.example", FLAG_UP | FLAG_UV, 9);
        let client_data_json =
            client_data("webauthn.get", &challenge, "https://kival.example", false, false);
        let client_hash = digest::digest(&digest::SHA256, &client_data_json);
        let mut signed = authenticator_data.clone();
        signed.extend_from_slice(client_hash.as_ref());
        let signature = key_pair.sign(&random, &signed).expect("test signing must succeed");
        let credential_id = [3_u8; 32];
        let user_handle = [4_u8; 16];
        let credential = authentication_credential(
            &credential_id,
            &authenticator_data,
            &client_data_json,
            signature.as_ref(),
            Some(&user_handle),
        );

        let verified = verify_authentication(&credential, key_pair.public_key().as_ref(), expected)
            .expect("valid assertion must verify");
        assert_eq!(verified.signature_count, 9);
        assert_eq!(verified.user_handle.as_deref(), Some(user_handle.as_slice()));

        let mut modified_client_data = client_data_json;
        modified_client_data.push(b' ');
        let modified = authentication_credential(
            &credential_id,
            &authenticator_data,
            &modified_client_data,
            signature.as_ref(),
            None,
        );
        assert!(
            verify_authentication(&modified, key_pair.public_key().as_ref(), expected).is_err()
        );
    }

    #[test]
    fn browser_credential_json_uses_webauthn_acronym_casing() {
        let registration = serde_json::json!({
            "id": "credential",
            "rawId": "credential",
            "type": "public-key",
            "response": {
                "clientDataJSON": "client-data",
                "attestationObject": "attestation"
            }
        });
        assert!(serde_json::from_value::<RegistrationCredential>(registration).is_ok());

        let authentication = serde_json::json!({
            "id": "credential",
            "rawId": "credential",
            "type": "public-key",
            "response": {
                "authenticatorData": "authenticator-data",
                "clientDataJSON": "client-data",
                "signature": "signature",
                "userHandle": null
            }
        });
        assert!(serde_json::from_value::<AuthenticationCredential>(authentication).is_ok());
    }

    #[test]
    fn canonical_url_derives_exact_webauthn_configuration() {
        for (canonical_url, expected_origin, expected_rp_id) in [
            ("https://kival.example.com", "https://kival.example.com", "kival.example.com"),
            (
                "https://kival.example.com:8443",
                "https://kival.example.com:8443",
                "kival.example.com",
            ),
            ("http://localhost", "http://localhost", "localhost"),
            ("http://localhost:3000", "http://localhost:3000", "localhost"),
        ] {
            let config = WebAuthnConfig::from_canonical_url(canonical_url)
                .unwrap_or_else(|error| panic!("{canonical_url} should be valid: {error}"));

            assert_eq!(config.origin(), expected_origin);
            assert_eq!(config.rp_id(), expected_rp_id);
            assert_eq!(config.rp_name(), "Kival");
        }
    }

    #[test]
    fn canonical_url_canonicalizes_default_ports() {
        let config = WebAuthnConfig::from_canonical_url("https://kival.example.com:443")
            .expect("default HTTPS port should be valid");
        assert_eq!(config.origin(), "https://kival.example.com");
        assert_eq!(config.rp_id(), "kival.example.com");
    }

    #[test]
    fn canonical_url_accepts_configured_origins_with_matching_rp_ids() {
        let config = WebAuthnConfig::from_canonical_url_with_allowed_origins(
            "https://kival.example.com",
            &["https://kival.internal.example".to_owned(), "https://kival.lan:8443".to_owned()],
        )
        .expect("additional HTTPS origins should be valid");

        assert!(config.uses_implicit_rp_id());
        assert_eq!(config.alternate_origins().len(), 2);
        assert_eq!(config.alternate_origins()[0].1, "kival.internal.example");
        assert_eq!(config.alternate_origins()[1].1, "kival.lan");

        let challenge = [7_u8; 32];
        let client_data =
            client_data("webauthn.get", &challenge, "https://kival.lan:8443", false, false);
        let rp_id = validate_client_data(
            &client_data,
            "webauthn.get",
            CeremonyExpectation {
                challenge: &challenge,
                origin: config.origin_value(),
                rp_id: config.rp_id(),
                alternate_origins: config.alternate_origins(),
            },
        )
        .expect("configured origin should be accepted");
        assert_eq!(rp_id, "kival.lan");
    }

    #[test]
    fn loopback_canonical_url_allows_localhost_on_development_ports() {
        let config = WebAuthnConfig::from_canonical_url("http://localhost:3000")
            .expect("loopback public URL should be valid");
        let accepted = config
            .origins
            .iter()
            .map(|(origin, rp_id)| (origin.ascii_serialization(), rp_id.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(
            accepted,
            [
                ("http://localhost:3000".to_owned(), "localhost"),
                ("http://localhost:5173".to_owned(), "localhost"),
            ]
        );
        assert!(config.uses_implicit_rp_id());
    }

    #[test]
    fn canonical_url_rejects_untrusted_or_unsupported_forms() {
        for canonical_url in [
            "",
            " https://kival.example.com",
            "kival.example.com",
            "ftp://kival.example.com",
            "http://kival.example.com",
            "https://user@kival.example.com",
            "https://user:password@kival.example.com",
            "https://kival.example.com/path",
            "https://kival.example.com?foo=bar",
            "https://kival.example.com#fragment",
            "http://127.0.0.1:3000",
            "https://192.168.1.2",
            "https://[::1]",
            "https://kival.example.com.",
            "https://bad_label.example.com",
        ] {
            assert!(
                WebAuthnConfig::from_canonical_url(canonical_url).is_err(),
                "{canonical_url} should fail"
            );
        }

        for origin in
            ["http://kival.internal.example", "http://127.0.0.1:5173", "https://192.168.1.2"]
        {
            assert!(
                WebAuthnConfig::from_canonical_url_with_allowed_origins(
                    "https://kival.example.com",
                    &[origin.to_owned()],
                )
                .is_err(),
                "{origin} should not be accepted as an additional origin"
            );
        }
    }
}
