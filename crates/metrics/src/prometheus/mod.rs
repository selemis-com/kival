//! Prometheus exposition recorder for Metrics.
//!
//! This module is intentionally **transport-free**. It produces a Prometheus
//! text body via [`PrometheusHandle::render`] and lets the caller ship the
//! result however they want (their own HTTP server, a push-gateway client, a
//! file dump).
//!
//! Construct a recorder with [`PrometheusBuilder::build_recorder`], install
//! it (typically wrapped in a [`crate::Stack`] / [`crate::PrefixLayer`]),
//! then call [`PrometheusHandle::render`] for the exposition body.

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    hash::{Hash, Hasher},
    sync::{Arc, RwLock, atomic::Ordering},
};

use render::{
    MetricDescription, MetricFamily, key_parts, render_counters, render_gauges, render_histograms,
    sanitize_metric_name,
};
use storage::HistogramStorage;

use crate::{AtomicU64, Counter, Gauge, Histogram, Key, KeyName, Recorder};

mod render;
mod storage;

/// Builder for [`PrometheusRecorder`].
///
/// Default-constructed via `PrometheusBuilder::new().build_recorder()`;
/// knobs can be added here as they are needed.
#[derive(Copy, Clone, Debug, Default)]
pub struct PrometheusBuilder {
    /// Prevent external construction while keeping future options extensible.
    _private: (),
}

impl PrometheusBuilder {
    /// Construct a new builder with default settings.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    /// Build the recorder and return it. Caller is responsible for installing
    /// it as the global [`Recorder`] (typically wrapped in a
    /// [`crate::Stack`] / [`crate::PrefixLayer`]).
    pub fn build_recorder(self) -> PrometheusRecorder {
        PrometheusRecorder { inner: Arc::new(Inner::default()) }
    }
}

/// `Recorder` that accumulates metric values into in-memory storage and lets
/// the caller render the current state via a [`PrometheusHandle`].
#[derive(Clone)]
pub struct PrometheusRecorder {
    /// Shared recorder state.
    inner: Arc<Inner>,
}

impl std::fmt::Debug for PrometheusRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrometheusRecorder").finish()
    }
}

impl PrometheusRecorder {
    /// Cheap, ref-counted handle for rendering the metric state.
    pub fn handle(&self) -> PrometheusHandle {
        PrometheusHandle { inner: self.inner.clone() }
    }
}

/// Cheap clone-able handle for producing a Prometheus exposition body.
#[derive(Clone)]
pub struct PrometheusHandle {
    /// Shared recorder state.
    inner: Arc<Inner>,
}

impl std::fmt::Debug for PrometheusHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrometheusHandle").finish()
    }
}

impl PrometheusHandle {
    /// Render the full Prometheus text body for the current metric state.
    ///
    /// Output follows the standard Prometheus exposition format with
    /// `# HELP` and `# TYPE` headers per metric name. Histograms are emitted
    /// as `summary`-typed blocks with quantile labels.
    ///
    /// # Panics
    ///
    /// Panics if an internal registry lock is poisoned.
    pub fn render(&self) -> String {
        let descs = self.inner.descriptions.read().expect("descriptions lock poisoned");

        // BTreeMap<String, _> grouping gives one HELP/TYPE block per name and
        // deterministic name ordering across scrapes (diff-friendly output).
        let counters = group_family(&self.inner.counters, &descs, |a: &Arc<AtomicU64>| {
            a.load(Ordering::Relaxed)
        });
        let gauges = group_family(&self.inner.gauges, &descs, |a: &Arc<AtomicU64>| {
            f64::from_bits(a.load(Ordering::Relaxed))
        });
        let mut histograms =
            group_family(&self.inner.histograms, &descs, |h: &Arc<HistogramStorage>| h.snapshot());

        drop(descs);

        let mut out = String::new();
        for (name, family) in &counters {
            render_counters(&mut out, name, family);
        }
        for (name, family) in &gauges {
            render_gauges(&mut out, name, family);
        }
        for (name, family) in &mut histograms {
            render_histograms(&mut out, name, family);
        }

        out
    }

