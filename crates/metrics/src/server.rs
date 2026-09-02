//! Server for serving the Prometheus metrics endpoint.

use std::net::SocketAddr;

use axum::{
    http::{HeaderValue, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::get,
};
use eyre::{Result, WrapErr};
use kival_tracing::info;
use tokio::net::TcpListener;

use crate::{
    hooks::Hooks, process::Collector, recorder::install_prometheus_recorder, version::VersionInfo,
};

/// Starts the Prometheus metrics server.
///
/// # Arguments
/// * `prefix` - Metric name prefix (for example `"kival"`).
/// * `address` - Socket address to bind the HTTP metrics endpoint.
/// * `version_info` - Build/version labels exported by the metrics endpoint.
/// * `hooks` - Metrics hooks to execute before rendering each scrape.
/// * `shutdown` - Future that starts graceful endpoint shutdown when it resolves.
///
/// # Errors
///
/// Returns an error when recorder initialization or endpoint binding fails.
pub async fn start_metrics_server<S>(
    prefix: &'static str,
    address: &SocketAddr,
    version_info: VersionInfo,
    hooks: Hooks,
    shutdown: S,
) -> Result<tokio::task::JoinHandle<Result<()>>>
where
    S: Future<Output = ()> + Send + 'static,
{
    info!("Starting metrics server: http://{address}");

    let recorder = install_prometheus_recorder(prefix)?;
    recorder.spawn_upkeep();
    let listener = TcpListener::bind(*address)
        .await
        .wrap_err_with(|| format!("Could not start Prometheus endpoint at {address}"))?;

    Collector::default().describe();
    describe_io_stats();
    version_info.register_version_metrics();

    Ok(tokio::spawn(async move {
        axum::serve(
            listener,
            get(move || {
                let hooks = hooks.clone();
                async move {
                    hooks.run();
                    metrics_response(recorder.handle().render())
                }
            })
            .into_make_service(),
        )
        .with_graceful_shutdown(shutdown)
        .await
        .wrap_err("Metrics server stopped with an error")
    }))
}

/// Builds one Prometheus text exposition response.
fn metrics_response(metrics: String) -> Response {
    let mut response = metrics.into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"));
    response
}

#[cfg(target_os = "linux")]
/// Describe process IO metrics exposed by the scrape hooks.
fn describe_io_stats() {
    use crate::describe_counter;

    describe_counter!("io.rchar", "Characters read");
    describe_counter!("io.wchar", "Characters written");
    describe_counter!("io.syscr", "Read syscalls");
    describe_counter!("io.syscw", "Write syscalls");
    describe_counter!("io.read_bytes", "Bytes read");
    describe_counter!("io.write_bytes", "Bytes written");
    describe_counter!("io.cancelled_write_bytes", "Cancelled write bytes");
}

#[cfg(not(target_os = "linux"))]
/// No-op IO metric description on non-Linux targets.
const fn describe_io_stats() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_response_uses_prometheus_text_content_type() {
        let response = metrics_response(String::new());

        assert_eq!(
            response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
            Some("text/plain; version=0.0.4; charset=utf-8")
        );
    }
}
