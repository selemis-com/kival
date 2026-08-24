//! Backing storage for histograms.
//!
//! Rolling-summary behavior with a fixed 3-bucket rotation:
//!
//! * `record` writes into the current bucket.
//! * `run_upkeep` advances `current` to `(current + 1) % 3` and clears the new current bucket.
//!   Driven by the caller on whatever cadence makes sense (e.g. a 5s timer gives a ~15s sliding
//!   window).
//! * `snapshot` merges count/sum/samples across all three buckets.
//!
//! Each bucket caps its sample buffer at `BUCKET_CAP` and uses Vitter's
//! reservoir algorithm R for replacements once full, so individual bursts
//! can't blow memory.

use std::{
    cell::Cell,
    cmp::Ordering,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::HistogramFn;

/// Per-bucket retention cap; total cap across the 3 rolling buckets is `3 * BUCKET_CAP`.
pub(crate) const BUCKET_CAP: usize = 1365; // ≈ 4096 / 3 — keeps the historical SAMPLE_CAP target

/// Number of rolling buckets in the histogram window.
const BUCKETS: usize = 3;

#[derive(Debug, Default)]
/// A single reservoir bucket in the rolling histogram window.
struct Bucket {
    /// Number of samples observed in this bucket (may exceed `samples.len()`
    /// once the reservoir starts replacing entries). Used as the running `n`
    /// for Vitter's algorithm R within this bucket only.
    count: u64,
    /// Bounded reservoir of retained samples for quantile estimation.
    samples: Vec<f64>,
}

impl Bucket {
    /// Drop retained samples and reset the bucket-local count.
    fn clear(&mut self) {
        self.count = 0;
        self.samples.clear();
    }
}

/// Histogram backing storage shared between writers (`record`) and the
/// renderer (`snapshot`).
#[derive(Debug)]
pub(crate) struct HistogramStorage {
    /// All state is under one mutex — simpler and avoids subtle races between
    /// the count/sum/samples updates that need to stay consistent for a given
    /// `record` call.
    state: Mutex<State>,
}

#[derive(Debug, Default)]
/// Mutable histogram state protected by [`HistogramStorage::state`].
struct State {
    /// Lifetime totals — Prometheus summary `_count` and `_sum` are
    /// cumulative even when quantiles are windowed, so dashboards using
    /// `rate()` over those series stay monotonic across upkeep rotations.
    total_count: u64,
    /// Lifetime sum rendered as the Prometheus summary `_sum` series.
    total_sum: f64,
    /// Rolling reservoir buckets — only the *quantile* sample is windowed.
    buckets: [Bucket; BUCKETS],
    /// Index of the bucket currently receiving writes.
    current: usize,
}

impl HistogramStorage {
    /// Create empty histogram storage.
    pub(crate) fn new() -> Self {
        Self { state: Mutex::new(State::default()) }
    }

    /// Snapshot lifetime count/sum and the merged sample across rolling
    /// buckets. Returned samples are not sorted.
    pub(crate) fn snapshot(&self) -> HistogramSnapshot {
        let state = self.state.lock().expect("histogram lock poisoned");
        let mut samples: Vec<f64> = Vec::with_capacity(BUCKET_CAP * BUCKETS);
        for b in &state.buckets {
            samples.extend_from_slice(&b.samples);
        }
        HistogramSnapshot { count: state.total_count, sum: state.total_sum, samples }
    }

    /// Rotate the buckets: advance `current` and clear the bucket that now
    /// holds new writes. The two buckets behind it remain readable until the
    /// next two rotations. Lifetime `_count`/`_sum` are not affected.
    pub(crate) fn run_upkeep(&self) {
        let mut state = self.state.lock().expect("histogram lock poisoned");
        let next = (state.current + 1) % BUCKETS;
        state.buckets[next].clear();
        state.current = next;
    }
}

impl HistogramFn for HistogramStorage {
    fn record(&self, value: f64) {
        let mut state = self.state.lock().expect("histogram lock poisoned");

        state.total_count = state.total_count.saturating_add(1);
        state.total_sum += value;

        let cur = state.current;
        let bucket = &mut state.buckets[cur];
        bucket.count = bucket.count.saturating_add(1);

        if bucket.samples.len() < BUCKET_CAP {
            bucket.samples.push(value);
        } else {
            let j = thread_rng_below(bucket.count);
            if (j as usize) < BUCKET_CAP {
                bucket.samples[j as usize] = value;
            }
        }

        drop(state);
    }
}

/// Borrowed snapshot used by the renderer.
#[derive(Debug)]
pub(crate) struct HistogramSnapshot {
    /// Lifetime sample count.
    pub(crate) count: u64,
    /// Lifetime sample sum.
    pub(crate) sum: f64,
    /// Windowed retained samples used for quantile estimation.
    pub(crate) samples: Vec<f64>,
}

impl HistogramSnapshot {
    /// Compute the value at quantile `q` (0.0 to 1.0) from the snapshot.
    ///
    /// Uses the **nearest-rank** definition (`R-1` from Hyndman & Fan): for a
    /// sorted sample of length `n`, the q-th quantile is at index
    /// `max(ceil(q * n), 1) - 1`.
    ///
    /// Returns `0.0` for an empty snapshot. Returning `NaN` would render as
    /// `NaN` text and trip strict Prometheus parsers, so the renderer never
    /// has to special-case an `Option` value.
    pub(crate) fn quantile(&mut self, q: f64) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        // Sort lazily on first quantile call; subsequent calls reuse the order.
        if !is_sorted(&self.samples) {
            self.samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
        }
        let n = self.samples.len();
        // Clamp q into [0, 1] defensively. `q == 0.0` picks the minimum;
        // `q == 1.0` picks the maximum.
        let q = q.clamp(0.0, 1.0);
        let rank = (q * n as f64).ceil() as usize;
        let idx = rank.saturating_sub(1).min(n - 1);
        self.samples[idx]
    }
}

