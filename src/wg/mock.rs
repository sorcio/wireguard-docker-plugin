use super::{Config, WgError};
use crate::types::EndpointId;

/// Mock WireGuard implementation for testing.
///
/// This implementation simulates WireGuard operations without requiring
/// actual network interfaces or kernel modules. It's used in integration
/// tests to isolate the volume plugin functionality from the network layer.
#[derive(Debug)]
pub struct WgMock;

impl super::Wg for WgMock {
    fn new() -> Result<Self, WgError> {
        Ok(WgMock)
    }

    async fn create_interface(
        &self,
        endpoint_id: &EndpointId,
        _config: Config,
        _ifalias: &str,
    ) -> Result<String, WgError> {
        // Return a predictable interface name for testing
        Ok(format!("wg-mock-{}", endpoint_id.as_str()))
    }

    async fn delete_interface(&self, _endpoint_id: &EndpointId) {}
}
