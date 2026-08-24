//! Compile-and-run compatibility tests for the entry-point macros. Regular
//! `#[test]` runtime tests (rather than `trybuild`-style compile-only files)
//! so a successful run also proves the macros dispatch through to the
//! recorder, not just that they parse.

#[cfg(test)]
mod tests {
    use kival_metrics::{
        LocalRecorderGuard, counter, describe_counter, gauge, histogram,
        prometheus::{PrometheusBuilder, PrometheusHandle},
        set_default_local_recorder,
    };

    fn install() -> (LocalRecorderGuard, PrometheusHandle) {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let guard = set_default_local_recorder(recorder);
        (guard, handle)
    }

    /// Every macro arm must accept an optional trailing comma. Pin all
    /// three label shapes — none, `=>` literals, and a labels expression.
    #[test]
    fn macros_accept_trailing_commas_in_every_arm() {
        let (_g, h) = install();

        // No labels.
        counter!("tc_a",).increment(1);
        gauge!("tc_b",).set(2.0);
        histogram!("tc_c",).record(0.5);

        // `=>` literal labels with trailing comma after the last pair.
        counter!("tc_d", "k" => "v",).increment(1);
        gauge!("tc_e", "k" => "v",).set(3.0);
        histogram!("tc_f", "k" => "v",).record(0.25);

        // Owned `Vec` labels expression with trailing comma after the expr.
        let labels: Vec<(&str, &str)> = vec![("k", "v")];
        counter!("tc_g", labels.clone(),).increment(1);
        gauge!("tc_h", labels.clone(),).set(4.0);
        histogram!("tc_i", labels,).record(0.75);

        let body = h.render();
        for name in ["tc_a", "tc_b", "tc_c", "tc_d", "tc_e", "tc_f", "tc_g", "tc_h", "tc_i"] {
            assert!(body.contains(name), "missing {name} in:\n{body}");
        }
    }

    /// Every macro arm must accept a *non-literal* metric name (a
    /// `String`, a `format!()` expression, or a `let`-bound `&str`). Our
    /// `__metrics_key!` macro takes `$name:expr` for these arms, so this is
    /// the regression test that proves the `expr` matcher actually expands
    /// as advertised.
    #[test]
    fn macros_accept_nonliteral_string_keys() {
        let (_g, h) = install();

        let owned: String = String::from("nl_owned");
        counter!(owned.clone()).increment(1);

        let n = 7_u16;
        counter!(format!("nl_format_{n}")).increment(2);

        let borrowed = "nl_borrowed";
        counter!(borrowed).increment(3);

        let body = h.render();
        assert!(body.contains("nl_owned 1"), "{body}");
        assert!(body.contains("nl_format_7 2"), "{body}");
        assert!(body.contains("nl_borrowed 3"), "{body}");
    }

    /// A `const &str` key works the same as a literal. This pins that the
    /// `OnceLock`-cached arm (which only matches `$name:literal`) does NOT
    /// silently swallow `const` items — they take the `$name:expr` arm and
    /// still compile and dispatch.
    #[test]
    fn macros_accept_const_string_keys() {
        let (_g, h) = install();

        const KEY: &str = "const_key";
        const DESC: &str = "a counter";
        describe_counter!(KEY, DESC);
        counter!(KEY).increment(17);

        let body = h.render();
        assert!(body.contains("const_key 17"), "{body}");
    }

    /// When a user has their own `metrics` module in scope (here:
    /// `framework::metrics`), the macros must still resolve `metrics::*`
    /// types via the absolute `$crate::*` path inside the macro body.
    /// Without `$crate`, `metrics::Key` would resolve against
    /// `framework::metrics` and fail to compile or — worse — pick up the
    /// wrong type silently. This is the canonical macro-hygiene regression
    /// scenario.
    #[test]
    fn macros_resolve_via_absolute_path_when_metrics_mod_is_shadowed() {
        let (_g, h) = install();

        // Shadow the crate name with a local module — this is the trap.
        mod framework {
            pub(crate) mod metrics {
                pub(crate) const NAME: &str = "shadowed";
                pub(crate) const KEY: &str = "method";
                pub(crate) const VAL: &str = "GET";
            }
        }
        use framework::metrics as _shadow;

        // `metrics::counter!` must still expand against the *crate* `kival-metrics`, not
        // the in-scope `metrics` module above.
        ::kival_metrics::counter!(_shadow::NAME, _shadow::KEY => _shadow::VAL).increment(5);
        ::kival_metrics::counter!(_shadow::NAME, &[(_shadow::KEY, _shadow::VAL)]).increment(0);

        let body = h.render();
        assert!(body.contains(r#"shadowed{method="GET"} 5"#), "{body}");
    }
}
