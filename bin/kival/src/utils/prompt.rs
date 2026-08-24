//! Interactive prompt helpers.

use std::io::{self, BufRead};

use eyre::{Context, Result};

/// Reads a single trimmed line from stdin after printing a prompt.
///
/// # Errors
///
/// Returns an error if stdin cannot be read.
pub fn read_prompted_line(prompt: &str) -> Result<String> {
    eprintln!("{prompt}");

    let stdin = io::stdin();
    let mut line = String::new();

    stdin.lock().read_line(&mut line).wrap_err("failed to read from stdin")?;

    while matches!(line.as_bytes().last(), Some(b'\n' | b'\r')) {
        line.pop();
    }

    Ok(line)
}
