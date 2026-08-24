//! Layers used in the server.

use std::{
    any::Any,
    time::{Duration, Instant},
};

use axum::{
    BoxError, Router,
    body::Body,
    error_handling::HandleErrorLayer,
    extract::{MatchedPath, Request as AxumRequest},
    http::{HeaderMap, HeaderName, HeaderValue, Request, Response as HttpResponse, header},
    middleware::{Next, from_fn},
    response::{IntoResponse, Response},
};
use kival_metrics::{
    counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram,
};
use kival_tracing::{Level, Span, debug_span, error, field};
use tower::{ServiceBuilder, timeout::TimeoutLayer};
use tower_http::{
    catch_panic::{CatchPanicLayer, ResponseForPanic},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::{SetSensitiveRequestHeadersLayer, SetSensitiveResponseHeadersLayer},
    set_header::SetResponseHeaderLayer,
    trace::{DefaultOnFailure, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};

use crate::api::error::ApiError;

/// Maximum time allowed for a request, including body ingestion, to complete.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// Builds the layers for the server.
///
/// This creates a [`ServiceBuilder`] with request IDs, tracing, panic catching,
/// request timeout handling, sensitive header marking, and browser response hardening.
pub fn build_layers(router: Router) -> Router {
    build_layers_with_timeout(router, DEFAULT_REQUEST_TIMEOUT)
}

/// Builds the server layers with an explicit whole-request timeout.
pub fn build_layers_with_timeout(router: Router, request_timeout: Duration) -> Router {
    describe_http_metrics();

    router.layer(
        ServiceBuilder::new()
            .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
            .layer(PropagateRequestIdLayer::x_request_id())
            .layer(SetResponseHeaderLayer::if_not_present(
                HeaderName::from_static("x-content-type-options"),
                HeaderValue::from_static("nosniff"),
            ))
            .layer(SetResponseHeaderLayer::if_not_present(
                HeaderName::from_static("referrer-policy"),
                HeaderValue::from_static("same-origin"),
            ))
            .layer(SetResponseHeaderLayer::if_not_present(
                HeaderName::from_static("x-frame-options"),
                HeaderValue::from_static("DENY"),
            ))
            .layer(SetResponseHeaderLayer::if_not_present(
                HeaderName::from_static("permissions-policy"),
                HeaderValue::from_static(
                    "camera=(), microphone=(), geolocation=(), payment=(), usb=()",
                ),
            ))
            .layer(SetResponseHeaderLayer::if_not_present(
                HeaderName::from_static("content-security-policy"),
                HeaderValue::from_static(
                    "default-src 'self'; \
                    base-uri 'none'; \
                    connect-src 'self'; \
                    font-src 'self' data:; \
                    frame-ancestors 'none'; \
                    img-src 'self'; \
                    object-src 'none'; \
                    script-src 'self'; \
                    style-src 'self' 'unsafe-inline'; \
                    form-action 'self'",
                ),
            ))
            .layer(SetSensitiveRequestHeadersLayer::new([header::AUTHORIZATION, header::COOKIE]))
            .layer(SetSensitiveResponseHeadersLayer::new([header::SET_COOKIE]))
            .layer(from_fn(record_http_metrics))
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(|request: &Request<_>| make_request_span(request))
                    .on_request(DefaultOnRequest::new().level(Level::DEBUG))
                    .on_response(DefaultOnResponse::new().level(Level::DEBUG))
                    .on_failure(DefaultOnFailure::new().level(Level::WARN)),
            )
            .layer(CatchPanicLayer::custom(RecordHttpPanic))
            .layer(HandleErrorLayer::new(handle_request_timeout))
            .layer(TimeoutLayer::new(request_timeout)),
    )
}

/// Converts service-level request timeouts into the normal API error envelope.
async fn handle_request_timeout(_error: BoxError) -> Response {
    counter!("http.server.timeouts_total").increment(1);
    ApiError::service_unavailable("request timed out").with_origin("timeout").into_response()
}

