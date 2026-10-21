use std::{collections::BTreeMap, fmt, sync::Arc};

use crate::{
    capability::{DataKind, Operation},
    engine::ProcessingEngine,
    error::{ErrorKind, GsadError, Result},
    registry::CapabilityRegistry,
    resource::ResourceLimits,
};

/// A declared stage in a linear processing plan.
///
/// Parameter values are passed to the selected engine at execution time.
/// Until engines expose parameter schemas, this module cannot validate their
/// engine-specific meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanStage {
    pub engine_id: String,
    pub engine_version: String,
    pub operation: Operation,
    pub input_kind: DataKind,
    pub output_kind: DataKind,
    pub parameters: BTreeMap<String, String>,
    /// Whether this stage requires an engine declaring streaming support.
    pub requires_streaming: bool,
    /// Estimated memory needed by this stage, when known.
    ///
    /// This is a preflight estimate, not an enforced memory sandbox.
    pub memory_required_bytes: Option<u64>,
}

impl PlanStage {
    pub fn new(
        engine_id: impl Into<String>,
        engine_version: impl Into<String>,
        operation: Operation,
        input_kind: DataKind,
        output_kind: DataKind,
    ) -> Self {
        Self {
            engine_id: engine_id.into(),
            engine_version: engine_version.into(),
            operation,
            input_kind,
            output_kind,
            parameters: BTreeMap::new(),
            requires_streaming: false,
            memory_required_bytes: None,
        }
    }
}

/// A declarative, linear processing plan.
///
/// Validation checks all stages before an executor is allowed to start.
/// This type does not itself execute engines or enforce runtime resource limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    pub input_kind: DataKind,
    pub output_kind: DataKind,
    pub stages: Vec<PlanStage>,
}

#[derive(Clone)]
pub struct ValidatedPlanStage {
    definition: PlanStage,
    engine: Arc<dyn ProcessingEngine>,
}

impl ValidatedPlanStage {
    pub fn definition(&self) -> &PlanStage {
        &self.definition
    }

    /// Returns the exact engine instance resolved during validation.
    pub fn engine(&self) -> &dyn ProcessingEngine {
        self.engine.as_ref()
    }
}

impl fmt::Debug for ValidatedPlanStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValidatedPlanStage")
            .field("definition", &self.definition)
            .finish()
    }
}

/// Immutable, preflight-validated plan holding the resolved engine instances.
///
/// This prevents a later registry lookup from silently selecting a different
/// engine. It does not itself execute stages or guarantee runtime safety.
#[derive(Clone)]
pub struct ValidatedExecutionPlan {
    input_kind: DataKind,
    output_kind: DataKind,
    stages: Vec<ValidatedPlanStage>,
}

impl ValidatedExecutionPlan {
    pub fn input_kind(&self) -> DataKind {
        self.input_kind
    }

    pub fn output_kind(&self) -> DataKind {
        self.output_kind
    }

    pub fn stages(&self) -> &[ValidatedPlanStage] {
        &self.stages
    }
}

impl fmt::Debug for ValidatedExecutionPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValidatedExecutionPlan")
            .field("input_kind", &self.input_kind)
            .field("output_kind", &self.output_kind)
            .field("stages", &self.stages)
            .finish()
    }
}
impl ExecutionPlan {
    pub fn new(input_kind: DataKind, output_kind: DataKind, stages: Vec<PlanStage>) -> Self {
        Self {
            input_kind,
            output_kind,
            stages,
        }
    }

