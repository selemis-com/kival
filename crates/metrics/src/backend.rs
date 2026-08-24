//! The recording pipeline: the [`Recorder`] trait, the global / thread-local
//! recorder slots, composable [`Layer`]s ([`PrefixLayer`], [`Stack`]), and the
//! entry-point macros (`counter!`, `gauge!`, `histogram!`, `describe_*!`)
//! that consult the current recorder for every callsite.

use std::{
    borrow::Cow,
    cell::RefCell,
    sync::{Arc, OnceLock},
};

use crate::{
    handles::{Counter, Gauge, Histogram},
    key::{Key, KeyName},
};

/// Backend that owns metric state.
pub trait Recorder: Send + Sync {
    /// Record a description for a counter. Idempotent.
    fn describe_counter(&self, key: KeyName, description: Cow<'static, str>);
    /// Record a description for a gauge.
    fn describe_gauge(&self, key: KeyName, description: Cow<'static, str>);
    /// Record a description for a histogram.
    fn describe_histogram(&self, key: KeyName, description: Cow<'static, str>);

    /// Look up or create a counter handle.
    fn register_counter(&self, key: &Key) -> Counter;
    /// Look up or create a gauge handle.
    fn register_gauge(&self, key: &Key) -> Gauge;
    /// Look up or create a histogram handle.
    fn register_histogram(&self, key: &Key) -> Histogram;
}

// Blanket impls for common owning forms — lets `Stack`/`PrefixLayer` wrap any
// of these and lets users pass `Arc<MyRecorder>` directly to `set_global_recorder`.
impl<R: Recorder + ?Sized> Recorder for Arc<R> {
    fn describe_counter(&self, k: KeyName, d: Cow<'static, str>) {
        (**self).describe_counter(k, d)
    }
    fn describe_gauge(&self, k: KeyName, d: Cow<'static, str>) {
        (**self).describe_gauge(k, d)
    }
    fn describe_histogram(&self, k: KeyName, d: Cow<'static, str>) {
        (**self).describe_histogram(k, d)
    }
    fn register_counter(&self, k: &Key) -> Counter {
        (**self).register_counter(k)
    }
    fn register_gauge(&self, k: &Key) -> Gauge {
        (**self).register_gauge(k)
    }
    fn register_histogram(&self, k: &Key) -> Histogram {
        (**self).register_histogram(k)
    }
}

impl<R: Recorder + ?Sized> Recorder for Box<R> {
    fn describe_counter(&self, k: KeyName, d: Cow<'static, str>) {
        (**self).describe_counter(k, d)
    }
    fn describe_gauge(&self, k: KeyName, d: Cow<'static, str>) {
        (**self).describe_gauge(k, d)
    }
    fn describe_histogram(&self, k: KeyName, d: Cow<'static, str>) {
        (**self).describe_histogram(k, d)
    }
    fn register_counter(&self, k: &Key) -> Counter {
        (**self).register_counter(k)
    }
    fn register_gauge(&self, k: &Key) -> Gauge {
        (**self).register_gauge(k)
    }
    fn register_histogram(&self, k: &Key) -> Histogram {
        (**self).register_histogram(k)
    }
}

/// A `Recorder` that ignores everything. The default global recorder.
#[derive(Copy, Clone, Debug)]
pub struct NoopRecorder;

impl Recorder for NoopRecorder {
    fn describe_counter(&self, _: KeyName, _: Cow<'static, str>) {}
    fn describe_gauge(&self, _: KeyName, _: Cow<'static, str>) {}
    fn describe_histogram(&self, _: KeyName, _: Cow<'static, str>) {}
    fn register_counter(&self, _: &Key) -> Counter {
        Counter::noop()
    }
    fn register_gauge(&self, _: &Key) -> Gauge {
        Gauge::noop()
    }
    fn register_histogram(&self, _: &Key) -> Histogram {
        Histogram::noop()
    }
}

// Global recorder slot.
//
// Install-once semantics: the first `set_global_recorder` call wins; later
// callers get back the rejected recorder via `SetRecorderError`. This avoids
// the failure mode where handles obtained before a swap continue writing to
// the old registry while later callsites register against the new one.
/// Process-wide recorder slot.
static GLOBAL: OnceLock<Arc<dyn Recorder>> = OnceLock::new();

/// Returned when `set_global_recorder` is called after a recorder has already
/// been installed. The rejected recorder is returned so callers can recover
/// it (e.g. install it as a thread-local override instead).
#[derive(Debug)]
pub struct SetRecorderError<R>(pub R);

impl<R> std::fmt::Display for SetRecorderError<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("global metrics recorder already installed")
    }
}

impl<R: std::fmt::Debug> std::error::Error for SetRecorderError<R> {}

/// Install `recorder` as the process-wide recorder.
///
/// Returns `Err(SetRecorderError(recorder))` if a recorder has already been
/// installed.
///
/// # Errors
///
/// Returns [`SetRecorderError`] when a global recorder was already installed.
///
/// # Panics
///
/// Panics only if [`OnceLock::get_or_init`] invokes its initializer more than
/// once for a single call, which would violate `OnceLock`'s contract.
pub fn set_global_recorder<R: Recorder + 'static>(recorder: R) -> Result<(), SetRecorderError<R>> {
    // `OnceLock::set` on a generic value would require `R: Clone` on failure
    // because we can't recover the moved value. Instead, run the install
    // through `get_or_init` and observe whether our closure ran via the
    // `Option::take` side channel.
    let mut slot = Some(recorder);
    GLOBAL.get_or_init(|| {
        let r = slot.take().expect("get_or_init closure runs at most once");
        let arc: Arc<dyn Recorder> = Arc::new(r);
        arc
    });
    slot.map_or(Ok(()), |rejected| Err(SetRecorderError(rejected)))
}

/// Run `f` with a reference to the current effective recorder (thread-local
/// override if set, otherwise the global recorder, otherwise a no-op).
pub fn with_recorder<T>(f: impl FnOnce(&dyn Recorder) -> T) -> T {
    // Clone an `Arc` out of the slot before invoking `f`. Holding the
    // `RefCell` borrow across `f` would panic if `f` reentrantly mutated the
    // local slot (e.g., installed a nested local recorder), and holding any
    // global lock would similarly invite deadlocks.
    let local = LOCAL.with(|cell| cell.borrow().clone());
    if let Some(local) = local {
        return f(&*local);
    }
    if let Some(global) = GLOBAL.get() {
        return f(&**global);
    }
    f(&NoopRecorder)
}

thread_local! {
    static LOCAL:RefCell<Option<Arc<dyn Recorder>>> = const {RefCell::new(None) };
}

/// RAII guard returned by [`set_default_local_recorder`]; restores the
/// previous thread-local recorder when dropped.
pub struct LocalRecorderGuard {
    /// Recorder active before this guard installed its override.
    prev: Option<Arc<dyn Recorder>>,
}

impl std::fmt::Debug for LocalRecorderGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalRecorderGuard").field("had_previous", &self.prev.is_some()).finish()
    }
}

impl Drop for LocalRecorderGuard {
    fn drop(&mut self) {
        let prev = self.prev.take();
        LOCAL.with(|cell| *cell.borrow_mut() = prev);
    }
}

/// Set a thread-local recorder that overrides the global one for this thread
/// until the returned guard is dropped.
pub fn set_default_local_recorder<R: Recorder + 'static>(recorder: R) -> LocalRecorderGuard {
    let arc: Arc<dyn Recorder> = Arc::new(recorder);
    let prev = LOCAL.with(|cell| cell.borrow_mut().replace(arc));
    LocalRecorderGuard { prev }
}

