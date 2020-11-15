use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum TonlibError {
    #[error("failed to serialize query. {reason}")]
    SerializationError { reason: String },
    #[error("failed to deserialize response. {reason}")]
    DeserializationError { reason: String },
}

pub type TonlibResult<T> = Result<T, TonlibError>;
