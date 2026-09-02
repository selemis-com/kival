//! End-to-end tests for the Prometheus exporter.
//!
//! Each test installs the recorder under a thread-local override so tests
//! don't interfere with one another even when run in parallel.

#[cfg(test)]
mod tests {
    use kival_metrics::{
        Key, Label, LocalRecorderGuard, Recorder, counter, describe_counter, describe_gauge,
        describe_histogram, gauge, histogram,
        prometheus::{PrometheusBuilder, PrometheusHandle},
        set_default_local_recorder,
    };

    fn install() -> (LocalRecorderGuard, PrometheusHandle) {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let guard = set_default_local_recorder(recorder);
        (guard, handle)
    }

    #[test]
    fn renders_counter_with_help_and_type() {
        let (_g, h) = install();
        describe_counter!("requests_total", "Total HTTP requests");
        counter!("requests_total").increment(7);

        let body = h.render();
        assert!(body.contains("# HELP requests_total Total HTTP requests"), "{body}");
        assert!(body.contains("# TYPE requests_total counter"), "{body}");
        assert!(body.contains("requests_total 7"), "{body}");
    }

    #[test]
    fn renders_gauge_with_value() {
        let (_g, h) = install();
        describe_gauge!("temperature", "Current temperature");
        gauge!("temperature").set(42.5);

        let body = h.render();
        assert!(body.contains("# TYPE temperature gauge"), "{body}");
        assert!(body.contains("temperature 42.5"), "{body}");
    }

    #[test]
    fn renders_histogram_as_summary_with_quantiles() {
        let (_g, h) = install();
        describe_histogram!("latency_seconds", "Request latency");
        let hist = histogram!("latency_seconds");
        for v in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0] {
            hist.record(v);
        }

