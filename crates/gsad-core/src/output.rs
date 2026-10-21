//! Bounded output and transactional sink contracts.
//!
//! Processing engines receive [`OutputWriter`], not the commit/abort controls of
//! an [`OutputSink`]. The host owns the sink lifecycle and decides whether to
//! commit a completed output or abort it after failure/cancellation.

use crate::{
    capability::DataKind,
    engine::{EngineRequest, ExecutionContext, ExecutionReport, ProcessingEngine},
    error::{ErrorKind, GsadError, Result},
};

/// How strongly a sink can guarantee that an unsuccessful operation leaves no
/// externally visible partial output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommitSemantics {
    /// Commit publishes the complete output atomically, or publishes nothing.
    Atomic,
    /// The sink tries to remove partial output on abort, but cannot promise it.
    BestEffort,
    /// Writes may already be externally visible and cannot be rolled back.
    Irreversible,
}

/// Host-owned output destination. Engines should never receive this interface
/// directly; they receive only `OutputWriter`, so they cannot commit early.
///
/// Implementations must accept short writes. `write` may return fewer bytes
/// than supplied but must not report more than `buffer.len()`. A non-empty
/// buffer returning zero is treated as a write failure by the bounded writer.
/// `abort` should be idempotent and best-effort, including after a flush or
/// commit error. A successful `commit` finalizes the output according to
/// `commit_semantics`.
pub trait OutputSink: Send {
    fn write(&mut self, buffer: &[u8]) -> Result<usize>;
    fn flush(&mut self) -> Result<()>;
    fn commit(&mut self) -> Result<()>;
    fn abort(&mut self) -> Result<()>;
    fn commit_semantics(&self) -> CommitSemantics;
}

/// The restricted interface exposed to processing engines.
///
/// The host wraps a sink with a byte limit and cancellation checks. Engines
/// should write in reasonable chunks to bound latency and responsiveness.
pub trait OutputWriter {
    fn write(&mut self, buffer: &[u8]) -> Result<usize>;
    fn bytes_written(&self) -> u64;

    fn write_all(&mut self, mut buffer: &[u8]) -> Result<()> {
        while !buffer.is_empty() {
            let written = self.write(buffer)?;
            if written == 0 {
                return Err(GsadError::new(
                    ErrorKind::Io,
                    "output writer made no progress on a non-empty buffer",
                ));
            }
            if written > buffer.len() {
                return Err(GsadError::internal(
                    "output writer reported more bytes than requested",
                ));
            }
            buffer = &buffer[written..];
        }
        Ok(())
    }
}

/// Enforces the per-task output budget and cancellation before forwarding bytes
/// to the destination sink.
pub struct BoundedOutputWriter<'a> {
    sink: &'a mut dyn OutputSink,
    context: &'a ExecutionContext,
    bytes_written: u64,
}

impl<'a> BoundedOutputWriter<'a> {
    pub fn new(sink: &'a mut dyn OutputSink, context: &'a ExecutionContext) -> Self {
        Self {
            sink,
            context,
            bytes_written: 0,
        }
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }
}

impl OutputWriter for BoundedOutputWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        self.context.check_cancelled()?;
        if buffer.is_empty() {
            return Ok(0);
        }

        let requested = u64::try_from(buffer.len()).map_err(|_| {
            GsadError::new(
                ErrorKind::ResourceLimit,
                "write length cannot be represented",
            )
        })?;
        let next_total = self.bytes_written.checked_add(requested).ok_or_else(|| {
            GsadError::new(ErrorKind::ResourceLimit, "output byte count overflow")
        })?;
        if next_total > self.context.limits.max_output_bytes {
            return Err(GsadError::new(
                ErrorKind::ResourceLimit,
                "write would exceed max_output_bytes",
            )
            .during("output.write"));
        }

        let written = self.sink.write(buffer)?;
        if written > buffer.len() {
            return Err(GsadError::internal(
                "output sink reported more bytes than supplied",
            ));
        }
        if written == 0 {
            return Err(GsadError::new(
                ErrorKind::Io,
                "output sink returned zero for a non-empty write",
            )
            .during("output.write"));
        }
        self.bytes_written = self
            .bytes_written
            .checked_add(u64::try_from(written).map_err(|_| {
                GsadError::new(ErrorKind::ResourceLimit, "written byte count overflow")
            })?)
            .ok_or_else(|| {
                GsadError::new(ErrorKind::ResourceLimit, "output byte count overflow")
            })?;
        Ok(written)
    }

    fn bytes_written(&self) -> u64 {
        self.bytes_written
    }
}

