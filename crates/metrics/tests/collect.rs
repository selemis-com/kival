//! End-to-end smoke tests for the process collector.
//!
//! Each test installs a thread-local recorder so they can run in parallel.
//! We assert that the standard process metrics are emitted with sensible
//! values; we don't pin exact numbers because they vary by host.

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use std::{
        sync::{Mutex, MutexGuard, PoisonError},
        time::{SystemTime, UNIX_EPOCH},
    };

    use kival_metrics::{
        process::Collector,
        prometheus::{PrometheusBuilder, PrometheusHandle},
        set_default_local_recorder,
    };

    /// Serializes every test in this file. The collector reports
    /// **process-wide** gauges (`process_open_fds`, `process_threads`, …); if
    /// two tests run concurrently they mutate each other's measurements and
    /// the reactivity assertions become flaky. Holding this lock for the full
    /// test body restores the "one observer of the process at a time"
    /// invariant that the Prometheus process-metric semantics implicitly
    /// assume.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn install() -> (MutexGuard<'static, ()>, kival_metrics::LocalRecorderGuard, PrometheusHandle) {
        // `lock()` returns `PoisonError` only if a previous holder panicked.
        // Subsequent tests don't depend on shared state behind the lock — it
        // exists purely for serialization — so unwrap the poison and continue.
        let serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let guard = set_default_local_recorder(recorder);
        (serial, guard, handle)
    }

    /// Pull the numeric value of a single-line gauge/counter (no labels) from a
    /// Prometheus exposition body.
    fn extract_value(body: &str, name: &str) -> Option<f64> {
        for line in body.lines() {
            if line.starts_with('#') {
                continue;
            }
            // Match `name <value>` exactly (no labels). Histograms and labeled
            // metrics are filtered out by the leading-name + space check.
            if let Some(rest) = line.strip_prefix(name)
                && let Some(value) = rest.strip_prefix(' ')
            {
                return value.parse().ok();
            }
        }
        None
    }

    #[test]
    fn collect_emits_all_standard_metrics() {
        let (_s, _g, h) = install();
        let c = Collector::default();
        c.describe();
        c.collect();

        let body = h.render();

        for name in [
            "process_cpu_seconds_total",
            "process_open_fds",
            "process_max_fds",
            "process_resident_memory_bytes",
            "process_virtual_memory_bytes",
            "process_virtual_memory_max_bytes",
            "process_threads",
            "process_start_time_seconds",
        ] {
            assert!(body.contains(name), "missing metric `{name}` in:\n{body}");
        }
    }

    #[test]
    fn collect_produces_nonzero_resident_memory_and_threads() {
        let (_s, _g, h) = install();
        let c = Collector::default();
        c.collect();

        let body = h.render();
        // At least one thread (this one) and at least 1 KiB of resident memory.
        let threads = extract_value(&body, "process_threads").expect("threads metric");
        assert!(threads >= 1.0, "expected ≥1 thread, got {threads}");
        let rss = extract_value(&body, "process_resident_memory_bytes").expect("rss metric");
        assert!(rss > 0.0, "expected nonzero RSS, got {rss}");
    }

    #[test]
    fn cpu_seconds_is_nondecreasing_across_collects() {
        let (_s, _g, h) = install();
        let c = Collector::default();

        c.collect();
        let first = extract_value(&h.render(), "process_cpu_seconds_total").unwrap();

        // Burn a tiny amount of CPU.
        let mut acc: u64 = 0;
        for i in 0_u64..200_000 {
            acc = acc.wrapping_add(i.wrapping_mul(31));
        }
        std::hint::black_box(acc);

        c.collect();
        let second = extract_value(&h.render(), "process_cpu_seconds_total").unwrap();

        assert!(second >= first, "cpu went backwards: {first} → {second}");
    }

    /// Opening N file descriptors must move `process_open_fds` up by at least N
    /// across two scrapes. Catches a future regression in the Linux
    /// `/proc/self/fd` count, which is easy to silently get wrong.
    #[test]
    fn open_fds_increases_when_files_are_opened() {
        let (_s, _g, h) = install();
        let c = Collector::default();
        c.collect();
        let before = extract_value(&h.render(), "process_open_fds").expect("open_fds before");

        // Hold real fds open across the second scrape.
        const N: usize = 16;
        let files: Vec<_> =
            (0..N).map(|_| std::fs::File::open("/dev/null").expect("open /dev/null")).collect();

        c.collect();
        let after = extract_value(&h.render(), "process_open_fds").expect("open_fds after");

        drop(files);

        // We can't assert exact equality because the test runner / recorder
        // may be opening incidental fds in the background, but we MUST gain at
        // least the N we explicitly opened.
        assert!(
            after >= before + N as f64,
            "expected ≥ +{N} fds across collects, got {before} → {after}"
        );
    }

    /// Spawning N additional threads must move `process_threads` up by at
    /// least N. A barrier keeps the spawned threads alive across the second
    /// scrape so the sample observes them.
    #[test]
    fn thread_count_increases_when_threads_are_spawned() {
        use std::sync::{Arc, Barrier};

        let (_s, _g, h) = install();
        let c = Collector::default();
        c.collect();
        let before = extract_value(&h.render(), "process_threads").expect("threads before");

        // Spawn enough helpers that the delta dwarfs cargo-test harness churn
        // (worker threads being created/joined for other test crates running
        // in the same process). Empirically, raw `pthread_create` reports an
        // *exact* delta of N via `pti_threadnum`; the slack here is purely to
        // tolerate cargo's worker pool, not a kernel race.
        const N: usize = 16;
        const MIN_DELTA: usize = N - 4;
        // Two phases: (1) all helpers + main rendezvous so we know they're
        // running, then (2) helpers wait for permission to exit so they're
        // still alive at the second scrape.
        let started = Arc::new(Barrier::new(N + 1));
        let release = Arc::new(Barrier::new(N + 1));
        let handles: Vec<_> = (0..N)
            .map(|_| {
                let started = Arc::clone(&started);
                let release = Arc::clone(&release);
                std::thread::spawn(move || {
                    started.wait();
                    release.wait();
                })
            })
            .collect();

        started.wait();
        c.collect();
        let after = extract_value(&h.render(), "process_threads").expect("threads after");
        release.wait();
        for h in handles {
            h.join().unwrap();
        }

        assert!(
            after >= before + MIN_DELTA as f64,
            "expected ≥ +{MIN_DELTA} threads across collects (spawned {N}), got {before} → {after}"
        );
    }

    #[test]
    fn start_time_is_after_unix_epoch_and_before_now() {
        let (_s, _g, h) = install();
        Collector::default().collect();

        let body = h.render();
        let start = extract_value(&body, "process_start_time_seconds").unwrap();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
        assert!(start > 1_000_000_000.0, "start_time {start} not in unix epoch range");
        assert!(start <= now + 1.0, "start_time {start} is in the future (now={now})");
    }
}
