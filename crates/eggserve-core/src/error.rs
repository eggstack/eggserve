#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("path escapes configured root")]
    PathEscape,

    #[error("path not accessible: {0}")]
    PathNotAccessible(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("bind error: {0}")]
    Bind(String),

    #[error("runtime error: {0}")]
    Runtime(String),

    #[error("request rejected: {0}")]
    RequestRejected(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<crate::path::PathRejection> for Error {
    fn from(rejection: crate::path::PathRejection) -> Self {
        Error::RequestRejected(rejection.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
