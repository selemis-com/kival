//! Erased metric handles: `Counter`, `Gauge`, `Histogram`.
//!
//! A handle is a thin `Option<Arc<dyn Fn>>`. The `noop()` constructor returns
//! `None`, so unregistered metrics can still receive method calls — they just
//! become no-ops without touching the heap.
//!
//! Also provides `CounterFn` / `GaugeFn` impls for the platform-native
//! [`AtomicU64`] so an `Arc<AtomicU64>` plugs straight in as backing storage.

pub use std::sync::atomic::AtomicU64;
use std::sync::{Arc, atomic::Ordering};

/// Backing storage for a [`Counter`].
pub trait CounterFn {
    /// Add `value` to the counter.
    fn increment(&self, value: u64);

    /// Set the counter to the given absolute value (must be monotonic).
    fn absolute(&self, value: u64);
}

/// Backing storage for a [`Gauge`].
pub trait GaugeFn {
    /// Add `value` to the gauge.
    fn increment(&self, value: f64);

    /// Subtract `value` from the gauge.
    fn decrement(&self, value: f64);

    /// Replace the gauge value with `value`.
    fn set(&self, value: f64);
}

/// Backing storage for a [`Histogram`].
pub trait HistogramFn {
    /// Record a single sample.
    fn record(&self, value: f64);
}

/// Handle to a counter, cheaply cloneable and dispatched through the recorder.
#[derive(Clone, Default)]
pub struct Counter {
    /// Registered counter implementation, or `None` for a no-op handle.
    inner: Option<Arc<dyn CounterFn + Send + Sync>>,
}

impl std::fmt::Debug for Counter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Counter").field("registered", &self.inner.is_some()).finish()
    }
}

impl Counter {
    /// A handle that ignores all writes.
    pub const fn noop() -> Self {
        Self { inner: None }
    }

    /// Wrap an `Arc` of any `CounterFn` implementation.
    pub fn from_arc<F: CounterFn + Send + Sync + 'static>(arc: Arc<F>) -> Self {
        Self { inner: Some(arc) }
    }

    /// Add `value` to the counter; no-op when this handle is unregistered.
    pub fn increment(&self, value: u64) {
        if let Some(inner) = &self.inner {
            inner.increment(value);
        }
    }

    /// Set the counter to the given absolute value (must be monotonic).
    pub fn absolute(&self, value: u64) {
        if let Some(inner) = &self.inner {
            inner.absolute(value);
        }
    }
}

/// Handle to a gauge.
#[derive(Clone, Default)]
pub struct Gauge {
    /// Registered gauge implementation, or `None` for a no-op handle.
    inner: Option<Arc<dyn GaugeFn + Send + Sync>>,
}

impl std::fmt::Debug for Gauge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gauge").field("registered", &self.inner.is_some()).finish()
    }
}

impl Gauge {
    /// A handle that ignores all writes.
    pub const fn noop() -> Self {
        Self { inner: None }
    }

    /// Wrap an `Arc` of any `GaugeFn` implementation.
    pub fn from_arc<F: GaugeFn + Send + Sync + 'static>(arc: Arc<F>) -> Self {
        Self { inner: Some(arc) }
    }

    /// Add `value` to the gauge.
    pub fn increment(&self, value: f64) {
        if let Some(inner) = &self.inner {
            inner.increment(value);
        }
    }

    /// Subtract `value` from the gauge.
    pub fn decrement(&self, value: f64) {
        if let Some(inner) = &self.inner {
            inner.decrement(value);
        }
    }

    /// Replace the gauge value with `value`.
    pub fn set(&self, value: f64) {
        if let Some(inner) = &self.inner {
            inner.set(value);
        }
    }
}

/// Handle to a histogram.
#[derive(Clone, Default)]
pub struct Histogram {
    /// Registered histogram implementation, or `None` for a no-op handle.
    inner: Option<Arc<dyn HistogramFn + Send + Sync>>,
}

impl std::fmt::Debug for Histogram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Histogram").field("registered", &self.inner.is_some()).finish()
    }
}

impl Histogram {
    /// A handle that ignores all writes.
    pub const fn noop() -> Self {
        Self { inner: None }
    }

    /// Wrap an `Arc` of any `HistogramFn` implementation.
    pub fn from_arc<F: HistogramFn + Send + Sync + 'static>(arc: Arc<F>) -> Self {
        Self { inner: Some(arc) }
    }

    /// Record a single sample.
    pub fn record(&self, value: f64) {
        if let Some(inner) = &self.inner {
            inner.record(value);
        }
    }
}

impl CounterFn for AtomicU64 {
    fn increment(&self, value: u64) {
        // Counters are monotonic; relaxed ordering is fine because metric
        // values are tolerant to reordering across producers.
        self.fetch_add(value, Ordering::Relaxed);
    }

