//! Layered configuration resolution.

/// A partial configuration layer that can be merged with lower-precedence values.
pub trait ConfigLayer {
    /// Merges `self` over `lower`, preserving values from `self` when present.
    fn merge(self, lower: Self) -> Self;
}

/// Builds a configuration layer with every declared default materialized.
///
/// This is distinct from [`Default`]: for layered configuration types, `Default`
/// represents an empty layer where every field is unset. `with_defaults` returns
/// a concrete layer where each field is populated with its fallback value, making
/// it suitable for writing example/default config files or displaying resolved
/// defaults to users.
pub trait ConfigDefaults {
    /// Returns a config layer with all defaults applied.
    fn with_defaults() -> Self;
}

/// Defines an optional layered config type with generated merge and resolved accessors.
///
/// Fields are stored as `Option<T>` so each layer can represent only the values
/// it provides. The `= default` expression is not written into the layer; it is
/// used only by the generated accessor after CLI/env and file layers have been
/// merged.
#[macro_export]
macro_rules! config {
    (
        $(#[$struct_meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident: $ty:ty = $default:expr
            ),* $(,)?
        }
    ) => {
        $(#[$struct_meta])*
        #[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
        #[serde(default)]
        $vis struct $name {
            $(
                $(#[$field_meta])*
                $field_vis $field: Option<$ty>,
            )*
        }

        impl $crate::ConfigLayer for $name {
            fn merge(self, lower: Self) -> Self {
                let _ = lower;

                Self {
                    $($field: self.$field.or(lower.$field),)*
                }
            }
        }

        impl $crate::ConfigDefaults for $name {
            fn with_defaults() -> Self {
                Self {
                    $($field: Some($default),)*
                }
            }
        }

        impl $name {
            $(
                $(#[$field_meta])*
                pub fn $field(&self) -> $ty
                where
                    $ty: Clone,
                {
                    self.$field.clone().unwrap_or_else(|| $default)
                }
            )*
        }
    };
}
