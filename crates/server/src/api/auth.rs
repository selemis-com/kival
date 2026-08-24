//! Authentication handlers and extractors.

use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Extension, Json,
    extract::{self, ConnectInfo, FromRequestParts, Request, State},
    http::{
        HeaderMap, HeaderName, Method, StatusCode,
        header::{AUTHORIZATION, COOKIE, SET_COOKIE},
        request::Parts,
    },
    middleware::Next,
    response::{IntoResponse, Response},
    routing::MethodRouter,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use kival_common::security;
use kival_kernel::{
    ApiKeyAttribution, EventInsert, EventKind, SessionRow, active_session_csrf_hash,
    authenticate_api_key, authenticate_session, current_session_id, list_active_sessions,
    prune_terminal_sessions, revoke_session, revoke_session_for_logout, touch_api_key_last_used,
    touch_session_last_seen,
};
use kival_sdk::{API_PREFIX, Session, SessionListResponse, SessionOnlyResponse};
use kival_tracing::warn;
use kival_types::ApiKeyScope;
use serde_json::json;
use uuid::Uuid;

use crate::{
    ServerState,
    api::{
        emit::emit_event,
        error::{ApiError, ApiResult},
        maintenance::record_cleanup_rows,
        metrics::AuthenticationMetrics,
    },
};

/// Name of the session cookie.
pub(super) const SESSION_COOKIE: &str = "__Host-kival_session";
/// Name of the CSRF cookie.
pub(super) const CSRF_COOKIE: &str = "__Host-kival_csrf";
/// Header carrying the CSRF token for unsafe requests.
const CSRF_HEADER: HeaderName = HeaderName::from_static("x-csrf-token");
/// Prefix applied to generated API key secrets.
pub(super) const API_KEY_PREFIX: &str = "kvl_";
/// Lifetime assigned to newly created sessions.
pub(super) const SESSION_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 30);
/// Maximum old terminal sessions deleted by one opportunistic cleanup.
const SESSION_CLEANUP_BATCH_SIZE: i64 = 128;

/// Converts one session database row into an HTTP wire resource.
fn session_row_into_wire(row: SessionRow, is_current: bool) -> Session {
    Session {
        id: row.id,
        is_current,
        user_id: row.user_id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        expires_at: row.expires_at,
        revoked_at: row.revoked_at,
        revoked_by: row.revoked_by,
        revocation_reason: row.revocation_reason,
        last_seen_at: row.last_seen_at,
        user_agent: row.user_agent,
        ip_address: row.ip_address,
    }
}

/// Authenticated user resolved from either a browser session or an API key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthenticatedUser {
    /// Authenticated user ID.
    pub id: Uuid,
    /// Browser session used for this request, when authentication was interactive.
    pub session_id: Option<Uuid>,
    /// API key used for this request, when authentication used bearer credentials.
    pub api_key: Option<ApiKeyAttribution>,
}

impl AuthenticatedUser {
    /// Ensures the request was authenticated by an interactive session.
    pub(crate) fn require_session(&self) -> ApiResult<()> {
        if self.api_key.is_none() && self.session_id.is_some() {
            Ok(())
        } else {
            Err(ApiError::forbidden("this operation requires an interactive session"))
        }
    }

    /// Returns the authenticating API key ID, when bearer authentication was used.
    pub(crate) fn api_key_id(&self) -> Option<Uuid> {
        self.api_key.as_ref().map(|api_key| api_key.id)
    }

    /// Returns the authenticating browser session ID, when interactive authentication was used.
    pub(crate) const fn session_id(&self) -> Option<Uuid> {
        self.session_id
    }

    /// Creates an audit event attributed to this authentication context.
    pub(crate) fn event(&self, event_kind: EventKind, payload: serde_json::Value) -> EventInsert {
        EventInsert::new(self.id, event_kind, payload).api_key(self.api_key.clone())
    }
}

impl FromRequestParts<Arc<ServerState>> for AuthenticatedUser {
    type Rejection = ApiError;

    fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<ServerState>,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        let bearer = bearer_token(&parts.headers).map(|token| token.map(str::to_owned));
        let session = session_cookie(&parts.headers).map(str::to_owned);
        let api_key_policy = parts.extensions.get::<ApiKeyRoutePolicy>().copied();
        let direct_peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| address.ip());
        let path = parts.uri.path().to_owned();
        let state = Arc::clone(state);

        async move {
            let bearer = match bearer {
                Ok(bearer) => bearer,
                Err(error) => {
                    let mut metrics = AuthenticationMetrics::start("api_key");
                    metrics.complete("malformed");
                    return Err(error);
                }
            };

            if let Some(token) = bearer {
                let mut metrics = AuthenticationMetrics::start("api_key");
                let Some(policy) = api_key_policy else {
                    metrics.complete("route_denied");
                    return Err(ApiError::forbidden("API keys are not permitted on this route"));
                };
                return authenticate_api_key_request(
                    &state,
                    &token,
                    policy,
                    &path,
                    direct_peer,
                    &mut metrics,
                )
                .await;
            }

            let mut metrics = AuthenticationMetrics::start("session");
            let Some(token) = session else {
                metrics.complete("missing");
                return Err(ApiError::unauthorized("missing credentials"));
            };
            let session_token_hash = security::hash_token(&token);

            let authenticated =
                match authenticate_session(state.db(), session_token_hash.as_slice()).await {
                    Ok(Some(authenticated)) => authenticated,
                    Ok(None) => {
                        metrics.complete("invalid");
                        return Err(ApiError::unauthorized("invalid session"));
                    }
                    Err(error) => {
                        metrics.complete("error");
                        return Err(ApiError::from(error));
                    }
                };
            let session_id = authenticated.session_id;
            let user_id = authenticated.user_id;

            if let Err(error) = state.rate_limiter().check_authenticated_user(user_id) {
                metrics.complete("rate_limited");
                return Err(error);
            }

            if let Err(error) =
                touch_session_last_seen(state.db(), session_token_hash.as_slice()).await
            {
                metrics.complete("error");
                return Err(ApiError::from(error));
            }

            metrics.complete("success");
            Ok(Self { id: user_id, session_id: Some(session_id), api_key: None })
        }
    }
}

/// Authentication policy attached directly to a route that accepts bearer API keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ApiKeyRoutePolicy {
    /// Any valid API key may authenticate, without requiring an authorization scope.
    Any,
    /// The key must hold the required scope, without a workspace allowlist check.
    Global(ApiKeyScope),
    /// The key must hold the required scope and explicitly allow the workspace in the path.
    Workspace(ApiKeyScope),
}

/// Adds fail-closed API-key policy to individual method routes.
///
/// Methods without one of these annotations remain session-only by default.
pub(super) trait ApiKeyRouteExt {
    /// Allows authentication with any valid API key.
    fn any_api_key(self) -> Self;

    /// Allows API-key authentication with the required global scope.
    fn api_key(self, scope: ApiKeyScope) -> Self;

    /// Allows API-key authentication with the required scope and workspace allowlist check.
    fn workspace_api_key(self, scope: ApiKeyScope) -> Self;
}

impl<S> ApiKeyRouteExt for MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn any_api_key(self) -> Self {
        self.route_layer(Extension(ApiKeyRoutePolicy::Any))
    }

    fn api_key(self, scope: ApiKeyScope) -> Self {
        self.route_layer(Extension(ApiKeyRoutePolicy::Global(scope)))
    }

    fn workspace_api_key(self, scope: ApiKeyScope) -> Self {
        self.route_layer(Extension(ApiKeyRoutePolicy::Workspace(scope)))
    }
}

/// Authenticates and constrains a bearer API key using the policy attached to this route.
async fn authenticate_api_key_request(
    state: &ServerState,
    token: &str,
    policy: ApiKeyRoutePolicy,
    path: &str,
    direct_peer: Option<std::net::IpAddr>,
    metrics: &mut AuthenticationMetrics,
) -> ApiResult<AuthenticatedUser> {
    let path = path.strip_prefix(API_PREFIX).unwrap_or(path);
    if let Some(peer) = direct_peer
        && let Err(error) = state.rate_limiter().check_api_key_peer(peer)
    {
        metrics.complete("rate_limited");
        return Err(error);
    }

    let Some(token_hash) = api_key_token_hash(token) else {
        metrics.complete("malformed");
        return Err(ApiError::unauthorized("invalid API key"));
    };
    let workspace_id = match policy {
        ApiKeyRoutePolicy::Workspace(_) => match workspace_id_from_path(path) {
            Ok(workspace_id) => Some(workspace_id),
            Err(error) => {
                metrics.complete("error");
                return Err(error);
            }
        },
        ApiKeyRoutePolicy::Any | ApiKeyRoutePolicy::Global(_) => None,
    };
    let authenticated =
        match authenticate_api_key(state.db(), token_hash.as_slice(), workspace_id).await {
            Ok(Some(api_key)) => api_key,
            Ok(None) => {
                metrics.complete("invalid");
                return Err(ApiError::unauthorized("invalid API key"));
            }
            Err(error) => {
                metrics.complete("error");
                return Err(ApiError::from(error));
            }
        };
    let api_key_id = authenticated.id;
    let user_id = authenticated.user_id;
    let api_key_label = authenticated.label;
    let scopes = authenticated.scopes;
    let workspace_allowed = authenticated.workspace_allowed;

    if let Err(error) = state.rate_limiter().check_authenticated_user(user_id) {
        metrics.complete("rate_limited");
        return Err(error);
    }
    if let Err(error) = state.rate_limiter().check_api_key(api_key_id) {
        metrics.complete("rate_limited");
        return Err(error);
    }

    if let Some(required_scope) = match policy {
        ApiKeyRoutePolicy::Any => None,
        ApiKeyRoutePolicy::Global(scope) | ApiKeyRoutePolicy::Workspace(scope) => Some(scope),
    } && !scopes.iter().copied().any(|scope| scope.permits(required_scope))
    {
        metrics.complete("scope_denied");
        return Err(ApiError::forbidden(format!(
            "API key requires scope `{}`",
            required_scope.as_str()
        )));
    }

    if !workspace_allowed {
        metrics.complete("workspace_denied");
        return Err(ApiError::forbidden("API key is not permitted in this workspace"));
    }

    // Avoid turning every API request into a write while still keeping last-used information
    // useful.
    if let Err(error) = touch_api_key_last_used(state.db(), api_key_id).await {
        metrics.complete("error");
        return Err(ApiError::from(error));
    }

    metrics.complete("success");
    Ok(AuthenticatedUser {
        id: user_id,
        session_id: None,
        api_key: Some(ApiKeyAttribution { id: api_key_id, label: api_key_label }),
    })
}