/// Wraps a recorder and yields a new one. Implemented for things like
/// [`PrefixLayer`].
pub trait Layer<R> {
    /// Recorder type produced by [`layer`](Self::layer).
    type Output: Recorder;
    /// Wrap `inner` in this layer's transformation.
    fn layer(&self, inner: R) -> Self::Output;
}

/// Build a stack of layers around an inner recorder, then install the result
/// as the global recorder.
///
/// Construct with [`Stack::new`], wrap with [`Stack::push`] one layer at a
/// time, and finalize via [`Stack::install`].
pub struct Stack<R> {
    /// Recorder wrapped by the accumulated layer stack.
    inner: R,
}

impl<R: std::fmt::Debug> std::fmt::Debug for Stack<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stack").field("inner", &self.inner).finish()
    }
}

impl<R: Recorder + 'static> Stack<R> {
    /// Construct a new stack wrapping `inner`.
    pub const fn new(inner: R) -> Self {
        Self { inner }
    }

    /// Wrap the current top of the stack in `layer`, returning a new `Stack`.
    pub fn push<L: Layer<R>>(self, layer: &L) -> Stack<L::Output>
    where
        L::Output: 'static,
    {
        Stack { inner: layer.layer(self.inner) }
    }

    /// Install the assembled recorder as the global one.
    ///
    /// # Errors
    ///
    /// Returns [`SetRecorderError`] when a global recorder was already installed.
    pub fn install(self) -> Result<(), SetRecorderError<R>> {
        set_global_recorder(self.inner)
    }
}

