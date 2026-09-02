//! In-process request rate limiting for credentials and passkey ceremonies.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, Once},
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use kival_metrics::{counter, describe_counter, describe_gauge, gauge};
use uuid::Uuid;

use crate::{
    ServerState,
    api::error::{ApiError, ApiResult},
};

/// Fixed window used by all built-in request limits.
const WINDOW: Duration = Duration::from_secs(60);
/// Frequency at which expired rate-limit entries are pruned.
const PRUNE_INTERVAL: u64 = 1024;

/// Ensures rate-limit metric descriptions are emitted once.
static DESCRIBE_RATE_LIMIT_METRICS: Once = Once::new();

/// Registers rate-limit metric descriptions once per process.
fn describe_rate_limit_metrics() {
    DESCRIBE_RATE_LIMIT_METRICS.call_once(|| {
        describe_counter!(
            "rate_limit.checks_total",
            "Enabled in-process rate-limit checks by outcome."
        );
        describe_counter!(
            "rate_limit.rejections_total",
            "Requests rejected by the in-process rate limiter."
        );
        describe_counter!(
            "rate_limit.pruned_windows_total",
            "Expired in-process rate-limit windows removed from memory."
        );
        describe_gauge!(
            "rate_limit.tracked_windows",
            "In-process rate-limit identity windows retained for checking or periodic pruning."
        );
    });
}

/// One configured request class.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum RateLimitClass {
    /// Passkey or enrollment ceremony creation.
    AuthenticationStart,
    /// Passkey or enrollment ceremony completion.
    AuthenticationFinish,
    /// Requests authenticated by one user.
    AuthenticatedUser,
    /// Bearer-authentication attempts from one direct peer.
    ApiKeyPeer,
    /// Requests authenticated by one API key.
    ApiKey,
}

impl RateLimitClass {
    /// Returns the stable metric label for this request class.
    const fn as_str(self) -> &'static str {
        match self {
            Self::AuthenticationStart => "authentication_start",
            Self::AuthenticationFinish => "authentication_finish",
            Self::AuthenticatedUser => "authenticated_user",
            Self::ApiKeyPeer => "api_key_peer",
            Self::ApiKey => "api_key",
        }
    }
}

/// Identity constrained by one request class.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum RateLimitIdentity {
    /// Direct TCP peer address.
    Peer(IpAddr),
    /// Stable user record identifier.
    User(Uuid),
    /// Stable API-key record identifier.
    ApiKey(Uuid),
}

/// Composite key for one limiter window.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct RateLimitKey {
    /// Request class.
    class: RateLimitClass,
    /// Constrained identity.
    identity: RateLimitIdentity,
}

/// State for one fixed request window.
#[derive(Clone, Copy, Debug)]
struct WindowState {
    /// Start of the current window.
    started_at: Instant,
    /// Requests observed during the current window.
    requests: u32,
}

/// Mutable limiter state protected by a short process-local critical section.
#[derive(Debug, Default)]
struct RateLimitState {
    /// Active windows.
    windows: HashMap<RateLimitKey, WindowState>,
    /// Number of checks since process start.
    checks: u64,
}

/// In-process fixed-window request limiter.
#[derive(Clone, Debug)]
pub(crate) struct RateLimiter {
    /// Mutable request windows.
    state: Arc<Mutex<RateLimitState>>,
    /// Passkey start requests accepted per peer and minute.
    authentication_start_per_minute: u32,
    /// Passkey finish requests accepted per peer and minute.
    authentication_finish_per_minute: u32,
    /// Authenticated requests accepted per user and minute.
    authenticated_user_per_minute: u32,
    /// Bearer-authentication attempts accepted per direct peer and minute.
    api_key_peer_per_minute: u32,
    /// Requests accepted per API key and minute.
    api_key_per_minute: u32,
}

