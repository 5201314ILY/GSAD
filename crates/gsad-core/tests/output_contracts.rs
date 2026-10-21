use std::collections::BTreeMap;
use std::sync::Arc;

use gsad_core::source::MemorySource;
use gsad_core::{
    execute_to_sink, CapabilityDescriptor, CommitSemantics, ConcurrencyModel, DataKind, DataSource,
    EngineCompletion, EngineRequest, ErrorKind, ExecutionContext, GsadError, Operation, OutputSink,
    OutputWriter, ProcessingEngine, ResourceLimits,
};

struct TestSink {
    staged: Vec<u8>,
    committed: Option<Vec<u8>>,
    aborted: bool,
    max_write: usize,
    fail_flush: bool,
    fail_commit: bool,
}

impl TestSink {
    fn new(max_write: usize) -> Self {
        Self {
            staged: Vec::new(),
            committed: None,
            aborted: false,
            max_write,
            fail_flush: false,
            fail_commit: false,
        }
    }
}

impl OutputSink for TestSink {
    fn write(&mut self, buffer: &[u8]) -> gsad_core::Result<usize> {
        let count = buffer.len().min(self.max_write.max(1));
        self.staged.extend_from_slice(&buffer[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> gsad_core::Result<()> {
        if self.fail_flush {
            Err(GsadError::new(ErrorKind::Io, "injected flush failure"))
        } else {
            Ok(())
        }
    }

    fn commit(&mut self) -> gsad_core::Result<()> {
        if self.fail_commit {
            return Err(GsadError::new(ErrorKind::Io, "injected commit failure"));
        }
        self.committed = Some(std::mem::take(&mut self.staged));
        Ok(())
    }

    fn abort(&mut self) -> gsad_core::Result<()> {
        self.staged.clear();
        self.aborted = true;
        Ok(())
    }

    fn commit_semantics(&self) -> CommitSemantics {
        CommitSemantics::Atomic
    }
}

struct StreamingEngine {
    descriptor: CapabilityDescriptor,
    fail_after_first_chunk: bool,
    report_text: bool,
}

impl StreamingEngine {
    fn new() -> Self {
        Self {
            descriptor: CapabilityDescriptor {
                id: "test.streaming".to_owned(),
                version: "0.1.0".to_owned(),
                operations: vec![Operation::Transform],
                inputs: vec![DataKind::Bytes],
                outputs: vec![DataKind::Bytes],
                supports_streaming: true,
                supports_random_access: false,
                deterministic: true,
                concurrency: ConcurrencyModel::Concurrent,
                max_memory_bytes: Some(1024),
            },
            fail_after_first_chunk: false,
            report_text: false,
        }
    }
}

impl ProcessingEngine for StreamingEngine {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn execute_to(
        &self,
        _request: &EngineRequest,
        _context: &ExecutionContext,
        writer: &mut dyn OutputWriter,
    ) -> gsad_core::Result<EngineCompletion> {
        writer.write_all(b"hello ")?;
        if self.fail_after_first_chunk {
            return Err(GsadError::new(
                ErrorKind::CorruptData,
                "injected engine failure",
            ));
        }
        writer.write_all(b"world")?;
        Ok(EngineCompletion::new(if self.report_text {
            DataKind::Text
        } else {
            DataKind::Bytes
        }))
    }
}

fn request() -> EngineRequest {
    let input: Arc<dyn DataSource> = Arc::new(MemorySource::new(Arc::<[u8]>::from(&b"input"[..])));
    EngineRequest {
        operation: Operation::Transform,
        input_kind: DataKind::Bytes,
        input,
        parameters: BTreeMap::new(),
    }
}

fn context(max_output_bytes: u64) -> ExecutionContext {
    ExecutionContext::new(ResourceLimits {
        max_input_bytes: 1024,
        max_output_bytes,
        max_memory_bytes: 1024,
        max_expansion_ratio: None,
    })
    .expect("valid context")
}

#[test]
fn streaming_engine_commits_short_writes_and_reports_actual_byte_count() {
    let engine = StreamingEngine::new();
    let mut sink = TestSink::new(2);
    let report = execute_to_sink(&engine, &request(), &context(64), &mut sink).expect("execute");
    assert_eq!(report.bytes_written, 11);
    assert_eq!(report.data_kind, DataKind::Bytes);
    assert_eq!(report.commit_semantics, CommitSemantics::Atomic);
    assert_eq!(sink.committed.as_deref(), Some(&b"hello world"[..]));
    assert!(!sink.aborted);
}

#[test]
fn output_limit_is_enforced_before_an_over_limit_chunk_is_written() {
    let engine = StreamingEngine::new();
    let mut sink = TestSink::new(64);
    let error = execute_to_sink(&engine, &request(), &context(5), &mut sink)
        .expect_err("six-byte first chunk must exceed the limit");
    assert_eq!(error.kind, ErrorKind::ResourceLimit);
    assert!(sink.aborted);
    assert!(sink.staged.is_empty());
    assert!(sink.committed.is_none());
}

#[test]
fn engine_failure_aborts_partial_output() {
    let mut engine = StreamingEngine::new();
    engine.fail_after_first_chunk = true;
    let mut sink = TestSink::new(3);
    let error = execute_to_sink(&engine, &request(), &context(64), &mut sink)
        .expect_err("injected failure should be propagated");
    assert_eq!(error.kind, ErrorKind::CorruptData);
    assert!(sink.aborted);
    assert!(sink.staged.is_empty());
    assert!(sink.committed.is_none());
}

#[test]
fn undeclared_output_kind_aborts_output() {
    let mut engine = StreamingEngine::new();
    engine.report_text = true;
    let mut sink = TestSink::new(64);
    let error = execute_to_sink(&engine, &request(), &context(64), &mut sink)
        .expect_err("undeclared output kind must fail");
    assert_eq!(error.kind, ErrorKind::Internal);
    assert!(sink.aborted);
    assert!(sink.committed.is_none());
}

#[test]
fn flush_and_commit_failures_trigger_abort() {
    let engine = StreamingEngine::new();
    let mut flush_sink = TestSink::new(64);
    flush_sink.fail_flush = true;
    assert!(execute_to_sink(&engine, &request(), &context(64), &mut flush_sink).is_err());
    assert!(flush_sink.aborted);
    assert!(flush_sink.committed.is_none());

    let mut commit_sink = TestSink::new(64);
    commit_sink.fail_commit = true;
    assert!(execute_to_sink(&engine, &request(), &context(64), &mut commit_sink).is_err());
    assert!(commit_sink.aborted);
    assert!(commit_sink.committed.is_none());
}

#[test]
fn cancellation_before_execution_aborts_without_committing() {
    let engine = StreamingEngine::new();
    let context = context(64);
    context.cancellation.cancel();
    let mut sink = TestSink::new(64);
    let error = execute_to_sink(&engine, &request(), &context, &mut sink)
        .expect_err("cancelled task must not commit");
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert!(sink.aborted);
    assert!(sink.committed.is_none());
    assert!(sink.staged.is_empty());
}