    /// Periodic maintenance hook: rotates each histogram's rolling buckets
    /// so quantiles reflect a bounded recent window rather than the entire
    /// process lifetime.
    ///
    /// Intended to be called from a periodic timer (e.g. every 5 seconds
    /// for a ~15s effective window with the 3-bucket rotation). Counters
    /// and gauges are unaffected.
    ///
    /// # Panics
    ///
    /// Panics if the histogram registry lock is poisoned.
    pub fn run_upkeep(&self) {
        let map = self.inner.histograms.read().expect("histograms lock poisoned");
        for storage in map.values() {
            storage.run_upkeep();
        }
    }
}

fn group_family<S, V, F>(
    source: &RwLock<HashMap<HashKey, S>>,
    descs: &HashMap<String, MetricDescription>,
    extract: F,
) -> BTreeMap<String, MetricFamily<V>>
where
    F: Fn(&S) -> V,
{
    let mut out: BTreeMap<String, MetricFamily<V>> = BTreeMap::new();
    let guard = source.read().expect("metric lock poisoned");

    for (key, val) in guard.iter() {
        let (name, labels) = key_parts(&key.0);
        let entry = out.entry(name.clone()).or_default();

        if entry.description.help.is_empty()
            && let Some(description) = descs.get(&name)
        {
            entry.description.help = description.help.clone();
        }

        entry.instances.insert(labels, extract(val));
    }

    drop(guard);
    out
}

/// Newtype around `Key` that hashes via the precomputed FNV value, giving
/// `O(1)` `HashMap` lookups during the hot registration path.
#[derive(Debug, Clone)]
struct HashKey(Key);

impl PartialEq for HashKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for HashKey {}
impl Hash for HashKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.0.get_hash());
    }
}

/// All shared state owned by the recorder. The handle holds an `Arc` to the
/// same `Inner`, so renders and writes refer to the same registry.
#[derive(Default)]
struct Inner {
    /// Registered counters by raw key.
    counters: RwLock<HashMap<HashKey, Arc<AtomicU64>>>,
    /// Registered gauges by raw key.
    gauges: RwLock<HashMap<HashKey, Arc<AtomicU64>>>,
    /// Registered histograms by raw key.
    histograms: RwLock<HashMap<HashKey, Arc<HistogramStorage>>>,
    /// Metric descriptions by sanitized metric name.
    descriptions: RwLock<HashMap<String, MetricDescription>>,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner").finish()
    }
}

impl Recorder for PrometheusRecorder {
    fn describe_counter(&self, key: KeyName, description: Cow<'static, str>) {
        store_description(&self.inner, &key, description);
    }
    fn describe_gauge(&self, key: KeyName, description: Cow<'static, str>) {
        store_description(&self.inner, &key, description);
    }
    fn describe_histogram(&self, key: KeyName, description: Cow<'static, str>) {
        store_description(&self.inner, &key, description);
    }

    fn register_counter(&self, key: &Key) -> Counter {
        let counter = {
            let mut map = self.inner.counters.write().expect("counters lock poisoned");
            map.entry(HashKey(key.clone())).or_insert_with(|| Arc::new(AtomicU64::new(0))).clone()
        };
        Counter::from_arc(counter)
    }

    fn register_gauge(&self, key: &Key) -> Gauge {
        let gauge = {
            let mut map = self.inner.gauges.write().expect("gauges lock poisoned");
            map.entry(HashKey(key.clone())).or_insert_with(|| Arc::new(AtomicU64::new(0))).clone()
        };
        Gauge::from_arc(gauge)
    }

    fn register_histogram(&self, key: &Key) -> Histogram {
        let histogram = {
            let mut map = self.inner.histograms.write().expect("histograms lock poisoned");
            map.entry(HashKey(key.clone()))
                .or_insert_with(|| Arc::new(HistogramStorage::new()))
                .clone()
        };
        Histogram::from_arc(histogram)
    }
}

/// Store first-write-wins help text for a metric name.
fn store_description(inner: &Inner, key: &KeyName, description: Cow<'static, str>) {
    // Store under the sanitized name so render-time lookups (which key off
    // the rendered metric name) find the description for dotted callsites
    // like `network.connected_peers` → `network_connected_peers`.
    //
    // First-write-wins: a second `describe_*` call for the same metric is a
    // no-op, so callers can register descriptions defensively without
    // clobbering an earlier, possibly more authoritative one.
    let mut map = inner.descriptions.write().expect("descriptions lock poisoned");
    map.entry(sanitize_metric_name(key.as_str()))
        .or_insert_with(|| MetricDescription { help: description.into_owned() });
}
