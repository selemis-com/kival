//! End-to-end test: macros and custom recorder.

#[cfg(test)]
mod tests {
    use std::{
        borrow::Cow,
        collections::HashMap,
        sync::{Arc, Mutex, atomic::Ordering},
    };

    use kival_metrics::{
        AtomicU64, Counter, Gauge, Histogram, HistogramFn, Key, KeyName, Label, Layer,
        NoopRecorder, PrefixLayer, Recorder, Stack, counter, describe_counter, describe_gauge,
        describe_histogram, gauge, histogram, set_default_local_recorder, set_global_recorder,
    };

    /// Recording recorder that stores every key it sees, so tests can assert
    /// which metrics were registered.
    #[derive(Default, Clone)]
    struct TestRecorder {
        inner: Arc<Mutex<TestState>>,
    }

    #[derive(Default)]
    struct TestState {
        counters: HashMap<String, Arc<AtomicU64>>,
        gauges: HashMap<String, Arc<AtomicU64>>,
        histograms: HashMap<String, Arc<TestHistogram>>,
        descriptions: HashMap<String, String>,
    }

    #[derive(Default)]
    struct TestHistogram {
        samples: Mutex<Vec<f64>>,
    }

    impl HistogramFn for TestHistogram {
        fn record(&self, value: f64) {
            self.samples.lock().unwrap().push(value);
        }
    }

    impl TestRecorder {
        fn snapshot(&self) -> TestSnapshot {
            let s = self.inner.lock().unwrap();
            TestSnapshot {
                gauge_names: s.gauges.keys().cloned().collect(),
                descriptions: s.descriptions.clone(),
                counter_values: s
                    .counters
                    .iter()
                    .map(|(k, v)| (k.clone(), v.load(Ordering::Relaxed)))
                    .collect(),
                gauge_values: s
                    .gauges
                    .iter()
                    .map(|(k, v)| (k.clone(), f64::from_bits(v.load(Ordering::Relaxed))))
                    .collect(),
                histogram_samples: s
                    .histograms
                    .iter()
                    .map(|(k, h)| (k.clone(), h.samples.lock().unwrap().clone()))
                    .collect(),
            }
        }
    }

    #[derive(Debug)]
    struct TestSnapshot {
        gauge_names: Vec<String>,
        descriptions: HashMap<String, String>,
        counter_values: HashMap<String, u64>,
        gauge_values: HashMap<String, f64>,
        histogram_samples: HashMap<String, Vec<f64>>,
    }

    impl Recorder for TestRecorder {
        fn describe_counter(&self, k: KeyName, d: Cow<'static, str>) {
            self.inner.lock().unwrap().descriptions.insert(k.as_str().into(), d.into_owned());
        }
        fn describe_gauge(&self, k: KeyName, d: Cow<'static, str>) {
            self.inner.lock().unwrap().descriptions.insert(k.as_str().into(), d.into_owned());
        }
        fn describe_histogram(&self, k: KeyName, d: Cow<'static, str>) {
            self.inner.lock().unwrap().descriptions.insert(k.as_str().into(), d.into_owned());
        }

        fn register_counter(&self, key: &Key) -> Counter {
            let counter = {
                let mut state = self.inner.lock().unwrap();
                state
                    .counters
                    .entry(key.name().into())
                    .or_insert_with(|| Arc::new(AtomicU64::new(0)))
                    .clone()
            };
            Counter::from_arc(counter)
        }

        fn register_gauge(&self, key: &Key) -> Gauge {
            let gauge = {
                let mut state = self.inner.lock().unwrap();
                state
                    .gauges
                    .entry(key.name().into())
                    .or_insert_with(|| Arc::new(AtomicU64::new(0)))
                    .clone()
            };
            Gauge::from_arc(gauge)
        }

        fn register_histogram(&self, key: &Key) -> Histogram {
            let histogram = {
                let mut state = self.inner.lock().unwrap();
                state
                    .histograms
                    .entry(key.name().into())
                    .or_insert_with(|| Arc::new(TestHistogram::default()))
                    .clone()
            };
            Histogram::from_arc(histogram)
        }
    }

    #[test]
    fn counter_gauge_histogram_macros_dispatch_to_recorder() {
        let rec = TestRecorder::default();
        let _guard = set_default_local_recorder(rec.clone());

        counter!("requests").increment(3);
        counter!("requests").increment(2);
        gauge!("connections").set(7.0);
        histogram!("latency").record(0.5);
        histogram!("latency").record(1.5);

        let snap = rec.snapshot();
        assert_eq!(snap.counter_values.get("requests").copied(), Some(5));
        assert_eq!(snap.gauge_values.get("connections").copied(), Some(7.0));
        assert_eq!(snap.histogram_samples.get("latency").unwrap(), &vec![0.5, 1.5]);
    }

