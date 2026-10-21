use std::collections::BTreeMap;
use std::sync::Arc;

use crate::{
    capability::{CapabilityDescriptor, DataKind, Operation},
    error::{ErrorKind, GsadError, Result},
    output::{CommitSemantics, OutputWriter},
    resource::{CancellationToken, ResourceLimits},
    source::DataSource,
};

#[derive(Clone)]
pub struct EngineRequest {
    pub operation: Operation,
    pub input_kind: DataKind,
    pub input: Arc<dyn DataSource>,
    /// Small, validated, non-secret parameters. Large payloads belong in sources.
    pub parameters: BTreeMap<String, String>,
}

impl std::fmt::Debug for EngineRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineRequest")
            .field("operation", &self.operation)
            .field("input_kind", &self.input_kind)
            .field("source", &self.input.info())
            .field(
                "parameter_keys",
                &self.parameters.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineOutput {
    pub data: Vec<u8>,
    pub data_kind: DataKind,
    pub metadata: BTreeMap<String, String>,
}

impl EngineOutput {
    pub fn new(data: Vec<u8>, data_kind: DataKind) -> Self {
        Self {
            data,
            data_kind,
            metadata: BTreeMap::new(),
        }
    }
}

/// Metadata returned by an engine that writes bytes into an `OutputWriter`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineCompletion {
    pub data_kind: DataKind,
    pub metadata: BTreeMap<String, String>,
}

impl EngineCompletion {
    pub fn new(data_kind: DataKind) -> Self {
        Self {
            data_kind,
            metadata: BTreeMap::new(),
        }
    }
}

/// Host-generated summary for a successfully committed streaming execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReport {
    pub data_kind: DataKind,
    pub metadata: BTreeMap<String, String>,
    pub bytes_written: u64,
    pub commit_semantics: CommitSemantics,
}

#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub cancellation: CancellationToken,
    pub limits: ResourceLimits,
}

impl ExecutionContext {
    pub fn new(limits: ResourceLimits) -> Result<Self> {
        limits.validate()?;
        Ok(Self {
            cancellation: CancellationToken::new(),
            limits,
        })
    }

    pub fn check_cancelled(&self) -> Result<()> {
        self.cancellation.check()
    }

    pub fn validate_output(&self, output: &EngineOutput) -> Result<()> {
        self.check_cancelled()?;
        if output.data.len() as u64 > self.limits.max_output_bytes {
            return Err(GsadError::new(
                ErrorKind::ResourceLimit,
                "engine output exceeds max_output_bytes",
            ));
        }
        Ok(())
    }
}

/// Engine implementations must enforce relevant limits while processing, not
/// merely after allocating their entire output. The host's final output check
/// is defense in depth, not a sandbox.
pub trait ProcessingEngine: Send + Sync {
    fn descriptor(&self) -> &CapabilityDescriptor;

    /// Convenience path for engines whose output is known to be small. Large
    /// or unbounded outputs should override `execute_to` and stream into the
    /// provided writer. An engine may implement either method; implementing
    /// neither returns `Unsupported` at runtime.
    fn execute(
        &self,
        _request: &EngineRequest,
        _context: &ExecutionContext,
    ) -> Result<EngineOutput> {
        Err(GsadError::unsupported(
            "engine provides no in-memory execute implementation",
        ))
    }

    /// Writes output incrementally. The default adapter calls `execute` and
    /// writes its returned buffer, which is convenient but materializes output
    /// in memory. Production engines handling large outputs should override it.
    fn execute_to(
        &self,
        request: &EngineRequest,
        context: &ExecutionContext,
        writer: &mut dyn OutputWriter,
    ) -> Result<EngineCompletion> {
        let output = self.execute(request, context)?;
        context.check_cancelled()?;
        writer.write_all(&output.data)?;
        Ok(EngineCompletion {
            data_kind: output.data_kind,
            metadata: output.metadata,
        })
    }
}