/// Return whether samples are already sorted in ascending partial order.
fn is_sorted(samples: &[f64]) -> bool {
    samples.windows(2).all(|w| w[0].partial_cmp(&w[1]).is_none_or(Ordering::is_le))
}

/// Cheap thread-local PRNG returning a `u64` strictly less than `bound`.
///
/// `xorshift64*` seeded once per thread. Quality is adequate for reservoir
/// sampling; we don't need cryptographic randomness here.
fn thread_rng_below(bound: u64) -> u64 {
    thread_local! {
        static STATE: Cell<u64> = Cell::new(seed());
    }
    STATE.with(|s| {
        let mut x = s.get();
        if x == 0 {
            x = 0x9E37_79B9_7F4A_7C15;
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        // Slight modulo bias is acceptable for sampling decisions.
        x.wrapping_mul(0x2545_F491_4F6C_DD1D) % bound.max(1)
    })
}

/// Seed the per-thread xorshift state.
fn seed() -> u64 {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    // Hash the thread id by its address to get a per-thread distinct stream
    // without depending on the (unstable) `ThreadId::as_u64`.
    let tid_addr = std::thread::current().id();
    let tid_mix = (&raw const tid_addr) as usize as u64;
    (nanos as u64) ^ tid_mix.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

#[cfg(test)]
mod tests {
    //! Direct unit tests for the rolling-summary storage primitive.
    //!
    //! Covers construction, single-sample and multi-sample snapshots,
    //! upkeep-driven rotation, reservoir capping, and quantile correctness
    //! (including out-of-range and idempotent-sort behavior).
    use super::*;

    /// A freshly constructed storage reports zero count, zero sum, and an
    /// empty sample buffer.
    #[test]
    fn new_storage_is_empty() {
        let s = HistogramStorage::new();
        let snap = s.snapshot();
        assert_eq!(snap.count, 0);
        assert_eq!(snap.sum, 0.0);
        assert!(snap.samples.is_empty());
    }

    /// `quantile` on an empty snapshot returns `0.0` rather than `NaN`.
    /// `NaN` would render as the literal string `NaN` and trip strict
    /// Prometheus parsers; pin the safe value here so it can never silently
    /// regress.
    #[test]
    fn empty_snapshot_quantile_returns_zero_not_nan() {
        let s = HistogramStorage::new();
        let mut snap = s.snapshot();
        for q in [0.0, 0.5, 0.9, 0.95, 0.99, 1.0] {
            let v = snap.quantile(q);
            assert_eq!(v, 0.0, "q={q} returned {v}, expected 0.0");
            assert!(!v.is_nan(), "q={q} returned NaN");
        }
    }

    /// Recording a single value is reflected in count/sum/samples.
    #[test]
    fn record_single_value() {
        let s = HistogramStorage::new();
        s.record(42.0);
        let snap = s.snapshot();
        assert_eq!(snap.count, 1);
        assert_eq!(snap.sum, 42.0);
        assert_eq!(snap.samples, vec![42.0]);
    }

    /// Record several values, snapshot, and assert lifetime totals plus
    /// the standard q=0.5 / q=0 / q=1 reads.
    #[test]
    fn snapshot_after_several_records() {
        let s = HistogramStorage::new();
        for v in [10.0, 20.0, 30.0, 40.0, 50.0] {
            s.record(v);
        }
        let mut snap = s.snapshot();
        assert_eq!(snap.count, 5);
        assert_eq!(snap.sum, 150.0);
        // Nearest-rank: q=0.5 over 5 sorted samples → ceil(0.5*5)=3 → idx 2 → 30.
        assert_eq!(snap.quantile(0.5), 30.0);
        // q=0 returns the minimum, q=1 returns the maximum.
        assert_eq!(snap.quantile(0.0), 10.0);
        assert_eq!(snap.quantile(1.0), 50.0);
    }

    /// After `BUCKETS` upkeeps the bucket holding the original samples is
    /// cleared, so the *quantile window* drains; but lifetime `count`/`sum`
    /// survive (Prometheus summary contract).
    #[test]
    fn upkeep_drains_quantile_window_but_not_lifetime_totals() {
        let s = HistogramStorage::new();
        for _ in 0..7 {
            s.record(100.0);
        }
        // BUCKETS rotations clear every bucket the original samples could
        // be in, including the bucket they were originally written to.
        for _ in 0..BUCKETS {
            s.run_upkeep();
        }
        let mut snap = s.snapshot();
        assert_eq!(snap.count, 7, "lifetime count must survive rotation");
        assert_eq!(snap.sum, 700.0, "lifetime sum must survive rotation");
        assert!(snap.samples.is_empty(), "quantile window must drain after BUCKETS upkeeps");
        assert_eq!(snap.quantile(0.5), 0.0, "drained window quantile must be 0, not NaN");
    }

    /// Reservoir-sampling cap: recording past `BUCKET_CAP` into a single
    /// active bucket must not grow `samples` unboundedly. Lifetime count
    /// keeps growing past it.
    #[test]
    fn reservoir_caps_per_bucket_samples_at_bucket_cap() {
        let s = HistogramStorage::new();
        let n = (BUCKET_CAP as u64) * 2;
        for i in 0..n {
            s.record(i as f64);
        }
        let snap = s.snapshot();
        assert_eq!(snap.count, n, "lifetime count grows past BUCKET_CAP");
        assert_eq!(
            snap.samples.len(),
            BUCKET_CAP,
            "single-bucket samples must cap at BUCKET_CAP, got {}",
            snap.samples.len()
        );
    }

    /// `quantile` is robust against out-of-range inputs: clamp to [0, 1]
    /// rather than panic on indexing math. Mirrors a real correctness
    /// requirement that's easy to break in a refactor.
    #[test]
    fn quantile_clamps_out_of_range_inputs() {
        let s = HistogramStorage::new();
        for v in [1.0, 2.0, 3.0] {
            s.record(v);
        }
        let mut snap = s.snapshot();
        // Negative q clamps to 0.0 → minimum.
        assert_eq!(snap.quantile(-1.0), 1.0);
        // q > 1.0 clamps to 1.0 → maximum.
        assert_eq!(snap.quantile(2.0), 3.0);
    }

    /// Sort is performed on the first `quantile` call; the second call
    /// must reuse the existing sort order (verified by reading the
    /// `samples` vec after one quantile call and confirming it's sorted).
    #[test]
    fn quantile_sorts_samples_in_place_idempotently() {
        let s = HistogramStorage::new();
        // Insert in clearly out-of-order sequence so an unsorted snapshot
        // can be detected.
        for v in [5.0, 1.0, 4.0, 2.0, 3.0] {
            s.record(v);
        }
        let mut snap = s.snapshot();
        let q1 = snap.quantile(0.5);
        // After one quantile call the samples must be sorted ascending.
        assert!(
            snap.samples.windows(2).all(|w| w[0] <= w[1]),
            "samples not sorted after first quantile call: {:?}",
            snap.samples
        );
        // A second quantile call returns the same value and preserves the order.
        let q2 = snap.quantile(0.5);
        assert_eq!(q1, q2);
        assert!(snap.samples.windows(2).all(|w| w[0] <= w[1]));
    }
}