/// Extracts a workspace ID from a workspace-scoped API path.
fn workspace_id_from_path(path: &str) -> ApiResult<Uuid> {
    let mut segments = path.trim_start_matches('/').split('/');
    if segments.next() != Some("workspaces") {
        return Err(ApiError::internal("workspace-scoped route does not start with /workspaces"));
    }

    let workspace_id = segments
        .next()
        .ok_or_else(|| ApiError::internal("workspace route is missing a workspace ID"))?;

    workspace_id.parse().map_err(|_error| ApiError::bad_request("invalid workspace ID"))
}

/// Validates and hashes an API key token.
fn api_key_token_hash(token: &str) -> Option<[u8; 32]> {
    let encoded = token.strip_prefix(API_KEY_PREFIX)?;
    let decoded = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    (decoded.len() == security::SECRET_TOKEN_BYTES).then(|| security::hash_token(token))
}

/// Logs out the current session.
pub(crate) async fn handle_logout(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    if bearer_token(&headers)?.is_some() {
        return Err(ApiError::forbidden("this operation requires an interactive session"));
    }

    if let Some(token) = session_cookie(&headers) {
        let session_token_hash = security::hash_token(token);
        let mut tx = state.db().begin().await?;

        let session = revoke_session_for_logout(&mut tx, session_token_hash.as_slice()).await?;

        if let Some(session) = session {
            emit_event(
                &mut tx,
                state.durable_tasks().queue(),
                EventInsert::new(
                    session.user_id,
                    EventKind::AuthLogout,
                    json!({ "session_id": session.id }),
                )
                .target_user(session.user_id),
            )
            .await?;
        }

        tx.commit().await?;
    }

    let cookie = format!("{SESSION_COOKIE}=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax");
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .append(SET_COOKIE, cookie.parse().map_err(|_| ApiError::internal("invalid cookie"))?);
    response.headers_mut().append(
        SET_COOKIE,
        format!("{CSRF_COOKIE}=; Path=/; Max-Age=0; Secure; SameSite=Strict")
            .parse()
            .map_err(|_| ApiError::internal("invalid cookie"))?,
    );

    Ok(response)
}

/// Revokes one of the current user's browser sessions.
pub(crate) async fn handle_revoke_session(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    extract::Path(session_id): extract::Path<Uuid>,
) -> ApiResult<Json<SessionOnlyResponse>> {
    actor.require_session()?;

    let mut tx = state.db().begin().await?;
    let session = revoke_session(&mut tx, session_id, actor.id).await?;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(EventKind::AuthSessionRevoked, json!({ "session_id": session.id }))
            .target_user(actor.id),
    )
    .await?;

    tx.commit().await?;

    Ok(Json(SessionOnlyResponse { session: session_row_into_wire(session, false) }))
}

/// Lists active sessions for the current user.
pub(crate) async fn handle_list_sessions(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    headers: HeaderMap,
) -> ApiResult<Json<SessionListResponse>> {
    actor.require_session()?;
    prune_terminal_sessions_best_effort(&state).await;
    let token =
        session_cookie(&headers).ok_or_else(|| ApiError::unauthorized("missing session"))?;
    let current_session_id =
        current_session_id(state.db(), actor.id, security::hash_token(token).as_slice())
            .await?
            .ok_or_else(|| ApiError::unauthorized("invalid session"))?;

    let rows = list_active_sessions(state.db(), actor.id).await?;

    let sessions = rows
        .into_iter()
        .map(|row| {
            let is_current = row.id == current_session_id;
            session_row_into_wire(row, is_current)
        })
        .collect();

    Ok(Json(SessionListResponse::new(sessions)))
}

