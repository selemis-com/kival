//! Bounded, privacy-preserving API subsystem metrics.

use std::{sync::Once, time::Instant};

use kival_metrics::{counter, describe_counter, describe_histogram, histogram};

/// Ensures authentication metric descriptions are emitted once.
static DESCRIBE_AUTHENTICATION_METRICS: Once = Once::new();
/// Ensures search metric descriptions are emitted once.
static DESCRIBE_SEARCH_METRICS: Once = Once::new();
/// Ensures graph metric descriptions are emitted once.
static DESCRIBE_GRAPH_METRICS: Once = Once::new();

/// Tracks one authentication and attached API-key policy validation.
#[derive(Debug)]
pub(super) struct AuthenticationMetrics {
    /// Session or API-key validation.
    mechanism: &'static str,
    /// Validation start time.
    started_at: Instant,
    /// Whether a terminal outcome has already been recorded.
    completed: bool,
}

impl AuthenticationMetrics {
    /// Starts one authentication validation measurement.
    pub(super) fn start(mechanism: &'static str) -> Self {
        describe_authentication_metrics();
        Self { mechanism, started_at: Instant::now(), completed: false }
    }

    /// Records one terminal validation outcome exactly once.
    pub(super) fn complete(&mut self, outcome: &'static str) {
        if self.completed {
            return;
        }

        self.completed = true;
        counter!(
            "auth.validations_total",
            "mechanism" => self.mechanism,
            "outcome" => outcome
        )
        .increment(1);
        histogram!(
            "auth.validation_duration_seconds",
            "mechanism" => self.mechanism,
            "outcome" => outcome
        )
        .record(self.started_at.elapsed().as_secs_f64());
    }
}

impl Drop for AuthenticationMetrics {
    fn drop(&mut self) {
        if !self.completed {
            self.complete("cancelled");
        }
    }
}

/// Registers authentication metric descriptions once.
fn describe_authentication_metrics() {
    DESCRIBE_AUTHENTICATION_METRICS.call_once(|| {
        describe_counter!(
            "auth.validations_total",
            "Authentication and attached API-key policy validation outcomes."
        );
        describe_histogram!(
            "auth.validation_duration_seconds",
            "Authentication and attached API-key policy validation duration."
        );
    });
}

/// Tracks one workspace search after authorization and input validation.
#[derive(Debug)]
pub(super) struct SearchMetrics {
    /// Stable search-mode label.
    mode: &'static str,
    /// Stable archive-status label.
    status: &'static str,
    /// Stable version-scope label.
    scope: &'static str,
    /// Query start time.
    started_at: Instant,
    /// Whether the operation reached a completed response.
    completed: bool,
}

impl SearchMetrics {
    /// Starts one search measurement.
    pub(super) fn start(mode: &'static str, status: &'static str, scope: &'static str) -> Self {
        describe_search_metrics();
        Self { mode, status, scope, started_at: Instant::now(), completed: false }
    }

    /// Records a completed search and its result count.
    pub(super) fn complete(&mut self, result_count: usize) {
        self.record_duration("completed");
        histogram!(
            "search.results",
            "mode" => self.mode,
            "status" => self.status,
            "scope" => self.scope
        )
        .record(result_count as f64);
        if result_count == 0 {
            counter!(
                "search.zero_results_total",
                "mode" => self.mode,
                "status" => self.status,
                "scope" => self.scope
            )
            .increment(1);
        }
        self.completed = true;
    }

    /// Records elapsed time with a bounded outcome label.
    fn record_duration(&self, outcome: &'static str) {
        histogram!(
            "search.query_duration_seconds",
            "mode" => self.mode,
            "status" => self.status,
            "scope" => self.scope,
            "outcome" => outcome
        )
        .record(self.started_at.elapsed().as_secs_f64());
    }
}

impl Drop for SearchMetrics {
    fn drop(&mut self) {
        if !self.completed {
            self.record_duration("incomplete");
        }
    }
}

