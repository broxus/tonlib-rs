use failure::Fail;

#[derive(Debug, Clone, Fail)]
pub enum TonlibError {
    #[fail(display = "invalid address")]
    InvalidAddress,
    #[fail(display = "account not found")]
    AccountNotFound,
    #[fail(display = "adnl error. {}", 0)]
    AdnlError(String),
    #[fail(display = "failed to parse account state")]
    InvalidAccountState,
    #[fail(display = "liteserver error. {} - {}", code, message)]
    ExecutionError { code: i32, message: String },
    #[fail(display = "unknown error")]
    UnknownError,
}

pub type TonlibResult<T> = Result<T, TonlibError>;