/// Describe HTTP server metrics emitted by [`record_http_metrics`].
fn describe_http_metrics() {
    describe_counter!("http.server.requests_total", "Completed HTTP requests.");
    describe_counter!(
        "http.server.aborted_requests_total",
        "HTTP requests dropped before a response was produced."
    );
    describe_counter!("http.server.panics_total", "HTTP request panics caught by middleware.");
    describe_counter!(
        "http.server.timeouts_total",
        "HTTP requests terminated by the server timeout."
    );
    describe_gauge!(
        "http.server.in_flight_requests",
        "HTTP request futures currently awaiting an application response."
    );
    describe_histogram!(
        "http.server.request_duration_seconds",
        "Time until the application produced an HTTP response."
    );
    describe_histogram!(
        "http.server.aborted_request_duration_seconds",
        "Time spent in HTTP requests dropped before an application response was produced."
    );
}

/// Tracks one in-flight request and records completion or cancellation.
#[derive(Debug)]
struct HttpRequestMetrics {
    /// Stable bounded labels for this request.
    labels: Vec<(String, String)>,
    /// Request processing start time.
    started_at: Instant,
    /// Whether a response was produced.
    completed: bool,
}

impl HttpRequestMetrics {
    /// Starts tracking one request.
    fn new(labels: Vec<(String, String)>) -> Self {
        gauge!("http.server.in_flight_requests", labels.as_slice()).increment(1.0);
        Self { labels, started_at: Instant::now(), completed: false }
    }

    /// Records a completed response.
    fn complete(&mut self, status: axum::http::StatusCode) {
        if self.completed {
            return;
        }
        self.completed = true;

        let mut labels = self.labels.clone();
        labels.push(("status".to_owned(), status.as_u16().to_string()));

        counter!("http.server.requests_total", labels.as_slice()).increment(1);
        histogram!("http.server.request_duration_seconds", labels)
            .record(self.started_at.elapsed().as_secs_f64());
    }
}

impl Drop for HttpRequestMetrics {
    fn drop(&mut self) {
        gauge!("http.server.in_flight_requests", self.labels.as_slice()).decrement(1.0);
        if !self.completed {
            counter!("http.server.aborted_requests_total", self.labels.as_slice()).increment(1);
            histogram!("http.server.aborted_request_duration_seconds", self.labels.as_slice())
                .record(self.started_at.elapsed().as_secs_f64());
        }
    }
}

/// Record request count and duration metrics around the remaining service stack.
async fn record_http_metrics(request: AxumRequest, next: Next) -> Response {
    let method = request_method(request.method()).to_owned();
    let route = request_route(&request).to_owned();
    let request_labels = vec![("method".to_owned(), method), ("route".to_owned(), route)];
    let mut metrics = HttpRequestMetrics::new(request_labels);

    let response = next.run(request).await;
    metrics.complete(response.status());

    response
}

/// Returns a bounded method label for metrics.
fn request_method(method: &axum::http::Method) -> &'static str {
    match method.as_str() {
        "CONNECT" => "CONNECT",
        "DELETE" => "DELETE",
        "GET" => "GET",
        "HEAD" => "HEAD",
        "OPTIONS" => "OPTIONS",
        "PATCH" => "PATCH",
        "POST" => "POST",
        "PUT" => "PUT",
        "TRACE" => "TRACE",
        _ => "OTHER",
    }
}

/// Returns a bounded route label for metrics and request tracing.
fn request_route<B>(request: &Request<B>) -> &str {
    request.extensions().get::<MatchedPath>().map_or_else(
        || {
            let path = request.uri().path();
            if path == "/api" || path.starts_with("/api/") {
                "unmatched_api"
            } else {
                "web_fallback"
            }
        },
        MatchedPath::as_str,
    )
}

/// Panic response handler that records caught HTTP panics.
#[derive(Clone, Copy, Debug)]
struct RecordHttpPanic;

impl ResponseForPanic for RecordHttpPanic {
    type ResponseBody = Body;

    fn response_for_panic(
        &mut self,
        error: Box<dyn Any + Send + 'static>,
    ) -> HttpResponse<Self::ResponseBody> {
        counter!("http.server.panics_total").increment(1);
        let panic_message = error
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| error.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("non-string panic payload");
        error!(target: "kival::server::http", panic = panic_message, "Request handler panicked");
        ApiError::internal("request handler panicked").with_origin("panic").into_response()
    }
}

