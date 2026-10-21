//! Sequential and random-access views over a local file.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::{ErrorKind, GsadError, Result};

use super::{DataSource, SourceInfo, SourceReader, SourceSupport};

/// A path-backed data source. The file should not be modified concurrently
/// while an operation uses it; the recorded length is a snapshot from creation.
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
        Ok(Box::new(FileSourceReader { file, position: 0 }))
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        if buffer.is_empty() || offset >= self.initial_length {
            return Ok(0);
        }
        let mut file = File::open(&self.path).map_err(GsadError::from)?;
        file.seek(SeekFrom::Start(offset))
            .map_err(GsadError::from)?;
        file.read(buffer).map_err(GsadError::from)
    }
}

struct FileSourceReader {
    file: File,
    position: u64,
}

impl SourceReader for FileSourceReader {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let count = self.file.read(buffer).map_err(GsadError::from)?;
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