    #[test]
    fn macros_with_inline_labels_register_correct_keys() {
        let rec = TestRecorder::default();
        let _guard = set_default_local_recorder(rec.clone());

        gauge!("storage", "table" => "headers").set(1.0);
        gauge!("storage", "table" => "bodies").set(2.0);

        let snap = rec.snapshot();
        // Both register under the same name; recorder is keyed by name only here.
        assert!(snap.gauge_names.contains(&"storage".to_owned()));
    }

    #[test]
    fn describe_macros_propagate_description() {
        let rec = TestRecorder::default();
        let _guard = set_default_local_recorder(rec.clone());

        describe_counter!("io.bytes", "Total bytes transferred");
        describe_gauge!("connections", "Active connections");
        describe_histogram!("latency", "Request latency");

        let snap = rec.snapshot();
        assert_eq!(snap.descriptions.get("io.bytes"), Some(&"Total bytes transferred".to_owned()),);
        assert_eq!(snap.descriptions.get("connections"), Some(&"Active connections".to_owned()));
        assert_eq!(snap.descriptions.get("latency"), Some(&"Request latency".to_owned()));
    }

    #[test]
    fn macros_with_owned_labels_vec() {
        let rec = TestRecorder::default();
        let _guard = set_default_local_recorder(rec.clone());

        let labels = vec![Label::new("k1", "v1"), Label::new("k2", "v2")];
        counter!("with_labels", labels).increment(42);

        assert_eq!(rec.snapshot().counter_values.get("with_labels").copied(), Some(42));
    }

    #[test]
    fn prefix_layer_joins_prefix_and_name_with_dot() {
        // PrefixLayer wraps a TestRecorder so we can read the name as the inner
        // recorder sees it. Note: PrefixLayer rebuilds the Key, so the inner
        // recorder receives the prefixed name.
        let inner = TestRecorder::default();
        let snap_handle = inner.clone();
        let layered = PrefixLayer::new("app").layer(inner);
        let _guard = set_default_local_recorder(layered);

        counter!("network.connected_peers").increment(1);

        let snap = snap_handle.snapshot();
        assert!(
            snap.counter_values.contains_key("app.network.connected_peers"),
            "expected dotted prefix join, got keys: {:?}",
            snap.counter_values.keys().collect::<Vec<_>>(),
        );
    }

    #[test]
    fn macros_accept_borrowed_tuple_array_labels() {
        // The `gauge!("info", &labels)` callsite shape where `labels` is a
        // borrowed array of `(&str, &str)` / `(&str, String)` tuples.
        let rec = TestRecorder::default();
        let _guard = set_default_local_recorder(rec.clone());

        let labels: [(&str, &str); 2] = [("version", "1.2.3"), ("commit", "abc")];
        gauge!("info", &labels).set(1.0);

        let owned: [(&str, String); 1] = [("network", "mainnet".to_owned())];
        gauge!("chain_spec", &owned).set(1.0);

        let snap = rec.snapshot();
        assert!(snap.gauge_names.contains(&"info".to_owned()));
        assert!(snap.gauge_names.contains(&"chain_spec".to_owned()));
    }

    #[test]
    fn recorder_blanket_impls_forward_every_method() {
        let arc = Arc::new(TestRecorder::default());
        let arc_recorder = arc.clone();
        arc_recorder.describe_counter("arc.counter".into(), "counter".into());
        arc_recorder.describe_gauge("arc.gauge".into(), "gauge".into());
        arc_recorder.describe_histogram("arc.histogram".into(), "histogram".into());
        arc_recorder.register_counter(&Key::from_name("arc.counter")).increment(2);
        arc_recorder.register_gauge(&Key::from_name("arc.gauge")).set(3.0);
        arc_recorder.register_histogram(&Key::from_name("arc.histogram")).record(4.0);

        let snap = arc.snapshot();
        assert_eq!(snap.counter_values.get("arc.counter").copied(), Some(2));
        assert_eq!(snap.gauge_values.get("arc.gauge").copied(), Some(3.0));
        assert_eq!(snap.histogram_samples.get("arc.histogram"), Some(&vec![4.0]));
        assert_eq!(snap.descriptions.get("arc.counter"), Some(&"counter".to_owned()));
        assert_eq!(snap.descriptions.get("arc.gauge"), Some(&"gauge".to_owned()));
        assert_eq!(snap.descriptions.get("arc.histogram"), Some(&"histogram".to_owned()));

        let boxed_inner = TestRecorder::default();
        let boxed_snapshot = boxed_inner.clone();
        let boxed: Box<dyn Recorder> = Box::new(boxed_inner);
        boxed.describe_counter("box.counter".into(), "counter".into());
        boxed.describe_gauge("box.gauge".into(), "gauge".into());
        boxed.describe_histogram("box.histogram".into(), "histogram".into());
        boxed.register_counter(&Key::from_name("box.counter")).increment(5);
        boxed.register_gauge(&Key::from_name("box.gauge")).set(6.0);
        boxed.register_histogram(&Key::from_name("box.histogram")).record(7.0);

        let snap = boxed_snapshot.snapshot();
        assert_eq!(snap.counter_values.get("box.counter").copied(), Some(5));
        assert_eq!(snap.gauge_values.get("box.gauge").copied(), Some(6.0));
        assert_eq!(snap.histogram_samples.get("box.histogram"), Some(&vec![7.0]));
        assert_eq!(snap.descriptions.get("box.counter"), Some(&"counter".to_owned()));
        assert_eq!(snap.descriptions.get("box.gauge"), Some(&"gauge".to_owned()));
        assert_eq!(snap.descriptions.get("box.histogram"), Some(&"histogram".to_owned()));
    }