        let body = h.render();
        assert!(body.contains("# TYPE latency_seconds summary"), "{body}");
        assert!(body.contains("latency_seconds{quantile=\"0.5\"}"), "{body}");
        assert!(body.contains("latency_seconds_count 10"), "{body}");
        // Sum of 0.1..1.0 step 0.1 = 5.5
        assert!(body.contains("latency_seconds_sum 5.5"), "{body}");
    }

    #[test]
    fn labels_are_emitted_and_sorted_deterministically() {
        let (_g, h) = install();
        counter!("hits", "method" => "GET", "path" => "/").increment(1);
        counter!("hits", "method" => "POST", "path" => "/").increment(2);

        let body = h.render();
        // Label rendering keys come out alphabetically by label key.
        assert!(body.contains(r#"hits{method="GET",path="/"} 1"#), "{body}");
        assert!(body.contains(r#"hits{method="POST",path="/"} 2"#), "{body}");
        // Single TYPE/HELP block per name.
        assert_eq!(body.matches("# TYPE hits counter").count(), 1, "{body}");
    }

    #[test]
    fn label_values_are_escaped() {
        let (_g, h) = install();
        counter!("escaped", "k" => "a\"b\\c\nd").increment(1);
        let body = h.render();
        // " \ \n in the value all become escaped sequences.
        assert!(body.contains(r#"escaped{k="a\"b\\c\nd"} 1"#), "{body}");
    }

    #[test]
    fn unregistered_describe_still_emits_help_when_metric_exists() {
        let (_g, h) = install();
        describe_counter!("late_described", "Description registered before the metric");
        counter!("late_described").increment(0);
        let body = h.render();
        assert!(body.contains("# HELP late_described Description registered before the metric"));
    }

    #[test]
    fn render_is_stable_across_multiple_calls() {
        let (_g, h) = install();
        counter!("a").increment(1);
        counter!("b").increment(2);
        let first = h.render();
        let second = h.render();
        assert_eq!(first, second);
    }

    #[test]
    fn run_upkeep_preserves_counters_and_gauges() {
        let (_g, h) = install();
        counter!("preserved").increment(5);
        gauge!("also_preserved").set(7.0);
        // Upkeep only rotates histogram buckets; counter/gauge state must
        // survive any number of upkeep cycles.
        for _ in 0..10 {
            h.run_upkeep();
        }
        let body = h.render();
        assert!(body.contains("preserved 5"), "{body}");
        assert!(body.contains("also_preserved 7"), "{body}");
    }

    #[test]
    fn empty_histogram_renders_zero_quantiles_not_nan() {
        // A histogram that has been registered (so it appears in the
        // registry) but never recorded into must render `0` for every
        // quantile, never `NaN` — `NaN` is invalid in Prometheus exposition
        // and would trip strict parsers at scrape time.
        let (_g, h) = install();
        let _ = histogram!("empty_hist");
        let body = h.render();
        assert!(body.contains("# TYPE empty_hist summary"), "{body}");
        assert!(!body.contains("NaN"), "rendered NaN: {body}");
        assert!(body.contains(r#"empty_hist{quantile="0.5"} 0"#), "{body}");
        assert!(body.contains("empty_hist_count 0"), "{body}");
        assert!(body.contains("empty_hist_sum 0"), "{body}");
    }

    #[test]
    fn quantile_uses_nearest_rank_definition() {
        // Nearest-rank: q=0.5 over [1, 2] → index ceil(0.5*2)-1 = 0 →
        // value 1. The naive `round((q*(n-1)))` formula picked index 1 →
        // value 2, which disagrees with the nearest-rank definition.
        let (_g, h) = install();
        let hi = histogram!("two_sample");
        hi.record(1.0);
        hi.record(2.0);
        let body = h.render();
        assert!(body.contains(r#"two_sample{quantile="0.5"} 1"#), "{body}");
        assert!(body.contains(r#"two_sample{quantile="0"} 1"#), "{body}");
        assert!(body.contains(r#"two_sample{quantile="1"} 2"#), "{body}");
    }

    #[test]
    fn run_upkeep_evicts_quantile_window_but_keeps_lifetime_totals() {
        // Prometheus summary semantics: `_count` and `_sum` are lifetime
        // cumulative, while *quantiles* are computed from a bounded recent
        // window. After enough upkeep rotations to flush every sample bucket,
        // quantiles must read as 0 (empty window) but `_count`/`_sum` must
        // still reflect everything ever observed — otherwise dashboards using
        // `rate(name_count[5m])` would dip negative on rotations.
        let (_g, h) = install();
        let hi = histogram!("rolling");
        for _ in 0..50 {
            hi.record(100.0);
        }
        let body = h.render();
        assert!(body.contains("rolling_count 50"), "before rotation: {body}");
        assert!(body.contains("rolling_sum 5000"), "before rotation: {body}");

        // Three rotations clear every bucket the original samples could be in.
        h.run_upkeep();
        h.run_upkeep();
        h.run_upkeep();

        let body = h.render();
        // Lifetime totals are preserved.
        assert!(body.contains("rolling_count 50"), "after 3 rotations: {body}");
        assert!(body.contains("rolling_sum 5000"), "after 3 rotations: {body}");
        // But the quantile window has been drained, so quantiles render as 0.
        assert!(body.contains(r#"rolling{quantile="0.5"} 0"#), "{body}");
    }

    #[test]
    fn metric_names_with_dots_are_sanitized() {
        // Dotted callsite names like `network.connected_peers` are common.
        // Raw `.` is invalid in Prometheus exposition; the renderer must
        // sanitize to `_` and the description lookup must follow the same
        // sanitization so HELP/TYPE blocks still match the rendered name.
        let (_g, h) = install();
        describe_gauge!("network.connected_peers", "Number of connected peers");
        gauge!("network.connected_peers").set(42.0);

        let body = h.render();
        assert!(
            body.contains("# HELP network_connected_peers Number of connected peers"),
            "{body}"
        );
        assert!(body.contains("# TYPE network_connected_peers gauge"), "{body}");
        assert!(body.contains("network_connected_peers 42"), "{body}");
        assert!(!body.contains("network.connected_peers"), "raw dotted name leaked: {body}");
    }

    #[test]
    fn label_keys_with_dots_are_sanitized() {
        let (_g, h) = install();
        counter!("hits", "request.method" => "GET").increment(1);
        let body = h.render();
        assert!(body.contains(r#"hits{request_method="GET"} 1"#), "{body}");
        assert!(!body.contains("request.method"), "raw dotted label key leaked: {body}");
    }

    #[test]
    fn raw_label_keys_that_sanitize_identically_collapse_to_one_emitted_line() {
        // Two raw label keys (`request.method` and `request_method`)
        // sanitize to the same Prometheus label name. The Prometheus
        // exposition format can't represent two distinct samples with
        // identical labelsets, so the render-time grouping deduplicates by
        // sanitized labelset (last-write-wins on the rendered output).
        //
        // Distinct registry identity is still preserved: both
        // `register_counter` calls succeeded and both increments were
        // applied to their respective counters — only the final wire output
        // collapses.
        let (_g, h) = install();
        counter!("hits2", "request.method" => "GET").increment(1);
        counter!("hits2", "request_method" => "GET").increment(1);

        let body = h.render();
        let line_count = body.lines().filter(|l| l.starts_with("hits2{")).count();
        assert_eq!(
            line_count, 1,
            "expected 1 collapsed series after sanitization, got {line_count} in:\n{body}",
        );
        // The remaining sample is whichever raw key was inserted last in the
        // BTreeMap iteration order; both have value 1 so we just check the line
        // shape rather than depending on iteration order.
        assert!(body.contains(r#"hits2{request_method="GET"} 1"#), "{body}");
    }

    #[test]
    fn duplicate_sanitized_label_keys_within_one_series_are_collapsed() {
        // Two raw label keys on the *same* metric instance that sanitize to the
        // same Prometheus name (`a-b` and `a_b`) must collapse into a single
        // emitted label. Without dedupe the wire output would be
        // `{a_b="x",a_b="y"}`, which Prometheus parsers reject as a malformed
        // labelset.
        let (_g, h) = install();
        counter!("dups", "a-b" => "x", "a_b" => "y").increment(1);
        let body = h.render();

        let line = body.lines().find(|l| l.starts_with("dups{")).unwrap_or_default();
        // Exactly one occurrence of `a_b=` in the labelset.
        let occurrences = line.matches("a_b=").count();
        assert_eq!(occurrences, 1, "expected a single `a_b=` label, got {occurrences} in: {line}");
    }

    #[test]
    fn describe_is_first_write_wins() {
        // Descriptions are stored first-write-wins, so the *first*
        // describe_* call for a given metric name wins. A defensive second
        // call must be a no-op rather than clobbering the original help.
        let (_g, h) = install();
        describe_counter!("requests", "first wins");
        describe_counter!("requests", "second loses");
        counter!("requests").increment(1);

        let body = h.render();
        assert!(body.contains("# HELP requests first wins"), "{body}");
        assert!(!body.contains("second loses"), "later describe leaked: {body}");
    }

    #[test]
    fn concurrent_counter_increments_sum_to_total() {
        // Spawn N threads, each incrementing the same counter K times, and
        // assert the rendered counter value is exactly N*K. This is a real
        // correctness gate for the recorder's storage path: any lost update
        // (torn add, dropped handle clone, race in registration) shows up as
        // a value strictly less than N*K.
        use std::sync::Arc;

        const N: u64 = 16;
        const K: u64 = 1000;

        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let recorder = Arc::new(recorder);

        let threads: Vec<_> = (0..N)
            .map(|_| {
                let r = Arc::clone(&recorder);
                std::thread::spawn(move || {
                    let key = Key::from_static_name("concurrent_counter");
                    let c = r.register_counter(&key);
                    for _ in 0..K {
                        c.increment(1);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }

        let body = handle.render();
        let line = body
            .lines()
            .find(|l| l.starts_with("concurrent_counter "))
            .expect("counter line missing");
        let got: u64 = line.strip_prefix("concurrent_counter ").unwrap().parse().unwrap();
        assert_eq!(got, N * K, "expected exactly {} increments, got {got}", N * K);
    }

    #[test]
    fn recorder_can_be_used_via_register_apis_directly() {
        // Sanity check that the Recorder trait impl matches what `kival_metrics` macros call.
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();

        let key = Key::from_static_name("direct_counter");
        recorder.register_counter(&key).increment(42);

        let labelled = Key::from_parts("with_labels", vec![Label::new("k", "v")]);
        recorder.register_gauge(&labelled).set(2.5);

        let body = handle.render();
        assert!(body.contains("direct_counter 42"), "{body}");
        assert!(body.contains(r#"with_labels{k="v"} 2.5"#), "{body}");
    }
}
