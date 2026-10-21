use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Cooperative cancellation signal. Engines must check it at safe points.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn check(&self) -> crate::Result<()> {
        if self.is_cancelled() {
            Err(crate::GsadError::new(
                crate::ErrorKind::Cancelled,
                "task cancellation requested",
            ))
        } else {
            Ok(())
        }
    }
}

/// Per-task limits. These are enforced at selected core/engine boundaries; they do not
/// provide a process-wide memory cap or sandbox native code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceLimits {
    pub max_input_bytes: u64,
    pub max_output_bytes: u64,
    pub max_memory_bytes: u64,
    pub max_expansion_ratio: Option<u32>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 256 * 1024 * 1024,
            max_output_bytes: 512 * 1024 * 1024,
            max_memory_bytes: 256 * 1024 * 1024,
            max_expansion_ratio: Some(200),
        }
    }
}

impl ResourceLimits {
    pub fn validate(&self) -> crate::Result<()> {
        if self.max_input_bytes == 0 {
            return Err(crate::GsadError::invalid_argument(
                "max_input_bytes must be greater than zero",
            ));
        }
        if self.max_output_bytes == 0 {
            return Err(crate::GsadError::invalid_argument(
                "max_output_bytes must be greater than zero",
            ));
        }
        if self.max_memory_bytes == 0 {
            return Err(crate::GsadError::invalid_argument(
                "max_memory_bytes must be greater than zero",
            ));
        }
        if matches!(self.max_expansion_ratio, Some(0)) {
            return Err(crate::GsadError::invalid_argument(
                "max_expansion_ratio must be None or greater than zero",
            ));
        }
        Ok(())
    }
}
