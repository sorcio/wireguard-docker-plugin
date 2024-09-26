use super::{Config, WgError};
use crate::types::EndpointId;

#[derive(Debug)]
pub(crate) struct Wg;

impl Wg {
    pub(crate) fn new() -> Result<Self, WgError> {
        Ok(Wg)
    }

    pub(crate) async fn create_interface(
        &self,
        _endpoint_id: &EndpointId,
        _config: Config,
    ) -> Result<String, WgError> {
        Ok(String::from("dummy-interface-name-for-testing"))
    }

    pub(crate) async fn delete_interface(&self, _endpoint_id: &EndpointId) {}
}

#[derive(Debug, thiserror::Error)]
pub(super) enum WgErrorInner {
    #[error("I/O error")]
    Io(#[from] std::io::Error),
    #[error("error reading config: {0}")]
    ConfigParse(String),
}
