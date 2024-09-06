mod config;
pub(crate) use config::*;

use thiserror::Error;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub(crate) use linux::*;

#[derive(Debug, Error)]
#[error(transparent)]
pub(crate) struct WgError(#[from] WgErrorInner);
