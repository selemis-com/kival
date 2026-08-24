//! Metric identity: name + labels with a precomputed hash.

use std::{
    borrow::Cow,
    hash::{Hash, Hasher},
};

use crate::label::Label;

/// `const`-capable FNV-1a, run with per-stream seeds (see `SEED_*` below) to
/// precompute a stable `u64` per `Key`. Cheap on short metric-name / label
/// strings and usable in `const` so static keys built from string literals
/// get their hashes computed at compile time.
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// Compute FNV-1a over `bytes`, starting from a caller-provided seed.
const fn fnv1a_seed(bytes: &[u8], seed: u64) -> u64 {
    let mut hash = seed;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hash
}

/// Combine two `u64` hashes into one.
///
/// Uses the classic "xor + shift + multiply" mixer so that label order does
/// not affect the final hash (we sum hashed labels) but distinct label sets
/// still produce distinct combined values with very high probability.
#[inline]
const fn mix(a: u64, b: u64) -> u64 {
    let mut x = a ^ b.rotate_left(31);
    x = x.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    x ^= x >> 33;
    x
}

/// A metric name.
///
/// Wraps a `Cow<'static, str>` so that names built from string literals do
/// not allocate but owned `String` names are also supported.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyName(Cow<'static, str>);

impl KeyName {
    /// Construct from any string-like value.
    pub fn new<S: Into<Cow<'static, str>>>(name: S) -> Self {
        Self(name.into())
    }

    /// Construct from a `&'static str` in a `const` context.
    pub const fn from_const_str(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    /// Borrow as `&str`.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<S: Into<Cow<'static, str>>> From<S> for KeyName {
    fn from(s: S) -> Self {
        Self(s.into())
    }
}

impl AsRef<str> for KeyName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A metric identifier consisting of a name, an optional set of labels, and a
/// precomputed `u64` hash.
///
/// The hash is computed eagerly so that registry lookups during the hot path
/// of `counter!`/`gauge!`/`histogram!` are O(1) regardless of the length of
/// the name or the number of labels. Equality short-circuits on the hash.
#[derive(Clone, Debug)]
pub struct Key {
    /// Metric name.
    name: KeyName,
    /// Canonicalized label set.
    labels: Cow<'static, [Label]>,
    /// Precomputed hash over `name` and `labels`.
    hash: u64,
}

// Independent seeds for the three byte streams that make up a key. Using
// distinct seeds prevents hash collisions between, e.g., a name "foo" and a
// label whose key happens to be "foo".
/// Seed for the metric-name byte stream.
const SEED_NAME: u64 = 0x1111_1111_1111_1111;
/// Seed for the label-key byte stream.
const SEED_LABEL_KEY: u64 = 0x2222_2222_2222_2222;
/// Seed for the label-value byte stream.
const SEED_LABEL_VALUE: u64 = 0x3333_3333_3333_3333;

#[inline]
/// Hash one label into an order-independent label-set component.
fn hash_label(label: &Label) -> u64 {
    let k = fnv1a_seed(label.key().as_bytes(), SEED_LABEL_KEY);
    let v = fnv1a_seed(label.value().as_bytes(), SEED_LABEL_VALUE);
    mix(k, v)
}

/// Compute the precomputed hash for a metric name and label set.
fn compute_hash(name: &str, labels: &[Label]) -> u64 {
    let h = fnv1a_seed(name.as_bytes(), SEED_NAME);
    // Sum of label hashes is order-independent (so two keys differing only in
    // label order are still equal-by-hash); the per-label `mix` combines key
    // and value distinctly so swapping key<->value changes the hash.
    let mut acc: u64 = 0;
    for l in labels {
        acc = acc.wrapping_add(hash_label(l));
    }
    mix(h, acc)
}

impl Key {
    /// Construct a key from a name with no labels.
    pub fn from_name<N: Into<KeyName>>(name: N) -> Self {
        let name = name.into();
        let hash = compute_hash(name.as_str(), &[]);
        Self { name, labels: Cow::Borrowed(&[]), hash }
    }

