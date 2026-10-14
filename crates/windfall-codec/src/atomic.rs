//! A file that only takes its name once it is whole.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::error::CodecError;

/// Keeps temporary file names apart when one process writes several files
/// with the same name at once.
static NEXT_TEMP_ID: AtomicU32 = AtomicU32::new(0);

/// The file an encoder writes into: a temporary file beside the destination
/// that takes the destination's name in [`place`](Self::place).
///
/// If it is dropped first, the temporary file is removed and whatever was
/// at the destination before is left untouched.
#[derive(Debug)]
pub(crate) struct AtomicFile {
    /// `None` once the file has been handed over in `place`.
    file: Option<BufWriter<File>>,
    temp_path: PathBuf,
    final_path: PathBuf,
    /// Set once the file has its final name and nothing is left to clean up.
    placed: bool,
}

impl AtomicFile {
    /// Starts a file that will appear at `path` when placed.
    pub fn create(path: &Path) -> Result<Self, CodecError> {
        let final_path = path.to_path_buf();
        let name = final_path.file_name().ok_or_else(|| {
            CodecError::InvalidInput(
                windfall_core::notice!("codec.notAFileName", path = final_path.display())
                    .to_string(),
            )
        })?;

        // The name is unique among running processes, so a file already there
        // is left over from a crashed export and is safe to replace.
        let mut temp_name = std::ffi::OsString::from(".");
        temp_name.push(name);
        temp_name.push(format!(
            ".{}-e{}.tmp",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let temp_path = final_path.with_file_name(temp_name);
        let file = BufWriter::with_capacity(64 * 1024, File::create(&temp_path)?);
        Ok(Self {
            file: Some(file),
            temp_path,
            final_path,
            placed: false,
        })
    }

    /// Writes what is buffered, lets `patch` fill in the parts of the file
    /// that could not be known before its end, and moves the file to its
    /// destination, replacing any file already there.
    pub fn place(mut self, patch: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
        let buffered = self.file.take().ok_or_else(closed)?;
        let mut file = buffered.into_inner().map_err(|error| error.into_error())?;
        patch(&mut file)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&self.temp_path, &self.final_path)?;
        self.placed = true;
        Ok(())
    }
}

fn closed() -> io::Error {
    io::Error::other("the file is already closed")
}

impl Write for AtomicFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.file.as_mut().ok_or_else(closed)?.write(bytes)
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.file.as_mut().ok_or_else(closed)?.write_all(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.as_mut().ok_or_else(closed)?.flush()
    }
}

impl Drop for AtomicFile {
    fn drop(&mut self) {
        if self.placed {
            return;
        }
        // The file has to be closed before Windows will delete it.
        self.file = None;
        let _ = fs::remove_file(&self.temp_path);
    }
}