/// Executes one engine into a host-owned sink and commits only after the engine
/// succeeds, cancellation has been checked, the sink has flushed, and the
/// reported output kind is validated. On failure, the host attempts to abort.
///
/// This helper does not sandbox native engines and does not impose a wall-clock
/// timeout. Resource checks are cooperative at the writer boundary.
pub fn execute_to_sink(
    engine: &dyn ProcessingEngine,
    request: &EngineRequest,
    context: &ExecutionContext,
    sink: &mut dyn OutputSink,
) -> Result<ExecutionReport> {
    execute_to_sink_with_expected(engine, request, context, sink, None)
}

/// Executes an engine while requiring its actual output kind to match the plan.
///
/// The kind is checked before the sink is flushed or committed.
pub(crate) fn execute_to_sink_expected(
    engine: &dyn ProcessingEngine,
    request: &EngineRequest,
    context: &ExecutionContext,
    sink: &mut dyn OutputSink,
    expected_output_kind: DataKind,
) -> Result<ExecutionReport> {
    execute_to_sink_with_expected(engine, request, context, sink, Some(expected_output_kind))
}

fn execute_to_sink_with_expected(
    engine: &dyn ProcessingEngine,
    request: &EngineRequest,
    context: &ExecutionContext,
    sink: &mut dyn OutputSink,
    expected_output_kind: Option<DataKind>,
) -> Result<ExecutionReport> {
    let result = execute_to_sink_inner(engine, request, context, sink, expected_output_kind);

    match result {
        Ok(report) => Ok(report),
        Err(primary) => match sink.abort() {
            Ok(()) => Err(primary),
            Err(abort_error) => Err(GsadError::new(
                primary.kind,
                format!("{primary}; additionally output abort failed: {abort_error}"),
            )
            .during("output.abort")),
        },
    }
}

fn execute_to_sink_inner(
    engine: &dyn ProcessingEngine,
    request: &EngineRequest,
    context: &ExecutionContext,
    sink: &mut dyn OutputSink,
    expected_output_kind: Option<DataKind>,
) -> Result<ExecutionReport> {
    context.check_cancelled()?;
    let descriptor = engine.descriptor();
    descriptor.validate()?;
    if !descriptor.supports(request.operation, request.input_kind) {
        return Err(GsadError::new(
            ErrorKind::Unsupported,
            format!(
                "engine {} does not support {:?} for input kind {:?}",
                descriptor.id, request.operation, request.input_kind
            ),
        ));
    }

    let (completion, bytes_written) = {
        let mut writer = BoundedOutputWriter::new(sink, context);
        let completion = engine.execute_to(request, context, &mut writer)?;
        context.check_cancelled()?;
        let count = writer.bytes_written();
        (completion, count)
    };

    if !descriptor.outputs.contains(&completion.data_kind) {
        return Err(GsadError::new(
            ErrorKind::Internal,
            format!(
                "engine {} reported undeclared output kind {}",
                descriptor.id, completion.data_kind
            ),
        ));
    }

    if let Some(expected_kind) = expected_output_kind {
        if completion.data_kind != expected_kind {
            return Err(GsadError::new(
                ErrorKind::Internal,
                format!(
                    "engine {} reported output kind {}, but the plan stage requires {}",
                    descriptor.id, completion.data_kind, expected_kind,
                ),
            )
            .during("plan.output_kind"));
        }
    }
    sink.flush().map_err(|error| {
        GsadError::new(error.kind, format!("output flush failed: {error}")).during("output.flush")
    })?;
    context.check_cancelled()?;
    sink.commit().map_err(|error| {
        GsadError::new(error.kind, format!("output commit failed: {error}")).during("output.commit")
    })?;

    Ok(ExecutionReport {
        data_kind: completion.data_kind,
        metadata: completion.metadata,
        bytes_written,
        commit_semantics: sink.commit_semantics(),
    })
}
