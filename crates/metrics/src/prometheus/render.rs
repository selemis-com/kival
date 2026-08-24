//! Prometheus text exposition format rendering.
//!
//! Output follows the standard `# HELP` / `# TYPE` header convention with one
//! body line per (name, label-set) tuple. Histograms are emitted as
//! `summary`-typed blocks with quantile labels.

use std::{collections::BTreeMap, fmt::Write};

use crate::{Key, Label, prometheus::storage::HistogramSnapshot};

/// Quantiles emitted for every histogram.
pub(crate) const QUANTILES: &[f64] = &[0.0, 0.5, 0.9, 0.95, 0.99, 0.999, 1.0];

/// Sanitize a metric name so it satisfies Prometheus's grammar
/// `[a-zA-Z_:][a-zA-Z0-9_:]*`.
///
/// Dotted callsite names like `network.connected_peers` are common in this
/// crate, but raw `.` is not a valid character in the exposition format. Any
/// invalid character is replaced with `_`, so the example would render as
/// `network_connected_peers`.
pub(crate) fn sanitize_metric_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars();
    if let Some(first) = chars.next() {
        out.push(if first.is_ascii_alphabetic() || first == '_' || first == ':' {
            first
        } else {
            '_'
        });
    }
    for ch in chars {
        out.push(if ch.is_ascii_alphanumeric() || ch == '_' || ch == ':' { ch } else { '_' });
    }
    out
}

/// Sanitize a label key so it satisfies Prometheus's grammar
/// `[a-zA-Z_][a-zA-Z0-9_]*` (no `:` allowed in label names).
///
/// Applied at output-write time only; raw label keys are preserved for
/// identity so that two distinct raw series do not collapse into one
/// rendered line just because their keys sanitize identically.
fn sanitize_label_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut chars = key.chars();
    if let Some(first) = chars.next() {
        out.push(if first.is_ascii_alphabetic() || first == '_' { first } else { '_' });
    }
    for ch in chars {
        out.push(if ch.is_ascii_alphanumeric() || ch == '_' { ch } else { '_' });
    }
    out
}

/// Description metadata kept per metric name.
#[derive(Debug, Default)]
pub(crate) struct MetricDescription {
    /// Help text rendered in the `# HELP` line.
    pub(crate) help: String,
}

/// Per-name accumulator: every (name, label-set) instance plus a description.
#[derive(Debug)]
pub(crate) struct MetricFamily<V> {
    /// Per-metric-name description.
    pub(crate) description: MetricDescription,
    /// Rendered series instances keyed by sanitized labels.
    pub(crate) instances: BTreeMap<LabelKey, V>,
}

// Manual `Default` so `V` doesn't need to implement `Default` itself
// (the families are populated via `insert`, never via the value's default).
impl<V> Default for MetricFamily<V> {
    fn default() -> Self {
        Self { description: MetricDescription::default(), instances: BTreeMap::new() }
    }
}

/// Sorted, hash-stable representation of a label set used as a `BTreeMap` key.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub(crate) struct LabelKey(Vec<(String, String)>);

impl LabelKey {
    /// Build a render-key from raw labels.
    pub(crate) fn from_labels(labels: &[Label]) -> Self {
        // Sanitize *label keys* at the render-grouping boundary so two raw
        // series whose keys collapse to the same Prometheus name (e.g.
        // `a-b` and `a_b`) merge into a single emitted series rather than
        // emitting two identical lines on the wire (which is invalid
        // exposition). Last-write-wins on the resulting `BTreeMap`.
        // Registry-level identity (in the recorder's `HashMap<Key, _>`)
        // still uses raw labels, so distinct raw series remain distinct in
        // the registry — only the *render output* is collapsed.
        //
        // Values are kept raw and escaped at write time.
        //
        // Within a single labelset we also collapse duplicate *sanitized*
        // keys (last-wins). Without this, raw labels `("a-b", "x")` and
        // `("a_b", "y")` on the same metric instance would render as
        // `{a_b="x",a_b="y"}`, which Prometheus parsers reject.
        let mut by_sanitized: BTreeMap<String, String> = BTreeMap::new();
        for label in labels {
            by_sanitized.insert(sanitize_label_key(label.key()), label.value().to_owned());
        }
        Self(by_sanitized.into_iter().collect())
    }