    /// Validates the whole plan against the current registry and task limits.
    ///
    /// No engine is executed by this method.
    pub fn validate(
        &self,
        registry: &CapabilityRegistry,
        limits: &ResourceLimits,
    ) -> Result<ValidatedExecutionPlan> {
        limits.validate()?;

        if self.stages.is_empty() {
            return Err(GsadError::invalid_argument(
                "execution plan must contain at least one stage",
            ));
        }

        let mut current_kind = self.input_kind;
        let mut validated_stages = Vec::with_capacity(self.stages.len());

        for (index, stage) in self.stages.iter().enumerate() {
            if stage.engine_id.trim().is_empty()
                || stage.engine_id.trim() != stage.engine_id.as_str()
            {
                return Err(GsadError::invalid_argument(format!(
                    "plan stage {index} has an invalid engine id",
                )));
            }

            if stage.engine_version.trim().is_empty()
                || stage.engine_version.trim() != stage.engine_version.as_str()
            {
                return Err(GsadError::invalid_argument(format!(
                    "plan stage {index} has an invalid engine version",
                )));
            }

            if stage.input_kind != current_kind {
                return Err(GsadError::new(
                    ErrorKind::Conflict,
                    format!(
                        "plan stage {index} expects {:?}, but the preceding output is {:?}",
                        stage.input_kind, current_kind,
                    ),
                ));
            }

            if stage.parameters.keys().any(|key| key.trim().is_empty()) {
                return Err(GsadError::invalid_argument(format!(
                    "plan stage {index} contains an empty parameter key",
                )));
            }

            if let Some(required) = stage.memory_required_bytes {
                if required == 0 {
                    return Err(GsadError::invalid_argument(format!(
                        "plan stage {index} memory requirement must be greater than zero",
                    )));
                }

                if required > limits.max_memory_bytes {
                    return Err(GsadError::new(
                        ErrorKind::ResourceLimit,
                        format!(
                            "plan stage {index} estimates {required} memory bytes, exceeding the task limit of {}",
                            limits.max_memory_bytes,
                        ),
                    ));
                }
            }

            let engine = registry.get(&stage.engine_id)?.ok_or_else(|| {
                GsadError::new(
                    ErrorKind::NotFound,
                    format!(
                        "plan stage {index} references unregistered engine `{}`",
                        stage.engine_id,
                    ),
                )
            })?;

            let descriptor = engine.descriptor();
            descriptor.validate()?;

            if descriptor.version != stage.engine_version {
                return Err(GsadError::new(
                    ErrorKind::Conflict,
                    format!(
                        "plan stage {index} expects engine `{}` version `{}`, but the registered version is `{}`",
                        stage.engine_id, stage.engine_version, descriptor.version,
                    ),
                ));
            }

            if !descriptor.supports(stage.operation, stage.input_kind) {
                return Err(GsadError::new(
                    ErrorKind::Unsupported,
                    format!(
                        "engine `{}` does not support operation {:?} with input kind {:?}",
                        stage.engine_id, stage.operation, stage.input_kind,
                    ),
                ));
            }

            if !descriptor.outputs.contains(&stage.output_kind) {
                return Err(GsadError::new(
                    ErrorKind::Unsupported,
                    format!(
                        "engine `{}` does not declare output kind {:?}",
                        stage.engine_id, stage.output_kind,
                    ),
                ));
            }

            if stage.requires_streaming && !descriptor.supports_streaming {
                return Err(GsadError::new(
                    ErrorKind::Unsupported,
                    format!(
                        "plan stage {index} requires streaming, but engine `{}` does not declare streaming support",
                        stage.engine_id,
                    ),
                ));
            }

            validated_stages.push(ValidatedPlanStage {
                definition: stage.clone(),
                engine,
            });

            current_kind = stage.output_kind;
        }

        if current_kind != self.output_kind {
            return Err(GsadError::new(
                ErrorKind::Conflict,
                format!(
                    "plan declares final output {:?}, but its last stage produces {:?}",
                    self.output_kind, current_kind,
                ),
            ));
        }

        Ok(ValidatedExecutionPlan {
            input_kind: self.input_kind,
            output_kind: self.output_kind,
            stages: validated_stages,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{CapabilityDescriptor, ConcurrencyModel, ProcessingEngine};

    struct TestEngine {
        descriptor: CapabilityDescriptor,
    }

    impl ProcessingEngine for TestEngine {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }
    }

    fn registry(streaming: bool) -> CapabilityRegistry {
        let registry = CapabilityRegistry::new();

        let engine = TestEngine {
            descriptor: CapabilityDescriptor {
                id: "test.transform".to_owned(),
                version: "1.2.3".to_owned(),
                operations: vec![Operation::Transform],
                inputs: vec![DataKind::Bytes],
                outputs: vec![DataKind::Text],
                supports_streaming: streaming,
                supports_random_access: false,
                deterministic: true,
                concurrency: ConcurrencyModel::Concurrent,
                max_memory_bytes: Some(4096),
            },
        };

        registry.register(Arc::new(engine)).unwrap();
        registry
    }

    fn stage() -> PlanStage {
        PlanStage::new(
            "test.transform",
            "1.2.3",
            Operation::Transform,
            DataKind::Bytes,
            DataKind::Text,
        )
    }

    fn plan(stages: Vec<PlanStage>) -> ExecutionPlan {
        ExecutionPlan::new(DataKind::Bytes, DataKind::Text, stages)
    }

    #[test]
    fn valid_plan_passes() {
        let mut stage = stage();
        stage.requires_streaming = true;
        stage.memory_required_bytes = Some(512);
        stage
            .parameters
            .insert("mode".to_owned(), "safe".to_owned());

        assert!(plan(vec![stage])
            .validate(&registry(true), &ResourceLimits::default())
            .is_ok());
    }

    #[test]
    fn empty_plan_is_rejected() {
        let error = plan(vec![])
            .validate(&registry(true), &ResourceLimits::default())
            .unwrap_err();

        assert_eq!(error.kind, ErrorKind::InvalidArgument);
    }

    #[test]
    fn missing_engine_is_rejected() {
        let mut stage = stage();
        stage.engine_id = "test.missing".to_owned();

        let error = plan(vec![stage])
            .validate(&CapabilityRegistry::new(), &ResourceLimits::default())
            .unwrap_err();

        assert_eq!(error.kind, ErrorKind::NotFound);
    }

    #[test]
    fn engine_version_must_match() {
        let mut stage = stage();
        stage.engine_version = "9.9.9".to_owned();

        let error = plan(vec![stage])
            .validate(&registry(true), &ResourceLimits::default())
            .unwrap_err();

        assert_eq!(error.kind, ErrorKind::Conflict);
    }

    #[test]
    fn unsupported_operation_is_rejected() {
        let mut stage = stage();
        stage.operation = Operation::Compress;

        let error = plan(vec![stage])
            .validate(&registry(true), &ResourceLimits::default())
            .unwrap_err();

        assert_eq!(error.kind, ErrorKind::Unsupported);
    }

    #[test]
    fn unsupported_output_kind_is_rejected() {
        let mut stage = stage();
        stage.output_kind = DataKind::Audio;

        let error = ExecutionPlan::new(DataKind::Bytes, DataKind::Audio, vec![stage])
            .validate(&registry(true), &ResourceLimits::default())
            .unwrap_err();

        assert_eq!(error.kind, ErrorKind::Unsupported);
    }

    #[test]
    fn stage_input_must_match_previous_output() {
        let first = stage();
        let second = stage();

        let error = plan(vec![first, second])
            .validate(&registry(true), &ResourceLimits::default())
            .unwrap_err();

        assert_eq!(error.kind, ErrorKind::Conflict);
    }

    #[test]
    fn streaming_requirement_is_checked() {
        let mut stage = stage();
        stage.requires_streaming = true;

        let error = plan(vec![stage])
            .validate(&registry(false), &ResourceLimits::default())
            .unwrap_err();

        assert_eq!(error.kind, ErrorKind::Unsupported);
    }

    #[test]
    fn memory_requirement_must_fit_limits() {
        let mut stage = stage();
        stage.memory_required_bytes = Some(128);

        let limits = ResourceLimits {
            max_input_bytes: 1024,
            max_output_bytes: 1024,
            max_memory_bytes: 64,
            max_expansion_ratio: None,
        };

        let error = plan(vec![stage])
            .validate(&registry(true), &limits)
            .unwrap_err();

        assert_eq!(error.kind, ErrorKind::ResourceLimit);
    }

    #[test]
    fn empty_parameter_key_is_rejected() {
        let mut stage = stage();
        stage.parameters.insert("  ".to_owned(), "value".to_owned());

        let error = plan(vec![stage])
            .validate(&registry(true), &ResourceLimits::default())
            .unwrap_err();

        assert_eq!(error.kind, ErrorKind::InvalidArgument);
    }

    #[test]
    fn validated_plan_retains_resolved_engine() {
        let validated = plan(vec![stage()])
            .validate(&registry(true), &ResourceLimits::default())
            .expect("plan should validate");

        assert_eq!(validated.input_kind(), DataKind::Bytes);
        assert_eq!(validated.output_kind(), DataKind::Text);
        assert_eq!(validated.stages().len(), 1);

        let resolved = &validated.stages()[0];
        assert_eq!(resolved.definition().engine_version, "1.2.3");
        assert_eq!(resolved.engine().descriptor().id, "test.transform");
        assert_eq!(resolved.engine().descriptor().version, "1.2.3");
    }
}
