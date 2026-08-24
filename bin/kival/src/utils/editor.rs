//! External text-editor integration for projected object documents.

use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

use eyre::{Result, WrapErr};
use uuid::Uuid;

use crate::utils::error::CliError;

/// Object document edited in a temporary file that is retained until explicitly discarded.
#[derive(Debug)]
pub struct EditedDocument {
    /// Path to the temporary object document.
    path: PathBuf,
    /// Object document read after the editor exits.
    document: String,
}

impl EditedDocument {
    /// Returns the temporary object document path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the object document read after the editor exited.
    #[must_use]
    pub fn document(&self) -> &str {
        &self.document
    }

    /// Deletes the temporary file after its contents are no longer needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the temporary file cannot be removed.
    pub fn discard(self) -> Result<()> {
        fs::remove_file(&self.path).wrap_err_with(|| {
            format!("failed to remove temporary object document `{}`", self.path.display())
        })
    }
}

/// Parsed external editor command.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EditorCommand {
    /// Editor executable.
    program: OsString,
    /// Arguments configured before the object document path.
    args: Vec<OsString>,
}

impl EditorCommand {
    /// Converts this editor configuration into a process command for `path`.
    fn process(&self, path: &Path) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args).arg(path);
        command
    }
}

/// Opens `initial_document` in the configured editor and returns the resulting object document.
///
/// Editor precedence is `KIVAL_EDITOR`, `VISUAL`, then `EDITOR`. If none are configured, Kival
/// falls back to `vi` on Unix and `notepad` on Windows. The configured editor must remain running
/// until editing is complete; for example, use `KIVAL_EDITOR="code --wait"` with Visual Studio
/// Code.
///
/// The temporary file is retained until [`EditedDocument::discard`] is called. This lets callers
/// preserve local edits if a later server update fails.
///
/// # Errors
///
/// Returns an error when the editor configuration is invalid, the temporary file cannot be
/// created, the editor cannot be launched or exits unsuccessfully, or the resulting file cannot
/// be read as UTF-8.
pub fn edit_document(object_id: Uuid, initial_document: &str) -> Result<EditedDocument> {
    let editor = configured_editor()?;
    let path = temporary_body_path(object_id);
    edit_document_at(path, initial_document, |path| run_editor(&editor, path))
}

/// Runs an edit session at a caller-selected temporary path.
///
/// # Errors
///
/// Returns an error when the temporary file cannot be created, the editor callback fails, or the
/// resulting document cannot be read as UTF-8.
fn edit_document_at(
    path: PathBuf,
    initial_document: &str,
    edit: impl FnOnce(&Path) -> Result<()>,
) -> Result<EditedDocument> {
    write_private_file(&path, initial_document.as_bytes())?;

    if let Err(error) = edit(&path) {
        return Err(
            error.wrap_err(format!("editor failed; edited object remains at `{}`", path.display()))
        );
    }

    let document = fs::read_to_string(&path).wrap_err_with(|| {
        format!(
            "failed to read edited object document; temporary file remains at `{}`",
            path.display()
        )
    })?;

    Ok(EditedDocument { path, document })
}

/// Resolves the configured editor command using Kival's documented environment precedence.
///
/// # Errors
///
/// Returns an invalid-argument error if the configured editor command cannot be parsed.
fn configured_editor() -> Result<EditorCommand> {
    let mut selected = None;
    for name in ["KIVAL_EDITOR", "VISUAL", "EDITOR"] {
        match std::env::var(name) {
            Ok(value) if !value.trim().is_empty() => {
                selected = Some(value);
                break;
            }
            Ok(_) | Err(std::env::VarError::NotPresent) => {}
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(
                    CliError::invalid_argument(format!("{name} must contain valid UTF-8")).into()
                );
            }
        }
    }

    parse_editor_command(
        selected.as_deref().map_or_else(|| default_editor_str(), |command| command),
    )
}

