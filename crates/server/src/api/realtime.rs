//! Authenticated realtime invalidation delivery.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Once},
    time::Duration,
};

use axum::{
    extract::{
        State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header::ORIGIN},
    response::Response,
};
use futures_util::{SinkExt as _, StreamExt as _};
use kival_kernel::{
    realtime_api_key_active, realtime_api_key_object_authorized, realtime_object_authorized,
    realtime_session_active, realtime_workspace_authorized,
};
use kival_metrics::{counter, describe_counter, describe_gauge, gauge};
use kival_sdk::RealtimeMessage;
use kival_tracing::{error, trace, warn};
use serde::Deserialize;
use sqlx::{PgPool, postgres::PgListener};
use tokio::{
    sync::broadcast,
    time::{interval, sleep, timeout},
};
use uuid::Uuid;

use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        error::{ApiError, ApiResult},
    },
};

/// Per-recipient realtime fan-out capacity.
const RECIPIENT_CAPACITY: usize = 256;
/// Maximum active realtime connections retained for one user in this process.
const MAX_CONNECTIONS_PER_USER: usize = 8;
/// Maximum accepted WebSocket application message and frame size.
const MAX_CLIENT_MESSAGE_SIZE: usize = 16 * 1024;
/// Heartbeat interval for active WebSocket connections.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
/// Maximum time allowed for one client write.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// `PostgreSQL` listener reconnect delay.
const LISTENER_RETRY_DELAY: Duration = Duration::from_secs(2);
/// WebSocket close code requesting a later retry and HTTP recovery.
const TRY_AGAIN_LATER: u16 = 1013;
/// WebSocket close code used when the authenticating credential is no longer valid.
const POLICY_VIOLATION: u16 = 1008;
/// Ensures realtime metric descriptions are emitted once.
static DESCRIBE_REALTIME_METRICS: Once = Once::new();

/// Recipient-scoped invalidation received from `PostgreSQL`.
#[derive(Debug, Clone, Deserialize)]
struct RealtimeEnvelope {
    /// Intended recipient user ID.
    recipient_user_id: Uuid,
    /// Public invalidation payload.
    #[serde(flatten)]
    message: RealtimeMessage,
}

/// Authentication context retained for one active realtime connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RealtimePrincipal {
    /// Interactive browser session.
    Session {
        /// Authenticated user ID.
        user_id: Uuid,
        /// Browser session whose lifecycle bounds this connection.
        session_id: Uuid,
    },
    /// Bearer API key delegated by a user.
    ApiKey {
        /// User that owns the API key.
        user_id: Uuid,
        /// Authenticated API key ID.
        api_key_id: Uuid,
    },
}

impl RealtimePrincipal {
    /// Returns the user whose authority bounds this connection.
    const fn user_id(self) -> Uuid {
        match self {
            Self::Session { user_id, .. } | Self::ApiKey { user_id, .. } => user_id,
        }
    }

    /// Returns whether this is a bearer API-key connection.
    const fn is_api_key(self) -> bool {
        matches!(self, Self::ApiKey { .. })
    }
}

/// Next input observed while driving an active realtime connection.
#[derive(Debug)]
enum SocketEvent {
    /// Recipient-scoped invalidation from the process-local fan-out hub.
    Invalidation(Result<RealtimeMessage, broadcast::error::RecvError>),
    /// Periodic heartbeat tick.
    Heartbeat,
    /// Message or disconnect observed from the connected client.
    Incoming(Option<Result<Message, axum::Error>>),
}

/// In-process realtime fan-out hub, partitioned by recipient.
#[derive(Debug, Clone)]
pub(crate) struct RealtimeHub {
    /// Bounded broadcast channel for each user with at least one local connection.
    recipients: Arc<Mutex<HashMap<Uuid, broadcast::Sender<RealtimeMessage>>>>,
}

