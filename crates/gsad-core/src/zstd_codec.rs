//! Streaming Zstandard compression/decompression adapter.
//!
//! This adapter uses the mature `zstd` crate rather than making GSAD's core
//! responsible for the compression algorithm. Output is streamed through the
//! host's bounded writer. The encoder emits a content checksum; the decoder
//! enforces input/output limits and a maximum Zstandard window derived from the
//! task's memory budget. The memory budget is still a policy limit, not a
//! process sandbox or a precise cap on all library allocations.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::capability::{CapabilityDescriptor, ConcurrencyModel, DataKind, Operation};
use crate::engine::{EngineCompletion, EngineRequest, ExecutionContext, ProcessingEngine};
use crate::output::OutputWriter;
use crate::source::SourceReader;
use crate::{ErrorKind, GsadError, Result};

const TRANSFER_BUFFER_SIZE: usize = 64 * 1024;
const DEFAULT_COMPRESSION_LEVEL: i32 = 3;
const MIN_WINDOW_LOG: u32 = 10;
const MAX_WINDOW_LOG: u32 = 31;

/// Zstandard codec adapter for GSAD's engine contract.
#[derive(Debug, Clone)]
pub struct ZstdEngine {
    descriptor: CapabilityDescriptor,
}

impl Default for ZstdEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ZstdEngine {
    pub fn new() -> Self {
        Self {
            descriptor: CapabilityDescriptor {
                id: "gsad.codec.zstd".to_owned(),
                version: "0.1.0".to_owned(),
                operations: vec![Operation::Compress, Operation::Decompress],
                inputs: vec![DataKind::Bytes],
                outputs: vec![DataKind::Bytes],
                supports_streaming: true,
                supports_random_access: false,
                deterministic: true,
                concurrency: ConcurrencyModel::Concurrent,
                max_memory_bytes: Some(128 * 1024 * 1024),
            },
        }
    }

    fn validate_parameters(&self, request: &EngineRequest) -> Result<i32> {
        match request.operation {
            Operation::Compress => {
                for key in request.parameters.keys() {
                    if key != "level" {
                        return Err(GsadError::invalid_argument(format!(
                            "unsupported Zstandard parameter: {key}"
                        )));
                    }
                }
                let level = match request.parameters.get("level") {
                    Some(value) => value.parse::<i32>().map_err(|_| {
                        GsadError::invalid_argument("Zstandard level must be a valid integer")
                    })?,
                    None => DEFAULT_COMPRESSION_LEVEL,
                };
                if !zstd::compression_level_range().contains(&level) {
                    return Err(GsadError::invalid_argument(format!(
                        "Zstandard compression level {level} is outside the supported range"
                    )));
                }
                Ok(level)
            }
            Operation::Decompress => {
                if !request.parameters.is_empty() {
                    return Err(GsadError::invalid_argument(
                        "Zstandard decompression does not accept parameters in this version",
                    ));
                }
                Ok(DEFAULT_COMPRESSION_LEVEL)
            }
            operation => Err(GsadError::unsupported(format!(
                "Zstandard engine does not implement operation {operation:?}"
            ))),
        }
    }

