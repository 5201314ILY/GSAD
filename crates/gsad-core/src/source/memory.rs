use std::sync::Arc;

use crate::{ErrorKind, GsadError, Result};

use super::{DataSource, SourceInfo, SourceReader, SourceSupport};

/// Immutable in-memory source useful for tests and small payloads.
#[derive(Debug, Clone)]
pub struct MemorySource {
    label: String,
    bytes: Arc<[u8]>,
}

impl MemorySource {
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self::with_label("memory", bytes)
    }

    pub fn with_label(label: impl Into<String>, bytes: impl Into<Arc<[u8]>>) -> Self {
        Self {
            label: label.into(),
            bytes: bytes.into(),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl DataSource for MemorySource {
    fn info(&self) -> SourceInfo {
        SourceInfo {
            label: self.label.clone(),
            known_length: Some(self.bytes.len() as u64),
            support: SourceSupport {
                sequential_read: true,
                random_access: true,
                concurrent_readers: true,
            },
        }
    }

    fn open_reader(&self) -> Result<Box<dyn SourceReader>> {
        Ok(Box::new(MemoryReader {
            bytes: Arc::clone(&self.bytes),
            position: 0,
        }))
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        let offset = usize::try_from(offset).map_err(|_| {
            GsadError::new(
                ErrorKind::InvalidArgument,
                "offset does not fit this platform",
            )
        })?;
        if offset >= self.bytes.len() || buffer.is_empty() {
            return Ok(0);
        }
        let count = buffer.len().min(self.bytes.len() - offset);
        buffer[..count].copy_from_slice(&self.bytes[offset..offset + count]);
        Ok(count)
    }
}

pub struct MemoryReader {
    bytes: Arc<[u8]>,
    position: u64,
}

impl SourceReader for MemoryReader {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        let offset = usize::try_from(self.position)
            .map_err(|_| GsadError::new(ErrorKind::InvalidArgument, "reader position overflow"))?;
        if offset >= self.bytes.len() || buffer.is_empty() {
            return Ok(0);
        }
        let count = buffer.len().min(self.bytes.len() - offset);
        buffer[..count].copy_from_slice(&self.bytes[offset..offset + count]);
        self.position += count as u64;
        Ok(count)
    }

    fn position(&self) -> u64 {
        self.position
    }

    fn seek(&mut self, position: u64) -> Result<()> {
        if position > self.bytes.len() as u64 {
            return Err(GsadError::new(
                ErrorKind::InvalidArgument,
                "seek position is past the end of the memory source",
            ));
        }
        self.position = position;
        Ok(())
    }
}