/// Layer that prepends a fixed string to every metric name passed through.
#[derive(Copy, Clone, Debug)]
pub struct PrefixLayer(&'static str);

impl PrefixLayer {
    /// Construct a new prefix layer. The prefix is leaked to obtain a
    /// `'static` lifetime so that prefixed `KeyName` values built from it
    /// can be passed through the `Recorder` describe APIs without copying.
    pub fn new<S: Into<String>>(prefix: S) -> Self {
        let leaked: &'static str = Box::leak(prefix.into().into_boxed_str());
        Self(leaked)
    }
}

impl<R: Recorder> Layer<R> for PrefixLayer {
    type Output = Prefix<R>;
    fn layer(&self, inner: R) -> Self::Output {
        Prefix { prefix: self.0, inner }
    }
}

/// Recorder produced by [`PrefixLayer`].
pub struct Prefix<R> {
    /// Prefix prepended to every metric name.
    prefix: &'static str,
    /// Recorder receiving prefixed calls.
    inner: R,
}

impl<R: std::fmt::Debug> std::fmt::Debug for Prefix<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prefix").field("prefix", &self.prefix).field("inner", &self.inner).finish()
    }
}

impl<R: Recorder> Prefix<R> {
    /// Build a prefixed metric name.
    fn prefixed_name(&self, name: &str) -> KeyName {
        // Joins prefix and name with `.`. With `PrefixLayer::new("app")`, a
        // callsite `network.connected_peers` renders as
        // `app.network.connected_peers` (and after Prometheus sanitization,
        // `app_network_connected_peers`).
        let mut s = String::with_capacity(self.prefix.len() + 1 + name.len());
        s.push_str(self.prefix);
        s.push('.');
        s.push_str(name);
        KeyName::new(s)
    }

    /// Build a prefixed metric key while preserving labels.
    fn prefixed_key(&self, key: &Key) -> Key {
        // Reconstruct the key with the prefixed name; labels are cloned
        // because we may need to own them when building a fresh `Key`.
        let labels = key.labels().to_vec();
        Key::from_parts(self.prefixed_name(key.name()), labels)
    }
}

impl<R: Recorder> Recorder for Prefix<R> {
    fn describe_counter(&self, key: KeyName, description: Cow<'static, str>) {
        let name = self.prefixed_name(key.as_str());
        self.inner.describe_counter(name, description);
    }

    fn describe_gauge(&self, key: KeyName, description: Cow<'static, str>) {
        let name = self.prefixed_name(key.as_str());
        self.inner.describe_gauge(name, description);
    }

    fn describe_histogram(&self, key: KeyName, description: Cow<'static, str>) {
        let name = self.prefixed_name(key.as_str());
        self.inner.describe_histogram(name, description);
    }

    fn register_counter(&self, key: &Key) -> Counter {
        let new_key = self.prefixed_key(key);
        self.inner.register_counter(&new_key)
    }

    fn register_gauge(&self, key: &Key) -> Gauge {
        let new_key = self.prefixed_key(key);
        self.inner.register_gauge(&new_key)
    }

    fn register_histogram(&self, key: &Key) -> Histogram {
        let new_key = self.prefixed_key(key);
        self.inner.register_histogram(&new_key)
    }
}

