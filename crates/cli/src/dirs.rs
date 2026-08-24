//! Directory paths for data, configuration, and logs.

use std::{
    convert::Infallible,
    env,
    ffi::OsString,
    fmt::{Debug, Display, Formatter},
    marker::PhantomData,
    path::{Path, PathBuf},
    str::FromStr,
};

/// Resolves `$env_var` (if absolute) or `$HOME/<fallback>`, then appends `kival`.
fn xdg_dir(env_var: &str, fallback: &str) -> Option<PathBuf> {
    env::var_os(env_var)
        .and_then(is_absolute_path)
        .or_else(|| home_dir().map(|h| h.join(fallback)))
        .map(|root| root.join("kival"))
}

/// Path to the Kival data directory: `$XDG_DATA_HOME/kival` or `~/.local/share/kival`.
pub fn data_dir() -> Option<PathBuf> {
    xdg_dir("XDG_DATA_HOME", ".local/share")
}

/// Path to the Kival cache directory: `$XDG_CACHE_HOME/kival` or `~/.cache/kival`.
pub fn cache_dir() -> Option<PathBuf> {
    xdg_dir("XDG_CACHE_HOME", ".cache")
}

/// Path to the Kival logs directory: `<cache_dir>/logs`.
pub fn logs_dir() -> Option<PathBuf> {
    cache_dir().map(|root| root.join("logs"))
}

/// User's home directory, from `$HOME`.
fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").filter(|s| !s.is_empty()).map(PathBuf::from)
}

/// Returns `path` as a `PathBuf` if it is absolute.
fn is_absolute_path(path: OsString) -> Option<PathBuf> {
    Some(PathBuf::from(path)).filter(|p| p.is_absolute())
}

/// A marker type that resolves to a standard XDG path.
pub trait XdgPath {
    /// Resolve the standard path.
    fn resolve() -> Option<PathBuf>;
}

/// Marker type for the Kival data directory.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct DataDirPath;

impl XdgPath for DataDirPath {
    fn resolve() -> Option<PathBuf> {
        data_dir()
    }
}

/// Marker type for the Kival logs directory.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct LogsDir;

impl XdgPath for LogsDir {
    fn resolve() -> Option<PathBuf> {
        logs_dir()
    }
}

/// A wrapper type that either parses a user-given path or defaults to an OS-specific path.
///
/// The [`FromStr`] implementation parses a string into a path.
#[derive(Debug, PartialEq, Eq)]
pub struct PlatformPath<D>(PathBuf, PhantomData<D>);

impl<D> PlatformPath<D> {
    /// Returns the path joined with another path.
    pub fn join<P: AsRef<Path>>(&self, path: P) -> Self {
        Self(self.0.join(path), PhantomData)
    }
}

impl<D> Clone for PlatformPath<D> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

impl<D> Display for PlatformPath<D> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

impl<D: XdgPath> Default for PlatformPath<D> {
    fn default() -> Self {
        Self(D::resolve().expect("Could not resolve default path. Set one manually."), PhantomData)
    }
}

impl<D> FromStr for PlatformPath<D> {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(PathBuf::from(s), PhantomData))
    }
}

impl<D> AsRef<Path> for PlatformPath<D> {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl<D> From<PlatformPath<D>> for PathBuf {
    fn from(value: PlatformPath<D>) -> Self {
        value.0
    }
}

/// An optional wrapper type around [`PlatformPath`].
///
/// This is useful for when a path is optional, such as the `--data-dir` flag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaybePlatformPath<D>(Option<PlatformPath<D>>);

impl<D: XdgPath> MaybePlatformPath<D> {
    /// Returns true if a custom path is set.
    pub const fn is_some(&self) -> bool {
        self.0.is_some()
    }

    /// Returns the path if it is set, otherwise returns `None`.
    pub fn as_ref(&self) -> Option<&Path> {
        self.0.as_ref().map(PlatformPath::as_ref)
    }

    /// Returns the path if it is set, otherwise returns the default path.
    pub fn unwrap_or_default(&self) -> PlatformPath<D> {
        self.0.clone().unwrap_or_default()
    }
}

impl<D: XdgPath> Display for MaybePlatformPath<D> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(path) => path.fmt(f),
            // Workaround for Clap's `default_value_t`, which computes the default via
            // `Default -> Display -> FromStr`.
            None => f.write_str("default"),
        }
    }
}

impl<D> FromStr for MaybePlatformPath<D> {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(match s {
            // "default" round-trips with `Display` (see above).
            "default" => None,
            _ => Some(PlatformPath::from_str(s)?),
        }))
    }
}

impl<D> From<PathBuf> for MaybePlatformPath<D> {
    fn from(path: PathBuf) -> Self {
        Self(Some(PlatformPath(path, PhantomData)))
    }
}
