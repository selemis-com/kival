//! Shell completion generation command.

use std::io;

use clap::{Args, CommandFactory, ValueEnum};
use clap_complete::{
    Generator,
    Shell::{Bash, Fish, Zsh},
    generate,
};
use clap_complete_nushell::Nushell;
use clap_schema::schema_handler;
use eyre::Result;

use crate::{
    Cli,
    utils::{error::CliError, output::OutputMode},
};

/// Arguments for `kival completions`.
#[derive(Debug, Clone, Copy, Args)]
#[command(after_long_help = "\
Examples:
  kival completions bash > kival.bash
  kival completions zsh > _kival
  kival completions fish > kival.fish
  kival completions nushell > kival.nu

Install the generated script using your shell or package manager's completion mechanism.")]
pub struct CompletionsCommand {
    /// Shell to generate completions for.
    #[arg(value_enum)]
    pub shell: CompletionShell,
}

/// Shell completion generator supported by `kival`.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CompletionShell {
    /// Generate Bash completions.
    Bash,

    /// Generate Fish completions.
    Fish,

    /// Generate Zsh completions.
    Zsh,

    /// Generate Nushell completions.
    Nushell,
}

impl Generator for CompletionShell {
    fn file_name(&self, name: &str) -> String {
        match self {
            Self::Bash => Bash.file_name(name),
            Self::Fish => Fish.file_name(name),
            Self::Zsh => Zsh.file_name(name),
            Self::Nushell => Nushell.file_name(name),
        }
    }

    fn generate(&self, command: &clap::Command, output: &mut dyn io::Write) {
        match self {
            Self::Bash => Bash.generate(command, output),
            Self::Fish => Fish.generate(command, output),
            Self::Zsh => Zsh.generate(command, output),
            Self::Nushell => Nushell.generate(command, output),
        }
    }
}

#[schema_handler(run)]
impl CompletionsCommand {
    /// Run `kival completions`.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON output is requested.
    pub fn run(self, output: &OutputMode) -> Result<()> {
        if let OutputMode::Json { .. } = output {
            return Err(CliError::invalid_argument(
                "completions output is shell script text and does not support --json",
            )
            .into());
        }

        let mut cli = Cli::command();
        generate(self.shell, &mut cli, "kival", &mut io::stdout());
        Ok(())
    }
}