/// Registers search metric descriptions once.
fn describe_search_metrics() {
    DESCRIBE_SEARCH_METRICS.call_once(|| {
        describe_histogram!(
            "search.query_duration_seconds",
            "Authorized workspace search duration."
        );
        describe_histogram!("search.results", "Search results returned per completed query.");
        describe_counter!(
            "search.zero_results_total",
            "Completed search queries that returned no results."
        );
    });
}

/// Tracks one bounded graph projection after authorization and input validation.
#[derive(Debug)]
pub(super) struct GraphMetrics {
    /// Object-centered or workspace-wide projection.
    projection: &'static str,
    /// Projection start time.
    started_at: Instant,
    /// Whether the operation reached a completed response.
    completed: bool,
}

impl GraphMetrics {
    /// Starts one graph projection measurement.
    pub(super) fn start(projection: &'static str) -> Self {
        describe_graph_metrics();
        Self { projection, started_at: Instant::now(), completed: false }
    }

    /// Records a completed graph projection.
    pub(super) fn complete(
        &mut self,
        node_count: usize,
        edge_count: usize,
        truncated_nodes: bool,
        truncated_edges: bool,
    ) {
        self.record_duration("completed");
        histogram!("graph.projection_nodes", "projection" => self.projection)
            .record(node_count as f64);
        histogram!("graph.projection_edges", "projection" => self.projection)
            .record(edge_count as f64);

        if truncated_nodes {
            counter!(
                "graph.projection_truncations_total",
                "projection" => self.projection,
                "resource" => "nodes"
            )
            .increment(1);
        }
        if truncated_edges {
            counter!(
                "graph.projection_truncations_total",
                "projection" => self.projection,
                "resource" => "edges"
            )
            .increment(1);
        }

        self.completed = true;
    }

    /// Records elapsed time with a bounded outcome label.
    fn record_duration(&self, outcome: &'static str) {
        histogram!(
            "graph.projection_duration_seconds",
            "projection" => self.projection,
            "outcome" => outcome
        )
        .record(self.started_at.elapsed().as_secs_f64());
    }
}

impl Drop for GraphMetrics {
    fn drop(&mut self) {
        if !self.completed {
            self.record_duration("incomplete");
        }
    }
}

/// Registers graph metric descriptions once.
fn describe_graph_metrics() {
    DESCRIBE_GRAPH_METRICS.call_once(|| {
        describe_histogram!(
            "graph.projection_duration_seconds",
            "Authorized bounded graph projection duration."
        );
        describe_histogram!(
            "graph.projection_nodes",
            "Graph nodes returned per completed projection."
        );
        describe_histogram!(
            "graph.projection_edges",
            "Graph edges returned per completed projection."
        );
        describe_counter!(
            "graph.projection_truncations_total",
            "Graph projections truncated by configured response limits."
        );
    });
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
    fn authentication_metrics_record_one_completed_outcome() {
        let (_guard, handle) = test_metrics();
        let mut metrics = AuthenticationMetrics::start("session");

        metrics.complete("success");
        metrics.complete("error");
        drop(metrics);

        let rendered = handle.render();
        assert!(
            rendered.contains(r#"auth_validations_total{mechanism="session",outcome="success"} 1"#),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                r#"auth_validation_duration_seconds_count{mechanism="session",outcome="success"} 1"#
            ),
            "{rendered}"
        );
        assert!(!rendered.contains(r#"outcome="error""#), "{rendered}");
        assert!(!rendered.contains(r#"outcome="cancelled""#), "{rendered}");
    }

    #[test]
    fn authentication_metrics_record_cancellation_on_drop() {
        let (_guard, handle) = test_metrics();

        drop(AuthenticationMetrics::start("api_key"));

        let rendered = handle.render();
        assert!(
            rendered
                .contains(r#"auth_validations_total{mechanism="api_key",outcome="cancelled"} 1"#),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                r#"auth_validation_duration_seconds_count{mechanism="api_key",outcome="cancelled"} 1"#
            ),
            "{rendered}"
        );
    }
}
