//! [`argx::Args`] for the data directory configuration.

use std::path::PathBuf;

use argx::Args;

use crate::dirs::{DataDirPath, MaybePlatformPath, PlatformPath};

/// Parameters for the data directory configuration.
#[derive(Debug, Args, PartialEq, Eq, Default, Clone)]
pub struct DatadirArgs {
    /// The path to the data dir for all Kival files and subdirectories.
    ///
    /// Defaults to the OS-specific data directory (XDG on all platforms; see
    /// <https://github.com/xdg-rs/dirs/issues/45>):
    ///
    /// - Linux: `$XDG_DATA_HOME/kival/` or `$HOME/.local/share/kival/`
    /// - macOS: `$HOME/.local/share/kival/`
    #[argx(short, long, default)]
    pub datadir: MaybePlatformPath<DataDirPath>,
}

impl DatadirArgs {
    /// Resolves the final datadir path, falling back to the OS default if unset.
    pub fn resolve_datadir(&self) -> PlatformPath<DataDirPath> {
        self.datadir.unwrap_or_default()
    }

    /// Resolves the final datadir as a [`PathBuf`].
    pub fn resolve_path(&self) -> PathBuf {
        self.resolve_datadir().into()
    }
}

#[cfg(test)]
mod tests {
    use argx::Parser;

    use super::*;

    /// A helper type to parse [`Args`] more easily.
    #[derive(Parser)]
    struct CommandParser<T: Args> {
        #[argx(flatten)]
        args: T,
    }

    #[test]
    fn parse_datadir_args() {
        let default_args = DatadirArgs::default();
        let args = CommandParser::<DatadirArgs>::parse_from(["kival"]).args;
        assert_eq!(args, default_args);
    }
}
