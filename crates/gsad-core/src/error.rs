use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// Shared result type used across GSAD core contracts.
pub type Result<T> = std::result::Result<T, GsadError>;

/// Stable high-level error categories. Detailed implementation errors should
/// be carried in `message`, without leaking secrets or key material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    InvalidArgument,
    Unsupported,
    NotFound,
    UnexpectedEof,
    Io,
    CorruptData,
    AuthenticationFailed,
    ResourceLimit,
    Cancelled,
    Timeout,
    Conflict,
    Busy,
    Internal,
}

/// Error information suitable for logging and conversion at API boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GsadError {
    pub kind: ErrorKind,
    pub message: String,
    pub operation: Option<&'static str>,
    pub offset: Option<u64>,
    pub retryable: bool,
}

impl GsadError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            operation: None,
            offset: None,
            retryable: false,
        }
    }

    pub fn at_offset(mut self, offset: u64) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn during(mut self, operation: &'static str) -> Self {
        self.operation = Some(operation);
        self
    }

    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidArgument, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unsupported, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

impl Display for GsadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)?;
        if let Some(operation) = self.operation {
            write!(f, " [operation={operation}]")?;
        }
        if let Some(offset) = self.offset {
            write!(f, " [offset={offset}]")?;
        }
        if self.retryable {
            write!(f, " [retryable]")?;
        }
        Ok(())
    }
}

impl Error for GsadError {}

impl From<std::io::Error> for GsadError {
    fn from(error: std::io::Error) -> Self {
        let retryable = matches!(
            error.kind(),
            std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::TimedOut
        );
        Self::new(ErrorKind::Io, error.to_string()).retryable(retryable)
    }
}
