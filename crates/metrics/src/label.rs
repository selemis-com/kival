//! Key/value label attached to a metric.

use std::borrow::Cow;

/// A single key/value pair attached to a metric.
///
/// Both halves are `Cow<'static, str>`, so labels built from string literals
/// avoid allocation and labels built from owned strings are supported as well.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Label {
    /// Label key.
    key: Cow<'static, str>,
    /// Label value.
    value: Cow<'static, str>,
}

impl Label {
    /// Construct a label from any pair of string-like values.
    pub fn new<K, V>(key: K, value: V) -> Self
    where
        K: Into<Cow<'static, str>>,
        V: Into<Cow<'static, str>>,
    {
        Self { key: key.into(), value: value.into() }
    }

    /// Label key.
    #[inline]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Label value.
    #[inline]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl<K, V> From<(K, V)> for Label
where
    K: Into<Cow<'static, str>>,
    V: Into<Cow<'static, str>>,
{
    fn from((k, v): (K, V)) -> Self {
        Self::new(k, v)
    }
}

impl<K, V> From<&(K, V)> for Label
where
    K: Clone + Into<Cow<'static, str>>,
    V: Clone + Into<Cow<'static, str>>,
{
    fn from((k, v): &(K, V)) -> Self {
        Self::new(k.clone(), v.clone())
    }
}

/// Conversion into a `Vec<Label>`, used by the metric macros.
pub trait IntoLabels {
    /// Consume `self` into a fully-owned label vector.
    fn into_labels(self) -> Vec<Label>;
}

impl IntoLabels for Vec<Label> {
    fn into_labels(self) -> Vec<Label> {
        self
    }
}

impl IntoLabels for &[Label] {
    fn into_labels(self) -> Vec<Label> {
        self.to_vec()
    }
}

impl<const N: usize> IntoLabels for [Label; N] {
    fn into_labels(self) -> Vec<Label> {
        self.to_vec()
    }
}

// Tuple-of-strings impls below: macro callsites of the form
// `gauge!("info", &labels)` where `labels` is a `[(&str, &str); N]` or a
// `[(&str, String); N]` go through these conversions.

impl<K, V> IntoLabels for &[(K, V)]
where
    K: Clone + Into<Cow<'static, str>>,
    V: Clone + Into<Cow<'static, str>>,
{
    fn into_labels(self) -> Vec<Label> {
        self.iter().map(|(k, v)| Label::new(k.clone(), v.clone())).collect()
    }
}

impl<K, V, const N: usize> IntoLabels for &[(K, V); N]
where
    K: Clone + Into<Cow<'static, str>>,
    V: Clone + Into<Cow<'static, str>>,
{
    fn into_labels(self) -> Vec<Label> {
        self.iter().map(|(k, v)| Label::new(k.clone(), v.clone())).collect()
    }
}

impl<K, V, const N: usize> IntoLabels for [(K, V); N]
where
    K: Into<Cow<'static, str>>,
    V: Into<Cow<'static, str>>,
{
    fn into_labels(self) -> Vec<Label> {
        self.into_iter().map(|(k, v)| Label::new(k, v)).collect()
    }
}

impl<K, V> IntoLabels for Vec<(K, V)>
where
    K: Into<Cow<'static, str>>,
    V: Into<Cow<'static, str>>,
{
    fn into_labels(self) -> Vec<Label> {
        self.into_iter().map(|(k, v)| Label::new(k, v)).collect()
    }
}

#[cfg(test)]
mod tests {
    //! Exercise every `IntoLabels` impl once. Real callsites use *all* of
    //! these shapes (`Vec<Label>`, fixed-size arrays of tuples, borrowed
    //! slices, etc.), so any of these regressing silently drops a label at
    //! the macro expansion site.
    use super::*;

    fn expected() -> Vec<Label> {
        vec![Label::new("k1", "v1"), Label::new("k2", "v2")]
    }

    #[test]
    fn vec_of_labels_passthrough() {
        let v = vec![Label::new("k1", "v1"), Label::new("k2", "v2")];
        assert_eq!(v.into_labels(), expected());
    }

    #[test]
    fn borrowed_slice_of_labels() {
        let arr = [Label::new("k1", "v1"), Label::new("k2", "v2")];
        let s: &[Label] = &arr;
        assert_eq!(s.into_labels(), expected());
    }

    #[test]
    fn owned_array_of_labels() {
        let arr = [Label::new("k1", "v1"), Label::new("k2", "v2")];
        assert_eq!(arr.into_labels(), expected());
    }

    #[test]
    fn borrowed_slice_of_str_pairs() {
        let pairs = [("k1", "v1"), ("k2", "v2")];
        let s: &[(&str, &str)] = &pairs;
        assert_eq!(s.into_labels(), expected());
    }

    #[test]
    fn borrowed_array_of_str_pairs() {
        let arr = [("k1", "v1"), ("k2", "v2")];
        assert_eq!((&arr).into_labels(), expected());
    }

    #[test]
    fn owned_array_of_str_pairs() {
        let arr = [("k1", "v1"), ("k2", "v2")];
        assert_eq!(arr.into_labels(), expected());
    }

    #[test]
    fn vec_of_str_pairs() {
        let v: Vec<(&str, &str)> = vec![("k1", "v1"), ("k2", "v2")];
        assert_eq!(v.into_labels(), expected());
    }

    /// Specifically cover the `(K, V) where V: String` shape. Catches a
    /// regression that silently loses owned-string label values at the
    /// conversion boundary.
    #[test]
    fn array_of_str_string_pairs_keeps_owned_values() {
        let arr = [("k1", String::from("v1")), ("k2", String::from("v2"))];
        assert_eq!(arr.into_labels(), expected());
    }
}
