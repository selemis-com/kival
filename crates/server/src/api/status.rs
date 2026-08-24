//! Public handlers for the server.

use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use kival_kernel::database_ready;
use kival_sdk::StatusResponse;
use kival_tracing::warn;

use crate::{ServerState, api::error::ApiError};

/// Ready handler for the server.
pub(crate) async fn handle_get_ready(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    match database_ready(state.db()).await {
        Ok(_) => (StatusCode::OK, Json(StatusResponse::ok())),
        Err(error) => {
            warn!(target: "kival::server::readiness", %error, "Database readiness check failed");
            (StatusCode::SERVICE_UNAVAILABLE, Json(StatusResponse::error()))
        }
    }
}

/// Health check handler for the server.
pub(crate) async fn handle_get_health(State(_): State<Arc<ServerState>>) -> impl IntoResponse {
    (StatusCode::OK, Json(StatusResponse::ok()))
}

/// Fallback handler for unmatched routes.
pub(crate) async fn handle_get_fallback() -> Response {
    ApiError::not_found("route not found").into_response()
}

/// Fallback handler for methods that are not supported by a matched route.
pub(crate) async fn handle_method_not_allowed() -> Response {
    ApiError::new(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed", "method not allowed")
        .into_response()
}
