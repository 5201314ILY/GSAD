use crate::{ErrorKind, GsadError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TaskState {
    Queued,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        use TaskState::{Cancelled, Cancelling, Failed, Queued, Running, Succeeded};
        matches!(
            (self, next),
            (Queued, Running)
                | (Queued, Cancelling)
                | (Queued, Cancelled)
                | (Queued, Failed)
                | (Running, Succeeded)
                | (Running, Failed)
                | (Running, Cancelling)
                | (Cancelling, Cancelled)
                | (Cancelling, Failed)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(GsadError::new(
                ErrorKind::Conflict,
                format!("invalid task state transition: {self:?} -> {next:?}"),
            ))
        }
    }
}
