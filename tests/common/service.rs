use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use wireguard_docker_plugin::service::{
    CreateEndpointOptions, CreateNetworkOptions, NetworkPluginService,
};
use wireguard_docker_plugin::types::{ConfigName, EndpointId, NetworkId};
use wireguard_docker_plugin::wg::mock::WgMock;
use wireguard_docker_plugin::wg::{ConfigProvider, Wg};

pub type TestServiceMock = TestService<WgMock>;

/// Test service wrapper that manages temporary directories and service lifecycle
pub struct TestService<WgImpl: Wg = WgMock> {
    service: Arc<NetworkPluginService<WgImpl>>,
    _db_dir: TempDir,
    _dnsfs_dir: TempDir,
    pub config_dir: TempDir,
}

impl<WgImpl: Wg> TestService<WgImpl> {
    /// Create a new test service with temporary directories
    pub fn new() -> std::io::Result<Self> {
        let db_dir = TempDir::new()?;
        let dnsfs_dir = TempDir::new()?;
        let config_dir = TempDir::new()?;

        let config_provider = ConfigProvider::new_file(config_dir.path().to_path_buf());

        let service = NetworkPluginService::new(
            db_dir.path().to_path_buf(),
            dnsfs_dir.path().to_path_buf(),
            config_provider,
        )?;

        Ok(Self {
            service: Arc::new(service),
            _db_dir: db_dir,
            _dnsfs_dir: dnsfs_dir,
            config_dir,
        })
    }

    /// Get a reference to the service
    pub fn service(&self) -> Arc<NetworkPluginService<WgImpl>> {
        Arc::clone(&self.service)
    }

    /// Write a config file to the service's config directory
    pub fn write_config(&self, name: &str, content: &str) -> std::io::Result<PathBuf> {
        let path = self.config_dir.path().join(format!("{}.conf", name));
        std::fs::write(&path, content)?;
        Ok(path)
    }

    /// Get the path to the config directory
    pub fn config_path(&self) -> &std::path::Path {
        self.config_dir.path()
    }

    pub async fn install_interface(
        &self,
        network_id: &str,
        endpoint_id: &str,
        config_name: &str,
    ) -> Result<(), wireguard_docker_plugin::errors::Error> {
        let network_id: &NetworkId = network_id.try_into().unwrap();
        let endpoint_id: &EndpointId = endpoint_id.try_into().unwrap();
        let config_name: &ConfigName = config_name.try_into().unwrap();
        self.service
            .create_network(CreateNetworkOptions {
                network_id,
                config_name,
            })
            .await?;
        self.service
            .create_endpoint(CreateEndpointOptions {
                network_id,
                endpoint_id,
            })
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use wireguard_docker_plugin::wg::mock::WgMock;

    use super::*;

    #[tokio::test]
    async fn test_service_creation() {
        let test_service = TestService::<WgMock>::new().unwrap();
        assert!(test_service.config_path().exists());
    }

    #[tokio::test]
    async fn test_write_config() {
        let test_service = TestService::<WgMock>::new().unwrap();
        let path = test_service.write_config("test", "content").unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "content");
    }
}