impl RateLimiter {
    /// Creates a limiter from resolved server settings.
    pub(crate) fn new(
        authentication_start_per_minute: u32,
        authentication_finish_per_minute: u32,
        authenticated_user_per_minute: u32,
        api_key_peer_per_minute: u32,
        api_key_per_minute: u32,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(RateLimitState::default())),
            authentication_start_per_minute,
            authentication_finish_per_minute,
            authenticated_user_per_minute,
            api_key_peer_per_minute,
            api_key_per_minute,
        }
    }

    /// Checks a passkey-ceremony start request for one direct peer.
    fn check_authentication_start(&self, peer: IpAddr) -> ApiResult<()> {
        self.check(
            RateLimitKey {
                class: RateLimitClass::AuthenticationStart,
                identity: RateLimitIdentity::Peer(peer),
            },
            self.authentication_start_per_minute,
        )
    }

    /// Checks a passkey-ceremony finish request for one direct peer.
    fn check_authentication_finish(&self, peer: IpAddr) -> ApiResult<()> {
        self.check(
            RateLimitKey {
                class: RateLimitClass::AuthenticationFinish,
                identity: RateLimitIdentity::Peer(peer),
            },
            self.authentication_finish_per_minute,
        )
    }

    /// Checks one request attributed to an authenticated user.
    pub(crate) fn check_authenticated_user(&self, user_id: Uuid) -> ApiResult<()> {
        self.check(
            RateLimitKey {
                class: RateLimitClass::AuthenticatedUser,
                identity: RateLimitIdentity::User(user_id),
            },
            self.authenticated_user_per_minute,
        )
    }

    /// Checks one bearer-authentication attempt from a direct peer before database access.
    pub(crate) fn check_api_key_peer(&self, peer: IpAddr) -> ApiResult<()> {
        self.check(
            RateLimitKey {
                class: RateLimitClass::ApiKeyPeer,
                identity: RateLimitIdentity::Peer(peer),
            },
            self.api_key_peer_per_minute,
        )
    }

    /// Checks one authenticated API-key request.
    pub(crate) fn check_api_key(&self, api_key_id: Uuid) -> ApiResult<()> {
        self.check(
            RateLimitKey {
                class: RateLimitClass::ApiKey,
                identity: RateLimitIdentity::ApiKey(api_key_id),
            },
            self.api_key_per_minute,
        )
    }

    /// Checks and increments one fixed request window.
    fn check(&self, key: RateLimitKey, limit: u32) -> ApiResult<()> {
        if limit == 0 {
            return Ok(());
        }

        describe_rate_limit_metrics();
        let now = Instant::now();
        let (retry_after, pruned_windows) = {
            let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.checks = state.checks.wrapping_add(1);
            let pruned_windows = if state.checks.is_multiple_of(PRUNE_INTERVAL) {
                let previous_len = state.windows.len();
                state.windows.retain(|_, window| now.duration_since(window.started_at) < WINDOW);
                previous_len.saturating_sub(state.windows.len())
            } else {
                0
            };

            let retry_after = {
                let window = state
                    .windows
                    .entry(key)
                    .or_insert(WindowState { started_at: now, requests: 0 });
                let elapsed = now.duration_since(window.started_at);
                if elapsed >= WINDOW {
                    *window = WindowState { started_at: now, requests: 0 };
                }

                if window.requests >= limit {
                    Some(WINDOW.saturating_sub(now.duration_since(window.started_at)))
                } else {
                    window.requests = window.requests.saturating_add(1);
                    None
                }
            };

            // Publish the map size while holding the same lock that protects it. Otherwise a
            // slower check can overwrite a newer size after both checks release the mutex.
            gauge!("rate_limit.tracked_windows").set(state.windows.len() as f64);

            drop(state);

            (retry_after, pruned_windows)
        };
        if pruned_windows > 0 {
            counter!("rate_limit.pruned_windows_total").increment(pruned_windows as u64);
        }

        if let Some(retry_after) = retry_after {
            counter!(
                "rate_limit.checks_total",
                "class" => key.class.as_str(),
                "outcome" => "rejected"
            )
            .increment(1);
            counter!("rate_limit.rejections_total", "class" => key.class.as_str()).increment(1);
            return Err(ApiError::too_many_requests(
                "request rate limit exceeded",
                retry_after.as_secs().max(1),
            ));
        }

        counter!(
            "rate_limit.checks_total",
            "class" => key.class.as_str(),
            "outcome" => "allowed"
        )
        .increment(1);

        Ok(())
    }
}

/// Applies the passkey-ceremony start limit using the direct TCP peer address.
pub(crate) async fn enforce_authentication_start(
    State(state): State<Arc<ServerState>>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(peer) = direct_peer(&request)
        && let Err(error) = state.rate_limiter().check_authentication_start(peer)
    {
        return error.into_response();
    }

    next.run(request).await
}

/// Applies the passkey-ceremony completion limit using the direct TCP peer address.
pub(crate) async fn enforce_authentication_finish(
    State(state): State<Arc<ServerState>>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(peer) = direct_peer(&request)
        && let Err(error) = state.rate_limiter().check_authentication_finish(peer)
    {
        return error.into_response();
    }

    next.run(request).await
}

/// Returns the direct TCP peer address injected by Axum.
///
/// Forwarding headers are intentionally ignored. Deployments behind a reverse proxy should apply
/// an additional edge rate limit rather than trusting client-controlled forwarding metadata.
fn direct_peer(request: &Request) -> Option<IpAddr> {
    request.extensions().get::<ConnectInfo<SocketAddr>>().map(|ConnectInfo(address)| address.ip())
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use kival_metrics::{
        LocalRecorderGuard,
        prometheus::{PrometheusBuilder, PrometheusHandle},
        set_default_local_recorder,
    };

    use super::*;

    fn test_metrics() -> (LocalRecorderGuard, PrometheusHandle) {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let guard = set_default_local_recorder(recorder);
        (guard, handle)
    }

    #[test]
    fn fixed_window_rejects_requests_beyond_the_limit() {
        let limiter = RateLimiter::new(1, 1, 1, 1, 1);
        let peer = IpAddr::V4(Ipv4Addr::LOCALHOST);

        limiter.check_authentication_start(peer).expect("first request should pass");
        let error =
            limiter.check_authentication_start(peer).expect_err("second request should be limited");

        assert_eq!(error.into_response().status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn zero_limit_disables_one_request_class() {
        let limiter = RateLimiter::new(0, 0, 0, 0, 0);
        let peer = IpAddr::V4(Ipv4Addr::LOCALHOST);

        for _ in 0..10 {
            limiter.check_authentication_start(peer).expect("disabled limit should pass");
        }
    }

    #[test]
    fn rate_limit_metrics_record_allowed_rejected_and_tracked_windows() {
        let (_guard, handle) = test_metrics();
        let limiter = RateLimiter::new(1, 1, 1, 1, 1);
        let peer = IpAddr::V4(Ipv4Addr::LOCALHOST);

        limiter.check_authentication_start(peer).expect("first request should pass");
        assert!(
            limiter.check_authentication_start(peer).is_err(),
            "second request should be limited"
        );

        let rendered = handle.render();
        assert!(
            rendered.contains(
                r#"rate_limit_checks_total{class="authentication_start",outcome="allowed"} 1"#
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                r#"rate_limit_checks_total{class="authentication_start",outcome="rejected"} 1"#
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(r#"rate_limit_rejections_total{class="authentication_start"} 1"#),
            "{rendered}"
        );
        assert!(rendered.contains("rate_limit_tracked_windows 1"), "{rendered}");
    }
}
