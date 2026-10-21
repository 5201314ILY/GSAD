//! Atomic-visibility file output for local filesystems.
//!
//! The sink writes to a uniquely-created temporary file in the destination
//! directory, synchronizes file contents, then publishes with a same-directory
//! hard link. Publishing fails rather than replacing an existing destination.
//! This provides atomic visibility on filesystems that support hard links; it
//! does not guarantee directory-entry durability across sudden power loss.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::output::{CommitSemantics, OutputSink};
use crate::{ErrorKind, GsadError, Result};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

/// A local-file sink that never overwrites the destination path.
///
/// Temporary data is created beside the destination so publication remains on
/// the same filesystem. If the filesystem disallows hard links, commit returns
/// an error and the caller can choose a different sink implementation.
pub struct AtomicFileSink {
    destination: PathBuf,
    temporary: PathBuf,
    file: Option<File>,
    committed: bool,
    aborted: bool,
    temporary_cleanup_pending: bool,
}

impl AtomicFileSink {
    /// Creates a new temporary output beside `destination`.
    ///
    /// The final path may already exist at construction time, but commit will
    /// fail with `Conflict` and will not replace it.
    pub fn new(destination: impl AsRef<Path>) -> Result<Self> {
        let destination = destination.as_ref().to_path_buf();
        let file_name = destination
            .file_name()
            .ok_or_else(|| GsadError::invalid_argument("destination must name a file"))?;
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));

        if !parent.is_dir() {
            return Err(GsadError::new(
                ErrorKind::NotFound,
                format!(
                    "destination parent directory does not exist: {}",
                    parent.display()
                ),
            ));
        }

        for _ in 0..128 {
            let serial = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut temp_name = OsString::from(".");
            temp_name.push(file_name);
            let suffix = format!(".gsad-tmp-{}-{serial}", std::process::id());
            temp_name.push(suffix.as_str());
            let temporary = parent.join(temp_name);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => {
                    return Ok(Self {
                        destination,
                        temporary,
                        file: Some(file),
                        committed: false,
                        aborted: false,
                        temporary_cleanup_pending: true,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }

        Err(GsadError::new(
            ErrorKind::Busy,
            "could not allocate a unique temporary output after 128 attempts",
        ))
    }

    pub fn destination(&self) -> &Path {
        &self.destination
    }

    pub fn temporary_path(&self) -> &Path {
        &self.temporary
    }

    fn cleanup_after_commit_failure(
        &mut self,
        mut primary: GsadError,
        remove_temporary: impl FnOnce(&Path) -> std::io::Result<()>,
    ) -> GsadError {
        drop(self.file.take());
        self.aborted = true;

        match remove_temporary(&self.temporary) {
            Ok(()) => {
                self.temporary_cleanup_pending = false;
                primary
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.temporary_cleanup_pending = false;
                primary
            }
            Err(cleanup_error) => {
                self.temporary_cleanup_pending = true;
                primary.message = format!(
                    "{}; additionally failed to remove temporary output {}: {}",
                    primary.message,
                    self.temporary.display(),
                    cleanup_error
                );
                primary
            }
        }
    }
}

impl OutputSink for AtomicFileSink {
    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        if self.committed || self.aborted {
            return Err(GsadError::new(
                ErrorKind::Conflict,
                "cannot write to a finalized file sink",
            ));
        }
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| GsadError::internal("file sink has no open temporary file"))?;
        file.write(buffer).map_err(GsadError::from)
    }

    fn flush(&mut self) -> Result<()> {
        if self.committed || self.aborted {
            return Err(GsadError::new(
                ErrorKind::Conflict,
                "cannot flush a finalized file sink",
            ));
        }
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| GsadError::internal("file sink has no open temporary file"))?;
        file.flush().map_err(GsadError::from)
    }

    fn commit(&mut self) -> Result<()> {
        if self.committed || self.aborted {
            return Err(GsadError::new(
                ErrorKind::Conflict,
                "file sink has already been finalized",
            ));
        }

        let result = (|| {
            let file = self
                .file
                .as_mut()
                .ok_or_else(|| GsadError::internal("file sink has no open temporary file"))?;

            file.flush()
                .map_err(|error| GsadError::from(error).during("file_sink.commit"))?;
            file.sync_all()
                .map_err(|error| GsadError::from(error).during("file_sink.commit"))?;

            drop(self.file.take());

            match fs::hard_link(&self.temporary, &self.destination) {
                Ok(()) => {
                    // Publishing succeeded. Never report this as rolled back
                    // merely because removing the temporary name later fails.
                    self.committed = true;

                    match fs::remove_file(&self.temporary) {
                        Ok(()) => self.temporary_cleanup_pending = false,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            self.temporary_cleanup_pending = false;
                        }
                        Err(_) => {
                            // Publication succeeded; Drop can retry temporary-name cleanup.
                            self.temporary_cleanup_pending = true;
                        }
                    }

                    Ok(())
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    Err(GsadError::new(
                        ErrorKind::Conflict,
                        format!(
                            "destination already exists and was not replaced: {}",
                            self.destination.display()
                        ),
                    )
                    .during("file_sink.commit"))
                }
                Err(error) => Err(GsadError::new(
                    ErrorKind::Io,
                    format!("failed to publish output atomically: {error}"),
                )
                .during("file_sink.commit")),
            }
        })();

        match result {
            Ok(()) => Ok(()),
            Err(primary) => {
                Err(self.cleanup_after_commit_failure(primary, |path| fs::remove_file(path)))
            }
        }
    }

    fn abort(&mut self) -> Result<()> {
        if self.committed && !self.temporary_cleanup_pending {
            return Ok(());
        }

        drop(self.file.take());

        if !self.committed {
            self.aborted = true;
        }

        match fs::remove_file(&self.temporary) {
            Ok(()) => {
                self.temporary_cleanup_pending = false;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.temporary_cleanup_pending = false;
                Ok(())
            }
            Err(error) => {
                self.temporary_cleanup_pending = true;
                Err(GsadError::new(
                    ErrorKind::Io,
                    format!(
                        "failed to remove temporary output {}: {error}",
                        self.temporary.display()
                    ),
                )
                .during("file_sink.abort"))
            }
        }
    }

    fn commit_semantics(&self) -> CommitSemantics {
        CommitSemantics::Atomic
    }
}

