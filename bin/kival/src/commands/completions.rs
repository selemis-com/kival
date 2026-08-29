//! Shell completion generation command.

use argx::{Args, Parser as _, completion::Shell};
use eyre::Result;

use crate::{Cli, utils::output::OutputMode};

/// Arguments for `kival completions`.
#[derive(Debug, Clone, Copy, Args)]
pub struct CompletionsCommand {
    /// Shell to generate completions for.
    #[argx(value_enum)]
    pub shell: Shell,
}

impl CompletionsCommand {
    /// Run `kival completions`.
    ///
    /// # Errors
    ///
    /// Returns an error if completion adapter generation fails.
    pub fn run(self, _output: &OutputMode) -> Result<()> {
        print!("{}", Cli::render_completion(self.shell)?);
        Ok(())
    }
}
