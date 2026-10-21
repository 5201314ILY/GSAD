use std::fmt;

use crate::{ErrorKind, GsadError, Result};

/// Maximum encoded length of a task identifier, in bytes.
const MAX_TASK_ID_LEN: usize = 128;

/// Opaque, validated identifier assigned to a task by its caller.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(String);

impl TaskId {
    /// Creates a task identifier.
    ///
    /// Identifiers must be non-empty, at most 128 bytes, contain no control
    /// characters, and have no leading or trailing whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();

        if value.trim().is_empty() {
            return Err(GsadError::invalid_argument("task id must not be empty"));
        }

        if value.len() > MAX_TASK_ID_LEN {
            return Err(GsadError::invalid_argument(
                "task id must not exceed 128 bytes",
            ));
        }

        if value.trim() != value.as_str() {
            return Err(GsadError::invalid_argument(
                "task id must not have leading or trailing whitespace",
            ));
        }

        if value.chars().any(char::is_control) {
            return Err(GsadError::invalid_argument(
                "task id must not contain control characters",
            ));
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

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

/// A single successful state transition in a task's lifecycle.
///
/// Sequence numbers start at 1 and increase independently for each task.
/// The initial `Queued` state is represented by the lifecycle itself, not by
/// a transition event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTransition {
    pub task_id: TaskId,
    pub sequence: u64,
    pub from: TaskState,
    pub to: TaskState,
}

/// In-memory lifecycle tracker for one task.
///
/// This type validates transitions but does not execute tasks, schedule work,
/// persist events, or provide internal synchronization. Callers sharing a
/// tracker between threads must synchronize access themselves.
#[derive(Debug, PartialEq, Eq)]
pub struct TaskLifecycle {
    task_id: TaskId,
    state: TaskState,
    sequence: u64,
}

impl TaskLifecycle {
    /// Starts a new lifecycle in the `Queued` state.
    pub fn new(task_id: TaskId) -> Self {
        Self {
            task_id,
            state: TaskState::Queued,
            sequence: 0,
        }
    }

    pub fn task_id(&self) -> &TaskId {
        &self.task_id
    }

    pub fn state(&self) -> TaskState {
        self.state
    }

    /// Number of successful transitions recorded for this task.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }

    /// Applies a legal transition and returns its event.
    ///
    /// On error, neither the state nor the sequence number changes.
    pub fn transition(&mut self, next: TaskState) -> Result<TaskTransition> {
        let next_state = self.state.transition(next)?;

        let next_sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| GsadError::internal("task transition sequence overflow"))?;

        let event = TaskTransition {
            task_id: self.task_id.clone(),
            sequence: next_sequence,
            from: self.state,
            to: next_state,
        };

        self.state = next_state;
        self.sequence = next_sequence;

        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_id_accepts_valid_identifiers() {
        let id = TaskId::new("gsad-task-0001").unwrap();

        assert_eq!(id.as_str(), "gsad-task-0001");
        assert_eq!(id.to_string(), "gsad-task-0001");
    }

    #[test]
    fn task_id_rejects_empty_or_malformed_identifiers() {
        for value in ["", "   ", " leading", "trailing ", "line\nbreak"] {
            let error = TaskId::new(value).unwrap_err();
            assert_eq!(error.kind, ErrorKind::InvalidArgument);
        }

        let error = TaskId::new("x".repeat(MAX_TASK_ID_LEN + 1)).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidArgument);
    }

    #[test]
    fn lifecycle_records_ordered_transitions() {
        let id = TaskId::new("task-1").unwrap();
        let mut lifecycle = TaskLifecycle::new(id.clone());

        assert_eq!(lifecycle.state(), TaskState::Queued);
        assert_eq!(lifecycle.sequence(), 0);
        assert!(!lifecycle.is_terminal());

        let started = lifecycle.transition(TaskState::Running).unwrap();

        assert_eq!(started.task_id, id);
        assert_eq!(started.sequence, 1);
        assert_eq!(started.from, TaskState::Queued);
        assert_eq!(started.to, TaskState::Running);

        let completed = lifecycle.transition(TaskState::Succeeded).unwrap();

        assert_eq!(completed.sequence, 2);
        assert_eq!(completed.from, TaskState::Running);
        assert_eq!(completed.to, TaskState::Succeeded);
        assert_eq!(lifecycle.sequence(), 2);
        assert!(lifecycle.is_terminal());
    }

    #[test]
    fn invalid_transition_does_not_mutate_lifecycle() {
        let id = TaskId::new("task-invalid-transition").unwrap();
        let mut lifecycle = TaskLifecycle::new(id);

        let error = lifecycle.transition(TaskState::Succeeded).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Conflict);
        assert_eq!(lifecycle.state(), TaskState::Queued);
        assert_eq!(lifecycle.sequence(), 0);
    }

    #[test]
    fn cancellation_uses_explicit_lifecycle_states() {
        let id = TaskId::new("task-cancel").unwrap();
        let mut lifecycle = TaskLifecycle::new(id);

        let requested = lifecycle.transition(TaskState::Cancelling).unwrap();
        assert_eq!(requested.sequence, 1);
        assert_eq!(requested.to, TaskState::Cancelling);

        let cancelled = lifecycle.transition(TaskState::Cancelled).unwrap();
        assert_eq!(cancelled.sequence, 2);
        assert_eq!(cancelled.to, TaskState::Cancelled);
        assert!(lifecycle.is_terminal());
    }

    #[test]
    fn terminal_state_rejects_further_transitions() {
        let id = TaskId::new("task-terminal").unwrap();
        let mut lifecycle = TaskLifecycle::new(id);

        lifecycle.transition(TaskState::Running).unwrap();
        lifecycle.transition(TaskState::Succeeded).unwrap();

        assert!(lifecycle.transition(TaskState::Running).is_err());
        assert_eq!(lifecycle.state(), TaskState::Succeeded);
        assert_eq!(lifecycle.sequence(), 2);
    }
}
