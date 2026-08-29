//! Shell completion generation command.

use argx::{argx, Args, Parser as _, completion::Shell};

use crate::utils::error::CliError;
use crate::{Cli, utils::output::OutputMode};

/// Arguments for `kival completions`.
#[derive(Debug, Clone, Copy, Args)]
pub struct CompletionsCommand {
    /// Shell to generate completions for.
    #[argx(value_enum)]
    pub shell: Shell,
}

#[argx(handler = run)]
impl CompletionsCommand {
    /// Run `kival completions`.
    ///
    /// # Errors
    ///
    /// Returns an error if completion adapter generation fails.
    pub fn run(self, _output: &OutputMode) -> std::result::Result<(), CliError> {
        print!("{}", Cli::render_completion(self.shell)?);
        Ok(())
    }
}