/// Best-effort pruning keeps old revoked and expired browser sessions bounded globally.
pub(crate) async fn prune_terminal_sessions_best_effort(state: &ServerState) {
    let started_at = Instant::now();
    let result = prune_terminal_sessions(state.db(), SESSION_CLEANUP_BATCH_SIZE).await;

    record_cleanup_rows("browser_sessions", started_at, &result);

    if let Err(error) = result {
        warn!(
            target: "kival::server::auth",
            %error,
            "failed to prune terminal browser sessions"
        );
    }
}

/// Generates a URL-safe random authentication secret.
pub(super) fn generate_secret_token() -> ApiResult<String> {
    security::generate_secret_token().map_err(|_| ApiError::internal("random generation failed"))
}

/// Extracts a bearer token from the Authorization header.
///
/// A malformed Bearer attempt is rejected rather than treated as absent so it can never fall back
/// to cookie authentication. Non-Bearer authorization schemes do not affect session auth.
fn bearer_token(headers: &HeaderMap) -> ApiResult<Option<&str>> {
    let mut bearer = None;

    #[expect(
        clippy::explicit_iter_loop,
        reason = "`.iter()` preserves the HeaderMap-backed lifetime needed by the returned token."
    )]
    for value in headers.get_all(AUTHORIZATION).iter() {
        let value =
            value.to_str().map_err(|_| ApiError::unauthorized("invalid authorization header"))?;
        let scheme_end =
            value.find(|character: char| character.is_ascii_whitespace()).unwrap_or(value.len());
        let scheme = &value[..scheme_end];

        if !scheme.eq_ignore_ascii_case("bearer") {
            continue;
        }

        let remainder = &value[scheme_end..];
        if !remainder.starts_with(' ') {
            return Err(ApiError::unauthorized("invalid bearer authorization header"));
        }

        let token = remainder.trim_start_matches(' ');
        if token.is_empty() || token.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(ApiError::unauthorized("invalid bearer authorization header"));
        }

        if bearer.replace(token).is_some() {
            return Err(ApiError::unauthorized("multiple bearer authorization headers"));
        }
    }

    Ok(bearer)
}

/// Extracts the session cookie value from request headers.
pub(super) fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    cookie_value(headers, SESSION_COOKIE)
}

/// Extracts a named cookie value from a raw Cookie header.
fn cookie_value<'a>(headers: &'a HeaderMap, cookie_name: &str) -> Option<&'a str> {
    headers.get(COOKIE).and_then(|value| value.to_str().ok()).and_then(|header| {
        header.split(';').find_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            (name == cookie_name).then_some(value)
        })
    })
}

/// Enforces CSRF protection for unsafe requests authenticated by cookie.
pub(super) async fn enforce_csrf(
    State(state): State<Arc<ServerState>>,
    request: Request,
    next: Next,
) -> Response {
    match bearer_token(request.headers()) {
        Ok(Some(_token)) => return next.run(request).await,
        Ok(None) => {}
        Err(error) => return error.into_response(),
    }

    if !is_unsafe_method(request.method()) {
        return next.run(request).await;
    }

    let headers = request.headers();
    let Some(session_token) = session_cookie(headers) else {
        return next.run(request).await;
    };

    let Some(csrf_header) = headers.get(&CSRF_HEADER).and_then(|value| value.to_str().ok()) else {
        return ApiError::forbidden("missing csrf token").into_response();
    };

    let session_token_hash = security::hash_token(session_token);
    let csrf_token_hash = security::hash_token(csrf_header);
    let stored_csrf_hash =
        match active_session_csrf_hash(state.db(), session_token_hash.as_slice()).await {
            Ok(Some(hash)) => hash,
            Ok(None) => return next.run(request).await,
            Err(error) => return ApiError::from(error).into_response(),
        };

    if !constant_time_eq(stored_csrf_hash.as_slice(), csrf_token_hash.as_slice()) {
        return ApiError::forbidden("invalid csrf token").into_response();
    }

    next.run(request).await
}

/// Returns whether an HTTP method requires CSRF protection.
const fn is_unsafe_method(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE)
}

/// Compares two byte slices without data-dependent early exit.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    // Avoid data-dependent early returns while comparing token hashes.
    left.iter().zip(right).fold(0_u8, |acc, (left, right)| acc | (left ^ right)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_hash_accepts_the_kival_prefix() {
        let secret = URL_SAFE_NO_PAD.encode([0_u8; security::SECRET_TOKEN_BYTES]);
        let token = format!("{API_KEY_PREFIX}{secret}");

        assert_eq!(api_key_token_hash(&token), Some(security::hash_token(&token)));
    }
}
