//! Core contracts for the Generalized Stray-Arrow Decoding (GSAD).
//!
//! This crate deliberately contains few policy decisions. It defines shared
//! contracts and vocabulary; specialized algorithms belong in separate engines.

pub mod capability;
pub mod engine;
pub mod error;
pub mod file_sink;
pub mod output;
pub mod registry;
pub mod resource;
pub mod rle;
pub mod source;
pub mod task;
pub mod zstd_codec;

pub use capability::{CapabilityDescriptor, ConcurrencyModel, DataKind, Operation};
pub use engine::{
    EngineCompletion, EngineOutput, EngineRequest, ExecutionContext, ExecutionReport,
    ProcessingEngine,
};
pub use error::{ErrorKind, GsadError, Result};
pub use file_sink::AtomicFileSink;
pub use output::{execute_to_sink, BoundedOutputWriter, CommitSemantics, OutputSink, OutputWriter};
pub use registry::CapabilityRegistry;
pub use resource::{CancellationToken, ResourceLimits};
pub use rle::RleEngine;
pub use source::{DataSource, FileSource, SourceInfo, SourceReader, SourceSupport};
pub use task::{TaskId, TaskLifecycle, TaskState, TaskTransition};
pub use zstd_codec::ZstdEngine;
