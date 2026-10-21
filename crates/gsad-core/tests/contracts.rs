use std::collections::BTreeMap;
use std::sync::Arc;

use gsad_core::source::MemorySource;
use gsad_core::{
    CapabilityDescriptor, CapabilityRegistry, ConcurrencyModel, DataKind, DataSource, EngineOutput,
    EngineRequest, ErrorKind, ExecutionContext, GsadError, Operation, ProcessingEngine,
    ResourceLimits, TaskState,
};

struct EchoEngine {
    descriptor: CapabilityDescriptor,
}

impl EchoEngine {
    fn new(id: &str) -> Self {
        Self {
            descriptor: CapabilityDescriptor {
                id: id.to_owned(),
                version: "0.1.0".to_owned(),
                operations: vec![Operation::Transform],
                inputs: vec![DataKind::Bytes],
                outputs: vec![DataKind::Bytes],
                supports_streaming: true,
                supports_random_access: true,
                deterministic: true,
                concurrency: ConcurrencyModel::Concurrent,
                max_memory_bytes: Some(1024 * 1024),
            },
        }
    }
}

impl ProcessingEngine for EchoEngine {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn execute(
        &self,
        request: &EngineRequest,
        context: &ExecutionContext,
    ) -> gsad_core::Result<EngineOutput> {
        context.check_cancelled()?;
        let info = request.input.info();
        let length = info.known_length.ok_or_else(|| {
            GsadError::new(ErrorKind::Unsupported, "test engine requires known length")
        })?;
        if length > context.limits.max_input_bytes {
            return Err(GsadError::new(ErrorKind::ResourceLimit, "input too large"));
        }
        let capacity = usize::try_from(length).map_err(|_| {
            GsadError::new(ErrorKind::ResourceLimit, "input too large for platform")
        })?;
        let mut output = vec![0; capacity];
        let mut offset = 0;
        while offset < output.len() {
            context.check_cancelled()?;
            let count = request
                .input
                .read_at(offset as u64, &mut output[offset..])?;
            if count == 0 {
                return Err(GsadError::new(
                    ErrorKind::UnexpectedEof,
                    "source ended early",
                ));
            }
            offset += count;
        }
        let result = EngineOutput::new(output, DataKind::Bytes);
        context.validate_output(&result)?;
        Ok(result)
    }
}

#[test]
fn memory_source_sequential_and_random_access_contracts() {
    let source = MemorySource::new(Arc::<[u8]>::from(&b"abcdef"[..]));
    let mut reader = source.open_reader().expect("reader");
    let mut first = [0_u8; 2];
    assert_eq!(reader.read(&mut first).expect("read"), 2);
    assert_eq!(&first, b"ab");
    assert_eq!(reader.position(), 2);
    reader.seek(4).expect("seek");
    let mut last = [0_u8; 4];
    assert_eq!(reader.read(&mut last).expect("read"), 2);
    assert_eq!(&last[..2], b"ef");
    assert_eq!(reader.read(&mut last).expect("EOF"), 0);

    let mut random = [0_u8; 3];
    assert_eq!(source.read_at(2, &mut random).expect("read_at"), 3);
    assert_eq!(&random, b"cde");
    assert_eq!(source.read_at(99, &mut random).expect("past EOF"), 0);
}

#[test]
fn registry_rejects_duplicate_ids_and_finds_by_contract() {
    let registry = CapabilityRegistry::new();
    registry
        .register(Arc::new(EchoEngine::new("test.echo")))
        .expect("register");
    let error = registry
        .register(Arc::new(EchoEngine::new("test.echo")))
        .expect_err("duplicate should fail");
    assert_eq!(error.kind, ErrorKind::Conflict);

    assert_eq!(
        registry
            .find(Operation::Transform, DataKind::Bytes)
            .expect("find")
            .len(),
        1
    );
    assert_eq!(
        registry
            .find(Operation::Compress, DataKind::Bytes)
            .expect("find")
            .len(),
        0
    );
}

#[test]
fn processing_engine_obeys_output_and_input_contracts() {
    let engine = EchoEngine::new("test.echo");
    let source: Arc<dyn DataSource> = Arc::new(MemorySource::new(Arc::<[u8]>::from(&b"hello"[..])));
    let request = EngineRequest {
        operation: Operation::Transform,
        input_kind: DataKind::Bytes,
        input: source,
        parameters: BTreeMap::new(),
    };
    let context = ExecutionContext::new(ResourceLimits::default()).expect("context");
    let output = engine.execute(&request, &context).expect("execute");
    assert_eq!(output.data, b"hello");

    let small_context = ExecutionContext::new(ResourceLimits {
        max_input_bytes: 10,
        max_output_bytes: 2,
        max_memory_bytes: 10,
        max_expansion_ratio: None,
    })
    .expect("small context");
    let error = engine
        .execute(&request, &small_context)
        .expect_err("limit should fail");
    assert_eq!(error.kind, ErrorKind::ResourceLimit);
}

#[test]
fn cancellation_is_cooperative_and_visible() {
    let context = ExecutionContext::new(ResourceLimits::default()).expect("context");
    context.cancellation.cancel();
    assert_eq!(
        context.check_cancelled().expect_err("cancelled").kind,
        ErrorKind::Cancelled
    );
}

#[test]
fn task_state_machine_rejects_terminal_reentry() {
    assert_eq!(
        TaskState::Queued
            .transition(TaskState::Running)
            .expect("start"),
        TaskState::Running
    );
    assert!(TaskState::Succeeded.is_terminal());
    assert!(TaskState::Succeeded.transition(TaskState::Running).is_err());
    assert!(TaskState::Running.can_transition_to(TaskState::Cancelling));
    assert!(TaskState::Cancelling.can_transition_to(TaskState::Cancelled));
}

#[test]
fn descriptors_and_resource_limits_validate() {
    let invalid = CapabilityDescriptor {
        id: " ".to_owned(),
        version: "0.1.0".to_owned(),
        operations: vec![Operation::Transform],
        inputs: vec![DataKind::Bytes],
        outputs: vec![DataKind::Bytes],
        supports_streaming: false,
        supports_random_access: false,
        deterministic: true,
        concurrency: ConcurrencyModel::Serialized,
        max_memory_bytes: None,
    };
    assert_eq!(
        invalid.validate().expect_err("empty id").kind,
        ErrorKind::InvalidArgument
    );
    assert!(ResourceLimits::default().validate().is_ok());
}
