mod config;
pub use config::*;

use thiserror::Error;

pub mod dummy;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod mock;

use crate::types::EndpointId;

pub trait Wg: Sized {
    fn new() -> Result<Self, WgError>;
    fn create_interface(
        &self,
        endpoint_id: &EndpointId,
        config: Config,
        ifalias: &str,
    ) -> impl std::future::Future<Output = Result<String, WgError>> + Send;
    fn delete_interface(
        &self,
        endpoint_id: &EndpointId,
    ) -> impl std::future::Future<Output = ()> + Send;
}

#[derive(Debug, Error)]
enum ErrorInner {
    #[error("I/O error")]
    Io(#[from] std::io::Error),
    #[error("error reading config: {0}")]
    ConfigParse(String),
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    Linux(#[from] linux::Error),
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct WgError(ErrorInner);

impl<T> From<T> for WgError
where
    T: Into<ErrorInner>,
{
    fn from(err: T) -> Self {
        WgError(err.into())
    }
}

cfg_if::cfg_if! {
    if #[cfg(target_os = "linux")] {
        pub type WgDefault = linux::WgLinux;
    } else {
        pub type WgDefault = dummy::WgDummy;
    }
}
