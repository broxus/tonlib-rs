use ton_api::ton;

#[derive(thiserror::Error, Debug, Clone)]
pub enum TonlibError {
    #[error("invalid address")]
    InvalidAddress,
    #[error("account not found")]
    AccountNotFound,
    #[error("Connection error")]
    ConnectionError,
    #[error("Failed to serialize message")]
    FailedToSerialize,
    #[error("Lite server error. code: {}, reason: {}", .0.code(), .0.message())]
    LiteServer(ton::lite_server::Error),
    #[error("Invalid account state proof")]
    InvalidAccountStateProof,
    #[error("Invalid block")]
    InvalidBlock,
    #[error("Unknown")]
    Unknown,
    #[error("Not ready")]
    NotReady,
}

pub type TonlibResult<T> = Result<T, TonlibError>;