    fn compress(
        &self,
        request: &EngineRequest,
        context: &ExecutionContext,
        writer: &mut dyn OutputWriter,
        level: i32,
    ) -> Result<EngineCompletion> {
        context.check_cancelled()?;
        let input_counter = Arc::new(AtomicU64::new(0));
        let source_reader = SourceIoReader::new(
            request.input.open_reader()?,
            context,
            Arc::clone(&input_counter),
        );
        let mut output_writer = OutputIoWriter { inner: writer };

        let mut encoder = zstd::stream::write::Encoder::new(&mut output_writer, level)
            .map_err(|error| map_io_error(error, ErrorKind::Io, "zstd.encoder.create"))?;
        // Keep the frame window within this task memory budget so the same
        // GSAD engine can decode frames produced under this policy.
        encoder
            .window_log(maximum_window_log(context.limits.max_memory_bytes))
            .map_err(|error| {
                map_io_error(error, ErrorKind::ResourceLimit, "zstd.encoder.window")
            })?;
        encoder
            .include_checksum(true)
            .map_err(|error| map_io_error(error, ErrorKind::Io, "zstd.encoder.checksum"))?;

        let mut input = source_reader;
        let mut buffer = [0_u8; TRANSFER_BUFFER_SIZE];
        loop {
            context.check_cancelled()?;
            let count = input
                .read(&mut buffer)
                .map_err(|error| map_io_error(error, ErrorKind::Io, "zstd.compress.input"))?;
            if count == 0 {
                break;
            }
            encoder
                .write_all(&buffer[..count])
                .map_err(|error| map_io_error(error, ErrorKind::Io, "zstd.compress.output"))?;
        }
        // Explicit finish is required to write the complete frame and checksum.
        let _output_writer = encoder
            .finish()
            .map_err(|error| map_io_error(error, ErrorKind::Io, "zstd.encoder.finish"))?;
        context.check_cancelled()?;

        let mut metadata = BTreeMap::new();
        metadata.insert("format".to_owned(), "zstandard-frame".to_owned());
        metadata.insert("codec".to_owned(), "zstd".to_owned());
        metadata.insert("level".to_owned(), level.to_string());
        metadata.insert(
            "checksum".to_owned(),
            "zstd-frame-content-checksum".to_owned(),
        );
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
        context.check_cancelled()?;
        let input_counter = Arc::new(AtomicU64::new(0));
        let source_reader = SourceIoReader::new(
            request.input.open_reader()?,
            context,
            Arc::clone(&input_counter),
        );
        let mut decoder = zstd::stream::read::Decoder::new(source_reader)
            .map_err(|error| map_io_error(error, ErrorKind::CorruptData, "zstd.decoder.create"))?;
        let window_log = maximum_window_log(context.limits.max_memory_bytes);
        decoder.window_log_max(window_log).map_err(|error| {
            GsadError::new(
                ErrorKind::ResourceLimit,
                format!("Zstandard frame exceeds configured decode window: {error}"),
            )
            .during("zstd.decoder.window")
        })?;

        let mut guarded_writer = ExpansionRatioWriter {
            inner: writer,
            input_bytes_read: input_counter,
            max_expansion_ratio: context.limits.max_expansion_ratio,
            bytes_written: 0,
        };
        let mut buffer = [0_u8; TRANSFER_BUFFER_SIZE];
        loop {
            context.check_cancelled()?;
            let count = decoder
                .read(&mut buffer)
                .map_err(|error| map_io_error(error, ErrorKind::CorruptData, "zstd.decompress"))?;
            if count == 0 {
                break;
            }
            guarded_writer.write_all(&buffer[..count])?;
        }
        context.check_cancelled()?;

        let mut metadata = BTreeMap::new();
        metadata.insert("format".to_owned(), "zstandard-frame".to_owned());
        metadata.insert("codec".to_owned(), "zstd".to_owned());
        metadata.insert(
            "checksum".to_owned(),
            "validated-if-present-in-frame".to_owned(),
        );
        Ok(EngineCompletion {
            data_kind: DataKind::Bytes,
            metadata,
        })
    }
}

impl ProcessingEngine for ZstdEngine {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn execute_to(
        &self,
        request: &EngineRequest,
        context: &ExecutionContext,
        writer: &mut dyn OutputWriter,
    ) -> Result<EngineCompletion> {
        let level = self.validate_parameters(request)?;
        match request.operation {
            Operation::Compress => self.compress(request, context, writer, level),
            Operation::Decompress => self.decompress(request, context, writer),
            operation => Err(GsadError::unsupported(format!(
                "Zstandard engine does not implement operation {operation:?}"
            ))),
        }
    }
}

/// Adapts the GSAD reader contract to `std::io::Read`, applying input and
/// cancellation limits before bytes reach the codec.
struct SourceIoReader<'a> {
    reader: Box<dyn SourceReader>,
    context: &'a ExecutionContext,
    input_bytes_read: u64,
    counter: Arc<AtomicU64>,
}

impl<'a> SourceIoReader<'a> {
    fn new(
        reader: Box<dyn SourceReader>,
        context: &'a ExecutionContext,
        counter: Arc<AtomicU64>,
    ) -> Self {
        Self {
            reader,
            context,
            input_bytes_read: 0,
            counter,
        }
    }
}