/// Build a `Key` reference for a callsite.
///
/// The no-label literal case returns a `&'static Key` from a `OnceLock`, so
/// the FNV-1a hash is computed at most once per callsite for the lifetime of
/// the process. All other cases return a fresh `Key` value (which the caller
/// borrows inline as an argument), because labels may be dynamic.
#[doc(hidden)]
#[macro_export]
macro_rules! __metrics_key {
    ($name:literal) => {{
        static KEY: ::std::sync::OnceLock<$crate::Key> = ::std::sync::OnceLock::new();
        KEY.get_or_init(|| $crate::Key::from_static_name($name))
    }};
    ($name:expr) => {
        &$crate::Key::from_name($name)
    };
    ($name:literal, $($k:expr => $v:expr),+ $(,)?) => {
        &$crate::Key::from_parts(
            $name,
            ::std::vec![$($crate::Label::new($k, $v)),+],
        )
    };
    ($name:expr, $($k:expr => $v:expr),+ $(,)?) => {
        &$crate::Key::from_parts(
            $name,
            ::std::vec![$($crate::Label::new($k, $v)),+],
        )
    };
    ($name:literal, $labels:expr) => {
        &$crate::Key::from_parts($name, $crate::IntoLabels::into_labels($labels))
    };
    ($name:expr, $labels:expr) => {
        &$crate::Key::from_parts($name, $crate::IntoLabels::into_labels($labels))
    };
}

/// Obtain a [`Counter`] handle.
#[macro_export]
macro_rules! counter {
    ($name:expr $(,)?) => {
        $crate::with_recorder(|r| r.register_counter($crate::__metrics_key!($name)))
    };
    ($name:expr, $($k:expr => $v:expr),+ $(,)?) => {
        $crate::with_recorder(|r| r.register_counter(
            $crate::__metrics_key!($name, $($k => $v),+),
        ))
    };
    ($name:expr, $labels:expr $(,)?) => {
        $crate::with_recorder(|r| r.register_counter(
            $crate::__metrics_key!($name, $labels),
        ))
    };
}

/// Obtain a [`Gauge`] handle.
#[macro_export]
macro_rules! gauge {
    ($name:expr $(,)?) => {
        $crate::with_recorder(|r| r.register_gauge($crate::__metrics_key!($name)))
    };
    ($name:expr, $($k:expr => $v:expr),+ $(,)?) => {
        $crate::with_recorder(|r| r.register_gauge(
            $crate::__metrics_key!($name, $($k => $v),+),
        ))
    };
    ($name:expr, $labels:expr $(,)?) => {
        $crate::with_recorder(|r| r.register_gauge(
            $crate::__metrics_key!($name, $labels),
        ))
    };
}

/// Obtain a [`Histogram`] handle.
#[macro_export]
macro_rules! histogram {
    ($name:expr $(,)?) => {
        $crate::with_recorder(|r| r.register_histogram($crate::__metrics_key!($name)))
    };
    ($name:expr, $($k:expr => $v:expr),+ $(,)?) => {
        $crate::with_recorder(|r| r.register_histogram(
            $crate::__metrics_key!($name, $($k => $v),+),
        ))
    };
    ($name:expr, $labels:expr $(,)?) => {
        $crate::with_recorder(|r| r.register_histogram(
            $crate::__metrics_key!($name, $labels),
        ))
    };
}

/// Describe a counter with a description.
#[macro_export]
macro_rules! describe_counter {
    ($name:expr, $description:expr $(,)?) => {
        $crate::with_recorder(|r| {
            r.describe_counter(
                $crate::KeyName::new($name),
                ::std::borrow::Cow::Borrowed($description),
            )
        })
    };
}

/// Describe a gauge with a description.
#[macro_export]
macro_rules! describe_gauge {
    ($name:expr, $description:expr $(,)?) => {
        $crate::with_recorder(|r| {
            r.describe_gauge(
                $crate::KeyName::new($name),
                ::std::borrow::Cow::Borrowed($description),
            )
        })
    };
}

/// Describe a histogram with a description.
#[macro_export]
macro_rules! describe_histogram {
    ($name:expr, $description:expr $(,)?) => {
        $crate::with_recorder(|r| {
            r.describe_histogram(
                $crate::KeyName::new($name),
                ::std::borrow::Cow::Borrowed($description),
            )
        })
    };
}
