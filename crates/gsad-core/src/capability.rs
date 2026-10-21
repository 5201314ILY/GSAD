use std::fmt;

/// Broad data categories. These labels are routing hints, not security proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum DataKind {
    Bytes,
    Text,
    StructuredData,
    Archive,
    Image,
    Audio,
    Video,
    NetworkMessage,
    Executable,
    Unknown,
}

impl fmt::Display for DataKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// An operation a processing engine may perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Operation {
    Identify,
    Parse,
    Decode,
    Encode,
    Compress,
    Decompress,
    Encrypt,
    Decrypt,
    Validate,
    Transform,
    Index,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConcurrencyModel {
    /// Multiple calls may run concurrently on the same engine instance.
    Concurrent,
    /// Calls on the same instance must be serialized by the host.
    Serialized,
    /// Each task should receive a separately created engine instance.
    PerTaskInstance,
}

/// Machine-readable summary of an engine's contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDescriptor {
    /// Stable, namespaced identifier, e.g. `gsad.codec.zstd`.
    pub id: String,
    /// Engine contract version, not necessarily the upstream library version.
    pub version: String,
    pub operations: Vec<Operation>,
    pub inputs: Vec<DataKind>,
    pub outputs: Vec<DataKind>,
    pub supports_streaming: bool,
    pub supports_random_access: bool,
    pub deterministic: bool,
    pub concurrency: ConcurrencyModel,
    /// Advisory maximum per-task memory usage declared by the provider.
    pub max_memory_bytes: Option<u64>,
}

impl CapabilityDescriptor {
    pub fn validate(&self) -> crate::Result<()> {
        if self.id.trim().is_empty() {
            return Err(crate::GsadError::invalid_argument(
                "capability id must not be empty",
            ));
        }
        if self.version.trim().is_empty() {
            return Err(crate::GsadError::invalid_argument(
                "capability version must not be empty",
            ));
        }
        if self.operations.is_empty() {
            return Err(crate::GsadError::invalid_argument(
                "capability must declare at least one operation",
            ));
        }
        if self.inputs.is_empty() {
            return Err(crate::GsadError::invalid_argument(
                "capability must declare at least one input kind",
            ));
        }
        if self.outputs.is_empty() {
            return Err(crate::GsadError::invalid_argument(
                "capability must declare at least one output kind",
            ));
        }
        Ok(())
    }

    pub fn supports(&self, operation: Operation, input: DataKind) -> bool {
        self.operations.contains(&operation) && self.inputs.contains(&input)
    }
}