    #[test]
    fn noop_recorder_returns_noop_handles() {
        let noop = NoopRecorder;
        noop.describe_counter("noop.counter".into(), "counter".into());
        noop.describe_gauge("noop.gauge".into(), "gauge".into());
        noop.describe_histogram("noop.histogram".into(), "histogram".into());
        noop.register_counter(&Key::from_name("noop.counter")).increment(1);
        noop.register_gauge(&Key::from_name("noop.gauge")).set(2.0);
        noop.register_histogram(&Key::from_name("noop.histogram")).record(3.0);
    }

    #[test]
    fn prefix_layer_forwards_descriptions_and_all_handle_types() {
        let inner = TestRecorder::default();
        let snap_handle = inner.clone();
        let prefixed = PrefixLayer::new("svc").layer(inner);

        prefixed.describe_counter("requests".into(), "requests".into());
        prefixed.describe_gauge("connections".into(), "connections".into());
        prefixed.describe_histogram("latency".into(), "latency".into());
        prefixed.register_counter(&Key::from_name("requests")).increment(8);
        prefixed.register_gauge(&Key::from_name("connections")).set(9.0);
        prefixed.register_histogram(&Key::from_name("latency")).record(10.0);

        let snap = snap_handle.snapshot();
        assert_eq!(snap.counter_values.get("svc.requests").copied(), Some(8));
        assert_eq!(snap.gauge_values.get("svc.connections").copied(), Some(9.0));
        assert_eq!(snap.histogram_samples.get("svc.latency"), Some(&vec![10.0]));
        assert_eq!(snap.descriptions.get("svc.requests"), Some(&"requests".to_owned()));
        assert_eq!(snap.descriptions.get("svc.connections"), Some(&"connections".to_owned()));
        assert_eq!(snap.descriptions.get("svc.latency"), Some(&"latency".to_owned()));
    }

    #[test]
    fn set_global_recorder_is_install_once() {
        // First install must succeed; second install must return the
        // rejected recorder via `Err`.
        //
        // NB: this test mutates *process-global* state. It is the only test
        // in this binary that touches `set_global_recorder`; everything
        // else uses `set_default_local_recorder`.
        assert!(set_global_recorder(NoopRecorder).is_ok(), "first install must succeed");
        let again = set_global_recorder(NoopRecorder);
        assert!(again.is_err(), "second install must return Err");

        // And a layered install also fails after the slot is taken.
        let stack = Stack::new(NoopRecorder).push(&PrefixLayer::new("app"));
        assert!(stack.install().is_err(), "Stack::install must surface install-once Err");
    }

