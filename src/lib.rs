use std::fmt;

#[macro_export]
macro_rules! worker {
    ($target: expr, $($arg:tt)+) => {{
        let trigger = std::sync::Arc::new(tokio::sync::Notify::new());
        let tc = trigger.clone();
        let fut = tokio::task::spawn(async move {
            loop {
                tc.notified().await;
                $target($($arg)+).await;
            }
        });
        (trigger, fut)
    }};
    ($target: expr) => {{
        let trigger = std::sync::Arc::new(tokio::sync::Notify::new());
        let tc = trigger.clone();
        let fut = tokio::task::spawn(async move {
        loop {
        tc.notified().await;
        $target().await;
        }
        });
        (trigger, fut)
        }};
}

#[derive(Debug, Eq, PartialEq)]
pub enum ErrorKind {
    Duplicate,
    NotFound,
    Timeout,
    InvalidData,
    Internal,
}

impl ErrorKind {
    fn as_str(&self) -> &str {
        match self {
            ErrorKind::Duplicate => "Duplicate",
            ErrorKind::NotFound => "Not found",
            ErrorKind::Timeout => "Timeout",
            ErrorKind::Internal => "Internal",
            ErrorKind::InvalidData => "InvalidData",
        }
    }
}

#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: Option<String>,
}

#[allow(clippy::must_use_candidate)]
impl Error {
    pub fn duplicate<T: fmt::Display>(message: T) -> Self {
        Self {
            kind: ErrorKind::Duplicate,
            message: Some(message.to_string()),
        }
    }
    pub fn not_found<T: fmt::Display>(message: T) -> Self {
        Self {
            kind: ErrorKind::NotFound,
            message: Some(message.to_string()),
        }
    }
    pub fn timeout() -> Self {
        Self {
            kind: ErrorKind::Timeout,
            message: None,
        }
    }
    pub fn internal<T: fmt::Display>(message: T) -> Self {
        Self {
            kind: ErrorKind::Internal,
            message: Some(message.to_string()),
        }
    }
    pub fn invalid_data<T: fmt::Display>(message: T) -> Self {
        Self {
            kind: ErrorKind::InvalidData,
            message: Some(message.to_string()),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(ref message) = self.message {
            write!(f, "{}: {}", self.kind.as_str(), message)
        } else {
            write!(f, "{}", self.kind.as_str())
        }
    }
}

pub mod mpsc;
pub mod process;
pub mod sync;
pub mod tools;
pub mod workers;
