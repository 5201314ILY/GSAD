//! A tiny, dependency-free lossless run-length codec used to validate the GSAD
//! engine contract end-to-end. It is experimental and is not intended as a
//! competitive general-purpose compression format.
//!
//! Wire format: 8-byte magic `GSARLE01`, followed by records `(count, value)`.
//! `count` is an unsigned byte in `1..=255`. Each record expands to `count`
//! repetitions of `value`. The format has no checksum and should not be used as
//! a long-term archival format without an enclosing integrity layer.

use std::collections::BTreeMap;

use crate::capability::{CapabilityDescriptor, ConcurrencyModel, DataKind, Operation};
use crate::engine::{EngineCompletion, EngineRequest, ExecutionContext, ProcessingEngine};
use crate::output::OutputWriter;
use crate::source::SourceReader;
use crate::{ErrorKind, GsadError, Result};

const MAGIC: &[u8; 8] = b"GSARLE01";
const INPUT_BUFFER_SIZE: usize = 16 * 1024;

/// Reference run-length codec for validating streaming contracts.
#[derive(Debug, Clone)]
pub struct RleEngine {
    descriptor: CapabilityDescriptor,
}

impl Default for RleEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RleEngine {
    pub fn new() -> Self {
        Self {
            descriptor: CapabilityDescriptor {
                id: "gsad.codec.rle-v1".to_owned(),
                version: "0.1.0".to_owned(),
                operations: vec![Operation::Compress, Operation::Decompress],
                inputs: vec![DataKind::Bytes],
                outputs: vec![DataKind::Bytes],
                supports_streaming: true,
                supports_random_access: false,
                deterministic: true,
                concurrency: ConcurrencyModel::Concurrent,
                max_memory_bytes: Some((INPUT_BUFFER_SIZE + 255 + 64) as u64),
            },
        }
    }

    fn compress(
        &self,
        request: &EngineRequest,
        context: &ExecutionContext,
        writer: &mut dyn OutputWriter,
    ) -> Result<EngineCompletion> {
        writer.write_all(MAGIC)?;
        let reader = request.input.open_reader()?;
        let mut input = LimitedReader::new(reader, context);
        let mut buffer = [0_u8; INPUT_BUFFER_SIZE];
        let mut current: Option<u8> = None;
        let mut run_length: u8 = 0;

        loop {
            context.check_cancelled()?;
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            for &byte in &buffer[..count] {
                match current {
                    None => {
                        current = Some(byte);
                        run_length = 1;
                    }
                    Some(value) if value == byte && run_length < u8::MAX => {
                        run_length += 1;
                    }
                    Some(value) => {
                        writer.write_all(&[run_length, value])?;
                        current = Some(byte);
                        run_length = 1;
                    }
                }
            }
        }

        if let Some(value) = current {
            writer.write_all(&[run_length, value])?;
        }

        let mut metadata = BTreeMap::new();
        metadata.insert("format".to_owned(), "gsad-rle-v1".to_owned());
        metadata.insert("codec".to_owned(), "run-length".to_owned());
        Ok(EngineCompletion {
            data_kind: DataKind::Bytes,
            metadata,
        })
    }