/// Builds a request span with explicit header selection.
///
/// Only stable, low-risk request metadata should be logged here. Literal paths and query values
/// are deliberately excluded because they may contain object identifiers, workspace search terms,
/// attachment names, or other private organizational data.
fn make_request_span<B>(request: &Request<B>) -> Span {
    let headers = request.headers();

    let span = debug_span!(
        target: "kival::server::http",
        "request",
        method = %request.method(),
        route = request_route(request),
        version = ?request.version(),
        request_id = field::Empty,
        user_agent = field::Empty,
        content_type = field::Empty,
        content_length = field::Empty,
    );

    record_header(&span, "request_id", headers, &HeaderName::from_static("x-request-id"));
    record_header(&span, "user_agent", headers, &header::USER_AGENT);
    record_header(&span, "content_type", headers, &header::CONTENT_TYPE);
    record_header(&span, "content_length", headers, &header::CONTENT_LENGTH);

    span
}

/// Records a UTF-8 header value for headers that are safe to log directly.
fn record_header(span: &Span, field_name: &'static str, headers: &HeaderMap, name: &HeaderName) {
    if let Some(value) = headers.get(name).and_then(|value| value.to_str().ok()) {
        span.record(field_name, value);
    }
}

#[cfg(test)]
mod tests {
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
    fn unmatched_api_paths_have_a_bounded_route_label() {
        let request = Request::builder().uri("/api/v1/not-a-route").body(()).expect("request");

        assert_eq!(request_route(&request), "unmatched_api");
    }

    #[test]
    fn web_fallback_paths_have_a_bounded_route_label() {
        let request = Request::builder().uri("/objects/example").body(()).expect("request");

        assert_eq!(request_route(&request), "web_fallback");
    }

    #[test]
    fn similar_non_api_prefixes_remain_web_fallbacks() {
        let request = Request::builder().uri("/apiary").body(()).expect("request");

        assert_eq!(request_route(&request), "web_fallback");
    }

    #[test]
    fn standard_and_extension_methods_have_bounded_metric_labels() {
        assert_eq!(request_method(&axum::http::Method::GET), "GET");
        let extension =
            axum::http::Method::from_bytes(b"KIVAL-CUSTOM").expect("extension method should parse");
        assert_eq!(request_method(&extension), "OTHER");
    }

    #[test]
    fn completed_request_metrics_record_once_and_release_in_flight_gauge() {
        let (_guard, handle) = test_metrics();
        let labels = vec![
            ("method".to_owned(), "GET".to_owned()),
            ("route".to_owned(), "/api/v1/status".to_owned()),
        ];
        let mut metrics = HttpRequestMetrics::new(labels);

        metrics.complete(axum::http::StatusCode::OK);
        metrics.complete(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        drop(metrics);

        let rendered = handle.render();
        assert!(
            rendered.contains(
                r#"http_server_requests_total{method="GET",route="/api/v1/status",status="200"} 1"#
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                r#"http_server_request_duration_seconds_count{method="GET",route="/api/v1/status",status="200"} 1"#
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                r#"http_server_in_flight_requests{method="GET",route="/api/v1/status"} 0"#
            ),
            "{rendered}"
        );
        assert!(!rendered.contains("http_server_aborted_requests_total"), "{rendered}");
        assert!(!rendered.contains(r#"status="500""#), "{rendered}");
    }

    #[test]
    fn dropped_request_records_aborted_count_duration_and_releases_gauge() {
        let (_guard, handle) = test_metrics();
        let labels = vec![
            ("method".to_owned(), "POST".to_owned()),
            ("route".to_owned(), "/api/v1/objects".to_owned()),
        ];

        drop(HttpRequestMetrics::new(labels));

        let rendered = handle.render();
        assert!(
            rendered.contains(
                r#"http_server_aborted_requests_total{method="POST",route="/api/v1/objects"} 1"#
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                r#"http_server_aborted_request_duration_seconds_count{method="POST",route="/api/v1/objects"} 1"#
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                r#"http_server_in_flight_requests{method="POST",route="/api/v1/objects"} 0"#
            ),
            "{rendered}"
        );
        assert!(!rendered.contains("http_server_requests_total"), "{rendered}");
    }
}
