use crate::wg::WgError;

pub(crate) enum Error {
    Hyper(hyper::Error),
    SerdeJson(serde_json::Error),
    Io(std::io::Error),
    Wg(WgError),
    MissingConfig(Vec<&'static str>),
    Abort,
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::SerdeJson(e)
    }
}

impl From<hyper::Error> for Error {
    fn from(e: hyper::Error) -> Self {
        Error::Hyper(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<tokio::task::JoinError> for Error {
    fn from(_: tokio::task::JoinError) -> Self {
        Error::Abort
    }
}

impl From<WgError> for Error {
    fn from(e: WgError) -> Self {
        Error::Wg(e)
    }
}

#[derive(Debug)]
pub(crate) struct ValidationError(pub(crate) &'static str);

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