/// Resolves editor configuration through an injected environment lookup.
///
/// # Errors
///
/// Returns an invalid-argument error if the selected editor command cannot be parsed.
#[cfg(test)]
fn configured_editor_from(lookup: impl Fn(&str) -> Option<String>) -> Result<EditorCommand> {
    let editor = ["KIVAL_EDITOR", "VISUAL", "EDITOR"]
        .into_iter()
        .find_map(|name| lookup(name).filter(|value| !value.trim().is_empty()))
        .unwrap_or_else(default_editor);

    parse_editor_command(&editor)
}

/// Returns the platform-default editor command when no editor environment variable is set.
#[cfg(test)]
fn default_editor() -> String {
    default_editor_str().to_owned()
}

/// Returns the platform-default editor command as a static string.
const fn default_editor_str() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "notepad"
    }

    #[cfg(not(target_os = "windows"))]
    {
        "vi"
    }
}

/// Parses a simple quoted command line into an editor executable and arguments.
///
/// Single and double quotes group whitespace. A backslash escapes quotes, backslashes, and
/// whitespace; before any other character it is preserved. Shell operators are not interpreted.
///
/// # Errors
///
/// Returns an invalid-argument error for an empty command or unmatched quote.
fn parse_editor_command(value: &str) -> Result<EditorCommand> {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Quote {
        /// Single-quoted text.
        Single,
        /// Double-quoted text.
        Double,
    }

    let mut parts = Vec::<OsString>::new();
    let mut current = String::new();
    let mut quote = None;
    let mut token_started = false;
    let mut chars = value.chars().peekable();

    while let Some(character) = chars.next() {
        match (quote, character) {
            (None, '\'') => {
                quote = Some(Quote::Single);
                token_started = true;
            }
            (None, '"') => {
                quote = Some(Quote::Double);
                token_started = true;
            }
            (Some(Quote::Single), '\'') | (Some(Quote::Double), '"') => {
                quote = None;
            }
            (None, character) if character.is_whitespace() => {
                if token_started {
                    parts.push(OsString::from(std::mem::take(&mut current)));
                    token_started = false;
                }
            }
            (_, '\\') => {
                if let Some(next) = chars.peek().copied() {
                    if next == '\\' || next == '\'' || next == '"' || next.is_whitespace() {
                        if let Some(escaped) = chars.next() {
                            current.push(escaped);
                        }
                    } else {
                        current.push('\\');
                    }
                } else {
                    current.push('\\');
                }
                token_started = true;
            }
            (_, character) => {
                current.push(character);
                token_started = true;
            }
        }
    }

    if quote.is_some() {
        return Err(CliError::invalid_argument("editor command contains an unmatched quote").into());
    }
    if token_started {
        parts.push(OsString::from(current));
    }

    let mut parts = parts.into_iter();
    let Some(program) = parts.next() else {
        return Err(CliError::invalid_argument("editor command must not be empty").into());
    };

    Ok(EditorCommand { program, args: parts.collect() })
}

/// Runs the configured editor and requires a successful exit status.
///
/// # Errors
///
/// Returns an error if the editor cannot be started or exits unsuccessfully.
fn run_editor(editor: &EditorCommand, path: &Path) -> Result<()> {
    let status = editor.process(path).status().wrap_err("failed to start configured editor")?;
    ensure_editor_success(&status)
}

/// Validates an editor process exit status.
///
/// # Errors
///
/// Returns an invalid-argument error when the editor exits unsuccessfully.
fn ensure_editor_success(status: &ExitStatus) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(CliError::invalid_argument(format!(
            "configured editor exited unsuccessfully ({status})"
        ))
        .into())
    }
}

/// Builds a unique temporary object document path for an object edit session.
fn temporary_body_path(object_id: Uuid) -> PathBuf {
    std::env::temp_dir().join(format!(
        "kival-{object_id}-{}-{}.md",
        std::process::id(),
        Uuid::now_v7()
    ))
}

