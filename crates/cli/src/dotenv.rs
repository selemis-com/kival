//! Minimal `.env` loader.
//!
//! Walks up from the current directory looking for a `.env` file and loads
//! every `KEY=VALUE` pair into the process environment, leaving any variables
//! that are already set untouched.

use std::{
    collections::HashMap,
    env,
    fs::{self, File},
    io::{self, BufRead, BufReader, Read},
    mem,
    path::{Path, PathBuf},
};

/// Loads the nearest `.env` file (current directory or any ancestor).
///
/// Variables already present in the process environment are preserved.
/// Returns the path that was loaded.
///
/// # Errors
///
/// Returns an error if the current directory cannot be read, no `.env` file is found,
/// the file cannot be opened, or the file cannot be parsed.
pub fn dotenv() -> Result<PathBuf> {
    let path = find(&env::current_dir()?, Path::new(".env"))?;
    load(File::open(&path)?)?;
    Ok(path)
}

/// Parses `reader` as a `.env` file and sets every variable in the process
/// environment, preserving anything that is already defined.
fn load<R: Read>(reader: R) -> Result<()> {
    let mut lines = QuotedLines { buf: BufReader::new(reader) };

    // Strip an optional UTF-8 BOM (https://www.compart.com/en/unicode/U+FEFF).
    let buffer = lines.buf.fill_buf()?;
    if buffer.starts_with(&[0xEF, 0xBB, 0xBF]) {
        lines.buf.consume(3);
    }

    let mut substitution_data = HashMap::new();
    for line in lines {
        let line = line?;
        if let Some((key, value)) = LineParser::new(&line, &mut substitution_data).parse_line()?
            && env::var(&key).is_err()
        {
            // SAFETY: `dotenv` is meant to be called once, single-threaded,
            // at process startup before any other thread reads the env.
            unsafe {
                env::set_var(&key, value);
            }
        }
    }
    Ok(())
}