impl Drop for AtomicFileSink {
    fn drop(&mut self) {
        drop(self.file.take());

        if self.temporary_cleanup_pending {
            let _ = fs::remove_file(&self.temporary);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_cleanup_preserves_primary_error_and_drop_retries() {
        let serial = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "gsad-file-sink-cleanup-retry-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&dir).expect("create test directory");

        let destination = dir.join("output.bin");
        let mut sink = AtomicFileSink::new(&destination).expect("create sink");
        let temporary = sink.temporary_path().to_path_buf();

        let error = sink.cleanup_after_commit_failure(
            GsadError::new(ErrorKind::Conflict, "injected primary commit failure"),
            |_path| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected cleanup failure",
                ))
            },
        );

        // The original error remains the primary error.
        assert_eq!(error.kind, ErrorKind::Conflict);
        assert!(error.message.contains("injected primary commit failure"));

        // Cleanup failure details and the temporary path are preserved.
        assert!(error.message.contains("injected cleanup failure"));
        assert!(error.message.contains(&temporary.display().to_string()));

        // The sink is finalized, but cleanup remains pending.
        assert!(sink.aborted);
        assert!(sink.temporary_cleanup_pending);
        assert!(temporary.exists());

        // Drop retries the real filesystem deletion.
        drop(sink);

        assert!(
            !temporary.exists(),
            "Drop should retry temporary-file cleanup"
        );
        assert!(
            !destination.exists(),
            "failed commit must not publish output"
        );

        fs::remove_dir_all(&dir).expect("remove test directory");
    }
}