    fn decompress(
        &self,
        request: &EngineRequest,
        context: &ExecutionContext,
        writer: &mut dyn OutputWriter,
    ) -> Result<EngineCompletion> {
        let reader = request.input.open_reader()?;
        let mut input = LimitedReader::new(reader, context);
        let mut header = [0_u8; MAGIC.len()];
        input.read_exact_or_eof(&mut header, "truncated RLE header")?;
        if &header != MAGIC {
            return Err(GsadError::new(
                ErrorKind::CorruptData,
                "input does not have the GSARLE01 signature",
            )
            .during("rle.header"));
        }

        let mut decoded_bytes = 0_u64;
        loop {
            context.check_cancelled()?;
            let Some(run_length) = input.read_byte_or_eof()? else {
                break;
            };
            let value = input.read_byte_or_eof()?.ok_or_else(|| {
                GsadError::new(ErrorKind::UnexpectedEof, "truncated RLE record value")
                    .during("rle.record")
                    .at_offset(input.bytes_read())
            })?;
            if run_length == 0 {
                return Err(GsadError::new(
                    ErrorKind::CorruptData,
                    "RLE record count must not be zero",
                )
                .during("rle.record")
                .at_offset(input.bytes_read().saturating_sub(2)));
            }

            let next_decoded = decoded_bytes
                .checked_add(run_length as u64)
                .ok_or_else(|| {
                    GsadError::new(ErrorKind::ResourceLimit, "decoded byte count overflow")
                })?;
            if let Some(ratio) = context.limits.max_expansion_ratio {
                let allowed = input.bytes_read().saturating_mul(ratio as u64);
                if next_decoded > allowed {
                    return Err(GsadError::new(
                        ErrorKind::ResourceLimit,
                        "decompression expansion ratio exceeds configured limit",
                    )
                    .during("rle.decompress"));
                }
            }

            let expanded = [value; u8::MAX as usize];
            writer.write_all(&expanded[..run_length as usize])?;
            decoded_bytes = next_decoded;
        }

        let mut metadata = BTreeMap::new();
        metadata.insert("format".to_owned(), "gsad-rle-v1".to_owned());
        metadata.insert("codec".to_owned(), "run-length".to_owned());
        Ok(EngineCompletion {
            data_kind: DataKind::Bytes,
            metadata,
        })
    }
}

impl ProcessingEngine for RleEngine {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn execute_to(
        &self,
        request: &EngineRequest,
        context: &ExecutionContext,
        writer: &mut dyn OutputWriter,
    ) -> Result<EngineCompletion> {
        match request.operation {
            Operation::Compress => self.compress(request, context, writer),
            Operation::Decompress => self.decompress(request, context, writer),
            operation => Err(GsadError::unsupported(format!(
                "RLE engine does not implement operation {operation:?}"
            ))),
        }
    }
}

struct LimitedReader<'a> {
    reader: Box<dyn SourceReader>,
    context: &'a ExecutionContext,
    bytes_read: u64,
}

impl<'a> LimitedReader<'a> {
    fn new(reader: Box<dyn SourceReader>, context: &'a ExecutionContext) -> Self {
        Self {
            reader,
            context,
            bytes_read: 0,
        }
    }

    fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        self.context.check_cancelled()?;
        if buffer.is_empty() {
            return Ok(0);
        }
        let remaining = self
            .context
            .limits
            .max_input_bytes
            .saturating_sub(self.bytes_read);
        if remaining == 0 {
            // Probe only one byte so the source is not read far past the limit.
            let mut probe = [0_u8; 1];
            let count = self.reader.read(&mut probe)?;
            if count > 1 {
                return Err(GsadError::internal(
                    "data source returned more bytes than the requested buffer length",
                ));
            }
            if count == 0 {
                return Ok(0);
            }
            return Err(
                GsadError::new(ErrorKind::ResourceLimit, "input exceeds max_input_bytes")
                    .during("rle.input"),
            );
        }

        let allowed = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        let count = self.reader.read(&mut buffer[..allowed])?;
        if count > allowed {
            return Err(GsadError::internal(
                "data source returned more bytes than the requested buffer length",
            ));
        }
        let next = self
            .bytes_read
            .checked_add(count as u64)
            .ok_or_else(|| GsadError::new(ErrorKind::ResourceLimit, "input byte count overflow"))?;
        self.bytes_read = next;
        Ok(count)
    }

    fn read_exact_or_eof(&mut self, destination: &mut [u8], message: &str) -> Result<()> {
        let mut offset = 0;
        while offset < destination.len() {
            let count = self.read(&mut destination[offset..])?;
            if count == 0 {
                return Err(GsadError::new(ErrorKind::UnexpectedEof, message)
                    .during("rle.read")
                    .at_offset(self.bytes_read));
            }
            offset += count;
        }
        Ok(())
    }

    fn read_byte_or_eof(&mut self) -> Result<Option<u8>> {
        let mut byte = [0_u8; 1];
        let count = self.read(&mut byte)?;
        if count == 0 {
            Ok(None)
        } else {
            Ok(Some(byte[0]))
        }
    }
}
