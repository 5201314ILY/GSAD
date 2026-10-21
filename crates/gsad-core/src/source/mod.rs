//! Data-source abstraction. Sources may be files, memory buffers, network
//! adapters, archive entries, or virtual objects.

mod file;
mod memory;

pub use file::FileSource;
pub use memory::{MemoryReader, MemorySource};

use crate::{ErrorKind, GsadError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSupport {
    pub sequential_read: bool,
    pub random_access: bool,
    pub concurrent_readers: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInfo {
    pub label: String,
    pub known_length: Option<u64>,
    pub support: SourceSupport,
}

/// A source can open independent sequential readers. `read_at` is optional and
/// must return `Unsupported` when random access is unavailable.
pub trait DataSource: Send + Sync {
    fn info(&self) -> SourceInfo;
    fn open_reader(&self) -> Result<Box<dyn SourceReader>>;

    fn read_at(&self, _offset: u64, _buffer: &mut [u8]) -> Result<usize> {
        Err(GsadError::new(
            ErrorKind::Unsupported,
            "random access is not supported by this source",
        ))
    }
}

/// A single sequential read session. Independent sessions have independent
/// cursors unless a source implementation documents otherwise.
pub trait SourceReader: Send {
    /// Returns 0 only on EOF. Short reads are permitted and are not EOF unless
    /// the returned value is zero.
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize>;

    fn position(&self) -> u64;

    fn seek(&mut self, _position: u64) -> Result<()> {
        Err(GsadError::new(
            ErrorKind::Unsupported,
            "seeking is not supported by this reader",
        ))
    }
}