impl Read for SourceIoReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.context.check_cancelled().map_err(to_io_error)?;
        if buffer.is_empty() {
            return Ok(0);
        }

        let remaining = self
            .context
            .limits
            .max_input_bytes
            .saturating_sub(self.input_bytes_read);
        if remaining == 0 {
            let mut probe = [0_u8; 1];
            let count = self.reader.read(&mut probe).map_err(to_io_error)?;
            if count == 0 {
                return Ok(0);
            }
            return Err(to_io_error(GsadError::new(
                ErrorKind::ResourceLimit,
                "input exceeds max_input_bytes",
            )));
        }

        let allowed = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        let count = self
            .reader
            .read(&mut buffer[..allowed])
            .map_err(to_io_error)?;
        if count > allowed {
            return Err(to_io_error(GsadError::internal(
                "data source returned more bytes than requested",
            )));
        }
        self.input_bytes_read =
            self.input_bytes_read
                .checked_add(count as u64)
                .ok_or_else(|| {
                    to_io_error(GsadError::new(
                        ErrorKind::ResourceLimit,
                        "input byte count overflow",
                    ))
                })?;
        self.counter.fetch_add(count as u64, Ordering::Release);
        Ok(count)
    }
}

/// Adapts a GSAD writer to `std::io::Write`. The flush operation is deliberately
/// a no-op: the host owns and flushes the final sink after the engine succeeds.
struct OutputIoWriter<'a> {
    inner: &'a mut dyn OutputWriter,
}

impl Write for OutputIoWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.inner.write(buffer).map_err(to_io_error)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Checks the configured expansion ratio before forwarding decompressed bytes.
/// The denominator is bytes fetched from the source adapter; codec read-ahead
/// can make this check slightly lenient, so the absolute output cap remains the
/// strict upper bound.
struct ExpansionRatioWriter<'a> {
    inner: &'a mut dyn OutputWriter,
    input_bytes_read: Arc<AtomicU64>,
    max_expansion_ratio: Option<u32>,
    bytes_written: u64,
}

impl OutputWriter for ExpansionRatioWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let request_size = u64::try_from(buffer.len()).map_err(|_| {
            GsadError::new(ErrorKind::ResourceLimit, "output write length overflow")
        })?;
        let next_total = self
            .bytes_written
            .checked_add(request_size)
            .ok_or_else(|| {
                GsadError::new(ErrorKind::ResourceLimit, "decoded byte count overflow")
            })?;
        if let Some(ratio) = self.max_expansion_ratio {
            let input_bytes = self.input_bytes_read.load(Ordering::Acquire);
            let allowed = input_bytes.saturating_mul(ratio as u64);
            if next_total > allowed {
                return Err(GsadError::new(
                    ErrorKind::ResourceLimit,
                    "decompression expansion ratio exceeds configured limit",
                )
                .during("zstd.decompress.ratio"));
            }
        }
        let written = self.inner.write(buffer)?;
        self.bytes_written = self
            .bytes_written
            .checked_add(written as u64)
            .ok_or_else(|| {
                GsadError::new(ErrorKind::ResourceLimit, "decoded byte count overflow")
            })?;
        Ok(written)
    }

    fn bytes_written(&self) -> u64 {
        self.bytes_written
    }
}

fn maximum_window_log(max_memory_bytes: u64) -> u32 {
    // floor(log2(n)) without floating point. Zstandard's window limit has a
    // practical minimum of 2^10; max_memory_bytes remains an advisory budget
    // for the rest of the codec state, not a hard allocator ceiling.
    let floor_log = if max_memory_bytes == 0 {
        0
    } else {
        63 - max_memory_bytes.leading_zeros()
    };
    floor_log.clamp(MIN_WINDOW_LOG, MAX_WINDOW_LOG)
}

fn to_io_error(error: GsadError) -> io::Error {
    io::Error::other(error)
}

fn map_io_error(error: io::Error, fallback_kind: ErrorKind, operation: &'static str) -> GsadError {
    if let Some(source) = error.get_ref() {
        if let Some(gs_error) = source.downcast_ref::<GsadError>() {
            return gs_error.clone().during(operation);
        }
    }
    let kind = if error.kind() == io::ErrorKind::Interrupted {
        ErrorKind::Io
    } else {
        fallback_kind
    };
    GsadError::new(kind, error.to_string()).during(operation)
}