    /// A local recorder set on thread A must NOT be visible to thread B —
    /// otherwise the parallel-test pattern (every test installs its own
    /// local recorder and asserts on its own handle) silently sees other
    /// tests' metrics. This is the contract every test in this workspace
    /// relies on.
    #[test]
    fn local_recorder_is_isolated_per_thread() {
        use std::sync::{
            Arc, Barrier,
            atomic::{AtomicUsize, Ordering},
        };

        /// A recorder that just counts how many times `register_counter` is
        /// called, so each thread can observe via its OWN counter whether the
        /// other thread's local recorder leaked.
        #[derive(Default, Clone)]
        struct Counting {
            n: Arc<AtomicUsize>,
        }
        impl Recorder for Counting {
            fn describe_counter(&self, _: KeyName, _: Cow<'static, str>) {}
            fn describe_gauge(&self, _: KeyName, _: Cow<'static, str>) {}
            fn describe_histogram(&self, _: KeyName, _: Cow<'static, str>) {}
            fn register_counter(&self, _: &Key) -> Counter {
                self.n.fetch_add(1, Ordering::SeqCst);
                Counter::noop()
            }
            fn register_gauge(&self, _: &Key) -> Gauge {
                Gauge::noop()
            }
            fn register_histogram(&self, _: &Key) -> Histogram {
                Histogram::noop()
            }
        }

        let a = Counting::default();
        let b = Counting::default();
        let barrier = Arc::new(Barrier::new(2));

        let a_clone = a.clone();
        let b_clone = b.clone();
        let bar_a = Arc::clone(&barrier);
        let bar_b = Arc::clone(&barrier);

        let t_a = std::thread::spawn(move || {
            let _g = set_default_local_recorder(a_clone);
            bar_a.wait(); // both threads have their local recorder installed
            counter!("from_a").increment(1);
        });
        let t_b = std::thread::spawn(move || {
            let _g = set_default_local_recorder(b_clone);
            bar_b.wait();
            counter!("from_b").increment(1);
        });

        t_a.join().unwrap();
        t_b.join().unwrap();

        // Each thread saw EXACTLY one register_counter call — its own. If the
        // local recorder leaked between threads, one side would see 2 and the
        // other would see 0.
        assert_eq!(a.n.load(Ordering::SeqCst), 1, "thread A's recorder saw cross-thread traffic");
        assert_eq!(b.n.load(Ordering::SeqCst), 1, "thread B's recorder saw cross-thread traffic");
    }

    /// When the `LocalRecorderGuard` drops, `with_recorder` must fall back
    /// to the global recorder (or the `NoopRecorder` default). Without
    /// this, a test panic could leave the thread permanently shadowing the
    /// global recorder for every subsequent test on the same harness
    /// thread.
    #[test]
    fn local_recorder_restored_when_guard_dropped() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        #[derive(Default, Clone)]
        struct Counting {
            n: Arc<AtomicUsize>,
        }
        impl Recorder for Counting {
            fn describe_counter(&self, _: KeyName, _: Cow<'static, str>) {}
            fn describe_gauge(&self, _: KeyName, _: Cow<'static, str>) {}
            fn describe_histogram(&self, _: KeyName, _: Cow<'static, str>) {}
            fn register_counter(&self, _: &Key) -> Counter {
                self.n.fetch_add(1, Ordering::SeqCst);
                Counter::noop()
            }
            fn register_gauge(&self, _: &Key) -> Gauge {
                Gauge::noop()
            }
            fn register_histogram(&self, _: &Key) -> Histogram {
                Histogram::noop()
            }
        }

        let r = Counting::default();
        let r_clone = r.clone();
        {
            let _g = set_default_local_recorder(r_clone);
            counter!("inside").increment(1);
            assert_eq!(r.n.load(Ordering::SeqCst), 1, "register not seen inside guard");
        }
        // Guard dropped; the next macro call must NOT route to `r`.
        counter!("outside").increment(1);
        assert_eq!(r.n.load(Ordering::SeqCst), 1, "local recorder still active after guard drop");
    }

    /// `Box<R>` and `Arc<R>` (where `R: Recorder + ?Sized`) must themselves
    /// implement `Recorder`, so users can pass any of these as the default
    /// recorder without unwrapping. Compile-and-run sanity check that the
    /// blanket impls dispatch to the inner recorder.
    #[test]
    fn recorder_blanket_implementations_dispatch_through() {
        use std::sync::Arc;

        fn assert_recorder<R: Recorder>(_: &R) {}

        let inner: Box<dyn Recorder> = Box::new(NoopRecorder);
        assert_recorder(&inner);

        let arc_inner: Arc<dyn Recorder> = Arc::new(NoopRecorder);
        assert_recorder(&arc_inner);

        // Dispatching through the blanket impl must not panic and must
        // return real handles (the noop ones).
        let key = Key::from_static_name("blanket");
        let _c = inner.register_counter(&key);
        let _g = arc_inner.register_gauge(&key);
    }

    #[test]
    fn atomic_u64_can_back_a_counter_and_gauge() {
        // An Arc<AtomicU64> can be wrapped directly as Counter or Gauge backing
        // storage via the built-in CounterFn / GaugeFn impls on AtomicU64.
        let raw = Arc::new(AtomicU64::new(0));
        let c = Counter::from_arc(raw.clone());
        c.increment(7);
        c.increment(3);
        assert_eq!(raw.load(Ordering::Relaxed), 10);

        let raw = Arc::new(AtomicU64::new(0));
        let g = Gauge::from_arc(raw.clone());
        g.set(2.5);
        g.increment(0.5);
        assert!((f64::from_bits(raw.load(Ordering::Relaxed)) - 3.0).abs() < 1e-9);
    }
}
