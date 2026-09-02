//! A size-based rolling file appender.
//!
//! Follows a Debian-style naming convention for logfiles,
//! using basename, basename.1, ..., basename.N where N is
//! the maximum number of allowed historical logfiles.

use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io,
    io::{BufWriter, Write},
    path::Path,
};

use kival_common::fs::{self, FsPathError};

/// Writes data to a file, and "rolls over" to preserve older data in
/// a separate set of files. Old files have a Debian-style naming scheme
/// where we have `base_filename`, `base_filename.1`, ..., `base_filename.N`
/// where N is the maximum number of rollover files to keep.
#[derive(Debug)]
pub(crate) struct RollingFileAppender {
    /// Base log filename used for the active file and rollover files.
    base_filename: OsString,
    /// Maximum active logfile size before rollover.
    max_size: u64,
    /// Maximum number of historical rollover files to keep.
    max_files: usize,
    /// Number of bytes written to the currently open logfile.
    current_filesize: u64,
    /// Lazily opened writer for the active logfile.
    writer_opt: Option<BufWriter<File>>,
}

impl RollingFileAppender {
    /// Creates a new rolling file appender that rolls over once `max_size` bytes
    /// have been written, keeping at most `max_files` historical files.
    /// The parent directory of the base path must already exist.
    pub(crate) fn new<P>(path: P, max_size: u64, max_files: usize) -> io::Result<Self>
    where
        P: AsRef<Path>,
    {
        let mut rfa = Self {
            base_filename: path.as_ref().as_os_str().to_os_string(),
            max_size,
            max_files,
            current_filesize: 0,
            writer_opt: None,
        };
        // Fail if we can't open the file initially...
        rfa.open_writer_if_needed()?;
        Ok(rfa)
    }

    /// Determines the final filename, where n==0 indicates the current file.
    fn filename_for(&self, n: usize) -> OsString {
        let mut f = self.base_filename.clone();
        if n > 0 {
            f.push(OsString::from(format!(".{}", n)))
        }
        f
    }

    /// Rotates old files to make room for a new one.
    /// This may result in the deletion of the oldest file.
    fn rotate_files(&self) -> io::Result<()> {
        // ignore any failure removing the oldest file (may not exist)
        let _ = fs::remove_file_if_exists(self.filename_for(self.max_files.max(1)));
        let mut r = Ok(());
        for i in (0..self.max_files.max(1)).rev() {
            let rotate_from = self.filename_for(i);
            let rotate_to = self.filename_for(i + 1);
            match fs::rename(&rotate_from, &rotate_to) {
                Ok(()) => {}
                Err(FsPathError::Rename { source, .. })
                    if source.kind() == io::ErrorKind::NotFound => {}
                Err(e) => {
                    // capture the error, but continue the loop,
                    // to maximize ability to rename everything
                    r = Err(io::Error::other(e));
                }
            }
        }
        r
    }

    /// Forces a rollover to happen immediately.
    fn rollover(&mut self) -> io::Result<()> {
        // Before closing, make sure all data is flushed successfully.
        self.flush()?;
        // We must close the current file before rotating files
        self.writer_opt.take();
        self.current_filesize = 0;
        self.rotate_files()?;
        self.open_writer_if_needed()
    }

    /// Opens a writer for the current file.
    fn open_writer_if_needed(&mut self) -> io::Result<()> {
        if self.writer_opt.is_none() {
            let p = self.filename_for(0);
            let f = OpenOptions::new()
                .append(true)
                .create(true)
                .open(&p)
                .map_err(|e| io::Error::other(FsPathError::open(e, &p)))?;
            self.writer_opt = Some(BufWriter::new(f));
            self.current_filesize = fs::metadata(&p).map_or(0, |m| m.len());
        }
        Ok(())
    }
}