impl RealtimeHub {
    /// Creates an empty recipient-indexed realtime fan-out hub.
    pub(crate) fn new() -> Self {
        Self { recipients: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Subscribes one active connection only to invalidations for its user.
    ///
    /// Returns `None` when the user already holds the per-process connection limit.
    fn subscribe(&self, user_id: Uuid) -> Option<RealtimeSubscription> {
        let receiver = {
            let mut recipients =
                self.recipients.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

            let sender = recipients.entry(user_id).or_insert_with(|| {
                let (sender, _) = broadcast::channel(RECIPIENT_CAPACITY);
                sender
            });

            if sender.receiver_count() >= MAX_CONNECTIONS_PER_USER {
                return None;
            }

            let receiver = sender.subscribe();
            drop(recipients);

            receiver
        };

        Some(RealtimeSubscription { hub: self.clone(), user_id, receiver: Some(receiver) })
    }

    /// Publishes one database invalidation only to local connections for its recipient.
    fn publish(&self, envelope: RealtimeEnvelope) {
        let RealtimeEnvelope { recipient_user_id, message } = envelope;
        let mut recipients =
            self.recipients.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        let should_remove =
            recipients.get(&recipient_user_id).is_some_and(|sender| sender.send(message).is_err());
        if should_remove {
            recipients.remove(&recipient_user_id);
        }
    }

    /// Requests authoritative HTTP recovery from every active local connection.
    fn force_resync(&self) -> u64 {
        let mut recipients =
            self.recipients.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let message = resync_message();
        let mut delivered = 0_u64;

        recipients.retain(|_, sender| {
            sender.send(message.clone()).is_ok_and(|receiver_count| {
                delivered =
                    delivered.saturating_add(u64::try_from(receiver_count).unwrap_or(u64::MAX));
                true
            })
        });

        delivered
    }

    /// Removes an idle recipient channel after its final local connection disconnects.
    fn unsubscribe(&self, user_id: Uuid) {
        let mut recipients =
            self.recipients.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if recipients.get(&user_id).is_some_and(|sender| sender.receiver_count() == 0) {
            recipients.remove(&user_id);
        }
    }
}

/// Recipient-scoped connection subscription removed from the hub automatically on drop.
struct RealtimeSubscription {
    /// Hub owning the recipient channel.
    hub: RealtimeHub,
    /// User whose invalidations this subscription receives.
    user_id: Uuid,
    /// Bounded receiver shared only with this user's local connections.
    receiver: Option<broadcast::Receiver<RealtimeMessage>>,
}

impl RealtimeSubscription {
    /// Waits for the next invalidation addressed to this user.
    async fn recv(&mut self) -> Result<RealtimeMessage, broadcast::error::RecvError> {
        let Some(receiver) = self.receiver.as_mut() else {
            return Err(broadcast::error::RecvError::Closed);
        };
        receiver.recv().await
    }
}

impl Drop for RealtimeSubscription {
    fn drop(&mut self) {
        drop(self.receiver.take());
        self.hub.unsubscribe(self.user_id);
    }
}

/// Upgrades an authenticated session or scoped API key to one realtime connection.
pub(crate) async fn handle_realtime(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    actor: AuthenticatedUser,
    websocket: WebSocketUpgrade,
) -> ApiResult<Response> {
    let principal = match actor.api_key_id() {
        Some(api_key_id) => RealtimePrincipal::ApiKey { user_id: actor.id, api_key_id },
        None => {
            require_allowed_origin(&headers, &state)?;
            let session_id = actor.session_id().ok_or_else(|| {
                ApiError::internal("interactive authentication is missing session context")
            })?;
            RealtimePrincipal::Session { user_id: actor.id, session_id }
        }
    };

    // Subscribe before completing the upgrade so the browser's native `open`
    // event is a safe HTTP-resynchronization boundary. Invalidations committed
    // while the principal is revalidated remain buffered in this subscription.
    let subscription = state.realtime().subscribe(principal.user_id()).ok_or_else(|| {
        ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "realtime.connection_limit",
            "too many active realtime connections",
        )
    })?;

    Ok(websocket
        .max_message_size(MAX_CLIENT_MESSAGE_SIZE)
        .max_frame_size(MAX_CLIENT_MESSAGE_SIZE)
        .on_upgrade(move |socket| handle_socket(socket, state, principal, subscription)))
}

/// Requires a configured same-origin WebSocket handshake.
fn require_allowed_origin(headers: &HeaderMap, state: &ServerState) -> ApiResult<()> {
    let origin = headers
        .get(ORIGIN)
        .ok_or_else(|| ApiError::forbidden("realtime origin is required"))?
        .to_str()
        .map_err(|_| ApiError::forbidden("realtime origin is not allowed"))?;
    let allowed = origin == state.webauthn().origin()
        || state
            .webauthn()
            .alternate_origins()
            .iter()
            .any(|(accepted, _)| accepted.ascii_serialization() == origin);

    if allowed { Ok(()) } else { Err(ApiError::forbidden("realtime origin is not allowed")) }
}

/// Drives one realtime WebSocket until disconnect or forced HTTP recovery.
async fn handle_socket(
    socket: WebSocket,
    state: Arc<ServerState>,
    principal: RealtimePrincipal,
    mut subscription: RealtimeSubscription,
) {
    describe_realtime_metrics();
    gauge!("realtime.active_connections").increment(1.0);

    let (mut sender, mut incoming) = socket.split();
    match principal_is_active(state.db(), principal).await {
        Ok(true) => {}
        Ok(false) => {
            counter!("realtime.connections_closed_total", "reason" => "credential_inactive")
                .increment(1);
            close_inactive_principal(&mut sender).await;
            gauge!("realtime.active_connections").decrement(1.0);
            return;
        }
        Err(error) => {
            counter!("realtime.connections_closed_total", "reason" => "authorization_error")
                .increment(1);
            counter!("realtime.resync_required_total", "reason" => "authorization_error")
                .increment(1);
            warn!(
                target: "kival::server::realtime",
                error = ?error,
                "could not verify realtime credential state",
            );
            send_resync_and_close(&mut sender).await;
            gauge!("realtime.active_connections").decrement(1.0);
            return;
        }
    }

    let mut heartbeat = interval(HEARTBEAT_INTERVAL);
    heartbeat.tick().await;

    loop {
        let next = tokio::select! {
            invalidation = subscription.recv() => SocketEvent::Invalidation(invalidation),
            _ = heartbeat.tick() => SocketEvent::Heartbeat,
            incoming = incoming.next() => SocketEvent::Incoming(incoming),
        };

        match next {
            SocketEvent::Invalidation(Ok(message)) => {
                if message.kind == "realtime.resync_required" {
                    send_resync_and_close(&mut sender).await;
                    break;
                }

                if !principal_accepts_message_kind(principal, &message) {
                    counter!("realtime.messages_suppressed_total", "reason" => "personal_inbox")
                        .increment(1);
                    continue;
                }

                match principal_is_active(state.db(), principal).await {
                    Ok(true) => {}
                    Ok(false) => {
                        counter!(
                            "realtime.connections_closed_total",
                            "reason" => "credential_inactive"
                        )
                        .increment(1);
                        close_inactive_principal(&mut sender).await;
                        break;
                    }
                    Err(error) => {
                        counter!(
                            "realtime.connections_closed_total",
                            "reason" => "authorization_error"
                        )
                        .increment(1);
                        counter!(
                            "realtime.resync_required_total",
                            "reason" => "authorization_error"
                        )
                        .increment(1);
                        warn!(
                            target: "kival::server::realtime",
                            error = ?error,
                            "could not revalidate realtime credential state",
                        );
                        send_resync_and_close(&mut sender).await;
                        break;
                    }
                }

                match message_authorized(state.db(), principal, &message).await {
                    Ok(true) => {}
                    Ok(false) => {
                        counter!("realtime.messages_suppressed_total", "reason" => "authorization")
                            .increment(1);
                        continue;
                    }
                    Err(error) => {
                        counter!(
                            "realtime.connections_closed_total",
                            "reason" => "authorization_error"
                        )
                        .increment(1);
                        counter!(
                            "realtime.resync_required_total",
                            "reason" => "authorization_error"
                        )
                        .increment(1);
                        warn!(
                            target: "kival::server::realtime",
                            error = ?error,
                            "could not revalidate realtime resource authorization",
                        );
                        send_resync_and_close(&mut sender).await;
                        break;
                    }
                }

                let message_type = message.kind.clone();
                let payload = match serde_json::to_string(&message) {
                    Ok(payload) => payload,
                    Err(error) => {
                        error!(
                            target: "kival::server::realtime",
                            error = ?error,
                            "failed to serialize realtime invalidation",
                        );
                        continue;
                    }
                };

                if !send_message(&mut sender, Message::Text(payload.into())).await {
                    counter!("realtime.slow_or_disconnected_clients_total").increment(1);
                    break;
                }
                counter!("realtime.messages_sent_total", "type" => message_type).increment(1);
            }
            SocketEvent::Invalidation(Err(broadcast::error::RecvError::Lagged(_))) => {
                counter!("realtime.resync_required_total", "reason" => "lagged").increment(1);
                send_resync_and_close(&mut sender).await;
                break;
            }
            SocketEvent::Invalidation(Err(broadcast::error::RecvError::Closed))
            | SocketEvent::Incoming(Some(Ok(Message::Close(_))) | None) => break,
            SocketEvent::Heartbeat => {
                match principal_is_active(state.db(), principal).await {
                    Ok(true) => {}
                    Ok(false) => {
                        counter!(
                            "realtime.connections_closed_total",
                            "reason" => "credential_inactive"
                        )
                        .increment(1);
                        close_inactive_principal(&mut sender).await;
                        break;
                    }
                    Err(error) => {
                        counter!(
                            "realtime.connections_closed_total",
                            "reason" => "authorization_error"
                        )
                        .increment(1);
                        counter!(
                            "realtime.resync_required_total",
                            "reason" => "authorization_error"
                        )
                        .increment(1);
                        warn!(
                            target: "kival::server::realtime",
                            error = ?error,
                            "could not verify realtime credential state during heartbeat",
                        );
                        send_resync_and_close(&mut sender).await;
                        break;
                    }
                }

                if !send_message(&mut sender, Message::Ping(Vec::new().into())).await {
                    counter!("realtime.slow_or_disconnected_clients_total").increment(1);
                    break;
                }
            }
            SocketEvent::Incoming(Some(Ok(Message::Text(_) | Message::Binary(_)))) => {
                counter!("realtime.connections_closed_total", "reason" => "client_message")
                    .increment(1);
                close_unsupported_client_message(&mut sender).await;
                break;
            }
            SocketEvent::Incoming(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
            SocketEvent::Incoming(Some(Err(error))) => {
                trace!(
                    target: "kival::server::realtime",
                    error = ?error,
                    "realtime client disconnected with receive error",
                );
                break;
            }
        }
    }

    gauge!("realtime.active_connections").decrement(1.0);
}

/// Sends one bounded client message.
async fn send_message(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    message: Message,
) -> bool {
    matches!(timeout(SEND_TIMEOUT, sender.send(message)).await, Ok(Ok(())))
}

/// Builds the control message telling clients to reload authoritative state over HTTP.
fn resync_message() -> RealtimeMessage {
    RealtimeMessage {
        kind: "realtime.resync_required".to_owned(),
        workspace_id: None,
        object_id: None,
        event_id: None,
        inbox_entry_id: None,
    }
}

/// Requests HTTP recovery and closes one connection after a known realtime continuity gap.
async fn send_resync_and_close(sender: &mut futures_util::stream::SplitSink<WebSocket, Message>) {
    if let Ok(payload) = serde_json::to_string(&resync_message()) {
        let _ = send_message(sender, Message::Text(payload.into())).await;
    }
    let _ = send_message(
        sender,
        Message::Close(Some(CloseFrame {
            code: TRY_AGAIN_LATER,
            reason: "realtime updates were missed; refresh over HTTP".into(),
        })),
    )
    .await;
}

/// Closes one connection whose authenticating credential is no longer valid.
async fn close_inactive_principal(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
) {
    let _ = send_message(
        sender,
        Message::Close(Some(CloseFrame {
            code: POLICY_VIOLATION,
            reason: "realtime credentials are no longer valid".into(),
        })),
    )
    .await;
}

/// Closes one connection that attempted to send unsupported application data.
async fn close_unsupported_client_message(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
) {
    let _ = send_message(
        sender,
        Message::Close(Some(CloseFrame {
            code: POLICY_VIOLATION,
            reason: "realtime client messages are not supported".into(),
        })),
    )
    .await;
}

/// Returns whether this authentication context may receive this class of invalidation.
fn principal_accepts_message_kind(principal: RealtimePrincipal, message: &RealtimeMessage) -> bool {
    !(principal.is_api_key() && message.kind == "inbox.updated")
}

/// Returns whether the credential that opened this connection is still active.
async fn principal_is_active(
    pool: &PgPool,
    principal: RealtimePrincipal,
) -> Result<bool, sqlx::Error> {
    match principal {
        RealtimePrincipal::Session { user_id, session_id } => {
            realtime_session_active(pool, user_id, session_id).await
        }
        RealtimePrincipal::ApiKey { user_id, api_key_id } => {
            realtime_api_key_active(pool, user_id, api_key_id).await
        }
    }
}

/// Rechecks current resource authorization before delivering identifiers.
async fn message_authorized(
    pool: &PgPool,
    principal: RealtimePrincipal,
    message: &RealtimeMessage,
) -> Result<bool, sqlx::Error> {
    match principal {
        RealtimePrincipal::Session { user_id, .. } => {
            session_message_authorized(pool, user_id, message).await
        }
        RealtimePrincipal::ApiKey { user_id, api_key_id } => {
            api_key_message_authorized(pool, user_id, api_key_id, message).await
        }
    }
}

/// Applies current user authorization to a session-scoped invalidation.
async fn session_message_authorized(
    pool: &PgPool,
    user_id: Uuid,
    message: &RealtimeMessage,
) -> Result<bool, sqlx::Error> {
    let Some(workspace_id) = message.workspace_id else {
        return Ok(message.object_id.is_none()
            && message.event_id.is_none()
            && message.inbox_entry_id.is_none());
    };

    let Some(object_id) = message.object_id else {
        return realtime_workspace_authorized(pool, user_id, workspace_id).await;
    };

    realtime_object_authorized(pool, user_id, workspace_id, object_id).await
}

/// Applies the API key's live scope, workspace delegation, and owning-user authority.
async fn api_key_message_authorized(
    pool: &PgPool,
    user_id: Uuid,
    api_key_id: Uuid,
    message: &RealtimeMessage,
) -> Result<bool, sqlx::Error> {
    let (Some(workspace_id), Some(object_id)) = (message.workspace_id, message.object_id) else {
        return Ok(false);
    };
    realtime_api_key_object_authorized(pool, user_id, api_key_id, workspace_id, object_id).await
}

/// Listens for committed `PostgreSQL` invalidations until cancelled.
pub(crate) async fn run_listener(pool: PgPool, hub: RealtimeHub) {
    describe_realtime_metrics();
    // Until LISTEN is established the process has no continuity guarantee; if
    // a socket connected during startup, force it through HTTP recovery once
    // the listener becomes authoritative.
    let mut continuity_lost = true;
    let mut listener_established_once = false;

    loop {
        let mut listener = match PgListener::connect_with(&pool).await {
            Ok(listener) => listener,
            Err(error) => {
                continuity_lost = true;
                counter!("realtime.listener_failures_total", "stage" => "connect").increment(1);
                error!(
                    target: "kival::server::realtime",
                    error = ?error,
                    "failed to connect realtime PostgreSQL listener",
                );
                sleep(LISTENER_RETRY_DELAY).await;
                continue;
            }
        };

        if let Err(error) = listener.listen("kival_realtime").await {
            continuity_lost = true;
            counter!("realtime.listener_failures_total", "stage" => "listen").increment(1);
            error!(
                target: "kival::server::realtime",
                error = ?error,
                "failed to subscribe realtime PostgreSQL listener",
            );
            sleep(LISTENER_RETRY_DELAY).await;
            continue;
        }

        if listener_established_once {
            counter!("realtime.listener_reconnects_total").increment(1);
        } else {
            listener_established_once = true;
        }
        if std::mem::take(&mut continuity_lost) {
            force_listener_resync(
                &hub,
                "realtime PostgreSQL listener recovered after a continuity gap",
            );
        }

        loop {
            match listener.try_recv().await {
                Ok(Some(notification)) => {
                    match serde_json::from_str::<RealtimeEnvelope>(notification.payload()) {
                        Ok(envelope) => hub.publish(envelope),
                        Err(error) => {
                            counter!("realtime.invalid_payloads_total").increment(1);
                            warn!(
                                target: "kival::server::realtime",
                                error = ?error,
                                "ignored malformed realtime invalidation",
                            );
                        }
                    }
                }
                Ok(None) => {
                    counter!("realtime.listener_reconnects_total").increment(1);
                    force_listener_resync(
                        &hub,
                        "realtime PostgreSQL listener reconnected after a continuity gap",
                    );
                }
                Err(error) => {
                    continuity_lost = true;
                    counter!("realtime.listener_failures_total", "stage" => "receive").increment(1);
                    warn!(
                        target: "kival::server::realtime",
                        error = ?error,
                        "realtime PostgreSQL listener disconnected",
                    );
                    break;
                }
            }
        }

        sleep(LISTENER_RETRY_DELAY).await;
    }
}

/// Forces connected clients to recover after `PostgreSQL` listener continuity was uncertain.
fn force_listener_resync(hub: &RealtimeHub, message: &'static str) {
    let connections = hub.force_resync();
    if connections == 0 {
        return;
    }

    counter!("realtime.resync_required_total", "reason" => "listener_gap").increment(connections);
    warn!(
        target: "kival::server::realtime",
        connections,
        recovery = message,
        "forcing realtime HTTP recovery after a listener continuity gap",
    );
}

/// Registers realtime metric descriptions once.
fn describe_realtime_metrics() {
    DESCRIBE_REALTIME_METRICS.call_once(|| {
        describe_gauge!(
            "realtime.active_connections",
            "Active authenticated WebSocket connections."
        );
        describe_counter!(
            "realtime.messages_sent_total",
            "Realtime invalidations sent to clients."
        );
        describe_counter!(
            "realtime.messages_suppressed_total",
            "Realtime invalidations suppressed before delivery."
        );
        describe_counter!(
            "realtime.slow_or_disconnected_clients_total",
            "Realtime clients disconnected after a failed or timed-out write."
        );
        describe_counter!(
            "realtime.connections_closed_total",
            "Realtime connections closed by the server for bounded reasons."
        );
        describe_counter!(
            "realtime.resync_required_total",
            "Realtime connections forced to recover authoritative state over HTTP."
        );
        describe_counter!(
            "realtime.listener_failures_total",
            "PostgreSQL realtime listener failures by bounded stage."
        );
        describe_counter!(
            "realtime.listener_reconnects_total",
            "Successful PostgreSQL realtime listener reconnections after initial establishment."
        );
        describe_counter!(
            "realtime.invalid_payloads_total",
            "Malformed PostgreSQL realtime invalidation payloads ignored."
        );
    });
}

#[cfg(test)]
mod tests {
    use tokio::sync::broadcast::error::TryRecvError;

    use super::*;

    /// Builds one lightweight test invalidation.
    fn test_message(kind: &str) -> RealtimeMessage {
        RealtimeMessage {
            kind: kind.to_owned(),
            workspace_id: None,
            object_id: None,
            event_id: None,
            inbox_entry_id: None,
        }
    }

    #[test]
    fn personal_inbox_invalidations_are_session_only() {
        let user_id = Uuid::now_v7();
        let session = RealtimePrincipal::Session { user_id, session_id: Uuid::now_v7() };
        let api_key = RealtimePrincipal::ApiKey { user_id, api_key_id: Uuid::now_v7() };
        let inbox = test_message("inbox.updated");
        let commentary = test_message("comment.created");

        assert!(principal_accepts_message_kind(session, &inbox));
        assert!(!principal_accepts_message_kind(api_key, &inbox));
        assert!(principal_accepts_message_kind(session, &commentary));
        assert!(principal_accepts_message_kind(api_key, &commentary));
    }

    #[test]
    fn routes_invalidations_only_to_the_intended_recipient() {
        let hub = RealtimeHub::new();
        let recipient = Uuid::now_v7();
        let unrelated = Uuid::now_v7();
        let mut recipient_subscription = hub.subscribe(recipient).unwrap();
        let mut unrelated_subscription = hub.subscribe(unrelated).unwrap();

        hub.publish(RealtimeEnvelope {
            recipient_user_id: recipient,
            message: test_message("inbox.updated"),
        });

        assert_eq!(
            recipient_subscription.receiver.as_mut().unwrap().try_recv().unwrap().kind,
            "inbox.updated"
        );
        assert!(matches!(
            unrelated_subscription.receiver.as_mut().unwrap().try_recv(),
            Err(TryRecvError::Empty)
        ));
    }

    #[test]
    fn unrelated_recipient_traffic_cannot_lag_another_user() {
        let hub = RealtimeHub::new();
        let quiet_user = Uuid::now_v7();
        let noisy_user = Uuid::now_v7();
        let mut quiet_subscription = hub.subscribe(quiet_user).unwrap();
        let _noisy_subscription = hub.subscribe(noisy_user).unwrap();

        for _ in 0..=RECIPIENT_CAPACITY {
            hub.publish(RealtimeEnvelope {
                recipient_user_id: noisy_user,
                message: test_message("object.activity"),
            });
        }
        hub.publish(RealtimeEnvelope {
            recipient_user_id: quiet_user,
            message: test_message("inbox.updated"),
        });

        assert_eq!(
            quiet_subscription.receiver.as_mut().unwrap().try_recv().unwrap().kind,
            "inbox.updated"
        );
    }

    #[test]
    fn removes_recipient_channel_after_the_final_connection_disconnects() {
        let hub = RealtimeHub::new();
        let user_id = Uuid::now_v7();
        let first = hub.subscribe(user_id).unwrap();
        let second = hub.subscribe(user_id).unwrap();

        drop(first);
        assert!(
            hub.recipients
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_key(&user_id)
        );

        drop(second);
        assert!(
            !hub.recipients
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_key(&user_id)
        );
    }

    #[test]
    fn bounds_active_connections_per_user() {
        let hub = RealtimeHub::new();
        let user_id = Uuid::now_v7();
        let subscriptions = (0..MAX_CONNECTIONS_PER_USER)
            .map(|_| hub.subscribe(user_id).expect("connection below limit"))
            .collect::<Vec<_>>();

        assert!(hub.subscribe(user_id).is_none());
        drop(subscriptions);
        assert!(hub.subscribe(user_id).is_some());
    }

    /// Creates a session-backed realtime principal with a controlled lifecycle.
    async fn test_session_principal(
        pool: &PgPool,
        suffix: &str,
        hash_byte: u8,
        expired: bool,
    ) -> RealtimePrincipal {
        let user_id: Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO kival.users (username, display_name)
            VALUES ($1, $2)
            RETURNING id
            "#,
        )
        .bind(format!("realtime-{suffix}"))
        .bind(format!("Realtime {suffix}"))
        .fetch_one(pool)
        .await
        .unwrap();
        let session_id: Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO kival.sessions (
                user_id,
                session_token_hash,
                csrf_token_hash,
                created_at,
                expires_at
            )
            VALUES (
                $1,
                $2,
                $3,
                CASE WHEN $4 THEN now() - interval '2 hours' ELSE now() END,
                CASE WHEN $4 THEN now() - interval '1 hour' ELSE now() + interval '1 hour' END
            )
            RETURNING id
            "#,
        )
        .bind(user_id)
        .bind(vec![hash_byte; 32])
        .bind(vec![hash_byte.saturating_add(100); 32])
        .bind(expired)
        .fetch_one(pool)
        .await
        .unwrap();

        RealtimePrincipal::Session { user_id, session_id }
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn session_principal_tracks_revocation_expiry_and_user_disable(pool: PgPool) {
        let active = test_session_principal(&pool, "revoked-session", 1, false).await;
        assert!(principal_is_active(&pool, active).await.unwrap());

        let RealtimePrincipal::Session { session_id, .. } = active else {
            unreachable!();
        };
        sqlx::query(
            r#"
            UPDATE kival.sessions
            SET revoked_at = now(),
                revoked_by_operator = true,
                revocation_reason = 'test revocation'
            WHERE id = $1
            "#,
        )
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();
        assert!(!principal_is_active(&pool, active).await.unwrap());

        let expired = test_session_principal(&pool, "expired-session", 2, true).await;
        assert!(!principal_is_active(&pool, expired).await.unwrap());

        let disabled = test_session_principal(&pool, "disabled-user", 3, false).await;
        assert!(principal_is_active(&pool, disabled).await.unwrap());
        let user_id = disabled.user_id();
        sqlx::query(
            r#"
            UPDATE kival.users
            SET status = 'disabled',
                disabled_at = now(),
                disabled_by_operator = true
            WHERE id = $1
            "#,
        )
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
        assert!(!principal_is_active(&pool, disabled).await.unwrap());

        sqlx::query(
            r#"
            UPDATE kival.users
            SET status = 'active',
                disabled_at = NULL,
                disabled_by = NULL,
                disabled_by_operator = false
            WHERE id = $1
            "#,
        )
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
        assert!(principal_is_active(&pool, disabled).await.unwrap());
    }

    #[test]
    fn force_resync_reaches_every_active_local_connection() {
        let hub = RealtimeHub::new();
        let first_user = Uuid::now_v7();
        let second_user = Uuid::now_v7();
        let mut first = hub.subscribe(first_user).unwrap();
        let mut second = hub.subscribe(second_user).unwrap();

        assert_eq!(hub.force_resync(), 2);
        assert_eq!(
            first.receiver.as_mut().unwrap().try_recv().unwrap().kind,
            "realtime.resync_required"
        );
        assert_eq!(
            second.receiver.as_mut().unwrap().try_recv().unwrap().kind,
            "realtime.resync_required"
        );
    }
}
