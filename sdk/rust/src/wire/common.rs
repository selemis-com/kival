//! Common wire protocol types.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use time::{OffsetDateTime, serde::rfc3339};

/// Actor-relative state returned after changing a personal favorite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FavoriteState {
    /// Whether the resource is currently favorited by the authenticated user.
    pub favorited: bool,
}

/// Actor-relative state returned after changing a personal pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PinState {
    /// Whether the resource is currently pinned by the authenticated user.
    pub pinned: bool,

    /// Time at which the resource entered its current pinned position.
    #[schemars(with = "Option<String>")]
    #[serde(with = "rfc3339::option")]
    pub pinned_at: Option<OffsetDateTime>,
}

pub use kival_types::{ArchiveListStatus, ArchiveStatus};

/// Tri-state PATCH field that distinguishes omitted fields from explicit JSON null.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PatchField<T> {
    /// Field was omitted from the request body.
    #[default]
    Missing,
    /// Field was present as JSON null.
    Null,
    /// Field was present with a concrete value.
    Value(T),
}

impl<T> PatchField<T> {
    /// Returns true when the field was omitted.
    #[must_use]
    pub const fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }

    /// Returns true when the field was present as either null or a concrete value.
    #[must_use]
    pub const fn is_present(&self) -> bool {
        !self.is_missing()
    }

    /// Converts a present patch field into an optional value.
    pub fn into_option(self) -> Option<T> {
        match self {
            Self::Missing | Self::Null => None,
            Self::Value(value) => Some(value),
        }
    }
}

impl PatchField<String> {
    /// Converts a present nullable string patch field into a trimmed optional value.
    pub fn into_trimmed_option(self) -> Option<String> {
        self.into_option().map(|value| value.trim().to_owned())
    }
}

impl<'de, T> Deserialize<'de> for PatchField<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<T>::deserialize(deserializer)
            .map(|value| value.map_or_else(|| Self::Null, Self::Value))
    }
}

impl<T> Serialize for PatchField<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Missing => serializer.serialize_unit(),
            Self::Null => serializer.serialize_none(),
            Self::Value(value) => value.serialize(serializer),
        }
    }
}
