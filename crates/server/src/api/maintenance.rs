//! Metrics for opportunistic server maintenance work.

use std::{sync::Once, time::Instant};

use kival_metrics::{counter, describe_counter, describe_histogram, histogram};
/// Ensures maintenance metric descriptions are emitted once.
static DESCRIBE_MAINTENANCE_METRICS: Once = Once::new();

/// Records one cleanup run whose kernel binding returns only the affected row count.
pub(super) fn record_cleanup_rows<E>(
    task: &'static str,
    started_at: Instant,
    result: &Result<u64, E>,
) {
    DESCRIBE_MAINTENANCE_METRICS.call_once(|| {
        describe_counter!(
            "maintenance.cleanup_runs_total",
            "Best-effort maintenance cleanup runs."
        );
        describe_counter!(
            "maintenance.cleanup_rows_total",
            "Rows removed by best-effort maintenance cleanup."
        );
        describe_histogram!(
            "maintenance.cleanup_duration_seconds",
            "Best-effort maintenance cleanup query duration."
        );
    });

    let outcome = result.as_ref().map_or_else(
        |_| {
            counter!("maintenance.cleanup_runs_total", "task" => task, "outcome" => "error")
                .increment(1);
            "error"
        },
        |rows| {
            counter!("maintenance.cleanup_runs_total", "task" => task, "outcome" => "completed")
                .increment(1);
            counter!("maintenance.cleanup_rows_total", "task" => task).increment(*rows);
            "completed"
        },
    );
    histogram!("maintenance.cleanup_duration_seconds", "task" => task, "outcome" => outcome)
        .record(started_at.elapsed().as_secs_f64());
}
