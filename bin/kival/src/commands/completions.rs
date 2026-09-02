//! Shell completion generation command.

use argx::{Args, Parser as _, argx, completion::Shell};

use crate::{
    Cli,
    utils::{
        error::{CommandError, command_error_codes},
        output::OutputMode,
    },
};

command_error_codes! {
    pub(crate) enum CompletionsErrorCode {
        InvalidArgument => ("invalid.argument", InvalidArgument),
        Internal => ("internal", Internal),
    }
}

/// Error returned by the corresponding command handler.
type CompletionsError = CommandError<CompletionsErrorCode>;

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
    pub(crate) fn run(self, _output: &OutputMode) -> Result<(), CompletionsError> {
        let script =
            Cli::render_completion(self.shell).map_err(|_| CompletionsError::internal())?;
        print!("{script}");
        Ok(())
    }
}
