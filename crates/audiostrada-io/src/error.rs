use crate::BackendKind;

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("{0} backend is not available on this system")]
    BackendUnavailable(BackendKind),
    #[error("no audio backend available")]
    NoBackend,
    #[error("device not found: {0}")]
    DeviceNotFound(String),
    #[error("unsupported configuration: {0}")]
    UnsupportedConfig(String),
    #[error("{backend} error: {message}")]
    Backend {
        backend: BackendKind,
        message: String,
    },
}

impl IoError {
    pub(crate) fn backend(backend: BackendKind, message: impl ToString) -> Self {
        IoError::Backend {
            backend,
            message: message.to_string(),
        }
    }
}
