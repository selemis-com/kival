//! JSON request extractor that returns API error envelopes.

use axum::{
    Json,
    extract::{FromRequest, Request},
};
use serde::de::DeserializeOwned;

use crate::api::error::ApiError;

/// JSON request body extractor with API-shaped rejection responses.
pub(crate) struct JsonBody<T>(pub(crate) T);

impl<S, T> FromRequest<S> for JsonBody<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state).await.map(|Json(value)| Self(value)).map_err(
            |rejection| {
                if rejection.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
                    ApiError::payload_too_large(rejection.body_text())
                } else {
                    ApiError::bad_request(rejection.body_text())
                }
            },
        )
    }
}