    /// Write this label set, plus an optional synthetic label, to `out`.
    fn write_labels(&self, out: &mut String, extra: Option<(&str, &str)>) {
        if self.0.is_empty() && extra.is_none() {
            return;
        }
        out.push('{');
        let mut first = true;
        for (k, v) in &self.0 {
            if !first {
                out.push(',');
            }
            first = false;
            // Key already sanitized in `from_labels`; values are escaped here.
            out.push_str(k);
            out.push_str("=\"");
            write_escaped(out, v);
            out.push('"');
        }
        if let Some((k, v)) = extra {
            if !first {
                out.push(',');
            }
            // `extra` is the synthetic `quantile` label and is hard-coded
            // safe; emit it verbatim.
            out.push_str(k);
            out.push_str("=\"");
            write_escaped(out, v);
            out.push('"');
        }
        out.push('}');
    }
}

/// Write a Prometheus-escaped label value.
fn write_escaped(out: &mut String, value: &str) {
    // Prometheus exposition format escapes inside label values: backslash,
    // double-quote, and newline. Every backslash is escaped — including
    // backslashes that already appear inside an escape sequence.
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
}

/// Render a counter family.
pub(crate) fn render_counters(out: &mut String, name: &str, family: &MetricFamily<u64>) {
    write_help_type(out, name, &family.description, "counter");
    for (labels, value) in &family.instances {
        out.push_str(name);
        labels.write_labels(out, None);
        let _ = writeln!(out, " {value}");
    }
}

/// Render a gauge family.
pub(crate) fn render_gauges(out: &mut String, name: &str, family: &MetricFamily<f64>) {
    write_help_type(out, name, &family.description, "gauge");
    for (labels, value) in &family.instances {
        out.push_str(name);
        labels.write_labels(out, None);
        let _ = writeln!(out, " {value}");
    }
}

/// Render a histogram family as a Prometheus `summary`.
pub(crate) fn render_histograms(
    out: &mut String,
    name: &str,
    family: &mut MetricFamily<HistogramSnapshot>,
) {
    write_help_type(out, name, &family.description, "summary");
    for (labels, snap) in &mut family.instances {
        for &q in QUANTILES {
            let v = snap.quantile(q);
            out.push_str(name);
            labels.write_labels(out, Some(("quantile", &fmt_quantile(q))));
            let _ = writeln!(out, " {v}");
        }
        out.push_str(name);
        out.push_str("_sum");
        labels.write_labels(out, None);
        let _ = writeln!(out, " {}", snap.sum);

        out.push_str(name);
        out.push_str("_count");
        labels.write_labels(out, None);
        let _ = writeln!(out, " {}", snap.count);
    }
}

/// Write `# HELP` and `# TYPE` headers for one metric family.
fn write_help_type(out: &mut String, name: &str, desc: &MetricDescription, kind: &str) {
    if !desc.help.is_empty() {
        out.push_str("# HELP ");
        out.push_str(name);
        out.push(' ');
        write_escaped_help(out, &desc.help);
        out.push('\n');
    }
    out.push_str("# TYPE ");
    out.push_str(name);
    out.push(' ');
    out.push_str(kind);
    out.push('\n');
}

/// Escape a `# HELP` description per the Prometheus exposition format.
///
/// Only **backslash** and **newline** are escape-mandated on HELP lines —
/// embedded `"` is permitted verbatim. This deliberately differs from
/// label-value escaping ([`write_escaped`]). Every backslash is escaped,
/// including backslashes inside an existing escape sequence.
fn write_escaped_help(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
}

/// Format a quantile label value.
fn fmt_quantile(q: f64) -> String {
    // `f64`'s `Display` impl already drops trailing zeros and prints whole
    // values without a decimal (`0`, `0.5`, `0.999`, `1`), which is the
    // expected Prometheus quantile label format.
    format!("{q}")
}

