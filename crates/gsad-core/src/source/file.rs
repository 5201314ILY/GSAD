//! Sequential and random-access views over a local file.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::{ErrorKind, GsadError, Result};

use super::{DataSource, SourceInfo, SourceReader, SourceSupport};

/// A path-backed data source. The file should not be modified concurrently
/// while an operation uses it; the recorded length is a snapshot from creation.
///
/// Reads are bounded by that recorded length, but the source does not snapshot
/// file contents. Concurrent same-length rewrites are not reliably detected.
#[derive(Debug, Clone)]
pub struct FileSource {
    path: PathBuf,
    initial_length: u64,
}

impl FileSource {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let metadata = std::fs::metadata(&path).map_err(GsadError::from)?;
        if !metadata.is_file() {
            return Err(GsadError::new(
                ErrorKind::InvalidArgument,
                "file source path must refer to a regular file",
            ));
        }
        Ok(Self {
            path,
            initial_length: metadata.len(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl DataSource for FileSource {
    fn info(&self) -> SourceInfo {
        SourceInfo {
            label: self.path.display().to_string(),
            known_length: Some(self.initial_length),
            support: SourceSupport {
                sequential_read: true,
                random_access: true,
                concurrent_readers: true,
            },
        }
    }

    fn open_reader(&self) -> Result<Box<dyn SourceReader>> {
        let file = File::open(&self.path).map_err(GsadError::from)?;
        Ok(Box::new(FileSourceReader {
            file,
            position: 0,
            initial_length: self.initial_length,
        }))
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        if buffer.is_empty() || offset >= self.initial_length {
            return Ok(0);
        }

        let remaining = self.initial_length - offset;
        let max_count = buffer
            .len()
            .min(usize::try_from(remaining).unwrap_or(usize::MAX));

        let mut file = File::open(&self.path).map_err(GsadError::from)?;
        file.seek(SeekFrom::Start(offset))
            .map_err(GsadError::from)?;

        let count = file
            .read(&mut buffer[..max_count])
            .map_err(GsadError::from)?;

        if count == 0 && max_count > 0 {
            return Err(GsadError::new(
                ErrorKind::UnexpectedEof,
                "file ended before its recorded source length",
            )
            .at_offset(offset)
            .during("file_source.read_at"));
        }

        Ok(count)
    }
}

struct FileSourceReader {
    file: File,
    position: u64,
    initial_length: u64,
}

impl SourceReader for FileSourceReader {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        if buffer.is_empty() || self.position >= self.initial_length {
            return Ok(0);
        }

        let remaining = self.initial_length - self.position;
        let max_count = buffer
            .len()
            .min(usize::try_from(remaining).unwrap_or(usize::MAX));

        let count = self
            .file
            .read(&mut buffer[..max_count])
            .map_err(GsadError::from)?;

        if count == 0 && max_count > 0 {
            return Err(GsadError::new(
                ErrorKind::UnexpectedEof,
                "file ended before its recorded source length",
            )
            .at_offset(self.position)
            .during("file_source.read"));
        }

        self.position = self.position.checked_add(count as u64).ok_or_else(|| {
            GsadError::new(ErrorKind::ResourceLimit, "file reader position overflow")
        })?;

        Ok(count)
    }

    fn position(&self) -> u64 {
        self.position
    }

    fn seek(&mut self, position: u64) -> Result<()> {
        self.file
            .seek(SeekFrom::Start(position))
            .map_err(GsadError::from)?;
        self.position = position;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        fs::OpenOptions,
        io::Write,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;
    use crate::source::DataSource;

    static NEXT_TEST_FILE: AtomicU64 = AtomicU64::new(1);

    struct TestFile(PathBuf);

    impl TestFile {
        fn new(bytes: &[u8]) -> Self {
            for _ in 0..128 {
                let serial = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "gsad-file-source-test-{}-{serial}.bin",
                    std::process::id(),
                ));

                match OpenOptions::new().write(true).create_new(true).open(&path) {
                    Ok(mut file) => {
                        file.write_all(bytes).unwrap();
                        return Self(path);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("failed to create test file: {error}"),
                }
            }

            panic!("could not create a unique test file");
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn sequential_reader_does_not_read_appended_bytes_past_snapshot_length() {
        let file = TestFile::new(b"abc");
        let source = FileSource::open(file.path()).unwrap();

        let mut append = OpenOptions::new().append(true).open(file.path()).unwrap();
        append.write_all(b"def").unwrap();
        drop(append);

        assert_eq!(source.info().known_length, Some(3));

        let mut reader = source.open_reader().unwrap();
        let mut buffer = [0_u8; 8];

        assert_eq!(reader.read(&mut buffer).unwrap(), 3);
        assert_eq!(&buffer[..3], b"abc");
        assert_eq!(reader.position(), 3);
        assert_eq!(reader.read(&mut buffer).unwrap(), 0);
    }

    #[test]
    fn random_access_does_not_read_appended_bytes_past_snapshot_length() {
        let file = TestFile::new(b"abc");
        let source = FileSource::open(file.path()).unwrap();

        let mut append = OpenOptions::new().append(true).open(file.path()).unwrap();
        append.write_all(b"def").unwrap();
        drop(append);

        let mut buffer = [0_u8; 8];

        assert_eq!(source.read_at(1, &mut buffer).unwrap(), 2);
        assert_eq!(&buffer[..2], b"bc");
    }

    #[test]
    fn truncation_before_recorded_length_returns_unexpected_eof() {
        let file = TestFile::new(b"abcdef");
        let source = FileSource::open(file.path()).unwrap();

        fs::write(file.path(), b"abc").unwrap();

        let mut reader = source.open_reader().unwrap();
        let mut buffer = [0_u8; 8];

        assert_eq!(reader.read(&mut buffer).unwrap(), 3);

        let error = reader.read(&mut buffer).unwrap_err();
        assert_eq!(error.kind, ErrorKind::UnexpectedEof);
    }

    #[test]
    fn random_access_past_snapshot_end_returns_zero() {
        let file = TestFile::new(b"abc");
        let source = FileSource::open(file.path()).unwrap();

        let mut buffer = [0_u8; 4];
        assert_eq!(source.read_at(3, &mut buffer).unwrap(), 0);
        assert_eq!(source.read_at(99, &mut buffer).unwrap(), 0);
    }
}