/// Walks up from `directory` looking for `filename`.
fn find(directory: &Path, filename: &Path) -> Result<PathBuf> {
    let mut current = Some(directory);
    while let Some(dir) = current {
        let candidate = dir.join(filename);
        match fs::metadata(&candidate) {
            Ok(meta) if meta.is_file() => return Ok(candidate),
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
        current = dir.parent();
    }
    Err(io::Error::new(io::ErrorKind::NotFound, ".env not found").into())
}

/// Convenience alias for results returned by this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors produced while locating, reading, or parsing a `.env` file.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A line in the `.env` file could not be parsed. Carries the offending
    /// line and the byte offset at which parsing failed.
    #[error("Error parsing line: '{0}', error at line index: {1}")]
    LineParse(String, usize),
    /// An I/O error occurred while locating or reading the `.env` file.
    /// A missing `.env` is reported as [`io::ErrorKind::NotFound`].
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Iterator over logical `.env` lines with quoted multiline values joined.
struct QuotedLines<B> {
    /// Buffered source of physical input lines.
    buf: B,
}

/// Parser state used while scanning for the end of a logical `.env` line.
enum ParseState {
    /// Parser is outside quotes and escapes.
    Complete,
    /// Previous character was an escape outside quotes.
    Escape,
    /// Parser is inside a single-quoted string.
    StrongOpen,
    /// Previous character was an escape inside single quotes.
    StrongOpenEscape,
    /// Parser is inside a double-quoted string.
    WeakOpen,
    /// Previous character was an escape inside double quotes.
    WeakOpenEscape,
    /// Parser has entered a trailing comment.
    Comment,
    /// Parser is scanning whitespace after a completed value.
    WhiteSpace,
}

/// Evaluates how a physical input line changes the logical-line parser state.
fn eval_end_state(prev_state: ParseState, buf: &str) -> (usize, ParseState) {
    let mut cur_state = prev_state;
    let mut cur_pos: usize = 0;

    for (pos, c) in buf.char_indices() {
        cur_pos = pos;
        cur_state = match cur_state {
            ParseState::WhiteSpace => match c {
                '#' => return (cur_pos, ParseState::Comment),
                '\\' => ParseState::Escape,
                '"' => ParseState::WeakOpen,
                '\'' => ParseState::StrongOpen,
                _ => ParseState::Complete,
            },
            ParseState::Escape => ParseState::Complete,
            ParseState::Complete => match c {
                c if c.is_whitespace() && c != '\n' && c != '\r' => ParseState::WhiteSpace,
                '\\' => ParseState::Escape,
                '"' => ParseState::WeakOpen,
                '\'' => ParseState::StrongOpen,
                _ => ParseState::Complete,
            },
            ParseState::WeakOpen => match c {
                '\\' => ParseState::WeakOpenEscape,
                '"' => ParseState::Complete,
                _ => ParseState::WeakOpen,
            },
            ParseState::WeakOpenEscape => ParseState::WeakOpen,
            ParseState::StrongOpen => match c {
                '\\' => ParseState::StrongOpenEscape,
                '\'' => ParseState::Complete,
                _ => ParseState::StrongOpen,
            },
            ParseState::StrongOpenEscape => ParseState::StrongOpen,
            // Comments last the entire line.
            ParseState::Comment => panic!("should have returned early"),
        };
    }
    (cur_pos, cur_state)
}

impl<B: BufRead> Iterator for QuotedLines<B> {
    type Item = Result<String>;

    fn next(&mut self) -> Option<Result<String>> {
        let mut buf = String::new();
        let mut cur_state = ParseState::Complete;
        loop {
            let buf_pos = buf.len();
            match self.buf.read_line(&mut buf) {
                Ok(0) => {
                    return match cur_state {
                        ParseState::Complete => None,
                        _ => {
                            let len = buf.len();
                            Some(Err(Error::LineParse(buf, len)))
                        }
                    };
                }
                Ok(_) => {
                    // Skip comment-only lines as a small optimization.
                    if buf.trim_start().starts_with('#') {
                        return Some(Ok(String::new()));
                    }
                    let (cur_pos, next_state) = eval_end_state(cur_state, &buf[buf_pos..]);
                    cur_state = next_state;

                    match cur_state {
                        ParseState::Complete => {
                            buf.truncate(buf.trim_end_matches(['\r', '\n']).len());
                            return Some(Ok(buf));
                        }
                        ParseState::Comment => {
                            buf.truncate(buf_pos + cur_pos);
                            return Some(Ok(buf));
                        }
                        ParseState::Escape
                        | ParseState::StrongOpen
                        | ParseState::StrongOpenEscape
                        | ParseState::WeakOpen
                        | ParseState::WeakOpenEscape
                        | ParseState::WhiteSpace => {}
                    }
                }
                Err(err) => return Some(Err(err.into())),
            }
        }
    }
}

/// Parser for one logical `.env` assignment line.
struct LineParser<'a> {
    /// Untrimmed line used when reporting parse errors.
    original_line: &'a str,
    /// Previously parsed key-value data available for substitutions.
    substitution_data: &'a mut HashMap<String, Option<String>>,
    /// Remaining unparsed line slice.
    line: &'a str,
    /// Byte offset into `original_line` for diagnostics.
    pos: usize,
}

impl<'a> LineParser<'a> {
    /// Creates a parser for a single logical line.
    fn new(line: &'a str, substitution_data: &'a mut HashMap<String, Option<String>>) -> Self {
        LineParser { original_line: line, substitution_data, line: line.trim_end(), pos: 0 }
    }

    /// Builds a parse error at the current byte offset.
    fn err(&self) -> Error {
        Error::LineParse(self.original_line.into(), self.pos)
    }

    /// Parses the line into an optional key-value assignment.
    fn parse_line(&mut self) -> Result<Option<(String, String)>> {
        self.skip_whitespace();
        if self.line.is_empty() || self.line.starts_with('#') {
            return Ok(None);
        }

        let mut key = self.parse_key()?;
        self.skip_whitespace();

        // `export` may be either an optional prefix or the key itself.
        if key == "export" {
            if self.expect_equal().is_err() {
                key = self.parse_key()?;
                self.skip_whitespace();
                self.expect_equal()?;
            }
        } else {
            self.expect_equal()?;
        }
        self.skip_whitespace();

        if self.line.is_empty() || self.line.starts_with('#') {
            self.substitution_data.insert(key.clone(), None);
            return Ok(Some((key, String::new())));
        }

        let parsed_value = parse_value(self.line, self.substitution_data)?;
        self.substitution_data.insert(key.clone(), Some(parsed_value.clone()));
        Ok(Some((key, parsed_value)))
    }