impl Write for RollingFileAppender {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.current_filesize >= self.max_size
            && let Err(e) = self.rollover()
        {
            // If we can't rollover, just try to continue writing anyway
            // (better than missing data).
            // This will likely be used to implement logging, so
            // avoid using log::warn and log to stderr directly.
            eprintln!(
                "WARNING: Failed to rotate logfile {}: {}",
                self.base_filename.to_string_lossy(),
                e
            );
        }
        self.open_writer_if_needed()?;
        self.writer_opt.as_mut().map_or_else(
            || Err(io::Error::other("unexpected condition: writer is missing")),
            |writer| {
                let buf_len = buf.len();
                writer.write_all(buf).map(|_| {
                    self.current_filesize += u64::try_from(buf_len).unwrap_or(u64::MAX);
                    buf_len
                })
            },
        )
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(writer) = self.writer_opt.as_mut() {
            writer.flush()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    struct Context {
        _tempdir: TempDir,
        rolling: RollingFileAppender,
    }

    impl Context {
        #[track_caller]
        fn verify_contains(&self, needle: &str, n: usize) {
            let haystack = self.read(n);
            assert!(
                haystack.contains(needle),
                "file {:?} did not contain expected contents {}",
                self.path(n),
                needle
            );
        }

        fn flush(&mut self) {
            self.rolling.flush().unwrap();
        }

        fn read(&self, n: usize) -> String {
            fs::read_to_string(self.path(n)).unwrap()
        }

        fn path(&self, n: usize) -> OsString {
            self.rolling.filename_for(n)
        }
    }

    fn build_context(max_size: u64, max_files: usize) -> Context {
        let tempdir = TempDir::new().unwrap();
        let rolling =
            RollingFileAppender::new(tempdir.path().join("test.log"), max_size, max_files).unwrap();
        Context { _tempdir: tempdir, rolling }
    }

    #[test]
    fn max_size() {
        let mut c = build_context(10, 9);
        c.rolling.write_all(b"12345").unwrap();
        c.rolling.write_all(b"6789").unwrap();
        c.rolling.write_all(b"0").unwrap();
        c.rolling.write_all(b"abcdefghijklmn").unwrap();
        c.rolling.write_all(b"ZZZ").unwrap();
        assert!(!AsRef::<Path>::as_ref(&c.rolling.filename_for(3)).exists());
        c.flush();
        c.verify_contains("1234567890", 2);
        c.verify_contains("abcdefghijklmn", 1);
        c.verify_contains("ZZZ", 0);
    }

    #[test]
    fn max_size_existing() {
        let mut c = build_context(10, 9);
        c.rolling.write_all(b"12345").unwrap();
        // close the file and make sure that it can re-open it, and that it
        // resets the file size properly.
        c.rolling.writer_opt.take();
        c.rolling.current_filesize = 0;
        c.rolling.write_all(b"6789").unwrap();
        c.rolling.write_all(b"0").unwrap();
        c.rolling.write_all(b"abcdefghijklmn").unwrap();
        c.rolling.write_all(b"ZZZ").unwrap();
        assert!(!AsRef::<Path>::as_ref(&c.rolling.filename_for(3)).exists());
        c.flush();
        c.verify_contains("1234567890", 2);
        c.verify_contains("abcdefghijklmn", 1);
        c.verify_contains("ZZZ", 0);
    }

    #[test]
    fn max_size_limited_files() {
        let mut c = build_context(10, 2);
        c.rolling.write_all(b"12345").unwrap();
        c.rolling.write_all(b"6789").unwrap();
        c.rolling.write_all(b"0").unwrap();
        c.rolling.write_all(b"abcdefghijklmn").unwrap();
        c.rolling.write_all(b"ZZZ").unwrap();
        assert!(!AsRef::<Path>::as_ref(&c.rolling.filename_for(4)).exists());
        assert!(!AsRef::<Path>::as_ref(&c.rolling.filename_for(3)).exists());
        c.flush();
        c.verify_contains("1234567890", 2);
        c.verify_contains("abcdefghijklmn", 1);
        c.verify_contains("ZZZ", 0);
    }
}
