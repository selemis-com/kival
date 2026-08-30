//! Shell completion generation command.

use argx::{Args, Parser as _, argx, completion::Shell};

use crate::{
    Cli,
    utils::{error::CliError, output::OutputMode},
};

/// Arguments for `kival completions`.
///
/// Examples:
///
/// `kival completions bash > kival.bash`
///
/// `kival completions zsh > _kival`
///
/// `kival completions fish > kival.fish`
///
/// `kival completions nushell > kival.nu`
///
/// Install the generated script using your shell or package manager's completion mechanism.
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
    pub fn run(self, _output: &OutputMode) -> Result<(), CliError> {
        let script = Cli::render_completion(self.shell).map_err(|_| CliError::internal())?;
        print!("{script}");
        Ok(())
    }
}