/// Creates a private temporary file without replacing any existing path.
///
/// # Errors
///
/// Returns an error if the path cannot be created or written.
fn write_private_file(path: &Path, body: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path).wrap_err_with(|| {
        format!("failed to create temporary object document `{}`", path.display())
    })?;
    file.write_all(body)
        .wrap_err_with(|| format!("failed to write temporary object document `{}`", path.display()))
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::{OsStr, OsString},
        fs,
        io::Write,
        path::{Path, PathBuf},
    };

    use super::{
        EditorCommand, configured_editor_from, edit_document_at, ensure_editor_success,
        parse_editor_command,
    };
    use crate::utils::error::CliError;

    /// Verifies Kival-specific editor configuration wins over the standard editor variables.
    #[test]
    fn editor_configuration_uses_documented_precedence() {
        let editor = configured_editor_from(|name| match name {
            "KIVAL_EDITOR" => Some("kival-editor --wait".to_owned()),
            "VISUAL" => Some("visual-editor".to_owned()),
            "EDITOR" => Some("fallback-editor".to_owned()),
            _ => None,
        })
        .unwrap();

        assert_eq!(editor.program, OsString::from("kival-editor"));
        assert_eq!(editor.args, vec![OsString::from("--wait")]);

        let visual = configured_editor_from(|name| match name {
            "KIVAL_EDITOR" => Some("   ".to_owned()),
            "VISUAL" => Some("visual-editor --foreground".to_owned()),
            "EDITOR" => Some("fallback-editor".to_owned()),
            _ => None,
        })
        .unwrap();
        assert_eq!(visual.program, OsString::from("visual-editor"));
        assert_eq!(visual.args, vec![OsString::from("--foreground")]);
    }

    /// Verifies editor command parsing supports common executable-and-argument forms.
    #[test]
    fn editor_command_parses_arguments_and_quotes() {
        assert_eq!(
            parse_editor_command("code --wait").unwrap(),
            EditorCommand { program: OsString::from("code"), args: vec![OsString::from("--wait")] }
        );
        assert_eq!(
            parse_editor_command("\"/opt/My Editor/bin/editor\" --wait 'two words'").unwrap(),
            EditorCommand {
                program: OsString::from("/opt/My Editor/bin/editor"),
                args: vec![OsString::from("--wait"), OsString::from("two words")],
            }
        );
    }

    /// Verifies empty arguments and ordinary backslashes survive command parsing.
    #[test]
    fn editor_command_preserves_empty_arguments_and_backslashes() {
        assert_eq!(
            parse_editor_command("editor '' C:\\Users\\name").unwrap(),
            EditorCommand {
                program: OsString::from("editor"),
                args: vec![OsString::from(""), OsString::from("C:\\Users\\name")],
            }
        );
    }

    /// Verifies the Markdown path is appended after configured editor arguments.
    #[test]
    fn editor_process_appends_markdown_path() {
        let editor = parse_editor_command("code --wait").unwrap();
        let path = Path::new("note.md");
        let command = editor.process(path);

        assert_eq!(command.get_program(), OsStr::new("code"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("--wait"), OsStr::new("note.md")]
        );
    }

    /// Verifies an unsuccessful editor exit is rejected.
    #[cfg(unix)]
    #[test]
    fn editor_exit_status_must_be_successful() {
        use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

        let status = ExitStatus::from_raw(7 << 8);
        assert!(ensure_editor_success(&status).is_err());
    }

    /// Verifies malformed editor commands are rejected before a temporary file is created.
    #[test]
    fn editor_command_rejects_empty_and_unmatched_quotes() {
        assert!(parse_editor_command("   ").is_err());
        assert!(parse_editor_command("editor 'unfinished").is_err());
        assert!(parse_editor_command("editor \"unfinished").is_err());
    }

    /// Verifies an editor callback can replace the body exactly.
    #[test]
    fn edit_document_reads_changed_content_exactly() {
        let path = temp_path("changed");
        let _ = fs::remove_file(&path);
        let expected = "# Changed\r\n\r\nbody without final newline";

        let edited = edit_document_at(path.clone(), "original", |path| {
            assert_eq!(fs::read_to_string(path)?, "original");
            fs::write(path, expected.as_bytes())?;
            Ok(())
        })
        .unwrap();

        assert_eq!(edited.document(), expected);
        assert_eq!(edited.path(), path);
        edited.discard().unwrap();
        assert!(!path.exists());
    }

    /// Verifies an unchanged editor session preserves the original body exactly.
    #[test]
    fn edit_document_returns_unchanged_content() {
        let path = temp_path("unchanged");
        let _ = fs::remove_file(&path);
        let original = "body without final newline";

        let edited = edit_document_at(path.clone(), original, |_path| Ok(())).unwrap();

        assert_eq!(edited.document(), original);
        edited.discard().unwrap();
        assert!(!path.exists());
    }

    /// Verifies cleanup failures are reported instead of silently leaving Markdown behind.
    #[test]
    fn edited_document_reports_cleanup_failure() {
        let path = temp_path("cleanup-failure");
        let _ = fs::remove_file(&path);
        let edited = edit_document_at(path.clone(), "body", |_path| Ok(())).unwrap();
        fs::remove_file(&path).unwrap();

        assert!(edited.discard().is_err());
    }

    /// Verifies dropping an edit result without discarding it retains the recovery file.
    #[test]
    fn edited_document_is_not_deleted_implicitly() {
        let path = temp_path("retained");
        let _ = fs::remove_file(&path);
        let edited = edit_document_at(path.clone(), "body", |_path| Ok(())).unwrap();

        drop(edited);

        assert_eq!(fs::read_to_string(&path).unwrap(), "body");
        let _ = fs::remove_file(path);
    }

    /// Verifies an existing temporary path is never truncated or replaced.
    #[test]
    fn edit_body_refuses_to_replace_existing_path() {
        let path = temp_path("existing");
        fs::write(&path, b"existing").unwrap();

        assert!(edit_document_at(path.clone(), "new", |_path| Ok(())).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"existing");
        let _ = fs::remove_file(path);
    }

    /// Verifies editor failures retain the temporary object document for recovery.
    #[test]
    fn edit_body_retains_file_when_editor_fails() {
        let path = temp_path("editor-failure");
        let _ = fs::remove_file(&path);

        let error = edit_document_at(path.clone(), "original", |_path| {
            Err(CliError::invalid_argument("editor failed intentionally").into())
        })
        .unwrap_err();

        assert!(error.to_string().contains(&path.display().to_string()));
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        let _ = fs::remove_file(path);
    }

    /// Verifies invalid UTF-8 produced by an editor leaves the recovery file in place.
    #[test]
    fn edit_body_retains_file_when_result_is_not_utf8() {
        let path = temp_path("invalid-utf8");
        let _ = fs::remove_file(&path);

        let error = edit_document_at(path.clone(), "original", |path| {
            let mut file = fs::File::create(path)?;
            file.write_all(&[0xff, 0xfe])?;
            Ok(())
        })
        .unwrap_err();

        assert!(error.to_string().contains(&path.display().to_string()));
        assert!(path.exists());
        let _ = fs::remove_file(path);
    }

    /// Verifies temporary edit files are private on Unix platforms.
    #[cfg(unix)]
    #[test]
    fn edit_body_creates_private_file() {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_path("mode");
        let _ = fs::remove_file(&path);
        let edited = edit_document_at(path.clone(), "body", |_path| Ok(())).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;

        assert_eq!(mode, 0o600);
        edited.discard().unwrap();
    }

    /// Builds a unique-enough path for editor utility tests.
    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kival-editor-{name}-{}", std::process::id()))
    }
}
