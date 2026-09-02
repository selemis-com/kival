//! Shared local file and body-input helpers for object commands.

use std::{
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use eyre::{Result, WrapErr};

use super::ObjectError;
use crate::utils::input::{InputPath, read_text_input};

/// Resolves an inline body, standard input, or body file into Markdown text.
///
/// # Errors
///
/// Returns an input-read error when standard input or the selected body file cannot be read as
/// UTF-8 text.
pub(super) fn resolve_body(
    body: Option<String>,
    body_file: Option<PathBuf>,
) -> Result<Option<String>> {
    match (body, body_file) {
        (Some(body), None) if body == "-" => read_text_input(InputPath::Stdin).map(Some),
        (Some(body), None) => Ok(Some(body)),
        (None, Some(path)) if path == Path::new("-") => Err(ObjectError::invalid_argument(
            "`--body-file -` is not supported; use `--body -` to read from standard input",
        )
        .into()),
        (None, Some(path)) => read_text_input(InputPath::File(path)).map(Some),
        (None, None) => Ok(None),
        (Some(_), Some(_)) => {
            Err(ObjectError::invalid_argument("--body and --body-file cannot be combined").into())
        }
    }
}

/// Rejects an existing output path unless overwriting was explicitly requested.
///
/// # Errors
///
/// Returns an invalid-argument error when `path` already exists and `force` is false.
pub(super) fn ensure_output_available(path: &Path, force: bool) -> Result<()> {
    if !force && path.exists() {
        return Err(output_exists_error(path).into());
    }
    Ok(())
}

/// Builds the stable human-facing error for a protected output path.
fn output_exists_error(path: &Path) -> ObjectError {
    ObjectError::invalid_argument(format!(
        "output file `{}` already exists; pass --force to overwrite it",
        path.display(),
    ))
}

/// Writes bytes to a local output file without overwriting an existing file unless requested.
///
/// # Errors
///
/// Returns an invalid-argument error when the destination appears concurrently without `force`,
/// or an I/O error when the file cannot be created or written.
pub(super) fn write_output_file(path: &Path, bytes: &[u8], force: bool) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }

    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if !force && error.kind() == ErrorKind::AlreadyExists => {
            return Err(output_exists_error(path).into());
        }
        Err(error) => {
            return Err(error)
                .wrap_err_with(|| format!("failed to open output file `{}`", path.display()));
        }
    };

    file.write_all(bytes)
        .wrap_err_with(|| format!("failed to write output file `{}`", path.display()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::utils::error::FailureCode;

    /// Verifies output-file creation preserves content exactly.
    #[test]
    fn write_output_file_creates_exact_content() {
        let path = temp_content_path("output-new");
        let _ = fs::remove_file(&path);
        let expected = b"# Heading\r\nbody without final newline";

        write_output_file(&path, expected, false).unwrap();

        assert_eq!(fs::read(&path).unwrap(), expected);
        let _ = fs::remove_file(path);
    }

    /// Verifies an existing output file is preserved unless overwrite is explicit.
    #[test]
    fn write_output_file_refuses_to_overwrite_by_default() {
        let path = temp_content_path("output-protected");
        fs::write(&path, b"original").unwrap();

        let error = write_output_file(&path, b"replacement", false)
            .unwrap_err()
            .downcast::<ObjectError>()
            .unwrap();

        assert_eq!(error.code, FailureCode::InvalidArgument);
        assert_eq!(fs::read(&path).unwrap(), b"original");
        let _ = fs::remove_file(path);
    }

    /// Verifies forced output truncates and replaces an existing file.
    #[test]
    fn write_output_file_force_replaces_existing_content() {
        let path = temp_content_path("output-force");
        fs::write(&path, b"a much longer original value").unwrap();

        write_output_file(&path, b"new", true).unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"new");
        let _ = fs::remove_file(path);
    }

    /// Builds a unique-enough temporary path for object-body unit tests.
    fn temp_content_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kival-object-body-{name}-{}", std::process::id()))
    }
}
