use failure::Fail;

#[derive(Debug, Fail)]
pub enum TonlibError {
    #[fail(display = "failed to serialize query. {}", reason)]
    SerializationError { reason: String },
    #[fail(display = "failed to deserialize response. {}", reason)]
    DeserializationError { reason: String },
    #[fail(display = "tonlib error. {} - {}", code, message)]
    ExecutionError { code: u32, message: String },
}

pub type TonlibResult<T> = Result<T, TonlibError>;