    /// Construct a key from a name and an owned label vector.
    ///
    /// Labels are canonicalized by sorting on `(key, value)` so that two keys
    /// constructed from the same labels in different orders compare equal.
    /// We canonicalize because our per-label hash sum is order-independent
    /// and we need `eq` and `hash` to agree.
    pub fn from_parts<N: Into<KeyName>, L: Into<Vec<Label>>>(name: N, labels: L) -> Self {
        let name = name.into();
        let mut labels: Vec<Label> = labels.into();
        labels.sort_by(|a, b| (a.key(), a.value()).cmp(&(b.key(), b.value())));
        let hash = compute_hash(name.as_str(), &labels);
        Self { name, labels: Cow::Owned(labels), hash }
    }

    /// Construct a key from a `&'static str` name. Allocation-free for the
    /// name; used by the metric macros via a per-callsite `OnceLock` so the
    /// hash is computed exactly once per process for static metric names.
    pub fn from_static_name(name: &'static str) -> Self {
        let hash = compute_hash(name, &[]);
        Self { name: KeyName::from_const_str(name), labels: Cow::Borrowed(&[]), hash }
    }

    /// Metric name.
    #[inline]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Metric labels.
    #[inline]
    pub fn labels(&self) -> &[Label] {
        &self.labels
    }

    /// Precomputed hash for this key.
    #[inline]
    pub const fn get_hash(&self) -> u64 {
        self.hash
    }
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        // Hash mismatch implies inequality; only compare the (more expensive)
        // name and label slices when hashes collide.
        self.hash == other.hash && self.name == other.name && self.labels == other.labels
    }
}

impl Eq for Key {}

impl Hash for Key {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Feed the precomputed hash directly so HashMap lookups don't re-walk
        // the name and label byte streams.
        state.write_u64(self.hash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_label_order_insensitive() {
        let a = Key::from_parts("m", vec![Label::new("k1", "v1"), Label::new("k2", "v2")]);
        let b = Key::from_parts("m", vec![Label::new("k2", "v2"), Label::new("k1", "v1")]);
        // Hash is order-independent and labels are canonicalized to a sorted
        // order, so the keys must be fully equal (not just hash-equal).
        assert_eq!(a.get_hash(), b.get_hash());
        assert_eq!(a, b);
    }

    #[test]
    fn distinct_names_distinct_hashes() {
        let a = Key::from_name("foo");
        let b = Key::from_name("bar");
        assert_ne!(a.get_hash(), b.get_hash());
    }

    #[test]
    fn label_key_value_swap_changes_hash() {
        let a = Key::from_parts("m", vec![Label::new("k", "v")]);
        let b = Key::from_parts("m", vec![Label::new("v", "k")]);
        assert_ne!(a.get_hash(), b.get_hash());
    }

    /// `KeyName` equality holds across every constructor — `new`,
    /// `from_const_str`, and the `From<&str>` / `From<String>` impls all
    /// produce the same value for the same string.
    #[test]
    fn key_name_equality_across_constructors() {
        let a = KeyName::new("metric");
        let b = KeyName::from_const_str("metric");
        let c: KeyName = "metric".into();
        let d: KeyName = String::from("metric").into();
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_eq!(a, d);
        assert_eq!(b.as_str(), "metric");
        // Distinct names must NOT compare equal.
        assert_ne!(a, KeyName::new("other"));
    }

    /// Pin: `Key`'s `Hash` impl writes its precomputed `get_hash()` value
    /// into the `Hasher` (rather than re-hashing name + labels). Without
    /// this property, registry lookups via `HashMap<Key, _>` would compute
    /// a different hash than `Key::get_hash()`, which would silently break
    /// any callsite that mixes the two paths.
    #[test]
    fn key_hash_impl_agrees_with_get_hash() {
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };

        let key = Key::from_parts("metric", vec![Label::new("k", "v")]);
        let precomputed = key.get_hash();

        // Drive the same `Hasher` through `Hash::hash` and compare.
        let mut h = DefaultHasher::new();
        key.hash(&mut h);
        let via_hash_trait = h.finish();

        // Two independent `DefaultHasher`s fed the same single u64 must
        // produce the same finish() value, so equal precomputed hashes
        // must yield equal Hash-trait outputs.
        let mut control = DefaultHasher::new();
        precomputed.hash(&mut control);
        assert_eq!(via_hash_trait, control.finish());

        // Two equal keys must hash equal under the trait — the property
        // `HashMap<Key, _>` actually depends on.
        let key2 = Key::from_parts("metric", vec![Label::new("k", "v")]);
        let mut h2 = DefaultHasher::new();
        key2.hash(&mut h2);
        assert_eq!(via_hash_trait, h2.finish());
    }
}