/// Convert a `Key` from the recorder into a `(name, LabelKey)` tuple.
///
/// The metric name is sanitized to satisfy Prometheus's exposition grammar
/// (e.g. `network.connected_peers` → `network_connected_peers`). Sanitizing
/// here — rather than at registration — preserves the original `Key`
/// identity inside the registry while still producing valid output.
pub(crate) fn key_parts(key: &Key) -> (String, LabelKey) {
    (sanitize_metric_name(key.name()), LabelKey::from_labels(key.labels()))
}

#[cfg(test)]
mod tests {
    //! Pinned-input/output tests for the four exposition-format escapers /
    //! sanitizers. Pinning the expected outputs here surfaces any
    //! wire-format drift at test time rather than at scrape time, when a
    //! strict Prometheus parser would reject the output.
    use super::*;

    fn escape_label(value: &str) -> String {
        let mut out = String::new();
        write_escaped(&mut out, value);
        out
    }

    fn escape_help(value: &str) -> String {
        let mut out = String::new();
        write_escaped_help(&mut out, value);
        out
    }

    /// Pin metric-name sanitization for a handful of representative inputs
    /// (invalid leading char, invalid interior chars, valid colon, etc.).
    #[test]
    fn sanitize_metric_name_known_cases() {
        let cases = [
            ("*", "_"),
            ("\"", "_"),
            ("foo_bar", "foo_bar"),
            ("foo1_bar", "foo1_bar"),
            ("1foobar", "_foobar"),
            ("foo1:bar2", "foo1:bar2"),
            ("123", "_23"),
        ];
        for (input, expected) in cases {
            assert_eq!(sanitize_metric_name(input), expected, "input={input:?}");
        }
    }

    /// Pin label-key sanitization. Note label keys disallow `:` even though
    /// metric names allow it.
    #[test]
    fn sanitize_label_key_known_cases() {
        let cases = [
            ("*", "_"),
            ("\"", "_"),
            (":", "_"),
            ("foo_bar", "foo_bar"),
            ("1foobar", "_foobar"),
            ("__foobar", "__foobar"),
            ("foo1bar2", "foo1bar2"),
            ("123", "_23"),
        ];
        for (input, expected) in cases {
            assert_eq!(sanitize_label_key(input), expected, "input={input:?}");
        }
    }

    /// Pin label-value escaping. Label values escape backslash,
    /// double-quote, and newline. Every backslash is escaped, including
    /// those inside an existing escape sequence (so `\\` → `\\\\`).
    #[test]
    fn label_value_escape_known_cases() {
        let cases = [
            ("*", "*"),
            ("\"", "\\\""),
            ("\\", "\\\\"),
            ("\\\\", "\\\\\\\\"),
            ("\n", "\\n"),
            ("foo_bar", "foo_bar"),
            ("1foobar", "1foobar"),
        ];
        for (input, expected) in cases {
            assert_eq!(escape_label(input), expected, "input={input:?}");
        }
    }

    /// Pin HELP-line escaping. HELP lines escape backslash and newline
    /// only — embedded `"` is allowed. Every backslash is escaped,
    /// including those inside an existing escape sequence (so `\\` →
    /// `\\\\`).
    #[test]
    fn help_escape_known_cases() {
        let cases = [
            ("*", "*"),
            ("\"", "\""),
            ("\\", "\\\\"),
            ("\\\\", "\\\\\\\\"),
            ("\n", "\\n"),
            ("foo_bar", "foo_bar"),
            ("1foobar", "1foobar"),
        ];
        for (input, expected) in cases {
            assert_eq!(escape_help(input), expected, "input={input:?}");
        }
    }

    /// HELP escape MUST diverge from label-value escape on the `"`
    /// character — that's the whole point of having two escape paths.
    /// Pin both behaviors in one test so the asymmetry can't drift.
    #[test]
    fn help_escape_does_not_escape_double_quote_but_label_value_does() {
        assert_eq!(escape_label("a\"b"), "a\\\"b");
        assert_eq!(escape_help("a\"b"), "a\"b");
    }

    /// Empty input must not panic and must round-trip to empty for every
    /// sanitizer/escaper.
    #[test]
    fn all_sanitizers_handle_empty_input() {
        assert_eq!(sanitize_metric_name(""), "");
        assert_eq!(sanitize_label_key(""), "");
        assert_eq!(escape_label(""), "");
        assert_eq!(escape_help(""), "");
    }
}