    /// Parses an environment variable key.
    fn parse_key(&mut self) -> Result<String> {
        if !self.line.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
            return Err(self.err());
        }
        let index = self
            .line
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
            .unwrap_or(self.line.len());
        self.pos += index;
        let key = String::from(&self.line[..index]);
        self.line = &self.line[index..];
        Ok(key)
    }

    /// Consumes the assignment separator.
    fn expect_equal(&mut self) -> Result<()> {
        if !self.line.starts_with('=') {
            return Err(self.err());
        }
        self.line = &self.line[1..];
        self.pos += 1;
        Ok(())
    }

    /// Advances past leading whitespace in the remaining line.
    fn skip_whitespace(&mut self) {
        if let Some(index) = self.line.find(|c: char| !c.is_whitespace()) {
            self.pos += index;
            self.line = &self.line[index..];
        } else {
            self.pos += self.line.len();
            self.line = "";
        }
    }
}

/// State for parsing braced variable substitutions in dotenv values.
#[derive(Eq, PartialEq)]
enum SubstitutionMode {
    /// Parsing characters inside a substitution block.
    Block,
    /// Previous character inside a substitution block was escaped.
    EscapedBlock,
}

/// Parses and unescapes a `.env` value, applying variable substitution.
fn parse_value(input: &str, substitution_data: &HashMap<String, Option<String>>) -> Result<String> {
    let mut strong_quote = false; // '
    let mut weak_quote = false; // "
    let mut escaped = false;
    let mut expecting_end = false;

    let mut output = String::new();
    let mut substitution_mode: Option<SubstitutionMode> = None;
    let mut substitution_name = String::new();

    for (index, c) in input.chars().enumerate() {
        // `expecting_end` permits `k=v #comment` and `k=v#comment`,
        // but rejects `k=v w`.
        if expecting_end {
            match c {
                ' ' | '\t' => {}
                '#' => break,
                _ => return Err(Error::LineParse(input.to_owned(), index)),
            }
        } else if escaped {
            match c {
                '\\' | '\'' | '"' | '$' | ' ' => output.push(c),
                'n' => output.push('\n'),
                _ => return Err(Error::LineParse(input.to_owned(), index)),
            }
            escaped = false;
        } else if strong_quote {
            if c == '\'' {
                strong_quote = false;
            } else {
                output.push(c);
            }
        } else if let Some(mode) = &substitution_mode {
            if c.is_alphanumeric() {
                substitution_name.push(c);
            } else {
                match mode {
                    SubstitutionMode::Block => {
                        if c == '{' && substitution_name.is_empty() {
                            substitution_mode = Some(SubstitutionMode::EscapedBlock);
                        } else {
                            apply_substitution(
                                substitution_data,
                                &mut substitution_name,
                                &mut output,
                            );
                            if c == '$' {
                                substitution_mode =
                                    (!strong_quote && !escaped).then_some(SubstitutionMode::Block);
                            } else {
                                substitution_mode = None;
                                output.push(c);
                            }
                        }
                    }
                    SubstitutionMode::EscapedBlock => {
                        if c == '}' {
                            substitution_mode = None;
                            apply_substitution(
                                substitution_data,
                                &mut substitution_name,
                                &mut output,
                            );
                        } else {
                            substitution_name.push(c);
                        }
                    }
                }
            }
        } else if c == '$' {
            substitution_mode = (!strong_quote && !escaped).then_some(SubstitutionMode::Block);
        } else if weak_quote {
            if c == '"' {
                weak_quote = false;
            } else if c == '\\' {
                escaped = true;
            } else {
                output.push(c);
            }
        } else if c == '\'' {
            strong_quote = true;
        } else if c == '"' {
            weak_quote = true;
        } else if c == '\\' {
            escaped = true;
        } else if c == ' ' || c == '\t' {
            expecting_end = true;
        } else {
            output.push(c);
        }
    }

    if substitution_mode == Some(SubstitutionMode::EscapedBlock) || strong_quote || weak_quote {
        let value_length = input.len();
        Err(Error::LineParse(
            input.to_owned(),
            if value_length == 0 { 0 } else { value_length - 1 },
        ))
    } else {
        apply_substitution(substitution_data, &mut substitution_name, &mut output);
        Ok(output)
    }
}

