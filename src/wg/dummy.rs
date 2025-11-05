use super::{Config, WgError};
use crate::types::EndpointId;

#[derive(Debug)]
pub struct WgDummy;

impl super::Wg for WgDummy {
    fn new() -> Result<Self, WgError> {
        Ok(WgDummy)
    }

    async fn create_interface(
        &self,
        _endpoint_id: &EndpointId,
        _config: Config,
        _ifalias: &str,
    ) -> Result<String, WgError> {
        Ok(String::from("dummy-interface-name-for-testing"))
    }

    async fn delete_interface(&self, _endpoint_id: &EndpointId) {}
}