    fn absolute(&self, value: u64) {
        // Counters never decrease — only update if the new value is larger.
        let mut current = self.load(Ordering::Relaxed);
        while value > current {
            match self.compare_exchange_weak(current, value, Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }
}

impl GaugeFn for AtomicU64 {
    fn increment(&self, value: f64) {
        update_f64(self, |v| v + value);
    }

    fn decrement(&self, value: f64) {
        update_f64(self, |v| v - value);
    }

    fn set(&self, value: f64) {
        self.store(value.to_bits(), Ordering::Release);
    }
}

#[inline]
/// Apply a compare-and-swap update to an `f64` stored as raw bits.
fn update_f64<F: Fn(f64) -> f64>(cell: &AtomicU64, f: F) {
    // CAS loop reinterpreting bits as f64. Tolerant to rare benign races
    // where two threads increment concurrently — the loser retries.
    let mut bits = cell.load(Ordering::Relaxed);
    loop {
        let next = f(f64::from_bits(bits)).to_bits();
        match cell.compare_exchange_weak(bits, next, Ordering::AcqRel, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => bits = actual,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Pin the bit-cast semantics of `AtomicU64`-backed gauges and the
    //! monotonicity contract of counter `absolute()`. A future refactor
    //! that swaps to integer storage or drops the CAS loop would fail
    //! these tests immediately rather than silently mangling values.
    use super::*;

    #[test]
    fn counter_increment_accumulates() {
        let c = AtomicU64::new(0);
        for _ in 0..10 {
            <AtomicU64 as CounterFn>::increment(&c, 7);
        }
        assert_eq!(c.load(Ordering::Relaxed), 70);
    }

    #[test]
    fn counter_absolute_is_monotonic_only() {
        let c = AtomicU64::new(0);
        <AtomicU64 as CounterFn>::absolute(&c, 100);
        assert_eq!(c.load(Ordering::Relaxed), 100);
        // Lower value must not move the counter backwards.
        <AtomicU64 as CounterFn>::absolute(&c, 50);
        assert_eq!(c.load(Ordering::Relaxed), 100);
        // Higher value updates.
        <AtomicU64 as CounterFn>::absolute(&c, 250);
        assert_eq!(c.load(Ordering::Relaxed), 250);
    }

    #[test]
    fn noop_handles_are_debuggable_and_ignore_writes() {
        let counter = Counter::noop();
        let gauge = Gauge::noop();
        let histogram = Histogram::noop();

        assert_eq!(format!("{counter:?}"), "Counter { registered: false }");
        assert_eq!(format!("{gauge:?}"), "Gauge { registered: false }");
        assert_eq!(format!("{histogram:?}"), "Histogram { registered: false }");

        counter.increment(1);
        counter.absolute(10);
        gauge.increment(1.0);
        gauge.decrement(0.5);
        gauge.set(2.0);
        histogram.record(3.0);
    }

    fn read_gauge(g: &AtomicU64) -> f64 {
        f64::from_bits(g.load(Ordering::Acquire))
    }

    #[test]
    fn gauge_set_round_trips_through_bit_cast() {
        let g = AtomicU64::new(0);
        for v in [0.0, 1.0, -1.0, std::f64::consts::PI, 1e308, f64::MIN_POSITIVE] {
            <AtomicU64 as GaugeFn>::set(&g, v);
            assert_eq!(read_gauge(&g), v, "round-trip failed for {v}");
        }
    }

    #[test]
    fn gauge_increment_decrement_compose() {
        let g = AtomicU64::new(0);
        <AtomicU64 as GaugeFn>::set(&g, 10.0);
        <AtomicU64 as GaugeFn>::increment(&g, 2.5);
        assert_eq!(read_gauge(&g), 12.5);
        <AtomicU64 as GaugeFn>::decrement(&g, 0.5);
        assert_eq!(read_gauge(&g), 12.0);
    }

    /// `f64::NAN` survives a round-trip through the bit cell. NaN does not
    /// equal itself, so we compare via `to_bits()` instead. Pin so a
    /// future "guard NaN by replacing with 0.0" change is intentional.
    #[test]
    fn gauge_set_preserves_nan_bit_pattern() {
        let g = AtomicU64::new(0);
        let nan_bits = f64::NAN.to_bits();
        <AtomicU64 as GaugeFn>::set(&g, f64::NAN);
        assert_eq!(g.load(Ordering::Acquire), nan_bits);
        assert!(read_gauge(&g).is_nan());
    }

    #[test]
    fn concurrent_gauge_increments_sum_to_total() {
        const N: usize = 8;
        const K: usize = 10_000;
        let g = Arc::new(AtomicU64::new(0));
        let threads: Vec<_> = (0..N)
            .map(|_| {
                let g = Arc::clone(&g);
                std::thread::spawn(move || {
                    for _ in 0..K {
                        <AtomicU64 as GaugeFn>::increment(&g, 1.0);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(read_gauge(&g), (N * K) as f64);
    }
}