/// Appends the resolved substitution value to the parsed output.
fn apply_substitution(
    substitution_data: &HashMap<String, Option<String>>,
    substitution_name: &mut String,
    output: &mut String,
) {
    let name = mem::take(substitution_name);
    let value = env::var(&name)
        .ok()
        .or_else(|| substitution_data.get(&name).cloned().flatten())
        .unwrap_or_default();
    output.push_str(&value);
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard, PoisonError};

    use tempfile::TempDir;

    use super::*;

    // Tests mutate `current_dir` and the process environment, so they must
    // not run in parallel.
    static LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> MutexGuard<'static, ()> {
        LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Restores the previous working directory on drop so a panicking test
    /// doesn't strand the rest of the suite in a temp directory.
    struct CwdGuard(PathBuf);
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.0);
        }
    }

    fn enter_temp_dir() -> (TempDir, CwdGuard) {
        let tempdir = TempDir::new().unwrap();
        let guard = CwdGuard(env::current_dir().unwrap());
        env::set_current_dir(tempdir.path()).unwrap();
        (tempdir, guard)
    }

    #[test]
    fn returns_not_found_when_no_env_file() {
        let _lock = lock();
        let (_dir, _cwd) = enter_temp_dir();

        let err = dotenv().expect_err("expected NotFound");
        assert!(
            matches!(&err, Error::Io(io_err) if io_err.kind() == io::ErrorKind::NotFound),
            "got: {err:?}",
        );
    }

    #[test]
    fn loads_variables_from_env_file() {
        let _lock = lock();
        let (dir, _cwd) = enter_temp_dir();
        fs::write(dir.path().join(".env"), "DOTENV_LOAD_A=hello\nDOTENV_LOAD_B=world\n").unwrap();
        // SAFETY: dotenv tests hold a process-wide mutex before mutating the
        // environment, preventing concurrent test access in this module.
        unsafe {
            env::remove_var("DOTENV_LOAD_A");
            env::remove_var("DOTENV_LOAD_B");
        }

        dotenv().expect("load .env");

        assert_eq!(env::var("DOTENV_LOAD_A").unwrap(), "hello");
        assert_eq!(env::var("DOTENV_LOAD_B").unwrap(), "world");
        // SAFETY: the process-wide dotenv test mutex is still held.
        unsafe {
            env::remove_var("DOTENV_LOAD_A");
            env::remove_var("DOTENV_LOAD_B");
        }
    }

    #[test]
    fn preserves_existing_env_vars() {
        let _lock = lock();
        let (dir, _cwd) = enter_temp_dir();
        fs::write(dir.path().join(".env"), "DOTENV_KEEP=from_file\n").unwrap();
        // SAFETY: dotenv tests hold a process-wide mutex before mutating the
        // environment, preventing concurrent test access in this module.
        unsafe {
            env::set_var("DOTENV_KEEP", "preset");
        }

        dotenv().expect("load .env");

        assert_eq!(env::var("DOTENV_KEEP").unwrap(), "preset");
        // SAFETY: the process-wide dotenv test mutex is still held.
        unsafe {
            env::remove_var("DOTENV_KEEP");
        }
    }

    #[test]
    fn finds_env_in_parent_directory() {
        let _lock = lock();
        let (dir, _cwd) = enter_temp_dir();
        fs::write(dir.path().join(".env"), "DOTENV_PARENT=found\n").unwrap();
        let nested = dir.path().join("nested");
        fs::create_dir(&nested).unwrap();
        env::set_current_dir(&nested).unwrap();
        // SAFETY: dotenv tests hold a process-wide mutex before mutating the
        // environment, preventing concurrent test access in this module.
        unsafe {
            env::remove_var("DOTENV_PARENT");
        }

        let path = dotenv().expect("load .env");

        assert_eq!(path.file_name().unwrap(), ".env");
        assert_eq!(env::var("DOTENV_PARENT").unwrap(), "found");
        // SAFETY: the process-wide dotenv test mutex is still held.
        unsafe {
            env::remove_var("DOTENV_PARENT");
        }
    }
}
